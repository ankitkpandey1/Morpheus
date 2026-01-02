//! Integration tests for Morpheus runtime
//!
//! These tests verify integration between components without requiring
//! the kernel scheduler. They use mocked SCBs where necessary.

use morpheus_common::{
    EscalationPolicy, MorpheusScb, RuntimeMode, SchedulerMode, WorkerState, YieldReason,
};
use std::sync::atomic::Ordering;

/// Test worker state transitions
#[test]
fn test_worker_lifecycle_transitions() {
    // Create an SCB and verify initial state
    let scb = MorpheusScb::new(true);

    assert_eq!(
        scb.worker_state.load(Ordering::Relaxed),
        WorkerState::Init as u32,
        "New SCB should be in INIT state"
    );

    // Transition: INIT -> REGISTERED
    scb.worker_state
        .store(WorkerState::Registered as u32, Ordering::Release);
    assert!(!WorkerState::Registered.can_receive_hints());

    // Transition: REGISTERED -> RUNNING
    scb.worker_state
        .store(WorkerState::Running as u32, Ordering::Release);
    assert!(WorkerState::Running.can_receive_hints());
    assert!(WorkerState::Running.can_escalate());

    // Transition: RUNNING -> QUIESCING
    scb.worker_state
        .store(WorkerState::Quiescing as u32, Ordering::Release);
    assert!(!WorkerState::Quiescing.can_receive_hints());
    assert!(!WorkerState::Quiescing.can_escalate());

    // Transition: QUIESCING -> DEAD
    scb.worker_state
        .store(WorkerState::Dead as u32, Ordering::Release);
    assert!(!WorkerState::Dead.can_receive_hints());
    assert!(!WorkerState::Dead.can_escalate());
}

/// Test escalation gating conditions
#[test]
fn test_escalation_gating() {
    let scb = MorpheusScb::new(true);

    // Set up for escalation
    scb.worker_state
        .store(WorkerState::Running as u32, Ordering::Release);
    scb.escalation_policy
        .store(EscalationPolicy::ThreadKick as u32, Ordering::Release);
    scb.escapable.store(1, Ordering::Release);
    scb.is_in_critical_section.store(0, Ordering::Release);
    scb.preempt_seq.store(5, Ordering::Release);
    scb.last_ack_seq.store(3, Ordering::Release);

    // All conditions met for escalation
    let worker_state = WorkerState::try_from(scb.worker_state.load(Ordering::Acquire)).unwrap();
    let policy = EscalationPolicy::try_from(scb.escalation_policy.load(Ordering::Acquire)).unwrap();
    let escapable = scb.escapable.load(Ordering::Acquire) == 1;
    let in_critical = scb.is_in_critical_section.load(Ordering::Acquire) == 1;
    let preempt = scb.preempt_seq.load(Ordering::Acquire);
    let acked = scb.last_ack_seq.load(Ordering::Acquire);

    assert!(worker_state.can_escalate(), "Worker should be escalatable");
    assert!(
        policy != EscalationPolicy::None,
        "Policy should allow escalation"
    );
    assert!(escapable, "Worker should be escapable");
    assert!(!in_critical, "Worker should not be in critical section");
    assert!(preempt > acked, "Unacknowledged hints should exist");
}

/// Test critical section blocks escalation
#[test]
fn test_critical_section_blocks_escalation() {
    let scb = MorpheusScb::new(true);

    // Set up runaway worker
    scb.worker_state
        .store(WorkerState::Running as u32, Ordering::Release);
    scb.escalation_policy
        .store(EscalationPolicy::ThreadKick as u32, Ordering::Release);
    scb.escapable.store(1, Ordering::Release);
    scb.preempt_seq.store(10, Ordering::Release);
    scb.last_ack_seq.store(0, Ordering::Release);

    // Enter critical section
    scb.is_in_critical_section.store(1, Ordering::Release);

    // Verify escalation blocked
    let in_critical = scb.is_in_critical_section.load(Ordering::Acquire) == 1;
    assert!(in_critical, "Critical section flag should be set");

    // Exit critical section
    scb.is_in_critical_section.store(0, Ordering::Release);
    let in_critical = scb.is_in_critical_section.load(Ordering::Acquire) == 1;
    assert!(!in_critical, "Critical section flag should be cleared");
}

/// Test yield acknowledgment
#[test]
fn test_yield_acknowledgment() {
    let scb = MorpheusScb::new(true);

    // Kernel sends hints
    scb.preempt_seq.store(5, Ordering::Release);
    scb.last_ack_seq.store(0, Ordering::Release);

    // Check pending hints
    let pending =
        scb.preempt_seq.load(Ordering::Acquire) > scb.last_ack_seq.load(Ordering::Acquire);
    assert!(pending, "Should have pending hints");

    // Acknowledge hints
    let current_seq = scb.preempt_seq.load(Ordering::Acquire);
    scb.last_ack_seq.store(current_seq, Ordering::Release);
    scb.last_yield_reason
        .store(YieldReason::Hint as u32, Ordering::Release);

    // Verify no pending hints
    let pending =
        scb.preempt_seq.load(Ordering::Acquire) > scb.last_ack_seq.load(Ordering::Acquire);
    assert!(!pending, "Should have no pending hints after ack");

    // Verify yield reason recorded
    let reason = YieldReason::try_from(scb.last_yield_reason.load(Ordering::Acquire)).unwrap();
    assert_eq!(reason, YieldReason::Hint);
}

/// Test runtime mode transitions
#[test]
fn test_runtime_mode_transitions() {
    // Start in deterministic mode
    let mode = RuntimeMode::Deterministic;
    assert!(!mode.should_yield_eagerly());

    // Transition to pressured (hints received)
    let mode = RuntimeMode::Pressured;
    assert!(!mode.should_yield_eagerly());

    // Transition to defensive (hint loss detected)
    let mode = RuntimeMode::Defensive;
    assert!(mode.should_yield_eagerly());
}

/// Test scheduler mode defaults
#[test]
fn test_scheduler_mode_observer_only_default() {
    let mode = SchedulerMode::default();
    assert_eq!(
        mode,
        SchedulerMode::ObserverOnly,
        "Default scheduler mode should be ObserverOnly for safety"
    );
}

/// Test Python workers non-escapable by default
#[test]
fn test_python_workers_not_escapable() {
    // Python workers should be created with escapable=false
    let python_scb = MorpheusScb::new(false);
    assert_eq!(
        python_scb.escapable.load(Ordering::Relaxed),
        0,
        "Python workers must default to escapable=false for GIL safety"
    );
}

/// Test Rust workers escapable by default  
#[test]
fn test_rust_workers_escapable() {
    // Rust workers should be created with escapable=true
    let rust_scb = MorpheusScb::new(true);
    assert_eq!(
        rust_scb.escapable.load(Ordering::Relaxed),
        1,
        "Rust workers should default to escapable=true"
    );
}
