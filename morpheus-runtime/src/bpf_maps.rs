//! BPF map registration for kernel↔userspace communication
//!
//! This module provides functions to register worker TIDs with the kernel's
//! BPF maps, enabling the kernel scheduler to identify Morpheus workers.

use crate::error::{Error, Result};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// Handle to BPF maps used for worker registration
///
/// This struct holds file descriptors to the BPF maps exposed by scx_morpheus.
/// Workers use this to register their TID and access their SCB.
pub struct BpfMaps {
    /// worker_tid_map: TID -> worker_id mapping
    tid_map_fd: OwnedFd,
    /// scb_map: worker_id -> SCB mapping (mmappable)
    scb_map_fd: OwnedFd,
}

impl BpfMaps {
    /// Create a new BpfMaps handle from raw file descriptors
    ///
    /// # Safety
    /// The caller must ensure the file descriptors are valid BPF map fds
    /// for worker_tid_map and scb_map respectively.
    pub unsafe fn from_raw_fds(tid_map_fd: i32, scb_map_fd: i32) -> Self {
        Self {
            tid_map_fd: OwnedFd::from_raw_fd(tid_map_fd),
            scb_map_fd: OwnedFd::from_raw_fd(scb_map_fd),
        }
    }

    /// Create a new BpfMaps handle by looking up maps by name
    ///
    /// This function attempts to find the maps by their pinned paths or
    /// by iterating through available maps.
    /// Create a new BpfMaps handle by looking up maps by name
    ///
    /// This function attempts to find the maps by their pinned paths or
    /// by iterating through available maps.
    pub fn from_pinned_paths(tid_map_path: &str, scb_map_path: &str) -> Result<Self> {
        let tid_map_fd = Self::bpf_obj_get(tid_map_path).map_err(|e| {
            Error::BpfMap(format!("failed to open tid_map at {}: {}", tid_map_path, e))
        })?;

        let scb_map_fd = Self::bpf_obj_get(scb_map_path).map_err(|e| {
            Error::BpfMap(format!("failed to open scb_map at {}: {}", scb_map_path, e))
        })?;

        Ok(Self {
            tid_map_fd,
            scb_map_fd,
        })
    }

    fn bpf_obj_get(pathname: &str) -> std::io::Result<OwnedFd> {
        let c_path = std::ffi::CString::new(pathname)?;
        let attr = BpfObjGetAttr {
            pathname: c_path.as_ptr() as u64,
            bpf_fd: 0,
            file_flags: 0,
        };

        let fd = unsafe {
            libc::syscall(
                libc::SYS_bpf,
                7, // BPF_OBJ_GET
                &attr as *const _ as *const libc::c_void,
                std::mem::size_of::<BpfObjGetAttr>(),
            )
        };

        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        unsafe { Ok(OwnedFd::from_raw_fd(fd as i32)) }
    }

    /// Get the SCB map file descriptor (for mmap)
    pub fn scb_map_fd(&self) -> BorrowedFd<'_> {
        self.scb_map_fd.as_fd()
    }

    /// Get the TID map file descriptor
    pub fn tid_map_fd(&self) -> BorrowedFd<'_> {
        self.tid_map_fd.as_fd()
    }

    /// Register a worker thread with the kernel
    ///
    /// This writes the TID -> worker_id mapping to the BPF hash map,
    /// enabling the kernel to identify this thread as a Morpheus worker.
    pub fn register_worker(&self, tid: u32, worker_id: u32) -> Result<()> {
        let key = tid.to_ne_bytes();
        let value = worker_id.to_ne_bytes();

        // Use BPF syscall to update the map
        let ret = unsafe {
            libc::syscall(
                libc::SYS_bpf,
                2, // BPF_MAP_UPDATE_ELEM
                &BpfMapUpdateAttr {
                    map_fd: self.tid_map_fd.as_raw_fd() as u32,
                    _pad0: 0,
                    key: key.as_ptr() as u64,
                    value: value.as_ptr() as u64,
                    flags: 0, // BPF_ANY
                } as *const _ as *const libc::c_void,
                std::mem::size_of::<BpfMapUpdateAttr>(),
            )
        };

        if ret < 0 {
            return Err(Error::Registration(format!(
                "failed to register worker tid={} id={}: {}",
                tid,
                worker_id,
                std::io::Error::last_os_error()
            )));
        }

        tracing::debug!("registered worker tid={} -> id={}", tid, worker_id);
        Ok(())
    }

    /// Unregister a worker thread from the kernel
    ///
    /// This removes the TID from the BPF hash map.
    pub fn unregister_worker(&self, tid: u32) -> Result<()> {
        let key = tid.to_ne_bytes();

        // Use BPF syscall to delete from the map
        let ret = unsafe {
            libc::syscall(
                libc::SYS_bpf,
                3, // BPF_MAP_DELETE_ELEM
                &BpfMapDeleteAttr {
                    map_fd: self.tid_map_fd.as_raw_fd() as u32,
                    _pad0: 0,
                    key: key.as_ptr() as u64,
                } as *const _ as *const libc::c_void,
                std::mem::size_of::<BpfMapDeleteAttr>(),
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            // ENOENT is OK - worker might already be removed
            if err.raw_os_error() != Some(libc::ENOENT) {
                return Err(Error::Registration(format!(
                    "failed to unregister worker tid={}: {}",
                    tid, err
                )));
            }
        }

        tracing::debug!("unregistered worker tid={}", tid);
        Ok(())
    }
}

/// BPF_MAP_UPDATE_ELEM attribute structure
/// Note: The kernel expects specific field alignment
#[repr(C)]
struct BpfMapUpdateAttr {
    map_fd: u32,
    _pad0: u32, // Padding for 8-byte alignment of key pointer
    key: u64,
    value: u64,
    flags: u64,
}

#[allow(dead_code)]
struct BpfMapDeleteAttr {
    map_fd: u32,
    _pad0: u32, // Padding for 8-byte alignment
    key: u64,
}

#[repr(C)]
struct BpfObjGetAttr {
    pathname: u64,
    bpf_fd: u32,
    file_flags: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpf_attr_sizes() {
        // Ensure our attr structs match expected sizes with proper padding
        assert_eq!(std::mem::size_of::<BpfMapUpdateAttr>(), 32);
        assert_eq!(std::mem::size_of::<BpfMapDeleteAttr>(), 16);
    }
}
