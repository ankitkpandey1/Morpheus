//! Critical section guard
//!
//! Critical sections protect FFI calls, zero-copy operations, and other
//! code that must not be interrupted. While inside a critical section:
//! - The kernel will not force-preempt this worker
//! - The runtime will not yield at checkpoints
//!
//! # Design
//!
//! The guard is `!Send` and `!Sync` to prevent it from being held across
//! await points (which would defeat the purpose of cooperative scheduling).

use crate::worker;
use std::cell::Cell;
use std::marker::PhantomData;

/// RAII guard for critical sections
///
/// While this guard exists, the kernel will not escalate on this worker,
/// and checkpoints will not yield.
///
/// # Thread Safety
///
/// This type is intentionally `!Send` and `!Sync`. Holding a `CriticalGuard`
/// across an await point would allow the task to be rescheduled, potentially
/// leaving the critical section flag set incorrectly.
///
/// # Example
///
/// ```rust,no_run
/// use morpheus_runtime::critical_section;
///
/// fn perform_ffi() {
///     let _guard = critical_section();
///     // FFI calls here are protected from forced preemption
///     unsafe {
///         // libc::some_operation();
///     }
/// } // Guard dropped, critical section ends
/// ```
pub struct CriticalGuard {
    /// Prevent Send and Sync using PhantomData with *const ()
    /// *const () is !Send and !Sync
    _marker: PhantomData<*const ()>,
    /// Track nesting depth for this guard
    _depth: u32,
}

thread_local! {
    /// Track critical section nesting depth
    static CRITICAL_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Enter a critical section
///
/// Returns a guard that will exit the critical section when dropped.
/// Critical sections can be nested; the kernel flag is only cleared
/// when all guards are dropped.
///
/// # Panics
///
/// Panics if called from outside a Morpheus worker thread.
///
/// # Example
///
/// ```rust,no_run
/// use morpheus_runtime::critical_section;
///
/// fn outer() {
///     let _guard1 = critical_section();
///     inner();
///     // _guard1 still active
/// }
///
/// fn inner() {
///     let _guard2 = critical_section(); // Nested, OK
///     // Both guards active
/// }
/// ```
#[inline]
pub fn critical_section() -> CriticalGuard {
    CRITICAL_DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);

        // Only set the SCB flag on first entry
        if current == 0 {
            if let Some(scb) = worker::try_current_scb() {
                scb.enter_critical();
            }
        }

        CriticalGuard {
            _marker: PhantomData,
            _depth: current + 1,
        }
    })
}

impl Drop for CriticalGuard {
    fn drop(&mut self) {
        CRITICAL_DEPTH.with(|depth| {
            let current = depth.get();
            debug_assert!(current > 0, "CriticalGuard dropped without matching enter");
            depth.set(current - 1);

            // Only clear the SCB flag on last exit
            if current == 1 {
                if let Some(scb) = worker::try_current_scb() {
                    scb.exit_critical();
                }
            }
        });
    }
}

/// Check if we're currently in a critical section
#[inline]
pub fn in_critical_section() -> bool {
    CRITICAL_DEPTH.with(|depth| depth.get() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critical_section_nesting() {
        assert!(!in_critical_section());

        {
            let _g1 = critical_section();
            assert!(in_critical_section());

            {
                let _g2 = critical_section();
                assert!(in_critical_section());
            }

            assert!(in_critical_section());
        }

        assert!(!in_critical_section());
    }
    
    /// Compile-time assertion that CriticalGuard is !Send
    /// This prevents accidental transfer across threads
    #[allow(dead_code)]
    fn assert_critical_guard_not_send() {
        fn requires_send<T: Send>() {}
        // This line would fail to compile if CriticalGuard implemented Send
        // Uncomment to verify: requires_send::<CriticalGuard>();
    }
    
    /// Compile-time assertion that CriticalGuard is !Sync
    /// This prevents accidental sharing across threads
    #[allow(dead_code)]
    fn assert_critical_guard_not_sync() {
        fn requires_sync<T: Sync>() {}
        // This line would fail to compile if CriticalGuard implemented Sync
        // Uncomment to verify: requires_sync::<CriticalGuard>();
    }
    
    /// Verify the static properties using trait bounds
    #[test]
    fn test_critical_guard_not_send_or_sync() {
        // These static assertions verify at compile time that CriticalGuard
        // does NOT implement Send or Sync
        
        // The PhantomData<*const ()> marker makes the type !Send and !Sync
        // because raw pointers are neither Send nor Sync
        static_assertions::assert_not_impl_any!(CriticalGuard: Send, Sync);
    }
}

