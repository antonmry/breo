# PDS Core

ATProto-compatible repository engine implemented in pure Rust.

## Overview

This crate provides the core data structures and logic for a Personal Data Server (PDS) that implements the AT Protocol. It is designed to be platform-agnostic and can be used in various environments including native applications, WebAssembly, and servers.

## Features

- **Type-safe data models**: Strongly typed structures for DIDs, CIDs, Records, and Commits
- **Append-only commit graph**: Deterministic commit history with head tracking
- **Automerge integration**: CRDT support for mutable documents with conflict-free merging
- **Trait-based abstractions**: Pluggable storage (KvStore), clock, and crypto implementations
- **Record validation**: Built-in validation for ATProto records
- **JSON snapshots**: Export and import repository state
- **Pure Rust**: No platform-specific dependencies in the core library
- **Comprehensive tests**: 26+ unit tests covering all functionality

## Architecture

### Core Types

- **`Did`**: Decentralized Identifier (e.g., `did:plc:alice123`)
- **`Cid`**: Content Identifier for addressing records and commits
- **`Nsid`**: Namespaced Identifier for lexicon types (e.g., `app.bsky.feed.post`)
- **`RecordKey`**: Unique identifier for records within a collection
- **`Record`**: A single data entry in the repository
- **`Commit`**: Represents an atomic change to the repository

### Repository

The `Repository` struct manages:
- Append-only commit graph
- Current repository state (head pointer)
- Record CRUD operations
- Commit signing and verification

### Traits

Three core traits enable platform-agnostic operation:

- **`KvStore`**: Key-value storage abstraction
- **`Clock`**: Timestamp generation
- **`Crypto`**: Signing and verification

Default implementations:
- `MemoryKvStore`: In-memory storage for testing
- `SystemClock`: System time
- `Ed25519Crypto`: Ed25519 signature scheme

### Automerge Wrapper

The `AutomergeDoc` wrapper provides:
- JSON-compatible interface to Automerge CRDTs
- Conflict-free merging of concurrent updates
- Change tracking and synchronization
- Binary serialization for storage

### Snapshot Serializer

Export repository state in multiple formats:
- **`Snapshot`**: Complete repository export (records + commits)
- **`CommitLog`**: Chronological commit history
- **`RecordSnapshot`**: Individual record export

## Usage

### Basic Example

```rust
use pds_core::{
    repo::Repository,
    traits::{Ed25519Crypto, MemoryKvStore, SystemClock},
    types::{Did, Nsid, RecordKey},
};

fn main() -> pds_core::Result<()> {
    // Create repository
    let did = Did::new("did:plc:alice123")?;
    let store = MemoryKvStore::new();
    let clock = SystemClock;
    let crypto = Ed25519Crypto::new();
    
    let mut repo = Repository::new(did, store, clock, crypto);
    
    // Create a record
    let collection = Nsid::new("app.bsky.feed.post")?;
    let rkey = RecordKey::new("post1");
    let value = serde_json::json!({
        "text": "Hello ATProto!",
        "createdAt": "2025-01-01T00:00:00Z"
    });
    
    let cid = repo.create_record(collection, rkey, value)?;
    println!("Created record with CID: {}", cid);
    
    Ok(())
}
```

### Automerge Example

```rust
use pds_core::automerge_wrapper::AutomergeDoc;

fn main() -> pds_core::Result<()> {
    // Create a mutable document
    let profile = serde_json::json!({
        "displayName": "Alice",
        "bio": "ATProto developer"
    });
    
    let mut doc = AutomergeDoc::from_json(&profile)?;
    
    // Update the document
    let updated = serde_json::json!({
        "displayName": "Alice Smith",
        "bio": "ATProto developer and enthusiast"
    });
    doc.update(&updated)?;
    
    // Save and load
    let bytes = doc.save();
    let loaded = AutomergeDoc::load(&bytes)?;
    
    Ok(())
}
```

### Custom Storage

```rust
use pds_core::traits::KvStore;
use pds_core::error::Result;

struct MyCustomStore {
    // Your storage implementation
}

impl KvStore for MyCustomStore {
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        // Your implementation
        Ok(())
    }
    
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Your implementation
        Ok(None)
    }
    
    // ... implement other methods
}
```

## Running Tests

```bash
cargo test
```

All 26 tests should pass:
- Type validation tests
- Repository operations (create, update, delete)
- Commit graph tests
- Automerge document tests
- Snapshot serialization tests
- Crypto and storage tests

## Running Examples

```bash
cargo run --example basic_usage
```

This demonstrates:
- Creating a repository
- Adding and updating records
- Working with the commit graph
- Using Automerge for mutable documents
- Exporting snapshots

## Design Principles

1. **Deterministic**: All operations produce consistent results
2. **Type-safe**: Strong typing prevents common errors
3. **Modular**: Trait-based design allows custom implementations
4. **Testable**: Pure functions and dependency injection
5. **Minimal**: No unnecessary dependencies
6. **Platform-agnostic**: Works in any Rust environment

## Dependencies

- `serde`: Serialization framework
- `serde_json`: JSON support
- `automerge`: CRDT implementation
- `sha2`: Cryptographic hashing
- `ed25519-dalek`: Ed25519 signatures
- `chrono`: DateTime handling

## Future Enhancements

Potential areas for expansion:
- CAR file format support
- IPLD codec integration
- Advanced record validation (Lexicon schemas)
- DAG-CBOR serialization
- WebCrypto integration for WASM
- IndexedDB adapter for browsers

## License

MIT
