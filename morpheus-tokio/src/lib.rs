// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! # Morpheus Tokio Integration
//!
//! This crate provides integration between Morpheus-Hybrid and Tokio,
//! enabling kernel-guided cooperative scheduling within Tokio async runtimes.
//!
//! ## Features
//!
//! - **Checkpoint macro**: Check for kernel yield requests in Tokio tasks
//! - **Critical sections**: Protect FFI code from kernel escalation
//! - **Yield hook**: Automatic yielding when kernel pressure is high
//!
//! ## Usage
//!
//! ```rust,no_run
//! use morpheus_tokio::{checkpoint, critical_section};
//!
//! #[tokio::main]
//! async fn main() {
//!     tokio::spawn(async {
//!         for i in 0..1_000_000 {
//!             // Check for kernel yield requests
//!             if i % 1000 == 0 {
//!                 checkpoint!();
//!             }
//!             // ... compute ...
//!         }
//!     });
//! }
//! ```

pub use morpheus_runtime::{
    critical_section, checkpoint_sync, CriticalGuard, Error, Result,
    ScbHandle, BpfMaps,
};

pub use morpheus_common::{
    HintReason, MorpheusHint, MorpheusScb, GlobalPressure,
    SchedulerMode, WorkerState, EscalationPolicy, YieldReason, RuntimeMode,
};

/// Check for pending kernel yield requests and yield to the Tokio runtime if needed.
///
/// This is the primary integration point for Morpheus with Tokio. Call this
/// periodically in CPU-intensive async code.
///
/// # Example
///
/// ```rust,no_run
/// use morpheus_tokio::checkpoint;
///
/// async fn heavy_computation() {
///     for i in 0..1_000_000 {
///         // Check every 1000 iterations
///         if i % 1000 == 0 {
///             checkpoint!();
///         }
///         // ... compute ...
///     }
/// }
/// ```
#[macro_export]
macro_rules! checkpoint {
    () => {{
        if $crate::checkpoint_sync() {
            ::tokio::task::yield_now().await;
        }
    }};
}

/// Yield to the Tokio runtime, checking for kernel pressure.
///
/// This is a more explicit version of checkpoint that always yields
/// when kernel pressure is detected.
pub async fn yield_if_requested() {
    if checkpoint_sync() {
        tokio::task::yield_now().await;
    }
}

/// Run a future with Morpheus kernel-guided scheduling.
///
/// This wrapper periodically checks for kernel yield requests.
/// Use this when you can't add explicit checkpoints.
pub async fn with_checkpoints<F, T>(future: F, check_interval: std::time::Duration) -> T
where
    F: std::future::Future<Output = T>,
{
    use std::pin::pin;
    use std::task::{Context, Poll};
    
    let mut future = pin!(future);
    let mut interval = tokio::time::interval(check_interval);
    
    std::future::poll_fn(|cx: &mut Context<'_>| {
        // Check for kernel yield
        if checkpoint_sync() {
            // Wake ourselves to yield
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        
        // Try to advance the interval
        let _ = interval.poll_tick(cx);
        
        // Poll the inner future
        future.as_mut().poll(cx)
    }).await
}

/// Builder for configuring Morpheus with Tokio.
pub struct MorpheusTokioBuilder {
    escapable: bool,
    check_interval_ms: u64,
}

impl Default for MorpheusTokioBuilder {
    fn default() -> Self {
        Self {
            escapable: true, // Rust default
            check_interval_ms: 1,
        }
    }
}

impl MorpheusTokioBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether workers allow forced escalation.
    ///
    /// Default: `true` for Rust workers.
    pub fn escapable(mut self, escapable: bool) -> Self {
        self.escapable = escapable;
        self
    }

    /// Set the check interval in milliseconds.
    ///
    /// Lower values = more responsive but higher overhead.
    pub fn check_interval_ms(mut self, ms: u64) -> Self {
        self.check_interval_ms = ms;
        self
    }

    /// Get the escapable setting.
    pub fn is_escapable(&self) -> bool {
        self.escapable
    }

    /// Get the check interval.
    pub fn get_check_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.check_interval_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_yield_if_requested() {
        // Should not panic even without kernel
        yield_if_requested().await;
    }

    #[test]
    fn test_builder() {
        let builder = MorpheusTokioBuilder::new()
            .escapable(false)
            .check_interval_ms(10);
        
        assert!(!builder.is_escapable());
        assert_eq!(builder.get_check_interval().as_millis(), 10);
    }

    #[tokio::test]
    async fn test_checkpoint_sync() {
        // Should return false when no kernel connected
        assert!(!checkpoint_sync());
    }
}
