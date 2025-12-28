# SPDX-License-Identifier: GPL-2.0-only
"""Morpheus-Hybrid common types and constants."""

from ._morpheus_shared import (
    # Constants
    MAX_WORKERS,
    DEFAULT_SLICE_NS,
    GRACE_PERIOD_NS,
    RINGBUF_SIZE,
    MAP_NAMES,
    
    # Enums
    SchedulerMode,
    WorkerState,
    EscalationPolicy,
    YieldReason,
    RuntimeMode,
    HintReason,
    
    # Structures
    MorpheusScb,
    MorpheusHint,
    GlobalPressure,
    
    # Verification
    verify_offsets,
)

__all__ = [
    "MAX_WORKERS",
    "DEFAULT_SLICE_NS",
    "GRACE_PERIOD_NS",
    "RINGBUF_SIZE",
    "MAP_NAMES",
    "SchedulerMode",
    "WorkerState",
    "EscalationPolicy",
    "YieldReason",
    "RuntimeMode",
    "HintReason",
    "MorpheusScb",
    "MorpheusHint",
    "GlobalPressure",
    "verify_offsets",
]
