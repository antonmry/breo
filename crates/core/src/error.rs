//! Error types for the PDS core library

use thiserror::Error;

/// Core error type for PDS operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid record: {0}")]
    InvalidRecord(String),

    #[error("Invalid commit: {0}")]
    InvalidCommit(String),

    #[error("Invalid DID: {0}")]
    InvalidDid(String),

    #[error("Invalid CID: {0}")]
    InvalidCid(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Automerge error: {0}")]
    AutomergeError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Result type alias for PDS operations
pub type Result<T> = std::result::Result<T, Error>;

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::SerializationError(err.to_string())
    }
}

impl From<automerge::AutomergeError> for Error {
    fn from(err: automerge::AutomergeError) -> Self {
        Error::AutomergeError(err.to_string())
    }
}
