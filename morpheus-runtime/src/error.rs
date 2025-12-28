//! Error types for Morpheus runtime

use thiserror::Error;

/// Alias for `Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;

/// Morpheus runtime errors
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to access BPF map
    #[error("BPF map error: {0}")]
    BpfMap(String),

    /// Failed to mmap SCB
    #[error("mmap failed: {0}")]
    Mmap(#[from] std::io::Error),

    /// Worker registration failed
    #[error("worker registration failed: {0}")]
    Registration(String),

    /// Ring buffer error
    #[error("ring buffer error: {0}")]
    RingBuffer(String),

    /// Invalid worker ID
    #[error("invalid worker ID: {0}")]
    InvalidWorker(u32),

    /// Runtime not initialized
    #[error("runtime not initialized")]
    NotInitialized,

    /// Operation not supported
    #[error("operation not supported: {0}")]
    NotSupported(String),
}
