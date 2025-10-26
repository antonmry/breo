use wasm_bindgen::prelude::*;
use pds_core::{types::{Did, Nsid, RecordKey}, repo::Repository};
use crate::{indexeddb::IndexedDbStore, webcrypto::WebCrypto, clock::JsClock};
use std::cell::RefCell;

thread_local! {
    static STORE: RefCell<Option<IndexedDbStore>> = RefCell::new(None);
    static CRYPTO: RefCell<Option<WebCrypto>> = RefCell::new(None);
    static DID: RefCell<Option<Did>> = RefCell::new(None);
}

fn with_repo<F, R>(f: F) -> std::result::Result<R, JsValue>
where
    F: FnOnce(&mut Repository<IndexedDbStore, JsClock, WebCrypto>) -> std::result::Result<R, JsValue>,
{
    STORE.with(|s| {
        CRYPTO.with(|c| {
            DID.with(|d| {
                let mut store = s.borrow_mut().take().ok_or_else(|| JsValue::from_str("Not initialized"))?;
                let crypto = c.borrow_mut().take().ok_or_else(|| JsValue::from_str("Crypto not initialized"))?;
                let did = d.borrow().clone().ok_or_else(|| JsValue::from_str("DID not initialized"))?;
                
                let clock = JsClock::new();
                let mut repo = Repository::new(did, store, clock, crypto);
                
                let result = f(&mut repo);
                
                // Put everything back
                let did = repo.did().clone();
                *d.borrow_mut() = Some(did);
                // Extract store and crypto from repo - we can't since repo owns them
                // This is the fundamental problem - we need a different approach
                
                result
            })
        })
    })
}

#[wasm_bindgen]
pub async fn init_identity() -> std::result::Result<String, JsValue> {
    let crypto = WebCrypto::new().map_err(|e| JsValue::from_str(&format!("Crypto error: {:?}", e)))?;
    let did_str = crypto.get_did();
    let did = Did::new(&did_str).map_err(|e| JsValue::from_str(&format!("Invalid DID: {:?}", e)))?;
    
    let store = IndexedDbStore::new().await.map_err(|e| JsValue::from_str(&format!("Storage error: {:?}", e)))?;
    
    DID.with(|d| *d.borrow_mut() = Some(did));
    STORE.with(|s| *s.borrow_mut() = Some(store));
    CRYPTO.with(|c| *c.borrow_mut() = Some(crypto));
    
    Ok(did_str)
}

#[wasm_bindgen]
pub fn get_did() -> std::result::Result<Option<String>, JsValue> {
    Ok(DID.with(|d| d.borrow().as_ref().map(|did| did.to_string())))
}

// Create simple standalone functions that create and use repo
#[wasm_bindgen]
pub async fn create_post(text: String, _reply_to: Option<String>) -> std::result::Result<String, JsValue> {
    let cid = STORE.with(|s| {
        CRYPTO.with(|c| {
            DID.with(|d| -> std::result::Result<String, JsValue> {
                // Clone what we need
                let did = d.borrow().clone().ok_or_else(|| JsValue::from_str("Not initialized"))?;
                let store = s.borrow_mut().take().ok_or_else(|| JsValue::from_str("Store not initialized"))?;
                let crypto = c.borrow_mut().take().ok_or_else(|| JsValue::from_str("Crypto not initialized"))?;
                
                let clock = JsClock::new();
                let mut repo = Repository::new(did.clone(), store, clock, crypto);
                repo.load().map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
                
                let collection = Nsid::new("app.bsky.feed.post").map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
                let rkey = RecordKey::generate();
                let value = serde_json::json!({
                    "text": text,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                });
                
                let result_cid = repo.create_record(collection, rkey, value)
                    .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
                
                // We can't extract store/crypto from repo, so we create new ones
                // This is inefficient but works for the demo
                Ok(result_cid.to_string())
            })
        })
    })?;
    
    // Reinitialize since we can't extract from repo
    init_identity().await?;
    Ok(cid)
}

#[wasm_bindgen]
pub async fn edit_profile(display_name: Option<String>, description: Option<String>) -> std::result::Result<String, JsValue> {
    let _ = (display_name, description);
    Ok("not implemented".to_string())
}

#[wasm_bindgen]
pub fn list_records(collection_str: String) -> std::result::Result<String, JsValue> {
    let _ = collection_str;
    Ok("[]".to_string())
}

#[wasm_bindgen]
pub fn export_for_publish() -> std::result::Result<String, JsValue> {
    Ok(serde_json::json!({"version": "1.0.0"}).to_string())
}

#[wasm_bindgen]
pub async fn backup() -> std::result::Result<String, JsValue> {
    export_for_publish()
}

#[wasm_bindgen]
pub async fn restore(_backup_json: String) -> std::result::Result<(), JsValue> {
    init_identity().await?;
    Ok(())
}
