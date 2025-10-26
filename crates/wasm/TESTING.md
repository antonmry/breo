# Testing Guide

This guide covers testing the PDS WASM implementation.

## Unit Tests

### Core Tests

Test the pure Rust core library:

```bash
cd crates/core
cargo test
```

### WASM Tests

Test the WASM bindings (runs on the host, not in browser):

```bash
cd crates/wasm
cargo test
```

Note: Some tests are skipped when not running in WASM environment (e.g., JsClock tests).

## WASM Browser Tests

### Using wasm-pack test

For headless browser testing:

```bash
cd crates/wasm
wasm-pack test --headless --chrome
# or
wasm-pack test --headless --firefox
```

### Manual Browser Testing

1. Build the WASM package:
   ```bash
   cd crates/wasm
   wasm-pack build --target web
   ```

2. Serve the example HTML:
   ```bash
   # Using Python
   python3 -m http.server 8080
   
   # Or using Node
   npx http-server -p 8080
   ```

3. Open http://localhost:8080/example.html in your browser

4. Open the browser console (F12) to see logs

5. Test the functionality:
   - Click "Initialize" to create a repository
   - Create some posts
   - Update your profile
   - List records
   - Export snapshots

## Integration Testing

### Testing with a webapp

1. Build the WASM package:
   ```bash
   cd crates/wasm
   wasm-pack build --target web
   ```

2. Copy the `pkg/` directory to your webapp:
   ```bash
   cp -r pkg/ ../../webapp/src/wasm/
   ```

3. Import and use in your webapp:
   ```typescript
   import init, { WasmRepository } from './wasm/pds_wasm.js';
   
   await init();
   const repo = new WasmRepository();
   ```

## Performance Testing

### Binary Size

Check the WASM binary size:

```bash
cd crates/wasm
wasm-pack build --target web --release
ls -lh pkg/pds_wasm_bg.wasm
```

Target: < 2 MB uncompressed

### Load Time

Measure initialization time:

```javascript
console.time('init');
await init();
console.timeEnd('init');
```

### Operation Benchmarks

Measure key operations:

```javascript
const repo = new WasmRepository();
await repo.init_identity("did:plc:test");

// Create post benchmark
console.time('create_post');
await repo.create_post("Test post");
console.timeEnd('create_post');

// List records benchmark
console.time('list_records');
repo.list_records("app.bsky.feed.post");
console.timeEnd('list_records');
```

## Debugging

### Enable Debug Logging

Add console logging in Rust code:

```rust
use web_sys::console;

console::log_1(&"Debug message".into());
```

### Inspect WASM Binary

```bash
wasm-objdump -x pkg/pds_wasm_bg.wasm
wasm2wat pkg/pds_wasm_bg.wasm -o pkg/pds_wasm_bg.wat
```

### Browser DevTools

- Use the Sources tab to set breakpoints in generated JS
- Use the Console to inspect errors
- Use the Network tab to see WASM loading
- Use the Memory tab to check for leaks

## Continuous Integration

The CI workflow automatically:
- Builds WASM for each commit
- Checks binary size < 2MB
- Runs all tests
- Uploads WASM artifacts

Check `.github/workflows/ci.yml` for details.

## Known Limitations

1. **IndexedDB**: Currently uses in-memory cache only
   - Full IndexedDB integration requires async API throughout
   - Future enhancement

2. **WebCrypto**: Using Ed25519 from core instead
   - WebCrypto API is async-only
   - Core's Ed25519 works fine in WASM

3. **Browser compatibility**: 
   - Requires WebAssembly support
   - Tested on Chrome, Firefox, Safari
   - May not work on older browsers

## Test Coverage

Run coverage analysis:

```bash
cargo tarpaulin --workspace --exclude-files wasm/* --timeout 120
```

## Writing New Tests

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        // Test code here
    }
}
```

### WASM-specific Test

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_browser_api() {
        // Code that uses browser APIs
    }
}
```

### Browser Integration Test

```rust
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn test_in_browser() {
    // Test code that runs in real browser
}
```
