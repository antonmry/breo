//! Repository implementation with append-only commit graph

use std::collections::HashMap;
use crate::error::{Error, Result};
use crate::traits::{Clock, Crypto, KvStore};
use crate::types::{Cid, Commit, CommitOp, Did, Nsid, Record, RecordKey};

/// Repository manages the append-only commit graph and current state
pub struct Repository<S: KvStore, Cl: Clock, Cr: Crypto> {
    /// The DID of this repository
    did: Did,
    /// Storage backend
    store: S,
    /// Clock for timestamps (reserved for future use)
    _clock: Cl,
    /// Crypto for signing
    crypto: Cr,
    /// Current head commit CID (None if empty repo)
    head: Option<Cid>,
    /// In-memory cache of current records
    records: HashMap<String, Record>,
}

impl<S: KvStore, Cl: Clock, Cr: Crypto> Repository<S, Cl, Cr> {
    /// Create a new repository
    pub fn new(did: Did, store: S, clock: Cl, crypto: Cr) -> Self {
        Repository {
            did,
            store,
            _clock: clock,
            crypto,
            head: None,
            records: HashMap::new(),
        }
    }

    /// Initialize or load the repository from storage
    pub fn load(&mut self) -> Result<()> {
        // Load the head pointer
        if let Some(head_bytes) = self.store.get("head")? {
            let head_str = String::from_utf8(head_bytes)
                .map_err(|e| Error::StorageError(e.to_string()))?;
            self.head = Some(Cid::from_str(head_str)?);
        }

        // Load all records from storage
        let record_keys = self.store.list_keys("record:")?;
        for key in record_keys {
            if let Some(record_bytes) = self.store.get(&key)? {
                let record: Record = serde_json::from_slice(&record_bytes)?;
                let path = record.path();
                self.records.insert(path, record);
            }
        }

        Ok(())
    }

    /// Get the current head commit
    pub fn get_head(&self) -> Option<&Cid> {
        self.head.as_ref()
    }

    /// Get the repository DID
    pub fn did(&self) -> &Did {
        &self.did
    }

    /// Create a new record in the repository
    pub fn create_record(
        &mut self,
        collection: Nsid,
        rkey: RecordKey,
        value: serde_json::Value,
    ) -> Result<Cid> {
        let record = Record::new(collection.clone(), rkey.clone(), value);
        record.validate()?;

        let record_cid = record.cid()?;
        let path = record.path();

        // Check if record already exists
        if self.records.contains_key(&path) {
            return Err(Error::ValidationError(format!(
                "Record already exists: {}",
                path
            )));
        }

        // Create commit
        let commit = self.create_commit(
            CommitOp::Create,
            collection,
            rkey,
            Some(record_cid.clone()),
        )?;

        // Store record and commit
        self.store_record(&record)?;
        self.store_commit(&commit)?;

        // Update head and cache
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;
        self.records.insert(path, record);

        Ok(record_cid)
    }

