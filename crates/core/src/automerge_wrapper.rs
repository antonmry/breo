//! Automerge wrapper - minimal stub for now
use crate::error::Result;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AutomergeDoc {
    cached_json: Option<Value>,
}

impl AutomergeDoc {
    pub fn new() -> Self {
        AutomergeDoc {
            cached_json: Some(serde_json::json!({})),
        }
    }

    pub fn from_json(value: &Value) -> Result<Self> {
        Ok(AutomergeDoc {
            cached_json: Some(value.clone()),
        })
    }

    pub fn to_json(&self) -> Result<Value> {
        Ok(self.cached_json.clone().unwrap_or(serde_json::json!({})))
    }

    pub fn update(&mut self, value: &Value) -> Result<()> {
        self.cached_json = Some(value.clone());
        Ok(())
    }
}

impl Default for AutomergeDoc {
    fn default() -> Self {
        Self::new()
    }
}
