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
//!
//! ## Architectural Guardrails (Non-Goals)
//!
//! The following features are **explicitly out of scope**:
//!
//! 1. **Per-task kernel scheduling** - Kernel operates on worker threads only
//! 2. **Bytecode-level preemption** - Safe points are language-runtime controlled  
//! 3. **Kernel-managed budgets** - Budgets are advisory, not enforced by kernel

#![no_std]

use core::sync::atomic::{AtomicU32, AtomicU64};

// ============================================================================
// Scheduler Mode (Delta #1: Observer vs Enforcer)
// ============================================================================

/// Scheduler operating mode
///
/// Determines whether the scheduler only observes or actively enforces.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchedulerMode {
    /// Observer only: collect runtime data, emit hints, no enforcement.
    /// This is the safest mode and the default.
    #[default]
    ObserverOnly = 0,

    /// Enforced: full escalation + CPU kicks enabled.
    /// Requires explicit operator opt-in.
    Enforced = 1,
}

// ============================================================================
// Worker Lifecycle State Machine (Delta #2)
// ============================================================================

/// Worker thread lifecycle state
///
/// State transitions:
/// ```text
/// INIT → REGISTERED → RUNNING → QUIESCING → DEAD
///                       ↑          ↓
///                       └──────────┘ (recovery)
/// ```
///
/// Rules:
/// - Kernel emits hints only when state == RUNNING
/// - Escalation forbidden in INIT or QUIESCING
/// - Cleanup triggered only from DEAD
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkerState {
    /// Worker allocated but not yet registered with kernel
    #[default]
    Init = 0,

    /// Worker registered with kernel, TID known
    Registered = 1,

    /// Worker actively executing tasks
    Running = 2,

    /// Worker shutting down, no new tasks accepted
    Quiescing = 3,

    /// Worker terminated, ready for cleanup
    Dead = 4,
}

impl WorkerState {
    /// Check if hints can be emitted to this worker
    #[inline]
    pub fn can_receive_hints(self) -> bool {
        matches!(self, WorkerState::Running)
    }

    /// Check if escalation is allowed for this worker
    #[inline]
    pub fn can_escalate(self) -> bool {
        matches!(self, WorkerState::Running)
    }
}

impl TryFrom<u32> for WorkerState {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WorkerState::Init),
            1 => Ok(WorkerState::Registered),
            2 => Ok(WorkerState::Running),
            3 => Ok(WorkerState::Quiescing),
            4 => Ok(WorkerState::Dead),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Escalation Policy (Delta #3: Pluggable Policies)
// ============================================================================

/// Escalation policy for unresponsive workers
///
/// Determines what action (if any) the kernel takes when a worker
/// ignores yield hints past the grace period.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EscalationPolicy {
    /// No enforcement - hints only
    #[default]
    None = 0,

    /// Kick the CPU to force reschedule
    ThreadKick = 1,

    /// Apply cgroup throttling
    CgroupThrottle = 2,

    /// Combine kick + throttle (most aggressive)
    Hybrid = 3,
}

impl TryFrom<u32> for EscalationPolicy {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EscalationPolicy::None),
            1 => Ok(EscalationPolicy::ThreadKick),
            2 => Ok(EscalationPolicy::CgroupThrottle),
            3 => Ok(EscalationPolicy::Hybrid),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Yield Cause Ledger (Delta #5)
// ============================================================================

/// Reason for the last yield
///
/// Used for observability and tuning. Kernel can observe coarse categories.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YieldReason {
    /// No yield yet or unknown
    #[default]
    None = 0,

    /// Yielded in response to kernel hint
    Hint = 1,

    /// Yielded at explicit checkpoint
    Checkpoint = 2,

    /// Yielded due to budget exhaustion
    Budget = 3,

    /// Defensive yield (e.g., heuristic triggered)
    Defensive = 4,

    /// Recovery after escalation
    EscalationRecovery = 5,
}

