//! Snapshot serialization - minimal version
use crate::error::Result;
use crate::repo::Repository;
use crate::traits::{Clock, Crypto, KvStore};
use crate::types::{Commit, Record};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub did: String,
    pub records: Vec<Record>,
    pub commits: Vec<Commit>,
    pub exported_at: String,
    pub version: String,
}

impl Snapshot {
    pub fn from_repo<S: KvStore, Cl: Clock, Cr: Crypto>(repo: &Repository<S, Cl, Cr>) -> Result<Self> {
        let commits = repo.get_commits()?;
        let all_records = Vec::new();
        Ok(Snapshot {
            did: repo.did().to_string(),
            records: all_records,
            commits,
            exported_at: chrono::Utc::now().to_rfc3339(),
            version: "1.0.0".to_string(),
        })
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}
