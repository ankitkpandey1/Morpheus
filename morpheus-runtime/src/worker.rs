//! Worker thread management
//!
//! Each worker thread owns one SCB and runs a local async executor.
//! Workers are registered with the kernel via the worker_tid_map.

use crate::scb::ScbHandle;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::thread::JoinHandle;

thread_local! {
    /// The SCB handle for the current worker thread
    static CURRENT_SCB: RefCell<Option<Arc<ScbHandle>>> = const { RefCell::new(None) };

    /// The worker ID for the current thread
    static WORKER_ID: RefCell<Option<u32>> = const { RefCell::new(None) };
}

/// Get the current worker's SCB handle, if running on a worker thread
#[inline]
pub fn try_current_scb() -> Option<Arc<ScbHandle>> {
    CURRENT_SCB.with(|scb| scb.borrow().clone())
}

/// Get the current worker's SCB handle
///
/// # Panics
/// Panics if not called from a Morpheus worker thread
#[inline]
pub fn current_scb() -> Arc<ScbHandle> {
    try_current_scb().expect("not running on a Morpheus worker thread")
}

/// Get the current worker ID
pub fn current_worker_id() -> Option<u32> {
    WORKER_ID.with(|id| *id.borrow())
}

/// Set the current thread's SCB (called during worker initialization)
pub(crate) fn set_current_scb(scb: Arc<ScbHandle>, worker_id: u32) {
    CURRENT_SCB.with(|current| {
        *current.borrow_mut() = Some(scb);
    });
    WORKER_ID.with(|id| {
        *id.borrow_mut() = Some(worker_id);
    });
}

/// Worker thread state
pub struct Worker {
    /// Worker ID (index into SCB map)
    pub id: u32,

    /// OS thread ID (for kernel registration)
    pub tid: u32,

    /// SCB handle
    pub scb: Arc<ScbHandle>,

    /// Thread join handle
    pub handle: Option<JoinHandle<()>>,
}

/// Worker pool configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Number of workers (default: number of CPUs)
    pub num_workers: usize,

    /// Whether workers allow forced escalation (default: true for Rust)
    pub escapable: bool,

    /// Worker thread name prefix
    pub name_prefix: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            num_workers: std::thread::available_parallelism()
                .map(NonZeroUsize::get)
                .unwrap_or(1),
            escapable: true, // Rust default
            name_prefix: "morpheus-worker".to_string(),
        }
    }
}

/// Worker pool
pub struct WorkerPool {
    workers: Vec<Worker>,
    config: WorkerConfig,
    shutdown: Arc<Mutex<bool>>,
}

impl WorkerPool {
    /// Create a new worker pool (workers not yet started)
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            workers: Vec::with_capacity(config.num_workers),
            config,
            shutdown: Arc::new(Mutex::new(false)),
        }
    }

    /// Get the number of workers
    pub fn num_workers(&self) -> usize {
        self.config.num_workers
    }

    /// Get worker configuration
    pub fn config(&self) -> &WorkerConfig {
        &self.config
    }

    /// Shutdown all workers
    pub fn shutdown(&mut self) {
        *self.shutdown.lock() = true;

        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// Check if shutdown was requested
    pub fn is_shutdown(&self) -> bool {
        *self.shutdown.lock()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Get the current OS thread ID (TID)
#[cfg(target_os = "linux")]
pub fn get_tid() -> u32 {
    unsafe { libc::syscall(libc::SYS_gettid) as u32 }
}

#[cfg(not(target_os = "linux"))]
pub fn get_tid() -> u32 {
    // Fallback for non-Linux (won't work with BPF but useful for testing)
    std::process::id()
}
