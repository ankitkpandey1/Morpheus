//! Shared Control Block (SCB) management
//!
//! SCBs are per-worker structures shared between kernel and userspace.
//! This module provides safe Rust wrappers for SCB access.

use crate::error::{Error, Result};
use morpheus_common::{config, MorpheusScb};
use std::fs::File;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::io::FromRawFd;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

/// Handle to a memory-mapped SCB
///
/// Provides safe access to an SCB in the kernel's BPF map.
/// The SCB is memory-mapped for zero-copy access.
pub struct ScbHandle {
    ptr: NonNull<MorpheusScb>,
    worker_id: u32,
    // Keep the mmap alive
    _mmap: memmap2::MmapMut,
}

// SCB access is thread-safe through atomics
unsafe impl Send for ScbHandle {}
unsafe impl Sync for ScbHandle {}

impl ScbHandle {
    /// Create a new SCB handle by mapping the SCB map
    ///
    /// # Arguments
    /// * `map_fd` - File descriptor of the scb_map BPF map
    /// * `worker_id` - ID of this worker (index into the map)
    /// * `escapable` - Whether this worker allows forced escalation
    ///
    /// # Safety
    /// The caller must ensure the map_fd is valid and points to the scb_map.
    pub unsafe fn new(map_fd: BorrowedFd<'_>, worker_id: u32, escapable: bool) -> Result<Self> {
        if worker_id >= config::MAX_WORKERS {
            return Err(Error::InvalidWorker(worker_id));
        }

        // Calculate the offset for this worker's SCB
        let scb_size = std::mem::size_of::<MorpheusScb>();
        let offset = (worker_id as usize) * scb_size;

        // Memory map the SCB
        // Note: We map just this worker's SCB, not the entire map
        // Create a File from the borrowed fd for mmap (we need to dup it)
        let raw_fd = map_fd.as_raw_fd();
        let dup_fd = libc::dup(raw_fd);
        if dup_fd < 0 {
            return Err(Error::Mmap(std::io::Error::last_os_error()));
        }
        let file = File::from_raw_fd(dup_fd);

        let mmap = memmap2::MmapOptions::new()
            .offset(offset as u64)
            .len(scb_size)
            .map_mut(&file)
            .map_err(Error::Mmap)?;

        // Forget the file to avoid closing the fd (it's owned by libbpf)
        std::mem::forget(file);

        let ptr = NonNull::new(mmap.as_ptr() as *mut MorpheusScb)
            .ok_or_else(|| Error::Mmap(std::io::Error::other("mmap returned null")))?;

        // Initialize the SCB
        let scb = &*ptr.as_ptr();
        scb.preempt_seq.store(0, Ordering::Release);
        scb.budget_remaining_ns
            .store(config::DEFAULT_SLICE_NS, Ordering::Release);
        scb.kernel_pressure_level.store(0, Ordering::Release);
        scb.is_in_critical_section.store(0, Ordering::Release);
        scb.escapable
            .store(if escapable { 1 } else { 0 }, Ordering::Release);
        scb.last_ack_seq.store(0, Ordering::Release);
        scb.runtime_priority.store(500, Ordering::Release);

        Ok(Self {
            ptr,
            worker_id,
            _mmap: mmap,
        })
    }

    /// Get the worker ID
    #[inline]
    pub fn worker_id(&self) -> u32 {
        self.worker_id
    }

    /// Get a reference to the SCB
    #[inline]
    pub fn scb(&self) -> &MorpheusScb {
        // SAFETY: The pointer is valid for the lifetime of this handle
        unsafe { self.ptr.as_ref() }
    }

    /// Check if a yield was requested
    #[inline]
    pub fn yield_requested(&self) -> bool {
        let scb = self.scb();
        let preempt = scb.preempt_seq.load(Ordering::Acquire);
        let acked = scb.last_ack_seq.load(Ordering::Relaxed);
        preempt > acked
    }

    /// Acknowledge a yield request
    ///
    /// This should be called after yielding to tell the kernel we responded.
    /// Uses CAS to handle races with newer kernel requests.
    #[inline]
    pub fn acknowledge(&self) -> bool {
        let scb = self.scb();
        let target = scb.preempt_seq.load(Ordering::Acquire);
        let current = scb.last_ack_seq.load(Ordering::Relaxed);

        if target <= current {
            return true; // Already acknowledged
        }

        // CAS to update last_ack_seq
        scb.last_ack_seq
            .compare_exchange(current, target, Ordering::Release, Ordering::Relaxed)
            .is_ok()
    }

    /// Enter a critical section
    ///
    /// While in a critical section, the kernel will not escalate.
    /// Returns the previous critical section state.
    #[inline]
    pub fn enter_critical(&self) -> u32 {
        let scb = self.scb();
        scb.is_in_critical_section.swap(1, Ordering::Release)
    }

    /// Exit a critical section
    #[inline]
    pub fn exit_critical(&self) {
        let scb = self.scb();
        scb.is_in_critical_section.store(0, Ordering::Release);
    }

    /// Get the current kernel pressure level (0-100)
    #[inline]
    pub fn pressure_level(&self) -> u32 {
        self.scb().kernel_pressure_level.load(Ordering::Relaxed)
    }

    /// Get remaining budget in nanoseconds
    #[inline]
    pub fn budget_remaining_ns(&self) -> u64 {
        self.scb().budget_remaining_ns.load(Ordering::Relaxed)
    }

    /// Set the runtime priority (0-1000)
    #[inline]
    pub fn set_priority(&self, priority: u32) {
        self.scb()
            .runtime_priority
            .store(priority.min(1000), Ordering::Release);
    }
}

// Note: For creating SCB handles from libbpf-rs maps, use ScbHandle::new()
// directly with the map's file descriptor.
