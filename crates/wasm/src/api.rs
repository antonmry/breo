use crate::{indexeddb::IndexedDbStore, webcrypto::WebCrypto, clock::Clock};
use pds_core::{Repo, Profile, Post, Record, Backup};
use wasm_bindgen::prelude::*;
use std::sync::Arc;
use serde_json::json;

/// Initialize the PDS with a new or existing identity
#[wasm_bindgen]
pub async fn init_identity() -> Result<String, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    let did = repo.init_identity().await.map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(did)
}

/// Create a new post
#[wasm_bindgen]
pub async fn create_post(text: String, reply_to: Option<String>) -> Result<String, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    // Generate a random record key (timestamp-based)
    let rkey = format!("{}", js_sys::Date::now() as u64);
    
    // Create post value
    let now = js_sys::Date::new_0();
    let created_at = now.to_iso_string().as_string().unwrap();
    
    let mut post_value = json!({
        "$type": "app.bsky.feed.post",
        "text": text,
        "createdAt": created_at,
    });
    
    // Add reply info if provided
    if let Some(reply_uri) = reply_to {
        // In a real implementation, we'd parse the URI and get the CID
        post_value["reply"] = json!({
            "root": { "uri": reply_uri, "cid": "bafyreifake" },
            "parent": { "uri": reply_uri, "cid": "bafyreifake" }
        });
    }
    
    let record = repo
        .create_record("app.bsky.feed.post".to_string(), rkey, post_value)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(record.uri.to_string())
}

/// Update profile information
#[wasm_bindgen]
pub async fn edit_profile(
    display_name: Option<String>,
    description: Option<String>,
) -> Result<String, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    // Profile always uses "self" as the record key
    let rkey = "self".to_string();
    
    // Check if profile exists
    let existing = repo.get_record("app.bsky.actor.profile", &rkey).await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    // Merge with existing profile or create new
    let profile = if let Some(existing_record) = existing {
        let mut existing_profile: Profile = serde_json::from_value(existing_record.value)
            .unwrap_or_default();
        
        if let Some(name) = display_name {
            existing_profile.display_name = Some(name);
        }
        if let Some(desc) = description {
            existing_profile.description = Some(desc);
        }
        existing_profile
    } else {
        Profile {
            type_: "app.bsky.actor.profile".to_string(),
            display_name,
            description,
            avatar: None,
            banner: None,
        }
    };
    
    let profile_value = serde_json::to_value(&profile)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    let record = repo
        .update_record("app.bsky.actor.profile".to_string(), rkey, profile_value)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(record.uri.to_string())
}

/// List all records in a collection
#[wasm_bindgen]
pub async fn list_records(collection: String) -> Result<JsValue, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    let records = repo.list_records(&collection).await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    // Convert to JSON
    let json = serde_json::to_string(&records)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(JsValue::from_str(&json))
}

/// Export all records for publishing to external PDS
#[wasm_bindgen]
pub async fn export_for_publish() -> Result<JsValue, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    let records = repo.export_for_publish().await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    // Convert to JSON
    let json = serde_json::to_string(&records)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(JsValue::from_str(&json))
}

/// Create a backup of all data
#[wasm_bindgen]
pub async fn backup() -> Result<JsValue, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    let backup_data = repo.backup().await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    // Convert to JSON
    let json = serde_json::to_string(&backup_data)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(JsValue::from_str(&json))
}

/// Restore from a backup
#[wasm_bindgen]
pub async fn restore(backup_json: String) -> Result<(), JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    let backup_data: Backup = serde_json::from_str(&backup_json)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    repo.restore(backup_data).await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    
    Ok(())
}

/// Get current identity DID
#[wasm_bindgen]
pub async fn get_did() -> Result<JsValue, JsValue> {
    let store = Arc::new(IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&e.to_string()))?);
    let crypto = Arc::new(WebCrypto::new().map_err(|e| JsValue::from_str(&e.to_string()))?);
    let clock = Arc::new(Clock::new());
    
    let repo = Repo::new(store, crypto, clock);
    
    match repo.get_identity().await.map_err(|e| JsValue::from_str(&e.to_string()))? {
        Some(did) => Ok(JsValue::from_str(&did)),
        None => Ok(JsValue::NULL),
    }
}
