# Building and Running

## Prerequisites

- Rust (latest stable)
- wasm-pack
- Node.js (v18 or higher)

## Build Steps

### 1. Build the Core Crate

```bash
cd crates/core
cargo test
```

### 2. Build the WASM Module

```bash
cd crates/wasm
wasm-pack build --target web
```

This generates the WASM bindings in `crates/wasm/pkg/`.

Binary size: ~466KB (well under the 2MB target).

### 3. Run the Web App

```bash
cd webapp
npm install
npm run dev
```

Open http://localhost:5173 in your browser.

## Project Structure

```
/crates/core        # Core logic (traits, types, repository)
/crates/wasm        # WASM bindings (IndexedDB, WebCrypto, APIs)
/webapp             # Web UI (Vite + vanilla JS)
```

## Features

- **Local-first**: All data stored in IndexedDB
- **Identity Management**: Ed25519 keypair stored in localStorage
- **Posts**: Create and list posts
- **Profile**: Edit display name and bio
- **Backup/Restore**: Export and import all data
- **Binary Size**: ~466KB uncompressed

## API Reference

### `init_identity(): Promise<string>`
Initialize or get existing DID.

### `create_post(text: string, reply_to: string | null): Promise<string>`
Create a new post.

### `edit_profile(display_name: string | null, description: string | null): Promise<string>`
Update profile information.

### `list_records(collection: string): Promise<string>`
List all records in a collection (returns JSON string).

### `export_for_publish(): Promise<string>`
Export all records for publishing to external PDS (returns JSON string).

### `backup(): Promise<string>`
Create a complete backup (returns JSON string).

### `restore(backup_json: string): Promise<void>`
Restore from a backup.

### `get_did(): Promise<string | null>`
Get the current DID.

## Implementation Details

### Storage (IndexedDB)

- Database: `pds_store`
- Object Store: `kvstore`
- Keys: Hierarchical with prefixes (`identity`, `commits/`, `records/`)

### Cryptography (WebCrypto + ed25519-dalek)

- Algorithm: Ed25519
- Key Storage: localStorage (base64 encoded)
- DID Format: `did:key:z<base64-encoded-public-key>`

### Clock (JS Date)

- Timestamp: Milliseconds since Unix epoch

## Testing

### Core Tests

```bash
cd crates/core
cargo test
```

### WASM Build Test

```bash
cd crates/wasm
wasm-pack build --target web
```

### Browser Testing

1. Run `npm run dev` in the webapp directory
2. Open http://localhost:5173
3. Test the following flows:
   - Initialize identity
   - Edit profile
   - Create posts
   - Create backup
   - Restore backup

## Security Considerations

- Private keys stored in localStorage (same-origin access only)
- No network requests (fully local)
- IndexedDB can be cleared by browser
- **Regular backups strongly recommended**

## Performance

- WASM module: ~466KB
- Initial load: < 1 second
- Operations: Near-instant (all local)

## Browser Compatibility

- Chrome/Edge 90+
- Firefox 88+
- Safari 15.4+

Requirements:
- IndexedDB support
- WebAssembly support
- ES Modules support
