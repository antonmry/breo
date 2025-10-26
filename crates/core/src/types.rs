use serde::{Deserialize, Serialize};

/// DID identifier (e.g., did:key:z6Mk...)
pub type Did = String;

/// Record key (e.g., app.bsky.feed.post/3k2a4b...)
pub type RecordKey = String;

/// Collection name (e.g., app.bsky.feed.post)
pub type Collection = String;

/// Record URI (e.g., at://did:key:z6Mk.../app.bsky.feed.post/3k2a4b...)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtUri {
    pub did: Did,
    pub collection: Collection,
    pub rkey: RecordKey,
}

impl AtUri {
    pub fn new(did: Did, collection: Collection, rkey: RecordKey) -> Self {
        Self { did, collection, rkey }
    }
    
    pub fn to_string(&self) -> String {
        format!("at://{}/{}/{}", self.did, self.collection, self.rkey)
    }
}

/// A commit in the repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub did: Did,
    pub version: u64,
    pub prev: Option<String>, // CID of previous commit
    pub data: Vec<u8>,        // Serialized operations
    pub sig: Vec<u8>,         // Signature
    pub timestamp: u64,
    #[serde(skip)]
    pub cid: String,          // Computed CID (not serialized, computed on load)
}

/// A record in the repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub uri: AtUri,
    pub cid: String,
    pub value: serde_json::Value,
    pub timestamp: u64,
}

/// Profile record structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(rename = "$type")]
    pub type_: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            type_: "app.bsky.actor.profile".to_string(),
            display_name: None,
            description: None,
            avatar: None,
            banner: None,
        }
    }
}

/// Post record structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    #[serde(rename = "$type")]
    pub type_: String,
    pub text: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<ReplyRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyRef {
    pub root: StrongRef,
    pub parent: StrongRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

/// Backup format
#[derive(Debug, Serialize, Deserialize)]
pub struct Backup {
    pub version: String,
    pub did: Did,
    pub keypair: Vec<u8>,
    pub commits: Vec<Commit>,
    pub records: Vec<Record>,
    pub timestamp: u64,
}
