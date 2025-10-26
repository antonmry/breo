use crate::{
    error::Result,
    records::{keys, RecordOp},
    traits::{Clock, Crypto, KvStore},
    types::*,
};
use sha2::{Digest, Sha256};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::Arc;

/// The main repository manager
pub struct Repo<K, C, T>
where
    K: KvStore,
    C: Crypto,
    T: Clock,
{
    store: Arc<K>,
    crypto: Arc<C>,
    clock: Arc<T>,
}

impl<K, C, T> Repo<K, C, T>
where
    K: KvStore,
    C: Crypto,
    T: Clock,
{
    pub fn new(store: Arc<K>, crypto: Arc<C>, clock: Arc<T>) -> Self {
        Self {
            store,
            crypto,
            clock,
        }
    }

    /// Initialize a new identity (DID + keypair)
    pub async fn init_identity(&self) -> Result<Did> {
        // Check if identity already exists
        if let Some(existing) = self.get_identity().await? {
            return Ok(existing);
        }

        // Generate new keypair and get DID
        let did = self.crypto.generate_keypair().await?;

        // Store identity
        self.store
            .set(keys::IDENTITY_KEY, did.as_bytes().to_vec())
            .await?;

        Ok(did)
    }

    /// Get the current identity DID
    pub async fn get_identity(&self) -> Result<Option<Did>> {
        match self.store.get(keys::IDENTITY_KEY).await? {
            Some(data) => Ok(Some(String::from_utf8_lossy(&data).to_string())),
            None => Ok(None),
        }
    }

    /// Create a new record
    pub async fn create_record(
        &self,
        collection: Collection,
        rkey: RecordKey,
        value: serde_json::Value,
    ) -> Result<Record> {
        let did = self
            .get_identity()
            .await?
            .ok_or_else(|| crate::Error::InvalidOperation("No identity initialized".to_string()))?;

        let timestamp = self.clock.now();
        let uri = AtUri::new(did.clone(), collection.clone(), rkey.clone());

        // Create record
        let record = Record {
            uri: uri.clone(),
            cid: self.compute_cid(&value)?,
            value,
            timestamp,
        };

        // Store record
        let key = keys::record_key(&collection, &rkey);
        let data = serde_json::to_vec(&record)?;
        self.store.set(&key, data).await?;

        // Create commit
        let op = RecordOp::Create {
            collection,
            rkey,
            value: record.value.clone(),
        };
        self.create_commit(op).await?;

        Ok(record)
    }

    /// Update an existing record (for mutable records like profile)
    pub async fn update_record(
        &self,
        collection: Collection,
        rkey: RecordKey,
        value: serde_json::Value,
    ) -> Result<Record> {
        let did = self
            .get_identity()
            .await?
            .ok_or_else(|| crate::Error::InvalidOperation("No identity initialized".to_string()))?;

        let timestamp = self.clock.now();
        let uri = AtUri::new(did.clone(), collection.clone(), rkey.clone());

        // Create updated record
        let record = Record {
            uri: uri.clone(),
            cid: self.compute_cid(&value)?,
            value,
            timestamp,
        };

        // Store record
        let key = keys::record_key(&collection, &rkey);
        let data = serde_json::to_vec(&record)?;
        self.store.set(&key, data).await?;

        // Create commit
        let op = RecordOp::Update {
            collection,
            rkey,
            value: record.value.clone(),
        };
        self.create_commit(op).await?;

        Ok(record)
    }

    /// Get a record by collection and rkey
    pub async fn get_record(&self, collection: &str, rkey: &str) -> Result<Option<Record>> {
        let key = keys::record_key(collection, rkey);
        match self.store.get(&key).await? {
            Some(data) => {
                let record: Record = serde_json::from_slice(&data)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// List all records in a collection
    pub async fn list_records(&self, collection: &str) -> Result<Vec<Record>> {
        let prefix = keys::collection_prefix(collection);
        let keys = self.store.list_keys(&prefix).await?;

        let mut records = Vec::new();
        for key in keys {
            if let Some(data) = self.store.get(&key).await? {
                if let Ok(record) = serde_json::from_slice::<Record>(&data) {
                    records.push(record);
                }
            }
        }

        // Sort by timestamp (newest first)
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(records)
    }

    /// Delete a record
    pub async fn delete_record(&self, collection: Collection, rkey: RecordKey) -> Result<()> {
        let key = keys::record_key(&collection, &rkey);
        self.store.delete(&key).await?;

        // Create commit
        let op = RecordOp::Delete { collection, rkey };
        self.create_commit(op).await?;

        Ok(())
    }

    /// Create a commit for an operation
    async fn create_commit(&self, op: RecordOp) -> Result<Commit> {
        let did = self
            .get_identity()
            .await?
            .ok_or_else(|| crate::Error::InvalidOperation("No identity initialized".to_string()))?;

        // Get the latest commit version
        let version = self.get_latest_version().await? + 1;

        // Get previous commit CID if exists
        let prev = if version > 1 {
            self.get_commit(version - 1).await?.map(|c| c.cid)
        } else {
            None
        };

        // Serialize operation
        let data = serde_json::to_vec(&op)?;

        // Sign the commit data
        let sig = self.crypto.sign(&data).await?;

        let timestamp = self.clock.now();

        let mut commit = Commit {
            did,
            version,
            prev,
            data: data.clone(),
            sig,
            timestamp,
            cid: String::new(), // Will be computed below
        };

        // Compute CID from commit (excluding the CID field itself)
        let commit_for_cid = serde_json::json!({
            "did": commit.did,
            "version": commit.version,
            "prev": commit.prev,
            "data": commit.data,
            "sig": commit.sig,
            "timestamp": commit.timestamp,
        });
        commit.cid = self.compute_cid(&commit_for_cid)?;

        // Store commit
        let key = keys::commit_key(version);
        let commit_data = serde_json::to_vec(&commit)?;
        self.store.set(&key, commit_data).await?;

        Ok(commit)
    }

    /// Get latest commit version
    async fn get_latest_version(&self) -> Result<u64> {
        let keys = self.store.list_keys(keys::COMMITS_PREFIX).await?;
        let mut max_version = 0u64;

        for key in keys {
            if let Some(version_str) = key.strip_prefix(keys::COMMITS_PREFIX) {
                if let Ok(version) = version_str.parse::<u64>() {
                    max_version = max_version.max(version);
                }
            }
        }

        Ok(max_version)
    }

    /// Get a specific commit
    async fn get_commit(&self, version: u64) -> Result<Option<Commit>> {
        let key = keys::commit_key(version);
        match self.store.get(&key).await? {
            Some(data) => {
                let mut commit: Commit = serde_json::from_slice(&data)?;
                // Recompute CID
                let commit_for_cid = serde_json::json!({
                    "did": commit.did,
                    "version": commit.version,
                    "prev": commit.prev,
                    "data": commit.data,
                    "sig": commit.sig,
                    "timestamp": commit.timestamp,
                });
                commit.cid = self.compute_cid(&commit_for_cid)?;
                Ok(Some(commit))
            }
            None => Ok(None),
        }
    }

    /// Compute CID (simplified version using SHA-256)
    fn compute_cid(&self, value: &serde_json::Value) -> Result<String> {
        let json = serde_json::to_vec(value)?;
        let hash = Sha256::digest(&json);
        Ok(format!("bafyrei{}", URL_SAFE_NO_PAD.encode(hash)))
    }

    /// Export data for backup
    pub async fn backup(&self) -> Result<Backup> {
        let did = self
            .get_identity()
            .await?
            .ok_or_else(|| crate::Error::InvalidOperation("No identity initialized".to_string()))?;

        // Export keypair
        let keypair = self.crypto.export_keypair().await?;

        // Get all commits
        let commit_keys = self.store.list_keys(keys::COMMITS_PREFIX).await?;
        let mut commits = Vec::new();
        for key in commit_keys {
            if let Some(data) = self.store.get(&key).await? {
                if let Ok(commit) = serde_json::from_slice::<Commit>(&data) {
                    commits.push(commit);
                }
            }
        }

        // Get all records
        let record_keys = self.store.list_keys(keys::RECORDS_PREFIX).await?;
        let mut records = Vec::new();
        for key in record_keys {
            if let Some(data) = self.store.get(&key).await? {
                if let Ok(record) = serde_json::from_slice::<Record>(&data) {
                    records.push(record);
                }
            }
        }

        Ok(Backup {
            version: "1.0".to_string(),
            did,
            keypair,
            commits,
            records,
            timestamp: self.clock.now(),
        })
    }

    /// Restore from backup
    pub async fn restore(&self, backup: Backup) -> Result<()> {
        // Clear existing data
        self.store.clear().await?;

        // Import keypair
        self.crypto.import_keypair(&backup.keypair).await?;

        // Store identity
        self.store
            .set(keys::IDENTITY_KEY, backup.did.as_bytes().to_vec())
            .await?;

        // Restore commits
        for commit in backup.commits {
            let key = keys::commit_key(commit.version);
            let data = serde_json::to_vec(&commit)?;
            self.store.set(&key, data).await?;
        }

        // Restore records
        for record in backup.records {
            let key = keys::record_key(&record.uri.collection, &record.uri.rkey);
            let data = serde_json::to_vec(&record)?;
            self.store.set(&key, data).await?;
        }

        Ok(())
    }

    /// Export records for publishing to external PDS
    pub async fn export_for_publish(&self) -> Result<Vec<Record>> {
        // Get all records
        let record_keys = self.store.list_keys(keys::RECORDS_PREFIX).await?;
        let mut records = Vec::new();
        for key in record_keys {
            if let Some(data) = self.store.get(&key).await? {
                if let Ok(record) = serde_json::from_slice::<Record>(&data) {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // Mock implementations for testing
    struct MockKvStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockKvStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl KvStore for MockKvStore {
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }

        async fn set(&self, key: &str, value: Vec<u8>) -> Result<()> {
            self.data.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        async fn delete(&self, key: &str) -> Result<()> {
            self.data.lock().unwrap().remove(key);
            Ok(())
        }

        async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }

        async fn clear(&self) -> Result<()> {
            self.data.lock().unwrap().clear();
            Ok(())
        }
    }

    struct MockCrypto {
        did: Mutex<Option<String>>,
    }

    impl MockCrypto {
        fn new() -> Self {
            Self {
                did: Mutex::new(None),
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl Crypto for MockCrypto {
        async fn generate_keypair(&self) -> Result<String> {
            let did = "did:key:z6MkTest123".to_string();
            *self.did.lock().unwrap() = Some(did.clone());
            Ok(did)
        }

        async fn sign(&self, _data: &[u8]) -> Result<Vec<u8>> {
            Ok(vec![0u8; 64])
        }

        async fn verify(&self, _data: &[u8], _signature: &[u8], _public_key: &str) -> Result<bool> {
            Ok(true)
        }

        async fn get_did(&self) -> Result<Option<String>> {
            Ok(self.did.lock().unwrap().clone())
        }

        async fn export_keypair(&self) -> Result<Vec<u8>> {
            Ok(vec![0u8; 32])
        }

        async fn import_keypair(&self, _data: &[u8]) -> Result<String> {
            let did = "did:key:z6MkTest123".to_string();
            *self.did.lock().unwrap() = Some(did.clone());
            Ok(did)
        }
    }

    struct MockClock;

    impl Clock for MockClock {
        fn now(&self) -> u64 {
            1234567890000
        }
    }

    #[tokio::test]
    async fn test_init_identity() {
        let store = Arc::new(MockKvStore::new());
        let crypto = Arc::new(MockCrypto::new());
        let clock = Arc::new(MockClock);
        let repo = Repo::new(store, crypto, clock);

        let did = repo.init_identity().await.unwrap();
        assert_eq!(did, "did:key:z6MkTest123");

        // Should return same DID on second call
        let did2 = repo.init_identity().await.unwrap();
        assert_eq!(did, did2);
    }

    #[tokio::test]
    async fn test_create_and_get_record() {
        let store = Arc::new(MockKvStore::new());
        let crypto = Arc::new(MockCrypto::new());
        let clock = Arc::new(MockClock);
        let repo = Repo::new(store, crypto, clock);

        repo.init_identity().await.unwrap();

        let value = serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": "Hello World!",
            "created_at": "2025-01-01T00:00:00Z"
        });

        let record = repo
            .create_record("app.bsky.feed.post".to_string(), "test123".to_string(), value)
            .await
            .unwrap();

        assert_eq!(record.uri.collection, "app.bsky.feed.post");
        assert_eq!(record.uri.rkey, "test123");

        let fetched = repo
            .get_record("app.bsky.feed.post", "test123")
            .await
            .unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().value["text"], "Hello World!");
    }

    #[tokio::test]
    async fn test_list_records() {
        let store = Arc::new(MockKvStore::new());
        let crypto = Arc::new(MockCrypto::new());
        let clock = Arc::new(MockClock);
        let repo = Repo::new(store, crypto, clock);

        repo.init_identity().await.unwrap();

        // Create multiple posts
        for i in 1..=3 {
            let value = serde_json::json!({
                "$type": "app.bsky.feed.post",
                "text": format!("Post {}", i),
                "created_at": "2025-01-01T00:00:00Z"
            });
            repo.create_record("app.bsky.feed.post".to_string(), format!("post{}", i), value)
                .await
                .unwrap();
        }

        let records = repo.list_records("app.bsky.feed.post").await.unwrap();
        assert_eq!(records.len(), 3);
    }
}
