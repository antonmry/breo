# Browser PDS

A local-first **ATProto-compatible Personal Data Server** implemented in **Rust + WebAssembly**.  
Runs fully inside the browser, keeps all data in local storage (IndexedDB), and can **optionally publish** selected records to external PDS servers.

---

## Overview

This project implements the AT Protocol data model in a self-contained environment.
It focuses on *local-first ownership* of data and deterministic conflict resolution using **Automerge**.

There is **no inbound networking**: your PDS lives entirely inside the browser sandbox.  
When desired, you can export and push signed records to any external ATProto PDS for public visibility.

---

## Architecture

| Layer | Crate / Module | Description |
|-------|----------------|--------------|
| Core Logic | `crates/core` | Data models, commits, Automerge integration, repo management |
| Browser Runtime | `crates/wasm` | IndexedDB persistence, WebCrypto, WASM bindings |
| Web UI | `webapp/` | Browser UI that calls into WASM APIs |
| Remote Publish | JS + Fetch | Push CAR bundles or snapshots to external PDS endpoints |

---

## Data Model

- **Mutable singleton records** (profile, settings) use **Automerge** for conflict-free merging.
- **Immutable records** (posts, likes) are append-only and do not merge.
- **Repo commits** are cryptographically signed with the user’s keypair (stored in WebCrypto).

---

## Features (initial)

- ✅ Local-only PDS running in the browser  
- ✅ IndexedDB-backed repo storage  
- ✅ DID + keypair generated and persisted locally  
- ✅ Signed commits for all operations  
- ✅ Automerge-backed conflict resolution for mutable docs  
- ✅ Export / import backups  
- ✅ Optional publish to remote PDS via `fetch()`  
- 🚧 Remote snapshot pull and merge  
- 🚧 UI for posts, profile, and backup management  

---

## Security & Limitations

- The private key is stored in browser WebCrypto; any script under the same origin could access it.  
- IndexedDB can be cleared by the user or the browser (low storage, “clear site data”).  
- Regular backups are **mandatory** for data durability.  
- The browser PDS is not reachable from other peers (no inbound HTTP).  
- Published data on external PDSs is not automatically re-synced.

---

## Quick Start (development)

```bash
# build the core crate
cd crates/core
cargo test

# build wasm bindings
cd ../wasm
wasm-pack build --target web

# run the webapp
cd ../../webapp
npm install
npm run dev
```

Open http://localhost:5173 in a browser.

---

## Repository Layout

```
/crates/core        # pure Rust logic, no browser APIs
/crates/wasm        # wasm-bindgen bindings and browser storage adapters
/webapp             # browser UI (Vite + Typescript + Tailwind)
/docs               # design notes, diagrams
AGENTS.md           # role definitions for development
```

---

## License

MIT
