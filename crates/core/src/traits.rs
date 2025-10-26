//! Trait definitions for pluggable components

use crate::error::Result;
use chrono::{DateTime, Utc};

/// Key-Value storage abstraction for persisting repository data
pub trait KvStore: Send + Sync {
    /// Store a value with the given key
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()>;

    /// Retrieve a value by key
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete a value by key
    fn delete(&mut self, key: &str) -> Result<()>;

    /// Check if a key exists
    fn exists(&self, key: &str) -> Result<bool>;

    /// List all keys with a given prefix
    fn list_keys(&self, prefix: &str) -> Result<Vec<String>>;
}

/// Clock abstraction for deterministic timestamp generation
pub trait Clock: Send + Sync {
    /// Get the current timestamp
    fn now(&self) -> DateTime<Utc>;
}

/// Cryptographic operations abstraction
pub trait Crypto: Send + Sync {
    /// Sign data with the private key
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Verify a signature
    fn verify(&self, data: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool>;

    /// Get the public key bytes
    fn public_key(&self) -> Vec<u8>;
}

/// Default system clock implementation
#[derive(Debug, Clone, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// In-memory key-value store implementation (for testing)
#[derive(Debug, Clone, Default)]
pub struct MemoryKvStore {
    data: std::collections::HashMap<String, Vec<u8>>,
}

impl MemoryKvStore {
    /// Create a new in-memory store
    pub fn new() -> Self {
        Self::default()
    }
}

impl KvStore for MemoryKvStore {
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.data.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.data.get(key).cloned())
    }

    fn delete(&mut self, key: &str) -> Result<()> {
        self.data.remove(key);
        Ok(())
    }

    fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.data.contains_key(key))
    }

    fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let keys: Vec<String> = self
            .data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        Ok(keys)
    }
}

/// Mock crypto implementation using ed25519
#[derive(Debug, Clone)]
pub struct Ed25519Crypto {
    keypair: ed25519_dalek::SigningKey,
}

impl Ed25519Crypto {
    /// Create a new crypto instance with a random keypair
    pub fn new() -> Self {
        use rand::rngs::OsRng;
        let signing_key = ed25519_dalek::SigningKey::generate(&mut OsRng);
        Ed25519Crypto {
            keypair: signing_key,
        }
    }

    /// Create from existing seed bytes
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(bytes);
        Ed25519Crypto {
            keypair: signing_key,
        }
    }
}

impl Default for Ed25519Crypto {
    fn default() -> Self {
        Self::new()
    }
}

impl Crypto for Ed25519Crypto {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        use ed25519_dalek::Signer;
        let signature = self.keypair.sign(data);
        Ok(signature.to_bytes().to_vec())
    }

    fn verify(&self, data: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let verifying_key = VerifyingKey::from_bytes(
            public_key
                .try_into()
                .map_err(|_| crate::error::Error::CryptoError("Invalid public key".to_string()))?,
        )
        .map_err(|e| crate::error::Error::CryptoError(e.to_string()))?;

        let signature = Signature::from_bytes(
            signature
                .try_into()
                .map_err(|_| crate::error::Error::CryptoError("Invalid signature".to_string()))?,
        );

        Ok(verifying_key.verify(data, &signature).is_ok())
    }

    fn public_key(&self) -> Vec<u8> {
        self.keypair.verifying_key().to_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_kvstore() {
        let mut store = MemoryKvStore::new();

        // Test put and get
        store.put("key1", b"value1").unwrap();
        let value = store.get("key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test exists
        assert!(store.exists("key1").unwrap());
        assert!(!store.exists("key2").unwrap());

        // Test delete
        store.delete("key1").unwrap();
        assert!(!store.exists("key1").unwrap());

        // Test list_keys
        store.put("prefix:key1", b"val1").unwrap();
        store.put("prefix:key2", b"val2").unwrap();
        store.put("other:key", b"val3").unwrap();

        let keys = store.list_keys("prefix:").unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_system_clock() {
        let clock = SystemClock;
        let now = clock.now();
        assert!(now.timestamp() > 0);
    }

    #[test]
    fn test_ed25519_crypto() {
        let crypto = Ed25519Crypto::new();

        // Test signing and verification
        let data = b"test message";
        let signature = crypto.sign(data).unwrap();
        let public_key = crypto.public_key();

        assert!(crypto.verify(data, &signature, &public_key).unwrap());

        // Wrong data should fail verification
        let wrong_data = b"wrong message";
        assert!(!crypto.verify(wrong_data, &signature, &public_key).unwrap());
    }
}
