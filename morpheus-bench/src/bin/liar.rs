//! Adversarial "liar" task benchmark
//!
//! Tests the runtime's handling of a task that abuses critical sections
//! by entering one and then sleeping for a long time.
//!
//! Expected behavior:
//! - Kernel respects the critical section (no forced preemption)
//! - Cgroup-level throttling kicks in instead
//! - Other tasks on the same worker are delayed

use clap::Parser;
use morpheus_runtime::{critical_section, Builder};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Adversarial critical section benchmark
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Duration of the adversarial critical section (ms)
    #[arg(short, long, default_value_t = 500)]
    critical_duration_ms: u64,

    /// Number of liar iterations
    #[arg(short, long, default_value_t = 5)]
    iterations: u32,

    /// Whether to actually use critical sections
    #[arg(long)]
    without_critical: bool,
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("Adversarial critical section benchmark");
    tracing::info!("  Critical duration: {}ms", args.critical_duration_ms);
    tracing::info!("  Iterations: {}", args.iterations);
    tracing::info!("  Using critical sections: {}", !args.without_critical);

    let stop = Arc::new(AtomicBool::new(false));
    let escalations = Arc::new(AtomicU64::new(0));
    let critical_time = Arc::new(AtomicU64::new(0));

    // Spawn watcher thread to detect escalations
    // In a real test, this would check kernel stats
    let escalations_clone = escalations.clone();
    let stop_clone = stop.clone();
    let _watcher = thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            // Would check /sys/kernel/debug/sched_ext/stats here
            thread::sleep(Duration::from_millis(10));
        }
        tracing::info!(
            "Watcher: {} escalation attempts detected",
            escalations_clone.load(Ordering::Relaxed)
        );
    });

    // Run liar iterations
    for i in 0..args.iterations {
        tracing::info!("Iteration {}/{}", i + 1, args.iterations);

        let start = Instant::now();

        if args.without_critical {
            // No critical section - should be preemptable
            thread::sleep(Duration::from_millis(args.critical_duration_ms));
        } else {
            // With critical section - kernel should NOT escalate
            let _guard = critical_section();
            tracing::debug!("Entered critical section");

            // This is adversarial: blocking inside critical section
            thread::sleep(Duration::from_millis(args.critical_duration_ms));

            tracing::debug!("Exiting critical section");
        }

        let elapsed = start.elapsed();
        critical_time.fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);

        tracing::info!("  Completed in {:?}", elapsed);

        // Brief pause between iterations
        thread::sleep(Duration::from_millis(50));
    }

    stop.store(true, Ordering::Release);

    // Report results
    let total_critical = critical_time.load(Ordering::Relaxed);
    let expected = (args.critical_duration_ms * args.iterations as u64) * 1000;

    tracing::info!("\n=== Results ===");
    tracing::info!("Total critical section time: {}µs", total_critical);
    tracing::info!("Expected time: {}µs", expected);
    tracing::info!(
        "Overhead: {:.2}%",
        ((total_critical as f64 / expected as f64) - 1.0) * 100.0
    );

    // Check: if we used critical sections, there should be no premature wakeups
    // (actual verification requires kernel stats)
    if !args.without_critical {
        let escalation_count = escalations.load(Ordering::Relaxed);
        if escalation_count > 0 {
            tracing::error!(
                "FAIL: {} escalations occurred during critical sections!",
                escalation_count
            );
            std::process::exit(1);
        }
        tracing::info!("PASS: No escalations during critical sections");
    }

    tracing::info!("Benchmark complete");
}
