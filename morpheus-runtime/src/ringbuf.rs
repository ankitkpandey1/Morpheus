//! Ring buffer consumer for kernel hints
//!
//! Consumes yield hints from the kernel via the BPF ring buffer.
//! Detects overflow conditions and triggers defensive mode.

use morpheus_common::{HintReason, MorpheusHint};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, warn};

/// Statistics for ring buffer consumption
#[derive(Debug, Default)]
pub struct RingBufStats {
    /// Total hints received
    pub hints_received: AtomicU64,
    /// Hints dropped (detected by sequence gaps)
    pub hints_dropped: AtomicU64,
    /// Number of times defensive mode was triggered
    pub defensive_triggers: AtomicU64,
}

/// Ring buffer consumer
///
/// Note: The actual libbpf RingBuffer must be created and polled on a
/// dedicated thread, as it is not Sync. This struct only tracks state.
pub struct HintConsumer {
    /// Last seen sequence number (for gap detection)
    last_seq: AtomicU64,
    /// Whether defensive mode is active
    defensive_mode: Arc<AtomicBool>,
    /// Statistics
    stats: Arc<RingBufStats>,
}

impl HintConsumer {
    /// Create a new hint consumer
    pub fn new() -> Self {
        Self {
            last_seq: AtomicU64::new(0),
            defensive_mode: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RingBufStats::default()),
        }
    }

    /// Check if defensive mode is active
    pub fn is_defensive(&self) -> bool {
        self.defensive_mode.load(Ordering::Relaxed)
    }

    /// Get statistics
    pub fn stats(&self) -> &Arc<RingBufStats> {
        &self.stats
    }

    /// Get defensive mode flag (for sharing with worker threads)
    pub fn defensive_flag(&self) -> Arc<AtomicBool> {
        self.defensive_mode.clone()
    }

    /// Process a raw hint from the ring buffer
    ///
    /// Call this from the ring buffer callback.
    pub fn process_hint(&self, data: &[u8]) -> i32 {
        if data.len() < std::mem::size_of::<MorpheusHint>() {
            warn!("received truncated hint: {} bytes", data.len());
            return 0;
        }

        // Parse the hint
        let hint = unsafe { std::ptr::read_unaligned(data.as_ptr() as *const MorpheusHint) };

        self.stats.hints_received.fetch_add(1, Ordering::Relaxed);

        // Check for sequence gaps (indicates dropped hints)
        let last = self.last_seq.load(Ordering::Relaxed);
        if hint.seq > last + 1 && last > 0 {
            let dropped = hint.seq - last - 1;
            self.stats
                .hints_dropped
                .fetch_add(dropped, Ordering::Relaxed);
            warn!(
                "detected {} dropped hints (seq gap: {} -> {})",
                dropped, last, hint.seq
            );

            // Trigger defensive mode
            if !self.defensive_mode.swap(true, Ordering::Release) {
                self.stats
                    .defensive_triggers
                    .fetch_add(1, Ordering::Relaxed);
                warn!("entering defensive mode due to hint drops");
            }
        }

        self.last_seq.store(hint.seq, Ordering::Relaxed);

        debug!(
            "received hint: seq={}, reason={:?}, tid={}, deadline={}",
            hint.seq,
            HintReason::try_from(hint.reason).ok(),
            hint.target_tid,
            hint.deadline_ns
        );

        0 // Continue consuming
    }

    /// Reset defensive mode (call after quiet period)
    pub fn reset_defensive(&self) {
        if self.defensive_mode.swap(false, Ordering::Release) {
            debug!("exiting defensive mode");
        }
    }
}

// HintConsumer is now Sync because it only contains atomics
unsafe impl Sync for HintConsumer {}

impl Default for HintConsumer {
    fn default() -> Self {
        Self::new()
    }
}

/// Defensive mode state machine
pub struct DefensiveMode {
    /// Whether defensive mode is active
    active: AtomicBool,
    /// Number of yields remaining in defensive mode
    yields_remaining: AtomicU64,
    /// Default number of yields in defensive mode
    default_yields: u64,
}

impl DefensiveMode {
    /// Create a new defensive mode controller
    ///
    /// # Arguments
    /// * `default_yields` - Number of forced yields when entering defensive mode
    pub fn new(default_yields: u64) -> Self {
        Self {
            active: AtomicBool::new(false),
            yields_remaining: AtomicU64::new(0),
            default_yields,
        }
    }

    /// Enter defensive mode
    pub fn enter(&self) {
        self.active.store(true, Ordering::Release);
        self.yields_remaining
            .store(self.default_yields, Ordering::Release);
    }

    /// Check if we should yield (and decrement counter)
    pub fn should_yield(&self) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }

        let remaining = self.yields_remaining.fetch_sub(1, Ordering::AcqRel);
        if remaining <= 1 {
            self.active.store(false, Ordering::Release);
        }

        true
    }

    /// Check if defensive mode is active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Manually exit defensive mode
    pub fn exit(&self) {
        self.active.store(false, Ordering::Release);
        self.yields_remaining.store(0, Ordering::Release);
    }
}

impl Default for DefensiveMode {
    fn default() -> Self {
        Self::new(100) // Default: 100 forced yields
    }
}