impl TryFrom<u32> for YieldReason {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(YieldReason::None),
            1 => Ok(YieldReason::Hint),
            2 => Ok(YieldReason::Checkpoint),
            3 => Ok(YieldReason::Budget),
            4 => Ok(YieldReason::Defensive),
            5 => Ok(YieldReason::EscalationRecovery),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Runtime Determinism Mode (Delta #6)
// ============================================================================

/// Runtime scheduling mode
///
/// Encodes the determinism state as a proper state machine.
///
/// Transitions:
/// - DETERMINISTIC → PRESSURED: Any kernel hint received
/// - PRESSURED → DEFENSIVE: Hint loss detected
/// - DEFENSIVE → PRESSURED: Successful hint exchange
/// - * → DETERMINISTIC: No hints for sustained period
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeMode {
    /// No kernel hints active - fully deterministic behavior
    #[default]
    Deterministic = 0,

    /// Kernel hints being received - cooperative scheduling active
    Pressured = 1,

    /// Hint loss or errors detected - defensive yielding enabled
    Defensive = 2,
}

impl RuntimeMode {
    /// Whether the runtime should yield more eagerly
    #[inline]
    pub fn should_yield_eagerly(self) -> bool {
        matches!(self, RuntimeMode::Defensive)
    }
}

// ============================================================================
// Hint Reasons (existing, enhanced)
// ============================================================================

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

// ============================================================================
// Shared Control Block (SCB) - Extended
// ============================================================================

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

    /// Worker lifecycle state (WorkerState enum)
    pub worker_state: AtomicU32,

    /// Count of hints that were dropped/lost (ring buffer overflow)
    pub hint_loss_count: AtomicU32,

    /// Timestamp (ns) of last escalation event against this worker
    pub last_escalation_ns: AtomicU64,

    /// Count of ring buffer overflow events
    pub ringbuf_overflow_count: AtomicU32,

    /// Reserved for future use
    _reserved0: [u32; 5],

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

    /// Last yield reason (YieldReason enum) - for observability
    pub last_yield_reason: AtomicU32,

    /// Reservation token for future reservation protocol
    pub reservation_token: AtomicU64,

    /// Escalation policy for this worker
    pub escalation_policy: AtomicU32,

    _pad: u32,
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
            worker_state: AtomicU32::new(WorkerState::Init as u32),
            hint_loss_count: AtomicU32::new(0),
            last_escalation_ns: AtomicU64::new(0),
            ringbuf_overflow_count: AtomicU32::new(0),
            _reserved0: [0; 5],
            is_in_critical_section: AtomicU32::new(0),
            escapable: AtomicU32::new(if escapable { 1 } else { 0 }),
            last_ack_seq: AtomicU64::new(0),
            runtime_priority: AtomicU32::new(500), // Default mid-priority
            last_yield_reason: AtomicU32::new(YieldReason::None as u32),
            reservation_token: AtomicU64::new(0),
            escalation_policy: AtomicU32::new(EscalationPolicy::None as u32),
            _pad: 0,
        }
    }
}

impl Default for MorpheusScb {
    fn default() -> Self {
        Self::new(true) // Rust default: escapable
    }
}

// ============================================================================
// Global Pressure (Delta #4)
// ============================================================================

/// Global system pressure indicators
///
/// This structure provides system-wide pressure signals that runtimes
/// can use to voluntarily yield more eagerly.
///
/// Key rule: Global pressure can only **increase** yield eagerness, never force.
#[repr(C)]
#[derive(Debug, Default)]
pub struct GlobalPressure {
    /// CPU pressure percentage (0-100, PSI-derived)
    pub cpu_pressure_pct: AtomicU32,

    /// I/O pressure percentage (0-100, PSI-derived)
    pub io_pressure_pct: AtomicU32,

    /// Memory pressure percentage (0-100, PSI-derived)
    pub memory_pressure_pct: AtomicU32,

    /// Current runqueue depth (aggregate across CPUs)
    pub runqueue_depth: AtomicU32,
}

impl GlobalPressure {
    /// Create a new GlobalPressure with zero values
    pub const fn new() -> Self {
        Self {
            cpu_pressure_pct: AtomicU32::new(0),
            io_pressure_pct: AtomicU32::new(0),
            memory_pressure_pct: AtomicU32::new(0),
            runqueue_depth: AtomicU32::new(0),
        }
    }

    /// Check if system is under significant pressure
    #[inline]
    pub fn is_pressured(&self) -> bool {
        use core::sync::atomic::Ordering::Relaxed;
        self.cpu_pressure_pct.load(Relaxed) > 50
            || self.io_pressure_pct.load(Relaxed) > 50
            || self.memory_pressure_pct.load(Relaxed) > 50
    }
}

// ============================================================================
// Hint message (existing)
// ============================================================================

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

