use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpwnError {
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("vm not found: {0}")]
    VmNotFound(String),

    #[error("invalid vm state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },

    #[error("networking error: {0}")]
    Network(String),

    #[error("firecracker error: {0}")]
    Firecracker(String),

    #[error("process error: {0}")]
    Process(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
