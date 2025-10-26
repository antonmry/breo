# Copilot Instructions for Browser PDS

## Project Overview

Browser PDS is a local-first **ATProto-compatible Personal Data Server** implemented in **Rust + WebAssembly**. It runs entirely inside the browser, stores data in IndexedDB, and can optionally publish records to external PDS servers.

**Key Principles:**
- Local-first data ownership
- No inbound networking (browser sandbox only)
- Deterministic conflict resolution using Automerge
- Cryptographically signed commits

## Repository Structure

```
/crates/core        # Pure Rust logic, no browser APIs
/crates/wasm        # wasm-bindgen bindings and browser storage adapters
/webapp             # Browser UI (Vite + TypeScript + Tailwind)
/docs               # Design notes, diagrams
```

## Development Workflow

### Building and Testing

1. **Core Rust crate:**
   ```bash
   cd crates/core
   cargo test
   cargo clippy
   ```

2. **WASM bindings:**
   ```bash
   cd crates/wasm
   wasm-pack build --target web
   ```

3. **Web application:**
   ```bash
   cd webapp
   npm install
   npm run dev
   ```

### Testing Requirements

- All Rust code must have unit tests
- Run `cargo test` before committing changes to Rust code
- Run `cargo clippy` to ensure code quality
- For WASM changes, verify integration with webapp
- For webapp changes, test in the browser at http://localhost:5173

## Architecture Guidelines

### Data Model

- **Mutable singleton records** (profile, settings): Use Automerge for conflict-free merging
- **Immutable records** (posts, likes): Append-only, no merging
- **Repo commits**: Cryptographically signed with user's keypair (WebCrypto)

### Layer Separation

- **crates/core**: Contains pure Rust logic with no browser dependencies
  - Must be platform-agnostic
  - Should work in any Rust environment
  
- **crates/wasm**: Browser-specific code only
  - IndexedDB persistence
  - WebCrypto integration
  - WASM bindings via wasm-bindgen
  
- **webapp**: Frontend UI
  - TypeScript with Tailwind CSS
  - Calls into WASM APIs
  - Handles remote publish via fetch()

### Security Considerations

- Private keys are stored in browser WebCrypto
- Same-origin scripts can access keys
- IndexedDB can be cleared by browser/user
- Regular backups are mandatory for data durability
- No inbound HTTP connections (browser PDS is not reachable from peers)

## Coding Standards

### Rust Code

- Follow Rust standard naming conventions (snake_case for functions/variables, PascalCase for types)
- Use `clippy` recommendations
- Add documentation comments for public APIs
- Handle errors explicitly (avoid unwrap() in production code)
- Prefer Result<T, E> over panicking

### TypeScript/JavaScript Code

- Use TypeScript for all new code
- Follow existing code formatting
- Use async/await for asynchronous operations
- Handle errors gracefully in UI

### WASM Bindings

- Keep WASM interface minimal and focused
- Document all exported functions
- Use appropriate wasm-bindgen attributes
- Test WASM bindings with the webapp

## Important Constraints

1. **No inbound networking**: The browser PDS cannot receive connections
2. **Local storage limitations**: IndexedDB can be cleared; backups are essential
3. **Browser sandbox**: All operations must work within browser security constraints
4. **WebAssembly**: Core logic must compile to WASM
5. **Offline-first**: The PDS should work without network connectivity

## When Making Changes

1. Understand which layer (core, wasm, webapp) is affected
2. Maintain separation of concerns between layers
3. Add tests for new functionality
4. Run appropriate linters and tests
5. Verify changes in the browser when touching WASM or webapp
6. Consider security implications, especially around key management
7. Document any new public APIs

## Dependencies

- Minimize new dependencies
- For Rust: Prefer well-maintained crates from crates.io
- For JavaScript: Check npm package security before adding
- Ensure WASM-compatible dependencies for core and wasm crates

## Common Tasks

### Adding a new record type

1. Define the schema in `crates/core`
2. Implement commit logic
3. Add WASM bindings in `crates/wasm`
4. Update UI in `webapp`
5. Add tests at each layer

### Modifying Automerge integration

- Changes likely required in `crates/core`
- Consider conflict resolution behavior
- Test merge scenarios thoroughly

### Updating storage layer

- Changes in `crates/wasm` (IndexedDB)
- Ensure backward compatibility with existing data
- Consider migration paths for data schema changes
