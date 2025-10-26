use async_trait::async_trait;
use crate::Result;

/// Key-Value storage trait for persistence
#[async_trait(?Send)]
pub trait KvStore {
    /// Get a value by key
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    
    /// Set a value for a key
    async fn set(&self, key: &str, value: Vec<u8>) -> Result<()>;
    
    /// Delete a key
    async fn delete(&self, key: &str) -> Result<()>;
    
    /// List all keys with a given prefix
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>>;
    
    /// Clear all data (used for testing)
    async fn clear(&self) -> Result<()>;
}

/// Cryptographic operations trait
#[async_trait(?Send)]
pub trait Crypto {
    /// Generate a new keypair and return the public key (DID key)
    async fn generate_keypair(&self) -> Result<String>;
    
    /// Sign data with the stored private key
    async fn sign(&self, data: &[u8]) -> Result<Vec<u8>>;
    
    /// Verify a signature
    async fn verify(&self, data: &[u8], signature: &[u8], public_key: &str) -> Result<bool>;
    
    /// Get the current DID (public key)
    async fn get_did(&self) -> Result<Option<String>>;
    
    /// Export keypair for backup (returns encrypted/encoded keypair)
    async fn export_keypair(&self) -> Result<Vec<u8>>;
    
    /// Import keypair from backup
    async fn import_keypair(&self, data: &[u8]) -> Result<String>;
}

/// Clock trait for timestamp generation
pub trait Clock {
    /// Get current timestamp in milliseconds since epoch
    fn now(&self) -> u64;
}
