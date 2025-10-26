use pds_core::{Crypto as CryptoTrait, Error, Result};
use async_trait::async_trait;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, Crypto, SubtleCrypto, CryptoKey, CryptoKeyPair};
use js_sys::{Object, Reflect, Uint8Array, Array};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}};
use getrandom;

const KEYPAIR_STORAGE_KEY: &str = "pds_keypair";
const DID_STORAGE_KEY: &str = "pds_did";

/// WebCrypto-based cryptographic operations
pub struct WebCrypto {
    storage: web_sys::Storage,
}

impl WebCrypto {
    pub fn new() -> Result<Self> {
        let window = window().ok_or_else(|| Error::Crypto("No window object".to_string()))?;
        let storage = window
            .local_storage()
            .map_err(|_| Error::Crypto("Failed to access localStorage".to_string()))?
            .ok_or_else(|| Error::Crypto("localStorage not available".to_string()))?;
        
        Ok(Self { storage })
    }

    fn get_stored_keypair(&self) -> Result<Option<Vec<u8>>> {
        match self.storage.get_item(KEYPAIR_STORAGE_KEY) {
            Ok(Some(data)) => {
                let bytes = STANDARD.decode(&data)
                    .map_err(|e| Error::Crypto(format!("Failed to decode keypair: {}", e)))?;
                Ok(Some(bytes))
            }
            Ok(None) => Ok(None),
            Err(_) => Err(Error::Crypto("Failed to read keypair from storage".to_string())),
        }
    }

    fn store_keypair(&self, keypair: &[u8]) -> Result<()> {
        let encoded = STANDARD.encode(keypair);
        self.storage
            .set_item(KEYPAIR_STORAGE_KEY, &encoded)
            .map_err(|_| Error::Crypto("Failed to store keypair".to_string()))
    }

    fn get_stored_did(&self) -> Result<Option<String>> {
        self.storage
            .get_item(DID_STORAGE_KEY)
            .map_err(|_| Error::Crypto("Failed to read DID from storage".to_string()))
    }

    fn store_did(&self, did: &str) -> Result<()> {
        self.storage
            .set_item(DID_STORAGE_KEY, did)
            .map_err(|_| Error::Crypto("Failed to store DID".to_string()))
    }

    fn bytes_to_did(&self, public_key: &[u8]) -> String {
        // Create did:key from public key (simplified)
        // In real implementation, this should use multibase/multicodec encoding
        format!("did:key:z{}", URL_SAFE_NO_PAD.encode(public_key))
    }

    fn did_to_bytes(&self, did: &str) -> Result<Vec<u8>> {
        // Extract public key from did:key
        let key_part = did.strip_prefix("did:key:z")
            .ok_or_else(|| Error::InvalidDid(did.to_string()))?;
        URL_SAFE_NO_PAD.decode(key_part)
            .map_err(|e| Error::InvalidDid(format!("Failed to decode DID: {}", e)))
    }
}

#[async_trait(?Send)]
impl CryptoTrait for WebCrypto {
    async fn generate_keypair(&self) -> Result<String> {
        // Check if keypair already exists
        if let Some(did) = self.get_stored_did()? {
            return Ok(did);
        }

        // Generate Ed25519 keypair using ed25519-dalek
        // Generate random bytes for the secret key
        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes)
            .map_err(|e| Error::Crypto(format!("Failed to generate random bytes: {}", e)))?;
        
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let verifying_key = signing_key.verifying_key();
        
        // Store the keypair (32 bytes secret key)
        self.store_keypair(&secret_bytes)?;
        
        // Create and store DID from public key
        let did = self.bytes_to_did(verifying_key.as_bytes());
        self.store_did(&did)?;
        
        Ok(did)
    }

    async fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Get stored keypair
        let keypair_bytes = self
            .get_stored_keypair()?
            .ok_or_else(|| Error::Crypto("No keypair available".to_string()))?;
        
        // Reconstruct signing key
        let signing_key = SigningKey::from_bytes(
            keypair_bytes.as_slice().try_into()
                .map_err(|_| Error::Crypto("Invalid keypair format".to_string()))?
        );
        
        // Sign the data
        let signature = signing_key.sign(data);
        Ok(signature.to_bytes().to_vec())
    }

    async fn verify(&self, data: &[u8], signature: &[u8], public_key: &str) -> Result<bool> {
        // Extract public key bytes from DID
        let pubkey_bytes = self.did_to_bytes(public_key)?;
        
        // Reconstruct verifying key
        let verifying_key = VerifyingKey::from_bytes(
            pubkey_bytes.as_slice().try_into()
                .map_err(|_| Error::Crypto("Invalid public key format".to_string()))?
        )
        .map_err(|e| Error::Crypto(format!("Failed to create verifying key: {}", e)))?;
        
        // Reconstruct signature
        let sig = Signature::from_bytes(
            signature.try_into()
                .map_err(|_| Error::InvalidSignature)?
        );
        
        // Verify
        Ok(verifying_key.verify(data, &sig).is_ok())
    }

    async fn get_did(&self) -> Result<Option<String>> {
        self.get_stored_did()
    }

    async fn export_keypair(&self) -> Result<Vec<u8>> {
        self.get_stored_keypair()?
            .ok_or_else(|| Error::Crypto("No keypair to export".to_string()))
    }

    async fn import_keypair(&self, data: &[u8]) -> Result<String> {
        // Validate keypair length (32 bytes for Ed25519)
        if data.len() != 32 {
            return Err(Error::Crypto("Invalid keypair length".to_string()));
        }
        
        // Reconstruct keys to validate
        let signing_key = SigningKey::from_bytes(
            data.try_into()
                .map_err(|_| Error::Crypto("Invalid keypair format".to_string()))?
        );
        let verifying_key = signing_key.verifying_key();
        
        // Store keypair
        self.store_keypair(data)?;
        
        // Create and store DID
        let did = self.bytes_to_did(verifying_key.as_bytes());
        self.store_did(&did)?;
        
        Ok(did)
    }
}