// ============================================================================
// Configuration constants
// ============================================================================

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
    pub const GLOBAL_PRESSURE_MAP: &str = "global_pressure_map";
    pub const CONFIG_MAP: &str = "config_map";
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn test_scb_size_and_alignment() {
        assert_eq!(size_of::<MorpheusScb>(), 128, "SCB must be 128 bytes");
        assert_eq!(align_of::<MorpheusScb>(), 64, "SCB must be 64-byte aligned");
    }

    #[test]
    fn test_scb_cache_line_offsets() {
        // Cache Line 1: Kernel -> Runtime (bytes 0-63)
        assert_eq!(offset_of!(MorpheusScb, preempt_seq), 0);
        assert_eq!(offset_of!(MorpheusScb, budget_remaining_ns), 8);
        assert_eq!(offset_of!(MorpheusScb, kernel_pressure_level), 16);
        assert_eq!(offset_of!(MorpheusScb, worker_state), 20);

        // Cache Line 2: Runtime -> Kernel (bytes 64-127)
        assert_eq!(
            offset_of!(MorpheusScb, is_in_critical_section),
            64,
            "Cache line 2 must start at offset 64"
        );
        assert_eq!(offset_of!(MorpheusScb, escapable), 68);
        assert_eq!(offset_of!(MorpheusScb, last_ack_seq), 72);
        assert_eq!(offset_of!(MorpheusScb, runtime_priority), 80);
        assert_eq!(offset_of!(MorpheusScb, last_yield_reason), 84);
        assert_eq!(offset_of!(MorpheusScb, escalation_policy), 96);
    }

    #[test]
    fn test_hint_structure() {
        assert_eq!(size_of::<MorpheusHint>(), 24);
    }

    #[test]
    fn test_global_pressure_structure() {
        assert_eq!(size_of::<GlobalPressure>(), 16);
    }

    #[test]
    fn test_hint_reason_conversion() {
        assert_eq!(HintReason::try_from(1), Ok(HintReason::Budget));
        assert_eq!(HintReason::try_from(2), Ok(HintReason::Pressure));
        assert_eq!(HintReason::try_from(3), Ok(HintReason::Imbalance));
        assert_eq!(HintReason::try_from(4), Ok(HintReason::Deadline));
        assert_eq!(HintReason::try_from(5), Err(()));
        assert_eq!(HintReason::try_from(0), Err(()));
    }

    #[test]
    fn test_worker_state_transitions() {
        assert!(WorkerState::Running.can_receive_hints());
        assert!(!WorkerState::Init.can_receive_hints());
        assert!(!WorkerState::Registered.can_receive_hints());
        assert!(!WorkerState::Quiescing.can_receive_hints());
        assert!(!WorkerState::Dead.can_receive_hints());

        assert!(WorkerState::Running.can_escalate());
        assert!(!WorkerState::Init.can_escalate());
        assert!(!WorkerState::Quiescing.can_escalate());
    }

    #[test]
    fn test_runtime_mode() {
        assert!(!RuntimeMode::Deterministic.should_yield_eagerly());
        assert!(!RuntimeMode::Pressured.should_yield_eagerly());
        assert!(RuntimeMode::Defensive.should_yield_eagerly());
    }

    #[test]
    fn test_scheduler_mode_defaults() {
        assert_eq!(SchedulerMode::default(), SchedulerMode::ObserverOnly);
    }

    #[test]
    fn test_escalation_policy_defaults() {
        assert_eq!(EscalationPolicy::default(), EscalationPolicy::None);
    }

    #[test]
    fn test_scb_new_defaults() {
        let scb_escapable = MorpheusScb::new(true);
        let scb_not_escapable = MorpheusScb::new(false);

        use core::sync::atomic::Ordering;
        assert_eq!(scb_escapable.escapable.load(Ordering::Relaxed), 1);
        assert_eq!(scb_not_escapable.escapable.load(Ordering::Relaxed), 0);
        assert_eq!(
            scb_escapable.worker_state.load(Ordering::Relaxed),
            WorkerState::Init as u32
        );
        assert_eq!(
            scb_escapable.escalation_policy.load(Ordering::Relaxed),
            EscalationPolicy::None as u32
        );
    }

    #[test]
    fn test_global_pressure_is_pressured() {
        use core::sync::atomic::Ordering;

        let pressure = GlobalPressure::new();
        assert!(!pressure.is_pressured());

        pressure.cpu_pressure_pct.store(51, Ordering::Relaxed);
        assert!(pressure.is_pressured());

        pressure.cpu_pressure_pct.store(0, Ordering::Relaxed);
        pressure.io_pressure_pct.store(60, Ordering::Relaxed);
        assert!(pressure.is_pressured());

        pressure.io_pressure_pct.store(0, Ordering::Relaxed);
        pressure.memory_pressure_pct.store(75, Ordering::Relaxed);
        assert!(pressure.is_pressured());
    }
}
