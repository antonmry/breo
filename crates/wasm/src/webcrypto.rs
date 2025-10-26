use pds_core::{Error, Result};
use pds_core::traits::Crypto;
use wasm_bindgen::prelude::*;
use web_sys::window;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use base64::{Engine as _, engine::general_purpose::STANDARD};

const KEYPAIR_STORAGE_KEY: &str = "pds_keypair";

/// WebCrypto-based cryptographic operations (synchronous, uses localStorage)
/// 
/// SAFETY: This type is marked as Send + Sync even though it contains web_sys types.
/// This is safe because WASM is single-threaded, so Send + Sync are effectively no-ops.
pub struct WebCrypto {
    signing_key: SigningKey,
}

// SAFETY: WASM is single-threaded, so this is safe
unsafe impl Send for WebCrypto {}
unsafe impl Sync for WebCrypto {}

impl WebCrypto {
    pub fn new() -> Result<Self> {
        let window = window().ok_or_else(|| Error::CryptoError("No window object".to_string()))?;
        let storage = window
            .local_storage()
            .map_err(|_| Error::CryptoError("Failed to access localStorage".to_string()))?
            .ok_or_else(|| Error::CryptoError("localStorage not available".to_string()))?;
        
        // Try to load existing keypair or generate new one
        let signing_key = if let Ok(Some(stored)) = storage.get_item(KEYPAIR_STORAGE_KEY) {
            // Load existing keypair
            let bytes = STANDARD.decode(&stored)
                .map_err(|e| Error::CryptoError(format!("Failed to decode keypair: {}", e)))?;
            let key_bytes: [u8; 32] = bytes.try_into()
                .map_err(|_| Error::CryptoError("Invalid keypair length".to_string()))?;
            SigningKey::from_bytes(&key_bytes)
        } else {
            // Generate new keypair
            let mut seed = [0u8; 32];
            getrandom::getrandom(&mut seed)
                .map_err(|e| Error::CryptoError(format!("Failed to generate random bytes: {}", e)))?;
            let signing_key = SigningKey::from_bytes(&seed);
            
            // Store it
            let encoded = STANDARD.encode(&seed);
            storage.set_item(KEYPAIR_STORAGE_KEY, &encoded)
                .map_err(|_| Error::CryptoError("Failed to store keypair".to_string()))?;
            
            signing_key
        };
        
        Ok(Self { signing_key })
    }

    pub fn get_did(&self) -> String {
        let public_key = self.signing_key.verifying_key().to_bytes();
        format!("did:key:z{}", STANDARD.encode(public_key).replace("=", "").replace("+", "-").replace("/", "_"))
    }
}

impl Crypto for WebCrypto {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let signature = self.signing_key.sign(data);
        Ok(signature.to_bytes().to_vec())
    }

    fn verify(&self, data: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool> {
        let verifying_key = VerifyingKey::from_bytes(
            public_key.try_into()
                .map_err(|_| Error::CryptoError("Invalid public key length".to_string()))?
        )
        .map_err(|e| Error::CryptoError(format!("Failed to create verifying key: {}", e)))?;
        
        let sig = Signature::from_bytes(
            signature.try_into()
                .map_err(|_| Error::CryptoError("Invalid signature length".to_string()))?
        );
        
        Ok(verifying_key.verify(data, &sig).is_ok())
    }

    fn public_key(&self) -> Vec<u8> {
        self.signing_key.verifying_key().to_bytes().to_vec()
    }
}
