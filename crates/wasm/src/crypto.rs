//! Crypto implementation for WASM
//!
//! For now, we use the Ed25519 implementation from pds-core which works in WASM.
//! A full WebCrypto implementation would be more complex and require different patterns
//! due to async-only APIs and Send/Sync constraints.

// Re-export the Ed25519Crypto from core, which works fine in WASM
pub use pds_core::traits::Ed25519Crypto as WasmCrypto;

#[cfg(test)]
mod tests {
    use super::*;
    use pds_core::traits::Crypto;

    #[test]
    fn test_wasm_crypto_creation() {
        let crypto = WasmCrypto::new();
        let data = b"test message";
        let signature = crypto.sign(data).unwrap();
        let public_key = crypto.public_key();
        assert!(crypto.verify(data, &signature, &public_key).unwrap());
    }
}