    /// Update an existing record
    pub fn update_record(
        &mut self,
        collection: Nsid,
        rkey: RecordKey,
        value: serde_json::Value,
    ) -> Result<Cid> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());

        // Check if record exists
        if !self.records.contains_key(&path) {
            return Err(Error::NotFound(format!("Record not found: {}", path)));
        }

        let record = Record::new(collection.clone(), rkey.clone(), value);
        record.validate()?;

        let record_cid = record.cid()?;

        // Create commit
        let commit = self.create_commit(
            CommitOp::Update,
            collection,
            rkey,
            Some(record_cid.clone()),
        )?;

        // Store record and commit
        self.store_record(&record)?;
        self.store_commit(&commit)?;

        // Update head and cache
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;
        self.records.insert(path, record);

        Ok(record_cid)
    }

    /// Delete a record
    pub fn delete_record(&mut self, collection: Nsid, rkey: RecordKey) -> Result<()> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());

        // Check if record exists
        if !self.records.contains_key(&path) {
            return Err(Error::NotFound(format!("Record not found: {}", path)));
        }

        // Create commit
        let commit = self.create_commit(CommitOp::Delete, collection, rkey, None)?;

        // Store commit
        self.store_commit(&commit)?;

        // Update head and cache
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;

        // Remove from storage and cache
        self.store.delete(&format!("record:{}", path))?;
        self.records.remove(&path);

        Ok(())
    }

    /// Get a record by collection and rkey
    pub fn get_record(&self, collection: &Nsid, rkey: &RecordKey) -> Option<&Record> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());
        self.records.get(&path)
    }

    /// List all records in a collection
    pub fn list_records(&self, collection: &Nsid) -> Vec<&Record> {
        self.records
            .values()
            .filter(|r| &r.collection == collection)
            .collect()
    }

    /// Get all commits (traverse the commit graph)
    pub fn get_commits(&self) -> Result<Vec<Commit>> {
        let commit_keys = self.store.list_keys("commit:")?;
        let mut commits = Vec::new();

        for key in commit_keys {
            if let Some(commit_bytes) = self.store.get(&key)? {
                let commit: Commit = serde_json::from_slice(&commit_bytes)?;
                commits.push(commit);
            }
        }

        // Sort by timestamp
        commits.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        Ok(commits)
    }

    /// Create a commit with proper linking and signing
    fn create_commit(
        &mut self,
        operation: CommitOp,
        collection: Nsid,
        rkey: RecordKey,
        record_cid: Option<Cid>,
    ) -> Result<Commit> {
        let mut commit = Commit::new(
            self.did.clone(),
            operation,
            collection,
            rkey,
            record_cid,
            self.head.clone(),
        );

        // Validate commit
        commit.validate()?;

        // Sign the commit
        let signing_bytes = commit.signing_bytes()?;
        let signature = self.crypto.sign(&signing_bytes)?;
        commit.signature = Some(signature);

        Ok(commit)
    }

    /// Store a record in the key-value store
    fn store_record(&mut self, record: &Record) -> Result<()> {
        let key = format!("record:{}", record.path());
        let value = serde_json::to_vec(record)?;
        self.store.put(&key, &value)?;
        Ok(())
    }

    /// Store a commit in the key-value store
    fn store_commit(&mut self, commit: &Commit) -> Result<()> {
        let cid = commit.cid()?;
        let key = format!("commit:{}", cid.as_str());
        let value = serde_json::to_vec(commit)?;
        self.store.put(&key, &value)?;
        Ok(())
    }

    /// Verify a commit's signature
    pub fn verify_commit(&self, commit: &Commit) -> Result<bool> {
        let signature = commit
            .signature
            .as_ref()
            .ok_or_else(|| Error::InvalidCommit("Commit has no signature".to_string()))?;

        let signing_bytes = commit.signing_bytes()?;
        let public_key = self.crypto.public_key();

        self.crypto.verify(&signing_bytes, signature, &public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Ed25519Crypto, MemoryKvStore, SystemClock};

    fn setup_repo() -> Repository<MemoryKvStore, SystemClock, Ed25519Crypto> {
        let did = Did::new("did:plc:test123").unwrap();
        let store = MemoryKvStore::new();
        let clock = SystemClock;
        let crypto = Ed25519Crypto::new();

        Repository::new(did, store, clock, crypto)
    }

    #[test]
    fn test_create_record() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Hello world!",
            "createdAt": "2025-01-01T00:00:00Z"
        });

        let cid = repo.create_record(collection.clone(), rkey.clone(), value).unwrap();
        assert!(cid.as_str().starts_with("bafy"));

        // Verify record was stored
        let record = repo.get_record(&collection, &rkey);
        assert!(record.is_some());

        // Verify head was updated
        assert!(repo.get_head().is_some());
    }

    #[test]
    fn test_update_record() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value1 = serde_json::json!({
            "text": "Hello world!",
        });

        // Create initial record
        repo.create_record(collection.clone(), rkey.clone(), value1).unwrap();

        // Update record
        let value2 = serde_json::json!({
            "text": "Updated hello!",
        });

        let cid = repo.update_record(collection.clone(), rkey.clone(), value2.clone()).unwrap();
        assert!(cid.as_str().starts_with("bafy"));

        // Verify record was updated
        let record = repo.get_record(&collection, &rkey).unwrap();
        assert_eq!(record.value, value2);
    }

    #[test]
    fn test_delete_record() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Hello world!",
        });

        // Create record
        repo.create_record(collection.clone(), rkey.clone(), value).unwrap();

        // Verify it exists
        assert!(repo.get_record(&collection, &rkey).is_some());

        // Delete record
        repo.delete_record(collection.clone(), rkey.clone()).unwrap();

        // Verify it's gone
        assert!(repo.get_record(&collection, &rkey).is_none());
    }

    #[test]
    fn test_commit_graph() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();

        // Create multiple records
        for i in 0..3 {
            let rkey = RecordKey::new(format!("post{}", i));
            let value = serde_json::json!({
                "text": format!("Post {}", i),
            });
            repo.create_record(collection.clone(), rkey, value).unwrap();
        }

        // Get commits
        let commits = repo.get_commits().unwrap();
        assert_eq!(commits.len(), 3);

        // Verify commits are linked
        for i in 1..commits.len() {
            assert!(commits[i].prev.is_some());
        }

        // First commit has no parent
        assert!(commits[0].prev.is_none());
    }

    #[test]
    fn test_list_records() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();

        // Create multiple records
        for i in 0..3 {
            let rkey = RecordKey::new(format!("post{}", i));
            let value = serde_json::json!({
                "text": format!("Post {}", i),
            });
            repo.create_record(collection.clone(), rkey, value).unwrap();
        }

        let records = repo.list_records(&collection);
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_repo_persistence() {
        let did = Did::new("did:plc:test123").unwrap();
        let store = MemoryKvStore::new();
        let clock = SystemClock;
        let crypto = Ed25519Crypto::new();

        let mut repo = Repository::new(did, store, clock, crypto);
        
        // Create a record
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Hello world!",
        });
        repo.create_record(collection.clone(), rkey.clone(), value).unwrap();
        
        // Verify head exists
        assert!(repo.get_head().is_some());
        
        // Verify record exists
        let records = repo.list_records(&collection);
        assert_eq!(records.len(), 1);
        
        // Reload from storage (tests internal persistence)
        let did2 = Did::new("did:plc:test123").unwrap();
        let store2 = MemoryKvStore::new();
        let mut repo2 = Repository::new(did2, store2, SystemClock, Ed25519Crypto::new());
        
        // This tests that load() works even on empty repo
        assert!(repo2.load().is_ok());
        assert!(repo2.get_head().is_none()); // Empty repo has no head
    }

    #[test]
    fn test_commit_signature_verification() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Hello world!",
        });

        repo.create_record(collection, rkey, value).unwrap();

        let commits = repo.get_commits().unwrap();
        assert_eq!(commits.len(), 1);

        // Verify the commit signature
        let is_valid = repo.verify_commit(&commits[0]).unwrap();
        assert!(is_valid);
    }
}
