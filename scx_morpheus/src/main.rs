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
use libbpf_rs::RingBufferBuilder;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use bpf::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MorpheusEscalationEvent {
    worker_id: u32,
    pid: u32,
    severity: u32,
    _pad: u32,
    timestamp: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MorpheusConfig {
    slice_ns: u64,
    grace_period_ns: u64,
}

struct PenaltyManager {
    // pid -> (original_quota, expiration_time)
    active_penalties: HashMap<u32, (String, std::time::Instant)>,
}

impl PenaltyManager {
    fn new() -> Self {
        Self {
            active_penalties: HashMap::new(),
        }
    }
}

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

    /// Enable enforcement mode (cgroup throttling and kicking)
    #[arg(long)]
    enforce: bool,
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

    if args.enforce {
        open_skel.maps.rodata_data.scheduler_mode = 1;
        info!("Enforcement mode ENABLED");
    } else {
        info!("Enforcement mode DISABLED (Observer only)");
    }

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

    // Start escalation monitor
    let penalty_manager = Arc::new(Mutex::new(PenaltyManager::new()));
    let pm_clone = penalty_manager.clone();

    // We need a separate RingBuffer for the escalation map
    // Note: libbpf-rs RingBuffer APIs usually take a callback.
    // We'll spawn a thread to poll it.
    let mut rb_builder = RingBufferBuilder::new();
    let pm_clone_rb = pm_clone.clone();
    rb_builder
        .add(&skel.maps.escalation_ringbuf, move |data: &[u8]| {
            if data.len() < std::mem::size_of::<MorpheusEscalationEvent>() {
                return 0;
            }
            let event = unsafe {
                std::ptr::read_unaligned(data.as_ptr() as *const MorpheusEscalationEvent)
            };
            handle_escalation_event(&event, &pm_clone_rb);
            0
        })
        .context("Failed to add escalation ringbuf")?;

    let ringbuf = rb_builder.build().context("Failed to build ringbuf")?;
    let run_clone = running.clone();

    std::thread::spawn(move || {
        while run_clone.load(Ordering::SeqCst) {
            // Poll with timeout
            if let Err(e) = ringbuf.poll(Duration::from_millis(100)) {
                tracing::warn!("Ringbuf poll error: {}", e);
            }
            // Check for expired penalties
            check_expired_penalties(&pm_clone);
        }
    });

    while running.load(Ordering::SeqCst) {
        if let Some(interval) = stats_interval {
            std::thread::sleep(interval);
            // Update global pressure from PSI
            if let Err(e) = update_global_pressure(&skel) {
                tracing::warn!("Failed to update pressure: {}", e);
            }
            if let Err(e) = adaptive_tune(&skel, args.slice_ms * 1_000_000, args.grace_ms * 1_000_000) {
                tracing::warn!("Failed to auto-tune: {}", e);
            }
            print_stats(&skel)?;
        } else {
            std::thread::sleep(Duration::from_secs(1));
            // Still update pressure even if stats disabled
            let _ = update_global_pressure(&skel);
            let _ = adaptive_tune(&skel, args.slice_ms * 1_000_000, args.grace_ms * 1_000_000);
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

fn adaptive_tune(skel: &ScxMorpheusSkel, default_slice_ns: u64, default_grace_ns: u64) -> Result<()> {
    let num_cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as u32;
    let runqueue_depth = read_runqueue_depth().unwrap_or(0);

    // Adaptive Slicing Logic
    let new_slice_ns = if runqueue_depth > num_cpus * 2 {
        // High load: prioritize responsiveness
        2_000_000 // 2ms
    } else if runqueue_depth < num_cpus {
        // Low load: prioritize throughput (fewer yields)
        10_000_000 // 10ms
    } else {
        // Moderate load: default
        default_slice_ns
    };

    let config = MorpheusConfig {
        slice_ns: new_slice_ns,
        grace_period_ns: default_grace_ns,
    };

    let key = 0u32.to_ne_bytes();
    let val_bytes = unsafe {
        std::slice::from_raw_parts(
            &config as *const _ as *const u8,
            std::mem::size_of::<MorpheusConfig>(),
        )
    };

    skel.maps.config_map
        .update(&key, val_bytes, libbpf_rs::MapFlags::ANY)
        .context("Failed to update config_map")?;

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
            if let Some(avg10_part) = line.split_whitespace().find(|s| s.starts_with("avg10=")) {
                let value_str = avg10_part.strip_prefix("avg10=")?;
                let value: f32 = value_str.parse().ok()?;
                return Some(value.clamp(0.0, 100.0) as u32);
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

fn handle_escalation_event(event: &MorpheusEscalationEvent, pm: &Arc<Mutex<PenaltyManager>>) {
    info!(
        "ESCALATION: Worker {} (PID {}) - Severity {}",
        event.worker_id, event.pid, event.severity
    );

    let pid = event.pid;
    let mut guard = pm.lock().unwrap();

    if guard.active_penalties.contains_key(&pid) {
        return; // Already penalized
    }

    // Find Cgroup
    if let Ok(cgroup_path) = find_cgroup_path(pid) {
        let max_file = format!("{}/cpu.max", cgroup_path);

        // Read current
        if let Ok(current) = std::fs::read_to_string(&max_file) {
            // Write penalty (1000 100000 = 1%)
            if std::fs::write(&max_file, "1000 100000").is_ok() {
                info!("  -> Throttled cgroup {} to 1%", max_file);
                // Store original value to restore later
                guard.active_penalties.insert(
                    pid,
                    (current, std::time::Instant::now() + Duration::from_secs(5)),
                );
            } else {
                tracing::error!("  -> Failed to write to {}", max_file);
            }
        }
    }
}

fn check_expired_penalties(pm: &Arc<Mutex<PenaltyManager>>) {
    let mut guard = pm.lock().unwrap();
    let now = std::time::Instant::now();
    let mut expired = Vec::new();

    for (pid, (_, expiry)) in guard.active_penalties.iter() {
        if now >= *expiry {
            expired.push(*pid);
        }
    }

    for pid in expired {
        if let Some((original, _)) = guard.active_penalties.remove(&pid) {
            if let Ok(cgroup_path) = find_cgroup_path(pid) {
                let max_file = format!("{}/cpu.max", cgroup_path);
                if std::fs::write(&max_file, &original).is_ok() {
                    info!("  -> Restored quota for PID {}", pid);
                }
            }
        }
    }
}

fn find_cgroup_path(pid: u32) -> Result<String> {
    let content = std::fs::read_to_string(format!("/proc/{}/cgroup", pid))?;
    // Format: 0::/user.slice/...
    for line in content.lines() {
        if line.starts_with("0::") {
            let path = line.strip_prefix("0::").unwrap_or("/");
            // Keep it simple: assume cgroup v2 mount at /sys/fs/cgroup
            if path == "/" {
                return Ok("/sys/fs/cgroup".to_string());
            }
            return Ok(format!("/sys/fs/cgroup{}", path));
        }
    }
    Err(anyhow::anyhow!("Cgroup not found"))
}
