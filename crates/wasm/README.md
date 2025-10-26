# PDS WASM

Browser-based ATProto Personal Data Server implementation using WebAssembly.

## Overview

This crate provides WASM bindings for the PDS core library, enabling a complete ATProto-compatible repository to run entirely in the browser with:

- **IndexedDB persistence** (cache-based for now, full implementation pending)
- **Ed25519 signing** (using pure Rust implementation that works in WASM)
- **JavaScript Date integration** for accurate browser timestamps

## Building

```bash
wasm-pack build --target web
```

This generates a `pkg/` directory with:
- `pds_wasm_bg.wasm` - The WebAssembly binary (347KB)
- `pds_wasm.js` - JavaScript bindings
- `pds_wasm.d.ts` - TypeScript type definitions

## API

### WasmRepository

The main entry point for using the PDS in the browser.

```javascript
import init, { WasmRepository } from './pkg/pds_wasm.js';

// Initialize WASM module
await init();

// Create repository
const repo = new WasmRepository();

// Initialize with a DID
const did = await repo.init_identity("did:plc:example123");

// Create a post
const postCid = await repo.create_post("Hello, ATProto!");

// Edit profile
const profileCid = await repo.edit_profile("Alice", "I love decentralization");

// List records
const posts = repo.list_records("app.bsky.feed.post");

// Export for publishing
const snapshot = repo.export_for_publish();

// Backup
const backup = repo.backup();

// Restore
await repo.restore(backup);
```

## Implementation Status

- [x] KvStore trait using in-memory cache (IndexedDB integration pending)
- [x] Crypto trait using Ed25519 from core
- [x] Clock trait using JavaScript Date
- [x] WASM bindings for high-level APIs:
  - [x] `init_identity()`
  - [x] `create_post()`
  - [x] `edit_profile()`
  - [x] `list_records()`
  - [x] `export_for_publish()`
  - [x] `backup()` / `restore()`
- [x] Binary size < 2 MB (currently 347KB)
- [ ] Full IndexedDB persistence implementation
- [ ] Browser integration tests
- [ ] WebCrypto implementation (optional, Ed25519 works fine)

## Size Optimization

The WASM binary is optimized for size:
- Compiler optimization level: `z`
- Link-time optimization: enabled
- Code generation units: 1
- Strip symbols: enabled

Current size: **347KB** (well under the 2MB target)

## Testing

```bash
# Run unit tests
cargo test

# Run WASM tests (requires wasm-pack)
wasm-pack test --headless --chrome
```

## License

MIT
