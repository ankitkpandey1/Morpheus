//! # Morpheus Runtime
//!
//! A kernel-guided cooperative async runtime with opt-in escalation.
//!
//! Morpheus-Hybrid enables async runtimes (Rust, Python) to receive yield hints
//! from the kernel scheduler and respond at safe points. The kernel only forces
//! preemption on workers that have explicitly opted in and ignored hints.
//!
//! ## Key Components
//!
//! - **SCB (Shared Control Block)**: Per-worker state shared with kernel
//! - **Checkpoint**: Check for pending yield requests
//! - **Critical Section**: Protect FFI/invariant-sensitive code from interruption
//! - **Worker Pool**: Manages worker threads and their SCBs
//!
//! ## Usage
//!
//! ```rust,no_run
//! use morpheus_runtime::{Runtime, checkpoint};
//!
//! async fn my_task() {
//!     loop {
//!         // Do some CPU work...
//!         
//!         // Check for kernel yield requests
//!         checkpoint!();
//!         
//!         // More work...
//!     }
//! }
//! ```
//!
//! ## Critical Sections
//!
//! ```rust,no_run
//! use morpheus_runtime::critical_section;
//!
//! async fn ffi_work() {
//!     // Kernel will not escalate inside this block
//!     let _guard = critical_section();
//!     unsafe {
//!         // FFI calls, zero-copy operations
//!     }
//! } // Guard dropped, kernel can escalate again
//! ```

pub mod critical;
pub mod error;
pub mod executor;
pub mod ringbuf;
pub mod runtime;
pub mod scb;
pub mod worker;

pub use critical::{critical_section, CriticalGuard};
pub use error::{Error, Result};
pub use runtime::{Builder, Runtime};
pub use scb::ScbHandle;

/// Re-export common types
pub use morpheus_common::{HintReason, MorpheusHint, MorpheusScb};

/// Check for pending kernel yield requests and yield if needed.
///
/// This macro should be called at regular intervals in CPU-intensive code.
/// It is zero-cost when no yield is requested (just an atomic load and compare).
///
/// # Example
///
/// ```rust,no_run
/// use morpheus_runtime::checkpoint;
///
/// async fn heavy_computation() {
///     for i in 0..1_000_000 {
///         // ... compute ...
///         
///         // Check every 1000 iterations
///         if i % 1000 == 0 {
///             checkpoint!();
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! checkpoint {
    () => {{
        if let Some(scb_handle) = $crate::worker::try_current_scb() {
            if scb_handle.yield_requested() {
                $crate::executor::yield_now().await;
            }
        }
    }};
}

/// Synchronous checkpoint for use in non-async contexts.
///
/// Returns `true` if a yield was requested, allowing the caller to decide
/// how to respond.
#[inline]
pub fn checkpoint_sync() -> bool {
    if let Some(scb_handle) = worker::try_current_scb() {
        scb_handle.yield_requested()
    } else {
        false
    }
}
