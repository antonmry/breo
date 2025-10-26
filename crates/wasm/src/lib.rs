//! # PDS WASM
//!
//! Browser-based ATProto Personal Data Server implementation
//!
//! This crate provides WASM bindings for the PDS core library, allowing
//! the repository to run entirely in the browser with IndexedDB persistence
//! and WebCrypto signing.

mod clock;
mod crypto;
mod error;
mod storage;

use wasm_bindgen::prelude::*;

pub use clock::JsClock;
pub use crypto::WasmCrypto;
pub use error::{Result, WasmError};
pub use storage::IndexedDbStore;

// Re-export for convenience
use pds_core::{
    repo::Repository,
    types::{Did, Nsid, RecordKey},
};

/// Initialize the WASM module (call this when the module loads)
#[wasm_bindgen(start)]
pub fn init() {
    // Panic hook can be added as an optional feature later
}

/// Repository handle for WASM
#[wasm_bindgen]
pub struct WasmRepository {
    repo: Option<Repository<IndexedDbStore, JsClock, WasmCrypto>>,
    store: Option<IndexedDbStore>,
    crypto: Option<WasmCrypto>,
    did: Option<Did>,
}

#[wasm_bindgen]
impl WasmRepository {
    /// Create a new repository instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            repo: None,
            store: None,
            crypto: None,
            did: None,
        }
    }

    /// Initialize a new identity with a DID
    /// Returns the DID string
    #[wasm_bindgen]
    pub async fn init_identity(&mut self, did_str: String) -> std::result::Result<String, JsValue> {
        // Parse DID
        let did =
            Did::new(did_str).map_err(|e| JsValue::from_str(&format!("Invalid DID: {}", e)))?;

        // Create storage
        let mut store = IndexedDbStore::new("pds-repo");
        store
            .init()
            .await
            .map_err(|e| JsValue::from_str(&format!("Storage init failed: {}", e)))?;

        // Create crypto
        let crypto = WasmCrypto::new();

        // Create repository
        let clock = JsClock::new();
        let mut repo = Repository::new(did.clone(), store.clone(), clock, crypto.clone());

        // Load existing data
        repo.load()
            .map_err(|e| JsValue::from_str(&format!("Failed to load repo: {}", e)))?;

        self.did = Some(did.clone());
        self.store = Some(store);
        self.crypto = Some(crypto);
        self.repo = Some(repo);

        Ok(did.to_string())
    }

    /// Create a new post record
    /// Returns the CID of the created record
    #[wasm_bindgen]
    pub async fn create_post(&mut self, text: String) -> std::result::Result<String, JsValue> {
        let repo = self
            .repo
            .as_mut()
            .ok_or_else(|| JsValue::from_str("Repository not initialized"))?;

        let collection = Nsid::new("app.bsky.feed.post")
            .map_err(|e| JsValue::from_str(&format!("Invalid collection: {}", e)))?;

        // Generate a random rkey
        let rkey = RecordKey::new(format!("post_{}", js_sys::Date::now() as u64));

        let value = serde_json::json!({
            "text": text,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });

        let cid = repo
            .create_record(collection, rkey, value)
            .map_err(|e| JsValue::from_str(&format!("Failed to create post: {}", e)))?;

        // Flush to storage
        if let Some(store) = self.store.as_mut() {
            store
                .flush()
                .await
                .map_err(|e| JsValue::from_str(&format!("Failed to flush: {}", e)))?;
        }

        Ok(cid.to_string())
    }

    /// Edit profile record
    /// Returns the CID of the updated profile
    #[wasm_bindgen]
    pub async fn edit_profile(
        &mut self,
        display_name: String,
        description: String,
    ) -> std::result::Result<String, JsValue> {
        let repo = self
            .repo
            .as_mut()
            .ok_or_else(|| JsValue::from_str("Repository not initialized"))?;

        let collection = Nsid::new("app.bsky.actor.profile")
            .map_err(|e| JsValue::from_str(&format!("Invalid collection: {}", e)))?;

        let rkey = RecordKey::new("self");

        let value = serde_json::json!({
            "displayName": display_name,
            "description": description,
        });

        // Check if profile exists, create or update accordingly
        let cid = if repo.get_record(&collection, &rkey).is_some() {
            repo.update_record(collection, rkey, value)
        } else {
            repo.create_record(collection, rkey, value)
        }
        .map_err(|e| JsValue::from_str(&format!("Failed to edit profile: {}", e)))?;

        // Flush to storage
        if let Some(store) = self.store.as_mut() {
            store
                .flush()
                .await
                .map_err(|e| JsValue::from_str(&format!("Failed to flush: {}", e)))?;
        }

        Ok(cid.to_string())
    }

    /// List all records in a collection
    /// Returns a JSON string of records
    #[wasm_bindgen]
    pub fn list_records(&self, collection_str: String) -> std::result::Result<String, JsValue> {
        let repo = self
            .repo
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Repository not initialized"))?;

        let collection = Nsid::new(collection_str)
            .map_err(|e| JsValue::from_str(&format!("Invalid collection: {}", e)))?;

        let records = repo.list_records(&collection);

        let records_json: Vec<serde_json::Value> = records
            .iter()
            .map(|r| {
                serde_json::json!({
                    "collection": r.collection.to_string(),
                    "rkey": r.rkey.to_string(),
                    "value": r.value,
                })
            })
            .collect();

        serde_json::to_string(&records_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize: {}", e)))
    }

    /// Export repository snapshot for publishing
    /// Returns JSON string of the snapshot
    #[wasm_bindgen]
    pub fn export_for_publish(&self) -> std::result::Result<String, JsValue> {
        let repo = self
            .repo
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Repository not initialized"))?;

        let snapshot = pds_core::snapshot::Snapshot::from_repo(repo)
            .map_err(|e| JsValue::from_str(&format!("Failed to create snapshot: {}", e)))?;

        snapshot
            .to_json()
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize snapshot: {}", e)))
    }

    /// Create a backup of the repository
    /// Returns JSON string of the backup
    #[wasm_bindgen]
    pub fn backup(&self) -> std::result::Result<String, JsValue> {
        self.export_for_publish()
    }

    /// Restore repository from a backup
    /// Takes a JSON string of the backup
    #[wasm_bindgen]
    pub async fn restore(&mut self, backup_json: String) -> std::result::Result<(), JsValue> {
        // Parse the snapshot
        let snapshot = pds_core::snapshot::Snapshot::from_json(&backup_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse backup: {}", e)))?;

        // Re-initialize with the DID from backup
        self.init_identity(snapshot.did.clone()).await?;

        // TODO: Restore records and commits from snapshot
        // This would involve iterating through the snapshot and recreating records

        Ok(())
    }

    /// Get the repository DID
    #[wasm_bindgen]
    pub fn get_did(&self) -> Option<String> {
        self.did.as_ref().map(|d| d.to_string())
    }

    /// Get the public key as base64
    #[wasm_bindgen]
    pub fn get_public_key(&self) -> Option<String> {
        self.crypto.as_ref().map(|c| {
            use base64::{engine::general_purpose, Engine as _};
            use pds_core::traits::Crypto;
            general_purpose::STANDARD.encode(c.public_key())
        })
    }
}

impl Default for WasmRepository {
    fn default() -> Self {
        Self::new()
    }
}
