pub mod ip;
pub mod tap;
pub mod iptables;

pub use tap::{NetworkManager, TapDevice};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("command failed: {cmd}: {stderr}")]
    CommandFailed { cmd: String, stderr: String },

    #[error("tap device not found: {0}")]
    TapNotFound(String),

    #[error("ip allocation exhausted")]
    AllocationExhausted,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, NetworkError>;
