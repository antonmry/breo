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

pub mod automerge_wrapper;
pub mod error;
pub mod repo;
pub mod snapshot;
pub mod traits;
pub mod types;

pub use error::{Error, Result};
