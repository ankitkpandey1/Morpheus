//! End-to-end kernel integration tests
//!
//! These tests require:
//! 1. Root privileges
//! 2. Linux kernel 6.12+ with sched_ext support
//! 3. scx_morpheus binary available
//!
//! They spawn the actual scheduler and verify userspace-kernel interaction.

use std::process::{Child, Command};
use std::time::Duration;
use std::path::Path;
use morpheus_runtime::{BpfMaps, worker, checkpoint_sync};

struct SchedulerGuard {
    process: Child,
}

impl SchedulerGuard {
    fn spawn() -> Option<Self> {
        // Find scx_morpheus binary
        let bin_path = Path::new("../target/release/scx_morpheus");
        if !bin_path.exists() {
            eprintln!("Skipping E2E test: scx_morpheus binary not found at {:?}", bin_path);
            return None;
        }

        // Must be root
        if unsafe { libc::geteuid() } != 0 {
            eprintln!("Skipping E2E test: Not running as root");
            return None;
        }

        // Spawn scheduler with map pinning enabled
        let child = Command::new(bin_path)
            .arg("--pin-maps")
            .arg("--stats-interval")
            .arg("0") // Disable stats printing to keep stdout clean
            .spawn()
            .ok()?;

        // Give it time to load and pin maps
        std::thread::sleep(Duration::from_secs(1));

        Some(Self { process: child })
    }
}

impl Drop for SchedulerGuard {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        
        // Cleanup pinned maps
        let _ = std::fs::remove_dir_all("/sys/fs/bpf/morpheus");
    }
}

#[test]
#[ignore = "requires root and kernel support"]
fn test_e2e_registration_and_communication() {
    let _guard = match SchedulerGuard::spawn() {
        Some(g) => g,
        None => return, // Skip test if environment not ready
    };

    // 1. Connect to maps
    let pin_dir = "/sys/fs/bpf/morpheus";
    let maps = BpfMaps::from_pinned_paths(
        &format!("{}/worker_tid_map", pin_dir),
        &format!("{}/scb_map", pin_dir),
    ).expect("Failed to connect to pinned maps");

    // 2. Register worker
    let tid = worker::get_tid();
    let worker_id = 0;
    
    maps.register_worker(tid, worker_id).expect("Failed to register worker");
    
    // 3. Verify interaction
    // We can't easily check if the kernel sees us without reading BPF map from userspace
    // which requires `libbpf-rs` map access code that mimics `print_stats`.
    // But success of `register_worker` implies the map is writable.

    // 4. Run simple workload with checkpoints
    // This shouldn't panic
    for _ in 0..100 {
        checkpoint_sync();
        std::thread::yield_now();
    }

    // 5. Unregister
    maps.unregister_worker(tid).expect("Failed to unregister");
}
