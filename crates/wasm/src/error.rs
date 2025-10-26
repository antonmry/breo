//! Error types for WASM bindings

use thiserror::Error;
use wasm_bindgen::JsValue;

/// Result type for WASM operations
pub type Result<T> = std::result::Result<T, WasmError>;

/// Error types for WASM operations
#[derive(Error, Debug)]
pub enum WasmError {
    #[error("Core error: {0}")]
    Core(#[from] pds_core::Error),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("JavaScript error: {0}")]
    JsError(String),
}

impl From<WasmError> for JsValue {
    fn from(err: WasmError) -> JsValue {
        JsValue::from_str(&err.to_string())
    }
}

impl From<JsValue> for WasmError {
    fn from(value: JsValue) -> Self {
        WasmError::JsError(
            value
                .as_string()
                .unwrap_or_else(|| "Unknown JavaScript error".to_string()),
        )
    }
}
