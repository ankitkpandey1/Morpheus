//! Starvation recovery benchmark
//!
//! Tests the runtime's ability to recover from a "zombie" task that
//! runs in a tight CPU loop without yielding or checking for kernel hints.
//!
//! Expected behavior:
//! - Zombie task runs until grace period (default 100ms) expires
//! - Kernel escalates via scx_bpf_kick_cpu()
//! - Well-behaved tasks maintain low latency throughout

use clap::Parser;
use morpheus_runtime::Builder;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Starvation recovery benchmark
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Number of well-behaved tasks
    #[arg(short = 'n', long, default_value_t = 4)]
    num_good_tasks: usize,

    /// Duration to run the benchmark (seconds)
    #[arg(short, long, default_value_t = 10)]
    duration: u64,

    /// Zombie task loop iterations before checking stop flag
    #[arg(long, default_value_t = 1_000_000)]
    zombie_iterations: u64,
}

fn main() {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("Starvation recovery benchmark");
    tracing::info!("  {} well-behaved tasks", args.num_good_tasks);
    tracing::info!("  {} second duration", args.duration);

    // Shared state
    let stop = Arc::new(AtomicBool::new(false));
    let zombie_cycles = Arc::new(AtomicU64::new(0));
    let good_task_yields = Arc::new(AtomicU64::new(0));
    let good_task_latencies: Arc<parking_lot::Mutex<Vec<Duration>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));

    // Build runtime
    let runtime = Builder::new()
        .num_workers(args.num_good_tasks + 1)
        .escapable(true) // Allow escalation for zombies
        .build();

    // Spawn zombie task (tight CPU loop, no checkpoint)
    let stop_clone = stop.clone();
    let cycles_clone = zombie_cycles.clone();
    let zombie_handle = thread::Builder::new()
        .name("zombie".to_string())
        .spawn(move || {
            tracing::info!("Zombie task started");

            let mut counter: u64 = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                // Tight loop - intentionally no checkpoint!
                for _ in 0..args.zombie_iterations {
                    counter = counter.wrapping_add(1);
                    std::hint::black_box(counter);
                }
                cycles_clone.fetch_add(args.zombie_iterations, Ordering::Relaxed);
            }

            tracing::info!("Zombie task stopped after {} cycles", counter);
        })
        .unwrap();

    // Spawn well-behaved tasks that track their scheduling latency
    let mut good_handles = Vec::new();
    for i in 0..args.num_good_tasks {
        let stop_clone = stop.clone();
        let yields_clone = good_task_yields.clone();
        let latencies_clone = good_task_latencies.clone();

        let handle = thread::Builder::new()
            .name(format!("good-{}", i))
            .spawn(move || {
                tracing::debug!("Good task {} started", i);

                let mut last_tick = Instant::now();

                while !stop_clone.load(Ordering::Relaxed) {
                    // Track scheduling latency
                    let now = Instant::now();
                    let latency = now.duration_since(last_tick);

                    if latency > Duration::from_millis(1) {
                        latencies_clone.lock().push(latency);
                    }

                    last_tick = now;

                    // Do some work
                    let mut sum: u64 = 0;
                    for j in 0..10_000u64 {
                        sum = sum.wrapping_add(j);
                        std::hint::black_box(sum);
                    }

                    // Check for kernel yield requests (cooperative)
                    if morpheus_runtime::checkpoint_sync() {
                        yields_clone.fetch_add(1, Ordering::Relaxed);
                        thread::yield_now();
                    }

                    // Brief sleep to simulate I/O
                    thread::sleep(Duration::from_micros(100));
                }

                tracing::debug!("Good task {} stopped", i);
            })
            .unwrap();

        good_handles.push(handle);
    }

    // Let the benchmark run
    tracing::info!("Running benchmark for {} seconds...", args.duration);
    thread::sleep(Duration::from_secs(args.duration));

    // Stop all tasks
    stop.store(true, Ordering::Release);

    // Wait for tasks to finish
    zombie_handle.join().unwrap();
    for handle in good_handles {
        handle.join().unwrap();
    }

    // Report results
    let latencies = good_task_latencies.lock();

    tracing::info!("\n=== Results ===");
    tracing::info!("Zombie cycles: {}", zombie_cycles.load(Ordering::Relaxed));
    tracing::info!(
        "Good task yields: {}",
        good_task_yields.load(Ordering::Relaxed)
    );
    tracing::info!("Latency samples: {}", latencies.len());

    if !latencies.is_empty() {
        let mut sorted: Vec<_> = latencies.iter().map(|d| d.as_micros() as u64).collect();
        sorted.sort();

        let p50 = sorted[sorted.len() / 2];
        let p95 = sorted[sorted.len() * 95 / 100];
        let p99 = sorted[sorted.len() * 99 / 100];
        let max = *sorted.last().unwrap();

        tracing::info!("Latency p50: {}µs", p50);
        tracing::info!("Latency p95: {}µs", p95);
        tracing::info!("Latency p99: {}µs", p99);
        tracing::info!("Latency max: {}µs", max);

        // Check for starvation
        if p99 > 10_000 {
            tracing::warn!("HIGH LATENCY DETECTED - possible starvation");
            std::process::exit(1);
        }
    }

    tracing::info!("Benchmark complete");
}
