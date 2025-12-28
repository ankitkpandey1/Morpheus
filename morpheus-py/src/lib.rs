// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! Python bindings for Morpheus-Hybrid
//!
//! This module provides Python bindings for the Morpheus runtime,
//! enabling Python async code to participate in kernel-guided
//! cooperative scheduling.
//!
//! # Usage
//!
//! ```python
//! import morpheus
//!
//! # Initialize on a worker thread
//! morpheus.init_worker(escapable=False)  # Python: GIL safety
//!
//! # In async code, call checkpoint() periodically
//! async def heavy_computation():
//!     for i in range(1_000_000):
//!         # ... compute ...
//!         if i % 1000 == 0:
//!             morpheus.checkpoint()
//!
//! # Protect FFI/GIL-sensitive code
//! with morpheus.critical():
//!     # Kernel will not escalate here
//!     pass
//! ```

use morpheus_runtime::{self as rt, critical::in_critical_section};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::atomic::Ordering;

/// Check for pending kernel yield requests.
///
/// Returns True if a yield was requested by the kernel.
/// Python event loops should yield after calling this if it returns True.
///
/// This is a very fast operation (single atomic load and compare).
#[pyfunction]
fn checkpoint() -> bool {
    rt::checkpoint_sync()
}

/// Check if a yield is currently requested by the kernel.
///
/// Unlike checkpoint(), this doesn't affect any state - it just checks.
#[pyfunction]
fn yield_requested() -> bool {
    if let Some(scb) = rt::worker::try_current_scb() {
        scb.yield_requested()
    } else {
        false
    }
}

/// Acknowledge a kernel yield request.
///
/// Call this after the event loop has yielded to tell the kernel
/// that we responded to its request.
#[pyfunction]
fn acknowledge_yield() -> bool {
    if let Some(scb) = rt::worker::try_current_scb() {
        scb.acknowledge()
    } else {
        false
    }
}

/// Get the current kernel pressure level (0-100).
///
/// Returns None if not running on a Morpheus worker thread.
#[pyfunction]
fn pressure_level() -> Option<u32> {
    rt::worker::try_current_scb().map(|scb| scb.pressure_level())
}

/// Get the remaining budget in nanoseconds.
///
/// Returns None if not running on a Morpheus worker thread.
#[pyfunction]
fn budget_remaining_ns() -> Option<u64> {
    rt::worker::try_current_scb().map(|scb| scb.budget_remaining_ns())
}

/// Set the runtime priority (0-1000).
///
/// Higher priority workers may receive longer grace periods.
#[pyfunction]
fn set_priority(priority: u32) -> PyResult<()> {
    if let Some(scb) = rt::worker::try_current_scb() {
        scb.set_priority(priority);
        Ok(())
    } else {
        Err(PyRuntimeError::new_err("Not on a Morpheus worker thread"))
    }
}

/// Check if we're currently in a critical section.
#[pyfunction]
fn is_in_critical_section_py() -> bool {
    in_critical_section()
}

/// Get the current worker ID.
///
/// Returns None if not running on a Morpheus worker thread.
#[pyfunction]
fn worker_id() -> Option<u32> {
    rt::worker::current_worker_id()
}

/// Enter a critical section.
///
/// Must be paired with exit_critical_section().
#[pyfunction]
fn enter_critical_section() {
    // Call critical_section() but immediately forget the guard
    // The critical depth is tracked thread-locally in the runtime
    let _guard = rt::critical_section();
    std::mem::forget(_guard);
}

/// Exit a critical section.
///
/// Must be paired with enter_critical_section().
#[pyfunction]
fn exit_critical_section() {
    // Manually decrement the critical depth
    if let Some(scb) = rt::worker::try_current_scb() {
        scb.scb().is_in_critical_section.store(0, Ordering::Release);
    }
}

