//! Latency benchmark
//!
//! Measures scheduling latencies under various conditions to validate
//! that kernel-guided cooperation improves tail latency.

use clap::Parser;
use morpheus_runtime::{checkpoint_sync, Builder};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Latency distribution benchmark
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Duration to run the benchmark (seconds)
    #[arg(short, long, default_value_t = 30)]
    duration: u64,

    /// Number of worker threads
    #[arg(short = 'w', long, default_value_t = 4)]
    workers: usize,

    /// Target ops per second per worker
    #[arg(short, long, default_value_t = 10_000)]
    ops_per_second: u64,

    /// Enable kernel pressure simulation (CPU-bound background work)
    #[arg(long)]
    pressure: bool,

    /// Enable checkpoint calls
    #[arg(long)]
    with_checkpoints: bool,
}

struct LatencyHistogram {
    buckets: [AtomicU64; 32],
}

impl LatencyHistogram {
    fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn record(&self, latency_us: u64) {
        // Bucket index: log2(latency_us + 1), clamped to 31
        let bucket = (64 - (latency_us + 1).leading_zeros()).min(31) as usize;
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }

    fn percentile(&self, p: f64) -> u64 {
        let total: u64 = self.buckets.iter().map(|b| b.load(Ordering::Relaxed)).sum();

        let target = (total as f64 * p / 100.0) as u64;
        let mut count = 0u64;

        for (i, bucket) in self.buckets.iter().enumerate() {
            count += bucket.load(Ordering::Relaxed);
            if count >= target {
                return 1u64 << i;
            }
        }

        1 << 31
    }

    fn total(&self) -> u64 {
        self.buckets.iter().map(|b| b.load(Ordering::Relaxed)).sum()
    }
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("Latency benchmark");
    tracing::info!("  Duration: {}s", args.duration);
    tracing::info!("  Workers: {}", args.workers);
    tracing::info!("  Target ops/s per worker: {}", args.ops_per_second);
    tracing::info!("  Pressure simulation: {}", args.pressure);
    tracing::info!("  Checkpoints enabled: {}", args.with_checkpoints);

    let stop = Arc::new(AtomicBool::new(false));
    let histogram = Arc::new(LatencyHistogram::new());
    let total_ops = Arc::new(AtomicU64::new(0));
    let checkpoint_yields = Arc::new(AtomicU64::new(0));

    // Spawn pressure threads if requested
    let mut pressure_handles = Vec::new();
    if args.pressure {
        for i in 0..args.workers {
            let stop_clone = stop.clone();
            let handle = thread::Builder::new()
                .name(format!("pressure-{}", i))
                .spawn(move || {
                    let mut counter: u64 = 0;
                    while !stop_clone.load(Ordering::Relaxed) {
                        for _ in 0..100_000 {
                            counter = counter.wrapping_add(1);
                            std::hint::black_box(counter);
                        }
                    }
                })
                .unwrap();
            pressure_handles.push(handle);
        }
        tracing::info!("Started {} pressure threads", args.workers);
    }

    // Spawn latency worker threads
    let interval = Duration::from_nanos(1_000_000_000 / args.ops_per_second);
    let mut worker_handles = Vec::new();

    for i in 0..args.workers {
        let stop_clone = stop.clone();
        let hist_clone = histogram.clone();
        let ops_clone = total_ops.clone();
        let yields_clone = checkpoint_yields.clone();
        let with_checkpoints = args.with_checkpoints;

        let handle = thread::Builder::new()
            .name(format!("worker-{}", i))
            .spawn(move || {
                let mut last_op = Instant::now();

                while !stop_clone.load(Ordering::Relaxed) {
                    let start = Instant::now();

                    // Simulate work
                    let mut sum: u64 = 0;
                    for j in 0..1000u64 {
                        sum = sum.wrapping_add(j);
                    }
                    std::hint::black_box(sum);

                    // Checkpoint if enabled
                    if with_checkpoints && checkpoint_sync() {
                        yields_clone.fetch_add(1, Ordering::Relaxed);
                        thread::yield_now();
                    }

                    let elapsed = start.elapsed();
                    hist_clone.record(elapsed.as_micros() as u64);
                    ops_clone.fetch_add(1, Ordering::Relaxed);

                    // Rate limiting
                    let since_last = start.duration_since(last_op);
                    if since_last < interval {
                        thread::sleep(interval - since_last);
                    }
                    last_op = Instant::now();
                }
            })
            .unwrap();

        worker_handles.push(handle);
    }

    // Run benchmark
    tracing::info!("Running for {} seconds...", args.duration);
    thread::sleep(Duration::from_secs(args.duration));

    // Stop all threads
    stop.store(true, Ordering::Release);

    for handle in pressure_handles {
        handle.join().unwrap();
    }
    for handle in worker_handles {
        handle.join().unwrap();
    }

    // Report results
    let total = total_ops.load(Ordering::Relaxed);
    let p50 = histogram.percentile(50.0);
    let p95 = histogram.percentile(95.0);
    let p99 = histogram.percentile(99.0);
    let p999 = histogram.percentile(99.9);
    let yields = checkpoint_yields.load(Ordering::Relaxed);

    tracing::info!("\n=== Results ===");
    tracing::info!("Total operations: {}", total);
    tracing::info!("Ops/second: {:.0}", total as f64 / args.duration as f64);
    tracing::info!("Checkpoint yields: {}", yields);
    tracing::info!("");
    tracing::info!("Latency distribution:");
    tracing::info!("  p50:  {} µs", p50);
    tracing::info!("  p95:  {} µs", p95);
    tracing::info!("  p99:  {} µs", p99);
    tracing::info!("  p99.9: {} µs", p999);

    // Validation
    if p99 > 1000 {
        tracing::warn!("p99 latency > 1ms - may indicate scheduling issues");
    }

    if args.with_checkpoints && yields == 0 && args.pressure {
        tracing::warn!(
            "No checkpoint yields despite pressure - kernel hints may not be reaching userspace"
        );
    }

    tracing::info!("\nBenchmark complete");
}
