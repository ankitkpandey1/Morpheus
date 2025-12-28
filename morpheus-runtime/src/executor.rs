//! Async executor for Morpheus workers
//!
//! Each worker runs a local async executor. The executor checks for
//! kernel yield requests at poll boundaries.

use crate::critical::in_critical_section;
use crate::ringbuf::DefensiveMode;
use crate::worker;
use async_task::{Runnable, Task};
use crossbeam::deque::{Injector, Stealer, Worker as WorkQueue};
use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

/// Executor statistics
#[derive(Debug, Default)]
pub struct ExecutorStats {
    /// Total tasks spawned
    pub tasks_spawned: AtomicU64,
    /// Total tasks completed
    pub tasks_completed: AtomicU64,
    /// Total yields due to kernel pressure
    pub kernel_yields: AtomicU64,
    /// Total yields in defensive mode
    pub defensive_yields: AtomicU64,
    /// Total polls
    pub polls: AtomicU64,
}

/// Local executor for a single worker thread
pub struct LocalExecutor {
    /// Local task queue
    queue: WorkQueue<Runnable>,
    /// Global injector for cross-thread spawns
    injector: Arc<Injector<Runnable>>,
    /// Stealers from other workers (for work stealing)
    stealers: Vec<Stealer<Runnable>>,
    /// Defensive mode controller
    defensive: Arc<DefensiveMode>,
    /// Statistics
    stats: Arc<ExecutorStats>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
}

impl LocalExecutor {
    /// Create a new local executor
    pub fn new(
        injector: Arc<Injector<Runnable>>,
        stealers: Vec<Stealer<Runnable>>,
        defensive: Arc<DefensiveMode>,
    ) -> Self {
        Self {
            queue: WorkQueue::new_fifo(),
            injector,
            stealers,
            defensive,
            stats: Arc::new(ExecutorStats::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawn a task on this executor
    pub fn spawn<F>(&self, future: F) -> Task<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let _stats = self.stats.clone();
        let schedule = move |runnable: Runnable| {
            // Schedule to thread-local queue if on worker, else to injector
            // For simplicity, always push to the local queue
            // In a real implementation, this would check the current thread
            runnable.run();
        };

        let (runnable, task) = async_task::spawn(future, schedule);
        self.queue.push(runnable);
        self.stats.tasks_spawned.fetch_add(1, Ordering::Relaxed);
        task
    }

    /// Run the executor until shutdown
    pub fn run(&self) {
        while !self.shutdown.load(Ordering::Relaxed) {
            self.tick();
        }
    }

    /// Execute one tick of the executor
    pub fn tick(&self) -> bool {
        // Try to get a task from local queue
        if let Some(runnable) = self.queue.pop() {
            self.run_task(runnable);
            return true;
        }

        // Try to steal from global injector
        if let Some(runnable) = self.injector.steal().success() {
            self.run_task(runnable);
            return true;
        }

        // Try to steal from other workers
        for stealer in &self.stealers {
            if let Some(runnable) = stealer.steal().success() {
                self.run_task(runnable);
                return true;
            }
        }

        false
    }

    /// Run a single task, checking for yield requests
    fn run_task(&self, runnable: Runnable) {
        self.stats.polls.fetch_add(1, Ordering::Relaxed);

        // Check for kernel yield before polling
        if self.should_yield() {
            // Re-queue the task and yield
            self.queue.push(runnable);
            self.acknowledge_yield();
            return;
        }

        // Run the task
        runnable.run();
    }

    /// Check if we should yield before running a task
    fn should_yield(&self) -> bool {
        // Never yield inside critical sections
        if in_critical_section() {
            return false;
        }

        // Check defensive mode
        if self.defensive.should_yield() {
            self.stats.defensive_yields.fetch_add(1, Ordering::Relaxed);
            return true;
        }

        // Check kernel yield request
        if let Some(scb) = worker::try_current_scb() {
            if scb.yield_requested() {
                self.stats.kernel_yields.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }

        false
    }

    /// Acknowledge a yield to the kernel
    fn acknowledge_yield(&self) {
        if let Some(scb) = worker::try_current_scb() {
            scb.acknowledge();
        }

        // Brief yield to allow other threads to run
        std::thread::yield_now();
    }

    /// Request shutdown
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    /// Get executor statistics
    pub fn stats(&self) -> &Arc<ExecutorStats> {
        &self.stats
    }
}

/// Yield the current task, allowing other tasks to run
///
/// This is called from the `checkpoint!` macro when a kernel yield
/// request is detected.
pub async fn yield_now() {
    YieldNow { yielded: false }.await
}

/// Future that yields once
struct YieldNow {
    yielded: bool,
}

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

thread_local! {
    /// The current executor for this thread
    static CURRENT_EXECUTOR: RefCell<Option<Arc<LocalExecutor>>> = const { RefCell::new(None) };
}

/// Set the current executor for this thread
pub(crate) fn set_current_executor(executor: Arc<LocalExecutor>) {
    CURRENT_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(executor);
    });
}

/// Get the current executor
pub fn current_executor() -> Option<Arc<LocalExecutor>> {
    CURRENT_EXECUTOR.with(|e| e.borrow().clone())
}
