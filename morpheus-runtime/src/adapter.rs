// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! # Language Adapter Layer (Delta #7)
//!
//! This module defines the minimal API that language runtimes must implement
//! to integrate with Morpheus-Hybrid. The kernel is oblivious to this layer.
//!
//! ## Purpose
//!
//! Prevent Rust assumptions from leaking into Python and vice versa by
//! defining a minimal, language-neutral interface.
//!
//! ## Adapter Implementations
//!
//! - `RustAdapter`: Default implementation for Rust async runtimes
//! - Python adapter: Implemented in `morpheus-py` crate

use crate::critical::CriticalGuard;
use crate::worker;

/// Language Adapter trait
///
/// Each language runtime implements this trait to provide Morpheus integration
/// while maintaining language-specific semantics.
pub trait LanguageAdapter {
    /// Enter a safe point where yielding is permitted.
    ///
    /// This is the most permissive yield point. The runtime may yield
    /// immediately if a kernel hint is pending.
    fn enter_safe_point(&self);

    /// Enter a checkpoint.
    ///
    /// Returns `true` if a yield was performed, `false` otherwise.
    /// Checkpoints are explicit yield opportunities in CPU-heavy code.
    fn enter_checkpoint(&self) -> bool;

    /// Enter a critical section.
    ///
    /// Returns a guard that must be held while in the critical section.
    /// The kernel will not force-preempt while this guard is held.
    fn enter_critical(&self) -> CriticalGuard;

    /// Voluntarily yield the worker thread.
    ///
    /// This is an explicit yield request, not a checkpoint. Use sparingly.
    fn yield_worker(&self);

    /// Get the default escapability for this language.
    ///
    /// - Rust: `true` (safe to preempt outside critical sections)
    /// - Python: `false` (GIL safety requires cooperative scheduling)
    fn default_escapable(&self) -> bool;
}

/// Rust Language Adapter
///
/// Default implementation for Rust async runtimes.
/// Rust workers are escapable by default (safe to preempt).
#[derive(Debug, Default)]
pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn enter_safe_point(&self) {
        // Check for kernel hints and yield if requested
        crate::checkpoint_sync();
    }

    fn enter_checkpoint(&self) -> bool {
        crate::checkpoint_sync()
    }

    fn enter_critical(&self) -> CriticalGuard {
        crate::critical::critical_section()
    }

    fn yield_worker(&self) {
        // For Rust, we acknowledge any pending hints
        if let Some(scb) = worker::try_current_scb() {
            scb.acknowledge();
        }
        std::thread::yield_now();
    }

    fn default_escapable(&self) -> bool {
        true // Rust workers are safe to preempt
    }
}

/// Get the default adapter for Rust
pub fn rust_adapter() -> RustAdapter {
    RustAdapter
}
