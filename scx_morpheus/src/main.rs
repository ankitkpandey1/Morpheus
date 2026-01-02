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
use libbpf_rs::MapCore;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, Level};
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

    /// Pin BPF maps to /sys/fs/bpf/morpheus for runtime access
    #[arg(long)]
    pin_maps: bool,
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
    
    // libbpf-rs 0.24+ requires MaybeUninit for open()
    let mut open_object = MaybeUninit::uninit();
    let open_skel = skel_builder
        .open(&mut open_object)
        .context("Failed to open BPF skeleton")?;

    // Load the skeleton
    let mut skel = open_skel.load().context("Failed to load BPF program")?;

    // Attach the scheduler
    skel.attach().context("Failed to attach sched_ext ops")?;

    info!("scx_morpheus attached successfully");

    // Pin maps if requested (enables runtime access)
    if args.pin_maps {
        let pin_dir = "/sys/fs/bpf/morpheus";
        std::fs::create_dir_all(pin_dir).context("Failed to create pin directory")?;

        let tid_map_path = format!("{}/worker_tid_map", pin_dir);
        let scb_map_path = format!("{}/scb_map", pin_dir);

        skel.maps
            .worker_tid_map
            .pin(&tid_map_path)
            .context("Failed to pin worker_tid_map")?;
        skel.maps
            .scb_map
            .pin(&scb_map_path)
            .context("Failed to pin scb_map")?;

        info!("BPF maps pinned to {}", pin_dir);
        info!("  worker_tid_map: {}", tid_map_path);
        info!("  scb_map: {}", scb_map_path);
    }

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
            // Update global pressure from PSI
            if let Err(e) = update_global_pressure(&skel) {
                tracing::warn!("Failed to update pressure: {}", e);
            }
            print_stats(&skel)?;
        } else {
            std::thread::sleep(Duration::from_secs(1));
            // Still update pressure even if stats disabled
            let _ = update_global_pressure(&skel);
        }
    }

    info!("scx_morpheus exiting");
    Ok(())
}

fn print_stats(skel: &ScxMorpheusSkel) -> Result<()> {
    // Read stats from the per-CPU array
    let stats_map = &skel.maps.stats_map;
    let key: u32 = 0;
    let key_bytes = key.to_ne_bytes();

    let mut total_hints = 0u64;
    let mut total_dropped = 0u64;
    let mut total_escalations = 0u64;
    let mut total_blocked = 0u64;
    let mut total_ticks = 0u64;

    // Read and aggregate stats from the map
    if let Ok(Some(bytes)) = stats_map.lookup(&key_bytes, libbpf_rs::MapFlags::ANY) {
        // Parse the stats structure from bytes
        // struct morpheus_stats is 5 x u64 = 40 bytes
        let bytes: &[u8] = &bytes;
        if bytes.len() >= 40 {
            total_hints = u64::from_ne_bytes(bytes[0..8].try_into().unwrap_or([0u8; 8]));
            total_dropped = u64::from_ne_bytes(bytes[8..16].try_into().unwrap_or([0u8; 8]));
            total_escalations = u64::from_ne_bytes(bytes[16..24].try_into().unwrap_or([0u8; 8]));
            total_blocked = u64::from_ne_bytes(bytes[24..32].try_into().unwrap_or([0u8; 8]));
            total_ticks = u64::from_ne_bytes(bytes[32..40].try_into().unwrap_or([0u8; 8]));
        }
    }

    info!(
        "stats: ticks={} hints={} dropped={} escalations={} blocked={}",
        total_ticks, total_hints, total_dropped, total_escalations, total_blocked
    );

    Ok(())
}

/// Update global pressure from Linux PSI (Pressure Stall Information)
fn update_global_pressure(skel: &ScxMorpheusSkel) -> Result<()> {
    let cpu_pressure = read_psi_avg10("/proc/pressure/cpu").unwrap_or(0);
    let io_pressure = read_psi_avg10("/proc/pressure/io").unwrap_or(0);
    let memory_pressure = read_psi_avg10("/proc/pressure/memory").unwrap_or(0);
    
    // Calculate aggregate runqueue depth from /proc/loadavg
    let runqueue_depth = read_runqueue_depth().unwrap_or(0);

    // Pack the global pressure struct (4 x u32 = 16 bytes)
    let mut value = [0u8; 16];
    value[0..4].copy_from_slice(&cpu_pressure.to_ne_bytes());
    value[4..8].copy_from_slice(&io_pressure.to_ne_bytes());
    value[8..12].copy_from_slice(&memory_pressure.to_ne_bytes());
    value[12..16].copy_from_slice(&runqueue_depth.to_ne_bytes());

    let key = 0u32.to_ne_bytes();
    skel.maps
        .global_pressure_map
        .update(&key, &value, libbpf_rs::MapFlags::ANY)
        .context("Failed to update global_pressure_map")?;

    Ok(())
}

/// Read PSI avg10 value from /proc/pressure/* files
fn read_psi_avg10(path: &str) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    // Format: "some avg10=X.XX avg60=Y.YY avg300=Z.ZZ total=N"
    for line in content.lines() {
        if line.starts_with("some") {
            if let Some(avg10_part) = line.split_whitespace()
                .find(|s| s.starts_with("avg10="))
            {
                let value_str = avg10_part.strip_prefix("avg10=")?;
                let value: f32 = value_str.parse().ok()?;
                return Some((value.min(100.0).max(0.0)) as u32);
            }
        }
    }
    None
}

/// Read runqueue depth from /proc/loadavg
fn read_runqueue_depth() -> Option<u32> {
    let content = std::fs::read_to_string("/proc/loadavg").ok()?;
    // Format: "0.00 0.00 0.00 1/234 5678"
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() >= 4 {
        // Fourth field is running/total like "1/234"
        let rq_parts: Vec<&str> = parts[3].split('/').collect();
        if let Some(running) = rq_parts.first() {
            return running.parse().ok();
        }
    }
    None
}

