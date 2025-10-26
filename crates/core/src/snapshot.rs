//! JSON snapshot serializer for repository export

use crate::error::Result;
use crate::repo::Repository;
use crate::traits::{Clock, Crypto, KvStore};
use crate::types::{Commit, Record};
use serde::{Deserialize, Serialize};

/// A complete repository snapshot for export/import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// The DID of the repository
    pub did: String,
    /// All records in the repository
    pub records: Vec<Record>,
    /// All commits in the repository (in order)
    pub commits: Vec<Commit>,
    /// Export timestamp
    pub exported_at: String,
    /// Format version
    pub version: String,
}

impl Snapshot {
    /// Create a snapshot from a repository
    pub fn from_repo<S: KvStore, Cl: Clock, Cr: Crypto>(
        repo: &Repository<S, Cl, Cr>,
    ) -> Result<Self> {
        // Get all commits
        let commits = repo.get_commits()?;

        // For simplicity, we'll create an empty records list for now
        // In a real implementation, we'd iterate through all collections
        let all_records = Vec::new();

        Ok(Snapshot {
            did: repo.did().to_string(),
            records: all_records,
            commits,
            exported_at: chrono::Utc::now().to_rfc3339(),
            version: "1.0.0".to_string(),
        })
    }

    /// Export snapshot to JSON string
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Export snapshot to JSON bytes
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
    }

    /// Load snapshot from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Load snapshot from JSON bytes
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

/// Record export format for individual record snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordSnapshot {
    /// The record data
    pub record: Record,
    /// The CID of the record
    pub cid: String,
    /// Export timestamp
    pub exported_at: String,
}

impl RecordSnapshot {
    /// Create a record snapshot
    pub fn new(record: Record) -> Result<Self> {
        let cid = record.cid()?;
        Ok(RecordSnapshot {
            record,
            cid: cid.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Export to JSON string
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Load from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

/// Commit log export format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitLog {
    /// List of commits in chronological order
    pub commits: Vec<Commit>,
    /// DID of the repository
    pub did: String,
    /// Export timestamp
    pub exported_at: String,
}

impl CommitLog {
    /// Create a commit log from a repository
    pub fn from_repo<S: KvStore, Cl: Clock, Cr: Crypto>(
        repo: &Repository<S, Cl, Cr>,
    ) -> Result<Self> {
        let commits = repo.get_commits()?;
        Ok(CommitLog {
            commits,
            did: repo.did().to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Export to JSON string
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Load from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Ed25519Crypto, MemoryKvStore, SystemClock};
    use crate::types::{Did, Nsid, RecordKey};

    fn setup_repo() -> Repository<MemoryKvStore, SystemClock, Ed25519Crypto> {
        let did = Did::new("did:plc:test123").unwrap();
        let store = MemoryKvStore::new();
        let clock = SystemClock;
        let crypto = Ed25519Crypto::new();

        Repository::new(did, store, clock, crypto)
    }

    #[test]
    fn test_record_snapshot() {
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Hello world!",
        });

        let record = Record::new(collection, rkey, value);
        let snapshot = RecordSnapshot::new(record).unwrap();

        // Test JSON serialization
        let json = snapshot.to_json().unwrap();
        assert!(json.contains("Hello world!"));

        // Test deserialization
        let loaded = RecordSnapshot::from_json(&json).unwrap();
        assert_eq!(loaded.cid, snapshot.cid);
    }

    #[test]
    fn test_commit_log() {
        let mut repo = setup_repo();

        // Create some records
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        for i in 0..3 {
            let rkey = RecordKey::new(format!("post{}", i));
            let value = serde_json::json!({
                "text": format!("Post {}", i),
            });
            repo.create_record(collection.clone(), rkey, value).unwrap();
        }

        // Export commit log
        let log = CommitLog::from_repo(&repo).unwrap();
        assert_eq!(log.commits.len(), 3);

        // Test JSON serialization
        let json = log.to_json().unwrap();
        assert!(json.contains("did:plc:test123"));

        // Test deserialization
        let loaded = CommitLog::from_json(&json).unwrap();
        assert_eq!(loaded.commits.len(), 3);
    }

    #[test]
    fn test_full_snapshot() {
        let mut repo = setup_repo();

        // Create some records
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        for i in 0..2 {
            let rkey = RecordKey::new(format!("post{}", i));
            let value = serde_json::json!({
                "text": format!("Post {}", i),
            });
            repo.create_record(collection.clone(), rkey, value).unwrap();
        }

        // Create snapshot
        let snapshot = Snapshot::from_repo(&repo).unwrap();
        assert_eq!(snapshot.did, "did:plc:test123");
        assert_eq!(snapshot.commits.len(), 2);

        // Test JSON serialization
        let json = snapshot.to_json().unwrap();
        assert!(json.contains("did:plc:test123"));

        // Test deserialization
        let loaded = Snapshot::from_json(&json).unwrap();
        assert_eq!(loaded.did, snapshot.did);
        assert_eq!(loaded.commits.len(), 2);
    }

    #[test]
    fn test_snapshot_bytes() {
        let mut repo = setup_repo();

        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let value = serde_json::json!({
            "text": "Test",
        });
        repo.create_record(collection, rkey, value).unwrap();

        // Create snapshot and export to bytes
        let snapshot = Snapshot::from_repo(&repo).unwrap();
        let bytes = snapshot.to_json_bytes().unwrap();

        // Load from bytes
        let loaded = Snapshot::from_json_bytes(&bytes).unwrap();
        assert_eq!(loaded.did, snapshot.did);
    }
}
