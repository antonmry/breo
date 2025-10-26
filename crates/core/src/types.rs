//! Core data types - placeholder for now, will be populated from main
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::fmt;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

impl Did {
    pub fn new(did: impl Into<String>) -> Result<Self> {
        let did = did.into();
        if !did.starts_with("did:") {
            return Err(Error::InvalidDid(format!("DID must start with 'did:' prefix: {}", did)));
        }
        Ok(Did(did))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for Did {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cid(String);

impl Cid {
    pub fn from_bytes(data: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize();
        let hash_str = hex::encode(hash);
        Cid(format!("bafyrei{}", &hash_str[..52.min(hash_str.len())]))
    }
    pub fn from_string(cid: impl Into<String>) -> Result<Self> {
        let cid = cid.into();
        if !cid.starts_with("bafy") {
            return Err(Error::InvalidCid(format!("Invalid CID format: {}", cid)));
        }
        Ok(Cid(cid))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for Cid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nsid(String);

impl Nsid {
    pub fn new(nsid: impl Into<String>) -> Result<Self> {
        let nsid = nsid.into();
        if !nsid.contains('.') {
            return Err(Error::ValidationError(format!("Invalid NSID format: {}", nsid)));
        }
        Ok(Nsid(nsid))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for Nsid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordKey(String);

impl RecordKey {
    pub fn new(key: impl Into<String>) -> Self {
        RecordKey(key.into())
    }
    pub fn generate() -> Self {
        let now = Utc::now();
        let timestamp = now.timestamp_micros();
        RecordKey(format!("tid_{}", timestamp))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for RecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub collection: Nsid,
    pub rkey: RecordKey,
    pub value: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl Record {
    pub fn new(collection: Nsid, rkey: RecordKey, value: serde_json::Value) -> Self {
        Record { collection, rkey, value, created_at: Utc::now() }
    }
    pub fn validate(&self) -> Result<()> {
        if !self.value.is_object() {
            return Err(Error::ValidationError("Record value must be a JSON object".to_string()));
        }
        Ok(())
    }
    pub fn path(&self) -> String {
        format!("{}/{}", self.collection.as_str(), self.rkey.as_str())
    }
    pub fn cid(&self) -> Result<Cid> {
        let json = serde_json::to_vec(&self.value)?;
        Ok(Cid::from_bytes(&json))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitOp {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub did: Did,
    pub operation: CommitOp,
    pub collection: Nsid,
    pub rkey: RecordKey,
    pub record_cid: Option<Cid>,
    pub prev: Option<Cid>,
    pub timestamp: DateTime<Utc>,
    pub signature: Option<Vec<u8>>,
}

impl Commit {
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

    pub fn cid(&self) -> Result<Cid> {
        let bytes = self.signing_bytes()?;
        Ok(Cid::from_bytes(&bytes))
    }

    pub fn validate(&self) -> Result<()> {
        match self.operation {
            CommitOp::Create | CommitOp::Update => {
                if self.record_cid.is_none() {
                    return Err(Error::InvalidCommit("Create/Update commits must have a record CID".to_string()));
                }
            }
            CommitOp::Delete => {}
        }
        Ok(())
    }
}
