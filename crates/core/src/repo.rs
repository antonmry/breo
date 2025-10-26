//! Repository implementation - minimal version for WASM compatibility
use crate::error::{Error, Result};
use crate::traits::{Clock, Crypto, KvStore};
use crate::types::{Cid, Commit, CommitOp, Did, Nsid, Record, RecordKey};
use std::collections::HashMap;

pub struct Repository<S: KvStore, Cl: Clock, Cr: Crypto> {
    did: Did,
    store: S,
    _clock: Cl,
    crypto: Cr,
    head: Option<Cid>,
    records: HashMap<String, Record>,
}

impl<S: KvStore, Cl: Clock, Cr: Crypto> Repository<S, Cl, Cr> {
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

    pub fn load(&mut self) -> Result<()> {
        if let Some(head_bytes) = self.store.get("head")? {
            let head_str = String::from_utf8(head_bytes).map_err(|e| Error::StorageError(e.to_string()))?;
            self.head = Some(Cid::from_string(head_str)?);
        }
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

    pub fn get_head(&self) -> Option<&Cid> {
        self.head.as_ref()
    }

    pub fn did(&self) -> &Did {
        &self.did
    }

    pub fn create_record(&mut self, collection: Nsid, rkey: RecordKey, value: serde_json::Value) -> Result<Cid> {
        let record = Record::new(collection.clone(), rkey.clone(), value);
        record.validate()?;
        let record_cid = record.cid()?;
        let path = record.path();
        if self.records.contains_key(&path) {
            return Err(Error::ValidationError(format!("Record already exists: {}", path)));
        }
        let commit = self.create_commit(CommitOp::Create, collection, rkey, Some(record_cid.clone()))?;
        self.store_record(&record)?;
        self.store_commit(&commit)?;
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;
        self.records.insert(path, record);
        Ok(record_cid)
    }

    pub fn update_record(&mut self, collection: Nsid, rkey: RecordKey, value: serde_json::Value) -> Result<Cid> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());
        if !self.records.contains_key(&path) {
            return Err(Error::NotFound(format!("Record not found: {}", path)));
        }
        let record = Record::new(collection.clone(), rkey.clone(), value);
        record.validate()?;
        let record_cid = record.cid()?;
        let commit = self.create_commit(CommitOp::Update, collection, rkey, Some(record_cid.clone()))?;
        self.store_record(&record)?;
        self.store_commit(&commit)?;
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;
        self.records.insert(path, record);
        Ok(record_cid)
    }

    pub fn delete_record(&mut self, collection: Nsid, rkey: RecordKey) -> Result<()> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());
        if !self.records.contains_key(&path) {
            return Err(Error::NotFound(format!("Record not found: {}", path)));
        }
        let commit = self.create_commit(CommitOp::Delete, collection, rkey, None)?;
        self.store_commit(&commit)?;
        self.head = Some(commit.cid()?);
        self.store.put("head", self.head.as_ref().unwrap().as_str().as_bytes())?;
        self.store.delete(&format!("record:{}", path))?;
        self.records.remove(&path);
        Ok(())
    }

    pub fn get_record(&self, collection: &Nsid, rkey: &RecordKey) -> Option<&Record> {
        let path = format!("{}/{}", collection.as_str(), rkey.as_str());
        self.records.get(&path)
    }

    pub fn list_records(&self, collection: &Nsid) -> Vec<&Record> {
        self.records.values().filter(|r| &r.collection == collection).collect()
    }

    pub fn get_commits(&self) -> Result<Vec<Commit>> {
        let commit_keys = self.store.list_keys("commit:")?;
        let mut commits = Vec::new();
        for key in commit_keys {
            if let Some(commit_bytes) = self.store.get(&key)? {
                let commit: Commit = serde_json::from_slice(&commit_bytes)?;
                commits.push(commit);
            }
        }
        commits.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Ok(commits)
    }

    fn create_commit(&mut self, operation: CommitOp, collection: Nsid, rkey: RecordKey, record_cid: Option<Cid>) -> Result<Commit> {
        let mut commit = Commit::new(self.did.clone(), operation, collection, rkey, record_cid, self.head.clone());
        commit.validate()?;
        let signing_bytes = commit.signing_bytes()?;
        let signature = self.crypto.sign(&signing_bytes)?;
        commit.signature = Some(signature);
        Ok(commit)
    }

    fn store_record(&mut self, record: &Record) -> Result<()> {
        let key = format!("record:{}", record.path());
        let value = serde_json::to_vec(record)?;
        self.store.put(&key, &value)?;
        Ok(())
    }

    fn store_commit(&mut self, commit: &Commit) -> Result<()> {
        let cid = commit.cid()?;
        let key = format!("commit:{}", cid.as_str());
        let value = serde_json::to_vec(commit)?;
        self.store.put(&key, &value)?;
        Ok(())
    }

    pub fn verify_commit(&self, commit: &Commit) -> Result<bool> {
        let signature = commit.signature.as_ref().ok_or_else(|| Error::InvalidCommit("Commit has no signature".to_string()))?;
        let signing_bytes = commit.signing_bytes()?;
        let public_key = self.crypto.public_key();
        self.crypto.verify(&signing_bytes, signature, &public_key)
    }
}
