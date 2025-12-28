// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! # morpheus-common
//!
//! Shared types for Morpheus-Hybrid kernel↔runtime communication.
//!
//! This crate defines the Rust equivalents of the C structures in `morpheus_shared.h`.
//! All types are `#[repr(C)]` to ensure binary compatibility with the BPF program.
//!
//! ## Design Principles
//!
//! - **Language-neutral**: Operates at worker-thread level, not async task level
//! - **No pointers cross boundary**: SCB contains only integers
//! - **Cache-aligned**: SCB is 128 bytes (2 cache lines) for optimal performance

#![no_std]

use core::sync::atomic::{AtomicU32, AtomicU64};

/// Shared Control Block (SCB) - One per worker thread
///
/// This is the primary communication structure between the kernel scheduler
/// and userspace runtimes. Each worker thread owns exactly one SCB.
///
/// # Memory Layout
///
/// The SCB is split into two 64-byte cache lines:
/// - **Line 1 (bytes 0-63)**: Kernel → Runtime fields (read by runtime)
/// - **Line 2 (bytes 64-127)**: Runtime → Kernel fields (written by runtime)
///
/// # Thread Safety
///
/// All fields are accessed atomically. The kernel and runtime may access
/// the SCB concurrently without locks.
#[repr(C, align(64))]
#[derive(Debug)]
pub struct MorpheusScb {
    // === Cache Line 1: Kernel → Runtime ===
    /// Monotonically increasing sequence number.
    /// Kernel increments when yield is requested.
    pub preempt_seq: AtomicU64,

    /// Remaining time budget in nanoseconds (advisory).
    pub budget_remaining_ns: AtomicU64,

    /// System pressure level (0-100).
    pub kernel_pressure_level: AtomicU32,

    _pad0: u32,
    _reserved0: [u64; 4],

    // === Cache Line 2: Runtime → Kernel ===
    /// 1 if in critical section (FFI, GIL-held, etc.).
    pub is_in_critical_section: AtomicU32,

    /// 1 if worker opted in to forced escalation.
    /// Default: 0 for Python, 1 for Rust.
    pub escapable: AtomicU32,

    /// Last acknowledged preempt_seq.
    pub last_ack_seq: AtomicU64,

    /// Advisory priority (0-1000).
    pub runtime_priority: AtomicU32,

    _pad1: u32,
    _reserved1: [u64; 3],
}

// Compile-time size assertion
const _: () = assert!(
    core::mem::size_of::<MorpheusScb>() == 128,
    "MorpheusScb must be exactly 128 bytes"
);

impl MorpheusScb {
    /// Create a new SCB with default values.
    ///
    /// # Arguments
    /// * `escapable` - Whether this worker allows forced escalation.
    ///   - Rust workers: typically `true`
    ///   - Python workers: typically `false` (GIL safety)
    pub const fn new(escapable: bool) -> Self {
        Self {
            preempt_seq: AtomicU64::new(0),
            budget_remaining_ns: AtomicU64::new(0),
            kernel_pressure_level: AtomicU32::new(0),
            _pad0: 0,
            _reserved0: [0; 4],
            is_in_critical_section: AtomicU32::new(0),
            escapable: AtomicU32::new(if escapable { 1 } else { 0 }),
            last_ack_seq: AtomicU64::new(0),
            runtime_priority: AtomicU32::new(500), // Default mid-priority
            _pad1: 0,
            _reserved1: [0; 3],
        }
    }
}

impl Default for MorpheusScb {
    fn default() -> Self {
        Self::new(true) // Rust default: escapable
    }
}

/// Hint reasons - why the kernel is requesting a yield
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintReason {
    /// Worker exceeded time slice
    Budget = 1,
    /// System under CPU pressure
    Pressure = 2,
    /// Runqueue imbalance detected
    Imbalance = 3,
    /// Hard deadline approaching
    Deadline = 4,
}

impl TryFrom<u32> for HintReason {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(HintReason::Budget),
            2 => Ok(HintReason::Pressure),
            3 => Ok(HintReason::Imbalance),
            4 => Ok(HintReason::Deadline),
            _ => Err(()),
        }
    }
}

/// Hint message - sent via ring buffer (edge-triggered events)
///
/// Hints are advisory. A well-behaved runtime should respond by yielding
/// at the next safe point.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MorpheusHint {
    /// Matches the preempt_seq that triggered this hint
    pub seq: u64,

    /// One of HintReason variants
    pub reason: u32,

    /// Thread ID of the target worker
    pub target_tid: u32,

    /// Deadline in nanoseconds (monotonic clock)
    pub deadline_ns: u64,
}

/// Configuration constants
pub mod config {
    /// Maximum number of workers supported
    pub const MAX_WORKERS: u32 = 1024;

    /// Default time slice in nanoseconds (5ms)
    pub const DEFAULT_SLICE_NS: u64 = 5 * 1_000_000;

    /// Grace period before escalation in nanoseconds (100ms)
    pub const GRACE_PERIOD_NS: u64 = 100 * 1_000_000;

    /// Ring buffer size in bytes (256KB)
    pub const RINGBUF_SIZE: u32 = 256 * 1024;
}

/// Map names for BPF object lookup
pub mod map_names {
    pub const SCB_MAP: &str = "scb_map";
    pub const HINT_RINGBUF: &str = "hint_ringbuf";
    pub const WORKER_TID_MAP: &str = "worker_tid_map";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scb_size_and_alignment() {
        assert_eq!(core::mem::size_of::<MorpheusScb>(), 128);
        assert_eq!(core::mem::align_of::<MorpheusScb>(), 64);
    }

    #[test]
    fn test_hint_reason_conversion() {
        assert_eq!(HintReason::try_from(1), Ok(HintReason::Budget));
        assert_eq!(HintReason::try_from(5), Err(()));
    }
}
