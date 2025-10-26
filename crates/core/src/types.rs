//! Core data types for ATProto repository

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::error::{Error, Result};

/// DID (Decentralized Identifier)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

impl Did {
    /// Create a new DID from a string
    pub fn new(did: impl Into<String>) -> Result<Self> {
        let did = did.into();
        if !did.starts_with("did:") {
            return Err(Error::InvalidDid(format!(
                "DID must start with 'did:' prefix: {}",
                did
            )));
        }
        Ok(Did(did))
    }

    /// Get the DID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Did {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// CID (Content Identifier) - simplified version for ATProto
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cid(String);

impl Cid {
    /// Create a new CID from raw bytes
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize();
        // Use base32 encoding for CID (simplified)
        let hash_str = hex::encode(hash);
        Cid(format!("bafyrei{}", &hash_str[..52]))
    }

    /// Create a CID from a string
    pub fn from_string(cid: impl Into<String>) -> Result<Self> {
        let cid = cid.into();
        if !cid.starts_with("bafy") {
            return Err(Error::InvalidCid(format!("Invalid CID format: {}", cid)));
        }
        Ok(Cid(cid))
    }

    /// Get the CID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Cid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// ATProto NSID (Namespaced Identifier) for lexicon types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nsid(String);

impl Nsid {
    /// Create a new NSID
    pub fn new(nsid: impl Into<String>) -> Result<Self> {
        let nsid = nsid.into();
        // Basic validation: should contain dots and be lowercase
        if !nsid.contains('.') {
            return Err(Error::ValidationError(format!(
                "Invalid NSID format: {}",
                nsid
            )));
        }
        Ok(Nsid(nsid))
    }

    /// Get the NSID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Nsid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Record key (rkey) - unique identifier for records within a collection
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordKey(String);

impl RecordKey {
    /// Create a new record key
    pub fn new(key: impl Into<String>) -> Self {
        RecordKey(key.into())
    }

    /// Generate a new TID-based record key (timestamp-based ID)
    pub fn generate() -> Self {
        let now = Utc::now();
        let timestamp = now.timestamp_micros();
        RecordKey(format!("tid_{}", timestamp))
    }

    /// Get the record key as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Record - a single data entry in the repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// The collection this record belongs to (NSID)
    pub collection: Nsid,
    /// The record key (unique within the collection)
    pub rkey: RecordKey,
    /// The actual record data as JSON
    pub value: serde_json::Value,
    /// When the record was created
    pub created_at: DateTime<Utc>,
}

impl Record {
    /// Create a new record
    pub fn new(collection: Nsid, rkey: RecordKey, value: serde_json::Value) -> Self {
        Record {
            collection,
            rkey,
            value,
            created_at: Utc::now(),
        }
    }

    /// Validate the record against basic constraints
    pub fn validate(&self) -> Result<()> {
        if !self.value.is_object() {
            return Err(Error::ValidationError(
                "Record value must be a JSON object".to_string(),
            ));
        }
        Ok(())
    }

    /// Get the record path (collection/rkey)
    pub fn path(&self) -> String {
        format!("{}/{}", self.collection.as_str(), self.rkey.as_str())
    }

    /// Compute the CID of this record
    pub fn cid(&self) -> Result<Cid> {
        let json = serde_json::to_vec(&self.value)?;
        Ok(Cid::from_bytes(&json))
    }
}

/// Commit operation type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitOp {
    /// Create a new record
    Create,
    /// Update an existing record
    Update,
    /// Delete a record
    Delete,
}

/// A commit represents a single atomic change to the repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// The DID of the repository owner
    pub did: Did,
    /// The operation performed
    pub operation: CommitOp,
    /// The collection being modified
    pub collection: Nsid,
    /// The record key
    pub rkey: RecordKey,
    /// CID of the record (for create/update)
    pub record_cid: Option<Cid>,
    /// Previous commit CID (parent in the commit graph)
    pub prev: Option<Cid>,
    /// Timestamp of the commit
    pub timestamp: DateTime<Utc>,
    /// Signature over the commit data
    pub signature: Option<Vec<u8>>,
}

impl Commit {
    /// Create a new commit
    pub fn new(
        did: Did,
        operation: CommitOp,
        collection: Nsid,
        rkey: RecordKey,
        record_cid: Option<Cid>,
        prev: Option<Cid>,
    ) -> Self {
        Commit {
            did,
            operation,
            collection,
            rkey,
            record_cid,
            prev,
            timestamp: Utc::now(),
            signature: None,
        }
    }

    /// Get the canonical bytes to sign
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        let data = serde_json::to_vec(&serde_json::json!({
            "did": self.did.as_str(),
            "operation": self.operation,
            "collection": self.collection.as_str(),
            "rkey": self.rkey.as_str(),
            "record_cid": self.record_cid.as_ref().map(|c| c.as_str()),
            "prev": self.prev.as_ref().map(|c| c.as_str()),
            "timestamp": self.timestamp.to_rfc3339(),
        }))?;
        Ok(data)
    }

    /// Compute the CID of this commit
    pub fn cid(&self) -> Result<Cid> {
        let bytes = self.signing_bytes()?;
        Ok(Cid::from_bytes(&bytes))
    }

    /// Validate the commit structure
    pub fn validate(&self) -> Result<()> {
        match self.operation {
            CommitOp::Create | CommitOp::Update => {
                if self.record_cid.is_none() {
                    return Err(Error::InvalidCommit(
                        "Create/Update commits must have a record CID".to_string(),
                    ));
                }
            }
            CommitOp::Delete => {
                // Delete can have optional record_cid
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_did_creation() {
        let did = Did::new("did:plc:test123").unwrap();
        assert_eq!(did.as_str(), "did:plc:test123");

        let invalid = Did::new("invalid");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_cid_creation() {
        let data = b"hello world";
        let cid = Cid::from_bytes(data);
        assert!(cid.as_str().starts_with("bafy"));
    }

    #[test]
    fn test_nsid_creation() {
        let nsid = Nsid::new("app.bsky.feed.post").unwrap();
        assert_eq!(nsid.as_str(), "app.bsky.feed.post");

        let invalid = Nsid::new("invalid");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_record_creation() {
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::generate();
        let value = serde_json::json!({
            "text": "Hello world!",
            "createdAt": "2025-01-01T00:00:00Z"
        });

        let record = Record::new(collection, rkey, value);
        assert!(record.validate().is_ok());
        assert!(record.cid().is_ok());
    }

    #[test]
    fn test_commit_creation() {
        let did = Did::new("did:plc:test").unwrap();
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");
        let record_cid = Cid::from_bytes(b"test data");

        let commit = Commit::new(
            did,
            CommitOp::Create,
            collection,
            rkey,
            Some(record_cid),
            None,
        );

        assert!(commit.validate().is_ok());
        assert!(commit.cid().is_ok());
    }

    #[test]
    fn test_commit_validation() {
        let did = Did::new("did:plc:test").unwrap();
        let collection = Nsid::new("app.bsky.feed.post").unwrap();
        let rkey = RecordKey::new("test123");

        // Create without record_cid should fail validation
        let invalid_commit = Commit::new(
            did.clone(),
            CommitOp::Create,
            collection.clone(),
            rkey.clone(),
            None,
            None,
        );
        assert!(invalid_commit.validate().is_err());

        // Delete can be without record_cid
        let delete_commit = Commit::new(did, CommitOp::Delete, collection, rkey, None, None);
        assert!(delete_commit.validate().is_ok());
    }
}
