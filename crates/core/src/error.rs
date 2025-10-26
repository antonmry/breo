use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Crypto error: {0}")]
    Crypto(String),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Invalid DID: {0}")]
    InvalidDid(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Record not found: {0}")]
    RecordNotFound(String),
    
    #[error("Automerge error: {0}")]
    Automerge(String),
    
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
