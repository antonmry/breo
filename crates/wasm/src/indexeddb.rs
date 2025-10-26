use pds_core::{KvStore as KvStoreTrait, Error, Result};
use async_trait::async_trait;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, IdbFactory, IdbDatabase, IdbObjectStore, IdbTransaction, IdbRequest, IdbOpenDbRequest, IdbTransactionMode, IdbCursorWithValue, IdbKeyRange, Event};
use js_sys::{Uint8Array, Array, Promise};
use std::sync::Arc;
use std::cell::RefCell;

const DB_NAME: &str = "pds_store";
const DB_VERSION: u32 = 1;
const STORE_NAME: &str = "kvstore";

/// IndexedDB-based key-value store
pub struct IndexedDbStore {
    db: Arc<RefCell<Option<IdbDatabase>>>,
}

impl IndexedDbStore {
    pub async fn new() -> Result<Self> {
        let store = Self {
            db: Arc::new(RefCell::new(None)),
        };
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<()> {
        let window = window().ok_or_else(|| Error::Storage("No window object".to_string()))?;
        
        let idb_factory = window
            .indexed_db()
            .map_err(|_| Error::Storage("Failed to get IndexedDB".to_string()))?
            .ok_or_else(|| Error::Storage("IndexedDB not available".to_string()))?;

        let open_request = idb_factory
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(|e| Error::Storage(format!("Failed to open database: {:?}", e)))?;

        // Setup upgrade handler - create object store if needed
        let onupgradeneeded = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let target = event.target().unwrap();
            let request = target.dyn_ref::<IdbOpenDbRequest>().unwrap();
            let db = request.result().unwrap().dyn_into::<IdbDatabase>().unwrap();
            
            // Try to create object store (will fail silently if exists)
            let _ = db.create_object_store(STORE_NAME);
        }) as Box<dyn FnMut(_)>);

        open_request.set_onupgradeneeded(Some(onupgradeneeded.as_ref().unchecked_ref()));
        onupgradeneeded.forget();

        // Wait for database to open
        let promise = Self::request_to_promise(&open_request);
        let db = JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Failed to open database: {:?}", e)))?
            .dyn_into::<IdbDatabase>()
            .map_err(|_| Error::Storage("Invalid database object".to_string()))?;

        *self.db.borrow_mut() = Some(db);
        Ok(())
    }

    fn get_db(&self) -> Result<IdbDatabase> {
        self.db
            .borrow()
            .as_ref()
            .ok_or_else(|| Error::Storage("Database not initialized".to_string()))
            .map(|db| db.clone())
    }

    fn get_transaction(&self, mode: IdbTransactionMode) -> Result<IdbTransaction> {
        let db = self.get_db()?;
        db.transaction_with_str_and_mode(STORE_NAME, mode)
            .map_err(|e| Error::Storage(format!("Failed to create transaction: {:?}", e)))
    }

    fn get_store(&self, transaction: &IdbTransaction) -> Result<IdbObjectStore> {
        transaction
            .object_store(STORE_NAME)
            .map_err(|e| Error::Storage(format!("Failed to get object store: {:?}", e)))
    }

    // Helper to convert IdbRequest to Promise
    fn request_to_promise(request: &IdbRequest) -> Promise {
        Promise::new(&mut |resolve, reject| {
            let onsuccess = Closure::wrap(Box::new(move |event: Event| {
                let target = event.target().unwrap();
                let request = target.dyn_ref::<IdbRequest>().unwrap();
                let result = request.result().unwrap();
                resolve.call1(&JsValue::NULL, &result).unwrap();
            }) as Box<dyn FnMut(_)>);

            let onerror = Closure::wrap(Box::new(move |_event: Event| {
                reject.call0(&JsValue::NULL).unwrap();
            }) as Box<dyn FnMut(_)>);

            request.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
            request.set_onerror(Some(onerror.as_ref().unchecked_ref()));

            onsuccess.forget();
            onerror.forget();
        })
    }
}

#[async_trait(?Send)]
impl KvStoreTrait for IndexedDbStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let transaction = self.get_transaction(IdbTransactionMode::Readonly)?;
        let store = self.get_store(&transaction)?;
        
        let request = store
            .get(&JsValue::from_str(key))
            .map_err(|e| Error::Storage(format!("Failed to get value: {:?}", e)))?;

        let promise = Self::request_to_promise(&request);
        let result = JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Get operation failed: {:?}", e)))?;

        if result.is_undefined() || result.is_null() {
            return Ok(None);
        }

        // Convert JsValue to Vec<u8>
        let array = Uint8Array::new(&result);
        let mut bytes = vec![0u8; array.length() as usize];
        array.copy_to(&mut bytes);
        Ok(Some(bytes))
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<()> {
        let transaction = self.get_transaction(IdbTransactionMode::Readwrite)?;
        let store = self.get_store(&transaction)?;
        
        // Convert Vec<u8> to Uint8Array
        let array = Uint8Array::new_with_length(value.len() as u32);
        array.copy_from(&value);

        let request = store
            .put_with_key(&array, &JsValue::from_str(key))
            .map_err(|e| Error::Storage(format!("Failed to put value: {:?}", e)))?;

        let promise = Self::request_to_promise(&request);
        JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Put operation failed: {:?}", e)))?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let transaction = self.get_transaction(IdbTransactionMode::Readwrite)?;
        let store = self.get_store(&transaction)?;
        
        let request = store
            .delete(&JsValue::from_str(key))
            .map_err(|e| Error::Storage(format!("Failed to delete value: {:?}", e)))?;

        let promise = Self::request_to_promise(&request);
        JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Delete operation failed: {:?}", e)))?;

        Ok(())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let transaction = self.get_transaction(IdbTransactionMode::Readonly)?;
        let store = self.get_store(&transaction)?;
        
        // Get all keys from the store
        let request = store.get_all_keys()
            .map_err(|e| Error::Storage(format!("Failed to get all keys: {:?}", e)))?;

        let promise = Self::request_to_promise(&request);
        let result = JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Get all keys operation failed: {:?}", e)))?;

        // Convert JSArray to Vec<String>
        let js_array = js_sys::Array::from(&result);
        let mut keys = Vec::new();
        
        for i in 0..js_array.length() {
            if let Some(key_val) = js_array.get(i).as_string() {
                if prefix.is_empty() || key_val.starts_with(prefix) {
                    keys.push(key_val);
                }
            }
        }

        Ok(keys)
    }

    async fn clear(&self) -> Result<()> {
        let transaction = self.get_transaction(IdbTransactionMode::Readwrite)?;
        let store = self.get_store(&transaction)?;
        
        let request = store
            .clear()
            .map_err(|e| Error::Storage(format!("Failed to clear store: {:?}", e)))?;

        let promise = Self::request_to_promise(&request);
        JsFuture::from(promise)
            .await
            .map_err(|e| Error::Storage(format!("Clear operation failed: {:?}", e)))?;

        Ok(())
    }
}
