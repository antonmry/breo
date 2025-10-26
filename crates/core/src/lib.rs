//! # PDS Core
//!
//! Core ATProto-compatible repository engine implementation in Rust.
//! 
//! This crate provides:
//! - Record and commit types for ATProto data model
//! - Repository graph with append-only commits and head tracking
//! - Automerge wrapper for mutable documents
//! - JSON snapshot serializer
//! - Pure Rust implementations with trait-based abstractions

pub mod types;
pub mod traits;
pub mod repo;
pub mod automerge_wrapper;
pub mod snapshot;
pub mod error;

pub use error::{Error, Result};
