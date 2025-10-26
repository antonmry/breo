use serde::{Deserialize, Serialize};
use crate::types::*;

/// Represents different record operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordOp {
    Create {
        collection: Collection,
        rkey: RecordKey,
        value: serde_json::Value,
    },
    Update {
        collection: Collection,
        rkey: RecordKey,
        value: serde_json::Value,
    },
    Delete {
        collection: Collection,
        rkey: RecordKey,
    },
}

/// Record storage keys
pub mod keys {
    pub const IDENTITY_KEY: &str = "identity";
    pub const COMMITS_PREFIX: &str = "commits/";
    pub const RECORDS_PREFIX: &str = "records/";
    
    pub fn commit_key(version: u64) -> String {
        format!("{}{}", COMMITS_PREFIX, version)
    }
    
    pub fn record_key(collection: &str, rkey: &str) -> String {
        format!("{}{}/{}", RECORDS_PREFIX, collection, rkey)
    }
    
    pub fn collection_prefix(collection: &str) -> String {
        format!("{}{}/", RECORDS_PREFIX, collection)
    }
}
