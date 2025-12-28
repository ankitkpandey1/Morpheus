// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! scx_morpheus - sched_ext BPF scheduler loader for Morpheus-Hybrid
//!
//! This binary loads the scx_morpheus BPF program into the kernel and
//! exposes the BPF maps for userspace runtime access.

mod bpf {
    include!(concat!(env!("OUT_DIR"), "/scx_morpheus.skel.rs"));
}

use anyhow::{Context, Result};
use clap::Parser;
use libbpf_rs::skel::{OpenSkel, Skel, SkelBuilder};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use bpf::*;

/// Morpheus-Hybrid sched_ext scheduler
///
/// A kernel-guided cooperative async runtime scheduler that emits yield
/// hints to userspace and only escalates to forced preemption when workers
/// have explicitly opted in and ignore hints.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Time slice in milliseconds
    #[arg(short, long, default_value_t = 5)]
    slice_ms: u64,

    /// Grace period before escalation in milliseconds
    #[arg(short, long, default_value_t = 100)]
    grace_ms: u64,

    /// Enable debug output
    #[arg(short, long)]
    debug: bool,

    /// Print stats every N seconds (0 to disable)
    #[arg(long, default_value_t = 5)]
    stats_interval: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let level = if args.debug {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("scx_morpheus starting");
    info!(
        "  slice: {}ms, grace period: {}ms",
        args.slice_ms, args.grace_ms
    );

    // Build and load BPF skeleton
    let skel_builder = ScxMorpheusSkelBuilder::default();
    let mut open_skel = skel_builder.open().context("Failed to open BPF skeleton")?;

    // Set configuration before loading
    open_skel.rodata_mut().slice_ns = args.slice_ms * 1_000_000;
    open_skel.rodata_mut().grace_period_ns = args.grace_ms * 1_000_000;
    open_skel.rodata_mut().debug_mode = args.debug;

    let mut skel = open_skel.load().context("Failed to load BPF program")?;

    // Attach the scheduler
    skel.attach().context("Failed to attach sched_ext ops")?;

    info!("scx_morpheus attached successfully");

    // Set up graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt, shutting down...");
        r.store(false, Ordering::SeqCst);
    })
    .context("Error setting Ctrl-C handler")?;

    // Main loop: print stats periodically
    let stats_interval = if args.stats_interval > 0 {
        Some(Duration::from_secs(args.stats_interval))
    } else {
        None
    };

    while running.load(Ordering::SeqCst) {
        if let Some(interval) = stats_interval {
            std::thread::sleep(interval);
            print_stats(&skel)?;
        } else {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    info!("scx_morpheus exiting");
    Ok(())
}

fn print_stats(skel: &ScxMorpheusSkel) -> Result<()> {
    // Read stats from each CPU and aggregate
    let stats_map = &skel.maps().stats_map;
    let key: u32 = 0;
    let key_bytes = key.to_ne_bytes();

    let mut total_hints = 0u64;
    let mut total_dropped = 0u64;
    let mut total_escalations = 0u64;
    let mut total_blocked = 0u64;
    let mut total_ticks = 0u64;

    // Note: In a real implementation, we'd iterate over all CPUs
    // For now, just read the first entry as a placeholder
    if let Ok(value) = stats_map.lookup(&key_bytes, libbpf_rs::MapFlags::ANY) {
        if let Some(bytes) = value {
            // Parse the stats structure from bytes
            // This is a simplified version; real code would use proper deserialization
            if bytes.len() >= 40 {
                total_hints = u64::from_ne_bytes(bytes[0..8].try_into().unwrap());
                total_dropped = u64::from_ne_bytes(bytes[8..16].try_into().unwrap());
                total_escalations = u64::from_ne_bytes(bytes[16..24].try_into().unwrap());
                total_blocked = u64::from_ne_bytes(bytes[24..32].try_into().unwrap());
                total_ticks = u64::from_ne_bytes(bytes[32..40].try_into().unwrap());
            }
        }
    }

    info!(
        "stats: ticks={} hints={} dropped={} escalations={} blocked={}",
        total_ticks, total_hints, total_dropped, total_escalations, total_blocked
    );

    Ok(())
}