/// Critical section context manager.
///
/// While inside a critical section:
/// - The kernel will not force-preempt this worker
/// - checkpoint() will return False
///
/// Critical sections can be nested safely.
#[pyclass]
struct CriticalSection {
    active: bool,
}

#[pymethods]
impl CriticalSection {
    #[new]
    fn new() -> Self {
        Self { active: false }
    }

    fn __enter__(&mut self) -> PyResult<()> {
        enter_critical_section();
        self.active = true;
        Ok(())
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        if self.active {
            exit_critical_section();
            self.active = false;
        }
        Ok(false) // Don't suppress exceptions
    }
}

/// Create a critical section context manager.
///
/// Usage:
///     with morpheus.critical():
///         # FFI or GIL-sensitive code
///         pass
#[pyfunction]
fn critical() -> CriticalSection {
    CriticalSection::new()
}

/// Runtime statistics.
#[pyclass]
#[derive(Clone)]
struct Stats {
    #[pyo3(get)]
    hints_received: u64,
    #[pyo3(get)]
    hints_dropped: u64,
    #[pyo3(get)]
    defensive_triggers: u64,
}

/// Get ring buffer statistics.
///
/// Returns None if runtime is not initialized.
#[pyfunction]
fn get_stats() -> Option<Stats> {
    rt::runtime::runtime().map(|rt| {
        let hints = rt.hints();
        let stats = hints.stats();
        Stats {
            hints_received: stats.hints_received.load(Ordering::Relaxed),
            hints_dropped: stats.hints_dropped.load(Ordering::Relaxed),
            defensive_triggers: stats.defensive_triggers.load(Ordering::Relaxed),
        }
    })
}

/// Check if defensive mode is currently active.
///
/// Defensive mode is triggered when the ring buffer overflows or
/// sequence gaps are detected.
#[pyfunction]
fn is_defensive_mode() -> bool {
    rt::runtime::runtime()
        .map(|rt| rt.defensive().is_active())
        .unwrap_or(false)
}

/// Morpheus Python module
#[pymodule]
fn morpheus(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(checkpoint, m)?)?;
    m.add_function(wrap_pyfunction!(yield_requested, m)?)?;
    m.add_function(wrap_pyfunction!(acknowledge_yield, m)?)?;
    m.add_function(wrap_pyfunction!(pressure_level, m)?)?;
    m.add_function(wrap_pyfunction!(budget_remaining_ns, m)?)?;
    m.add_function(wrap_pyfunction!(set_priority, m)?)?;
    m.add_function(wrap_pyfunction!(is_in_critical_section_py, m)?)?;
    m.add_function(wrap_pyfunction!(worker_id, m)?)?;
    m.add_function(wrap_pyfunction!(critical, m)?)?;
    m.add_function(wrap_pyfunction!(enter_critical_section, m)?)?;
    m.add_function(wrap_pyfunction!(exit_critical_section, m)?)?;
    m.add_function(wrap_pyfunction!(get_stats, m)?)?;
    m.add_function(wrap_pyfunction!(is_defensive_mode, m)?)?;

    m.add_class::<CriticalSection>()?;
    m.add_class::<Stats>()?;

    // Constants
    m.add("HINT_BUDGET", morpheus_common::HintReason::Budget as u32)?;
    m.add(
        "HINT_PRESSURE",
        morpheus_common::HintReason::Pressure as u32,
    )?;
    m.add(
        "HINT_IMBALANCE",
        morpheus_common::HintReason::Imbalance as u32,
    )?;
    m.add(
        "HINT_DEADLINE",
        morpheus_common::HintReason::Deadline as u32,
    )?;

    m.add("MAX_WORKERS", morpheus_common::config::MAX_WORKERS)?;
    m.add(
        "DEFAULT_SLICE_NS",
        morpheus_common::config::DEFAULT_SLICE_NS,
    )?;
    m.add("GRACE_PERIOD_NS", morpheus_common::config::GRACE_PERIOD_NS)?;

    Ok(())
}
