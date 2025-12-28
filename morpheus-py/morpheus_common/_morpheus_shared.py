# SPDX-License-Identifier: GPL-2.0-only
# Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>
"""
Morpheus-Hybrid shared constants and type definitions.

This module provides Python equivalents of the C structures in morpheus_shared.h.
All offsets and sizes must match the C and Rust definitions exactly.
"""

from enum import IntEnum
from ctypes import Structure, c_uint32, c_uint64, sizeof

# ============================================================================
# Configuration Constants
# ============================================================================

MAX_WORKERS = 1024
DEFAULT_SLICE_NS = 5 * 1_000_000  # 5ms
GRACE_PERIOD_NS = 100 * 1_000_000  # 100ms
RINGBUF_SIZE = 256 * 1024  # 256KB


# ============================================================================
# Scheduler Mode (Delta #1: Observer vs Enforcer)
# ============================================================================

class SchedulerMode(IntEnum):
    """Scheduler operating mode."""
    OBSERVER_ONLY = 0  # Collect data, emit hints, no enforcement
    ENFORCED = 1       # Full escalation + kicks enabled


# ============================================================================
# Worker Lifecycle State Machine (Delta #2)
# ============================================================================

class WorkerState(IntEnum):
    """Worker thread lifecycle state."""
    INIT = 0        # Allocated but not registered
    REGISTERED = 1  # Registered with kernel
    RUNNING = 2     # Actively executing tasks
    QUIESCING = 3   # Shutting down
    DEAD = 4        # Terminated


# ============================================================================
# Escalation Policy (Delta #3)
# ============================================================================

class EscalationPolicy(IntEnum):
    """Escalation policy for unresponsive workers."""
    NONE = 0            # Hints only, no enforcement
    THREAD_KICK = 1     # Kick CPU to force reschedule
    CGROUP_THROTTLE = 2 # Apply cgroup throttling
    HYBRID = 3          # Kick + throttle (most aggressive)


# ============================================================================
# Yield Reason (Delta #5)
# ============================================================================

class YieldReason(IntEnum):
    """Reason for the last yield."""
    NONE = 0
    HINT = 1               # Yielded in response to kernel hint
    CHECKPOINT = 2         # Yielded at explicit checkpoint
    BUDGET = 3             # Yielded due to budget exhaustion
    DEFENSIVE = 4          # Defensive yield (heuristic)
    ESCALATION_RECOVERY = 5 # Recovery after escalation


# ============================================================================
# Runtime Mode (Delta #6)
# ============================================================================

class RuntimeMode(IntEnum):
    """Runtime scheduling mode."""
    DETERMINISTIC = 0  # No kernel hints - fully deterministic
    PRESSURED = 1      # Kernel hints active
    DEFENSIVE = 2      # Hint loss detected - defensive mode


# ============================================================================
# Hint Reasons
# ============================================================================

class HintReason(IntEnum):
    """Reason for kernel yield hint."""
    BUDGET = 1     # Worker exceeded time slice
    PRESSURE = 2   # System under CPU pressure
    IMBALANCE = 3  # Runqueue imbalance detected
    DEADLINE = 4   # Hard deadline approaching


# ============================================================================
# Shared Control Block (SCB) - ctypes structure
# ============================================================================

class MorpheusScb(Structure):
    """
    Shared Control Block - one per worker thread.
    
    Must match struct morpheus_scb in morpheus_shared.h exactly.
    Total size: 128 bytes (2 cache lines).
    
    Layout:
    - Cache Line 1 (bytes 0-63): Kernel -> Runtime
    - Cache Line 2 (bytes 64-127): Runtime -> Kernel
    """
    _pack_ = 8
    _fields_ = [
        # Cache Line 1: Kernel -> Runtime (bytes 0-63)
        ("preempt_seq", c_uint64),           # 0-7
        ("budget_remaining_ns", c_uint64),   # 8-15
        ("kernel_pressure_level", c_uint32), # 16-19
        ("worker_state", c_uint32),          # 20-23
        ("_reserved0", c_uint64 * 5),        # 24-63 (5*8=40 bytes)
        
        # Cache Line 2: Runtime -> Kernel (bytes 64-127)
        ("is_in_critical_section", c_uint32), # 64-67
        ("escapable", c_uint32),              # 68-71
        ("last_ack_seq", c_uint64),           # 72-79
        ("runtime_priority", c_uint32),       # 80-83
        ("last_yield_reason", c_uint32),      # 84-87
        ("_reserved1", c_uint64 * 1),         # 88-95 (1*8=8 bytes)
        ("escalation_policy", c_uint32),      # 96-99
        ("_pad", c_uint32),                   # 100-103
        ("_reserved2", c_uint64 * 3),         # 104-127 (3*8=24 bytes)
    ]


class MorpheusHint(Structure):
    """Hint message sent via ring buffer."""
    _pack_ = 8
    _fields_ = [
        ("seq", c_uint64),
        ("reason", c_uint32),
        ("target_tid", c_uint32),
        ("deadline_ns", c_uint64),
    ]


class GlobalPressure(Structure):
    """Global system pressure indicators."""
    _fields_ = [
        ("cpu_pressure_pct", c_uint32),
        ("io_pressure_pct", c_uint32),
        ("memory_pressure_pct", c_uint32),
        ("runqueue_depth", c_uint32),
    ]


# ============================================================================
# Offset verification (must match C and Rust)
# ============================================================================

def verify_offsets():
    """Verify that Python struct offsets match C definitions."""
    scb = MorpheusScb
    
    # Expected offsets from C struct (with fixed layout)
    expected = {
        "preempt_seq": 0,
        "budget_remaining_ns": 8,
        "kernel_pressure_level": 16,
        "worker_state": 20,
        "is_in_critical_section": 64,  # Cache line 2 start
        "escapable": 68,
        "last_ack_seq": 72,
        "runtime_priority": 80,
        "last_yield_reason": 84,
        "escalation_policy": 96,
    }
    
    for field_name, expected_offset in expected.items():
        actual_offset = getattr(scb, field_name).offset
        if actual_offset != expected_offset:
            raise AssertionError(
                f"Offset mismatch for {field_name}: "
                f"expected {expected_offset}, got {actual_offset}"
            )
    
    # Verify total size
    if sizeof(scb) != 128:
        raise AssertionError(
            f"SCB size mismatch: expected 128, got {sizeof(scb)}"
        )
    
    return True


# ============================================================================
# Map names (for BPF object lookup)
# ============================================================================

MAP_NAMES = {
    "scb": "scb_map",
    "hint_ringbuf": "hint_ringbuf",
    "worker_tid": "worker_tid_map",
    "global_pressure": "global_pressure_map",
    "config": "config_map",
}


# Run verification on import
if __name__ == "__main__":
    verify_offsets()
    print("✅ All offsets verified successfully")
    print(f"   SCB size: {sizeof(MorpheusScb)} bytes")
    print(f"   Hint size: {sizeof(MorpheusHint)} bytes")
