//! Morpheus runtime builder and main entry point
//!
//! The Runtime coordinates workers, SCBs, and executors.

use crate::ringbuf::{DefensiveMode, HintConsumer};
use crate::worker::{WorkerConfig, WorkerPool};
use crossbeam::deque::Injector;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Worker configuration
    pub workers: WorkerConfig,

    /// Defensive mode yield count
    pub defensive_yields: u64,

    /// Ring buffer poll timeout
    pub poll_timeout: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            workers: WorkerConfig::default(),
            defensive_yields: 100,
            poll_timeout: Duration::from_millis(1),
        }
    }
}

/// Morpheus runtime builder
pub struct Builder {
    config: RuntimeConfig,
}

impl Builder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: RuntimeConfig::default(),
        }
    }

    /// Set the number of worker threads
    pub fn num_workers(mut self, n: usize) -> Self {
        self.config.workers.num_workers = n;
        self
    }

    /// Set whether workers allow forced escalation
    ///
    /// - `true`: Kernel can force-preempt unresponsive workers (Rust default)
    /// - `false`: Kernel will never force-preempt (Python default, GIL safety)
    pub fn escapable(mut self, escapable: bool) -> Self {
        self.config.workers.escapable = escapable;
        self
    }

    /// Set defensive mode yield count
    pub fn defensive_yields(mut self, count: u64) -> Self {
        self.config.defensive_yields = count;
        self
    }

    /// Set ring buffer poll timeout
    pub fn poll_timeout(mut self, timeout: Duration) -> Self {
        self.config.poll_timeout = timeout;
        self
    }

    /// Build the runtime
    ///
    /// Note: This does not connect to the kernel scheduler. Call
    /// `runtime.connect()` with the BPF map file descriptors to enable
    /// kernel communication.
    pub fn build(self) -> Runtime {
        Runtime::new(self.config)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// Morpheus runtime
///
/// Manages worker threads, SCBs, and async executors.
pub struct Runtime {
    /// Configuration
    config: RuntimeConfig,

    /// Worker pool
    workers: RwLock<Option<WorkerPool>>,

    /// Global task injector
    injector: Arc<Injector<async_task::Runnable>>,

    /// Defensive mode controller
    defensive: Arc<DefensiveMode>,

    /// Hint consumer
    hints: Arc<HintConsumer>,

    /// Running flag
    running: AtomicBool,
}

impl Runtime {
    /// Create a new runtime with the given configuration
    fn new(config: RuntimeConfig) -> Self {
        Self {
            defensive: Arc::new(DefensiveMode::new(config.defensive_yields)),
            config,
            workers: RwLock::new(None),
            injector: Arc::new(Injector::new()),
            hints: Arc::new(HintConsumer::new()),
            running: AtomicBool::new(false),
        }
    }

    /// Get the runtime configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Check if the runtime is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get the hint consumer
    pub fn hints(&self) -> &Arc<HintConsumer> {
        &self.hints
    }

    /// Get the defensive mode controller
    pub fn defensive(&self) -> &Arc<DefensiveMode> {
        &self.defensive
    }

    /// Shutdown the runtime
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Release);

        if let Some(ref mut pool) = *self.workers.write() {
            pool.shutdown();
        }

        info!("Morpheus runtime shutdown complete");
    }

    /// Block the current thread on a future
    ///
    /// This is a simple blocking executor for use without the full
    /// worker pool infrastructure.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        futures_lite::future::block_on(future)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Global runtime instance
static RUNTIME: RwLock<Option<Arc<Runtime>>> = RwLock::new(None);

/// Initialize the global runtime
pub fn init(config: RuntimeConfig) -> Arc<Runtime> {
    let runtime = Arc::new(Runtime::new(config));
    *RUNTIME.write() = Some(runtime.clone());
    runtime
}

/// Get the global runtime
pub fn runtime() -> Option<Arc<Runtime>> {
    RUNTIME.read().clone()
}

/// Shutdown the global runtime
pub fn shutdown() {
    if let Some(rt) = RUNTIME.write().take() {
        rt.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        let runtime = Builder::new()
            .num_workers(4)
            .escapable(false)
            .defensive_yields(50)
            .build();

        assert_eq!(runtime.config().workers.num_workers, 4);
        assert!(!runtime.config().workers.escapable);
        assert_eq!(runtime.config().defensive_yields, 50);
    }
}
