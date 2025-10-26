use pds_core::{Error, Result};
use pds_core::traits::KvStore;
use std::collections::HashMap;
use std::cell::RefCell;

/// IndexedDB-based key-value store using cache for synchronous access
/// 
/// SAFETY: This type is marked as Send + Sync even though it uses RefCell.
/// This is safe because WASM is single-threaded.
pub struct IndexedDbStore {
    cache: RefCell<HashMap<String, Vec<u8>>>,
}

// SAFETY: WASM is single-threaded, so this is safe
unsafe impl Send for IndexedDbStore {}
unsafe impl Sync for IndexedDbStore {}

impl IndexedDbStore {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub async fn flush(&self) -> Result<()> {
        // TODO: Implement actual IndexedDB flush
        // For now just keep everything in memory
        Ok(())
    }
}

impl KvStore for IndexedDbStore {
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.cache.borrow_mut().insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.cache.borrow().get(key).cloned())
    }

    fn delete(&mut self, key: &str) -> Result<()> {
        self.cache.borrow_mut().remove(key);
        Ok(())
    }

    fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.cache.borrow().contains_key(key))
    }

    fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let cache = self.cache.borrow();
        let keys: Vec<String> = cache
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        Ok(keys)
    }
}
