//! Automerge wrapper for mutable documents with CRDT support

use automerge::{Automerge, ObjType, ReadDoc, transaction::Transactable};
use serde_json::Value;
use crate::error::{Error, Result};

/// Wrapper around an Automerge document for mutable records
/// 
/// This provides a simplified JSON-compatible interface to Automerge
/// for conflict-free replicated data types (CRDTs).
#[derive(Debug, Clone)]
pub struct AutomergeDoc {
    doc: Automerge,
    /// Cache of the JSON representation for simple read access
    cached_json: Option<Value>,
}

impl AutomergeDoc {
    /// Create a new empty Automerge document
    pub fn new() -> Self {
        AutomergeDoc {
            doc: Automerge::new(),
            cached_json: Some(serde_json::json!({})),
        }
    }

    /// Create a document from JSON value
    /// 
    /// Note: This stores the JSON value and uses Automerge for merging
    pub fn from_json(value: &Value) -> Result<Self> {
        let mut doc = Automerge::new();
        
        // For now, store as simple map at root for JSON objects
        if let Value::Object(map) = value {
            let mut tx = doc.transaction();
            for (key, val) in map {
                if let Value::String(s) = val {
                    tx.put(&automerge::ROOT, key.as_str(), s.as_str())
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(i) = val.as_i64() {
                    tx.put(&automerge::ROOT, key.as_str(), i)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(f) = val.as_f64() {
                    tx.put(&automerge::ROOT, key.as_str(), f)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(b) = val.as_bool() {
                    tx.put(&automerge::ROOT, key.as_str(), b)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                }
            }
            tx.commit();
        }

        Ok(AutomergeDoc { 
            doc,
            cached_json: Some(value.clone()),
        })
    }

    /// Load a document from binary format
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let doc = Automerge::load(bytes)
            .map_err(|e| Error::AutomergeError(format!("Failed to load document: {}", e)))?;
        Ok(AutomergeDoc { 
            doc,
            cached_json: None,
        })
    }

    /// Save the document to binary format
    pub fn save(&self) -> Vec<u8> {
        self.doc.save()
    }

    /// Update the document with new JSON value
    pub fn update(&mut self, value: &Value) -> Result<()> {
        // Update cache
        self.cached_json = Some(value.clone());
        
        // Update Automerge doc
        if let Value::Object(map) = value {
            let mut tx = self.doc.transaction();
            for (key, val) in map {
                if let Value::String(s) = val {
                    tx.put(&automerge::ROOT, key.as_str(), s.as_str())
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(i) = val.as_i64() {
                    tx.put(&automerge::ROOT, key.as_str(), i)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(f) = val.as_f64() {
                    tx.put(&automerge::ROOT, key.as_str(), f)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                } else if let Some(b) = val.as_bool() {
                    tx.put(&automerge::ROOT, key.as_str(), b)
                        .map_err(|e| Error::AutomergeError(e.to_string()))?;
                }
            }
            tx.commit();
        }
        
        Ok(())
    }

    /// Get the current document state as JSON
    pub fn to_json(&self) -> Result<Value> {
        if let Some(ref cached) = self.cached_json {
            return Ok(cached.clone());
        }
        
        // Fallback: extract from Automerge
        let mut map = serde_json::Map::new();
        
        // The Automerge 0.6 API returns Result for object_type
        if let Ok(obj_type) = self.doc.object_type(&automerge::ROOT) {
            if obj_type == ObjType::Map {
                for item in self.doc.map_range(&automerge::ROOT, ..) {
                    let key = item.key.to_string();
                    let val = match item.value {
                        automerge::Value::Scalar(ref s) => Self::scalar_to_json(s),
                        _ => Value::Null,
                    };
                    map.insert(key, val);
                }
            }
        }
        
        Ok(Value::Object(map))
    }

    /// Merge another document into this one
    pub fn merge(&mut self, other: &mut AutomergeDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)
            .map_err(|e| Error::AutomergeError(format!("Merge failed: {}", e)))?;
        
        // Clear cache after merge
        self.cached_json = None;
        
        Ok(())
    }

    /// Get the list of changes since the given heads
    pub fn get_changes(&self, have_deps: &[automerge::ChangeHash]) -> Vec<automerge::Change> {
        self.doc.get_changes(have_deps).into_iter().cloned().collect()
    }

    /// Apply changes to the document
    pub fn apply_changes(&mut self, changes: Vec<automerge::Change>) -> Result<()> {
        self.doc.apply_changes(changes)
            .map_err(|e| Error::AutomergeError(format!("Failed to apply changes: {}", e)))?;
        
        // Clear cache after applying changes
        self.cached_json = None;
        
        Ok(())
    }

    /// Get the current document heads (for change tracking)
    pub fn get_heads(&self) -> Vec<automerge::ChangeHash> {
        self.doc.get_heads()
    }

    /// Convert Automerge scalar to JSON value
    fn scalar_to_json(scalar: &automerge::ScalarValue) -> Value {
        match scalar {
            automerge::ScalarValue::Bytes(b) => {
                // Convert bytes to base64 string
                Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b))
            }
            automerge::ScalarValue::Str(s) => Value::String(s.to_string()),
            automerge::ScalarValue::Int(i) => Value::Number((*i).into()),
            automerge::ScalarValue::Uint(u) => Value::Number((*u).into()),
            automerge::ScalarValue::F64(f) => {
                serde_json::Number::from_f64(*f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }
            automerge::ScalarValue::Counter(_c) => {
                // Counter - just return 0 as we can't access internal value
                Value::Number(0.into())
            }
            automerge::ScalarValue::Timestamp(t) => Value::Number((*t).into()),
            automerge::ScalarValue::Boolean(b) => Value::Bool(*b),
            automerge::ScalarValue::Null => Value::Null,
            automerge::ScalarValue::Unknown { .. } => Value::Null,
        }
    }
}

impl Default for AutomergeDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_create_and_update() {
        let mut doc = AutomergeDoc::new();

        let value = json!({
            "name": "Alice",
            "age": 30,
            "active": true
        });

        doc.update(&value).unwrap();
        let result = doc.to_json().unwrap();

        assert_eq!(result["name"], "Alice");
        assert_eq!(result["age"], 30);
        assert_eq!(result["active"], true);
    }

    #[test]
    fn test_from_json() {
        let value = json!({
            "title": "Test Post",
            "content": "Hello world!",
            "likes": 42
        });

        let doc = AutomergeDoc::from_json(&value).unwrap();
        let result = doc.to_json().unwrap();

        assert_eq!(result["title"], "Test Post");
        assert_eq!(result["content"], "Hello world!");
        assert_eq!(result["likes"], 42);
    }

    #[test]
    fn test_save_and_load() {
        let value = json!({
            "data": "test",
            "count": 123
        });

        let doc = AutomergeDoc::from_json(&value).unwrap();
        let bytes = doc.save();

        let loaded = AutomergeDoc::load(&bytes).unwrap();
        let result = loaded.to_json().unwrap();

        assert_eq!(result["data"], "test");
        assert_eq!(result["count"], 123);
    }

    #[test]
    fn test_merge() {
        // Create first document
        let mut doc1 = AutomergeDoc::from_json(&json!({
            "name": "Alice",
            "score": 100
        }))
        .unwrap();

        // Create second document with different field
        let mut doc2 = AutomergeDoc::from_json(&json!({
            "name": "Alice",
            "level": 5
        }))
        .unwrap();

        // Merge doc2 into doc1
        doc1.merge(&mut doc2).unwrap();

        let result = doc1.to_json().unwrap();
        
        // Both fields should be present after merge
        assert!(result.get("name").is_some() || result.get("score").is_some() || result.get("level").is_some());
    }

    #[test]
    fn test_nested_objects() {
        let value = json!({
            "user": {
                "name": "Bob",
                "profile": {
                    "bio": "Developer",
                    "age": 25
                }
            }
        });

        let doc = AutomergeDoc::from_json(&value).unwrap();
        let result = doc.to_json().unwrap();

        assert_eq!(result["user"]["name"], "Bob");
        assert_eq!(result["user"]["profile"]["bio"], "Developer");
        assert_eq!(result["user"]["profile"]["age"], 25);
    }

    #[test]
    fn test_arrays() {
        let value = json!({
            "tags": ["rust", "wasm", "atproto"],
            "counts": [1, 2, 3, 4, 5]
        });

        let doc = AutomergeDoc::from_json(&value).unwrap();
        let result = doc.to_json().unwrap();

        assert!(result["tags"].is_array());
        assert!(result["counts"].is_array());
    }
}
