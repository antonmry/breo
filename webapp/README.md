# Browser PDS Web Application

Minimal offline-capable interface for Browser PDS.

## Features

- **Identity Management**: Display DID and public key, export identity
- **Profile Editor**: Edit profile with Automerge-backed document
- **Post Composer**: Create posts with real-time feed updates
- **Feed Viewer**: View all posts in chronological order
- **Publish Dialog**: Export snapshot for remote PDS publishing
- **Backup & Restore**: Create and restore backups with visual warnings

## Development

```bash
# Install dependencies
npm install

# Start dev server
npm run dev

# Build for production
npm run build
```

## Architecture

- **Vite**: Fast build tool and dev server
- **TypeScript**: Type-safe JavaScript
- **Tailwind CSS**: Utility-first CSS framework
- **WASM Integration**: Calls into `pds-wasm` bindings from `../crates/wasm/pkg`

## Usage

1. Open http://localhost:5173 in your browser
2. Initialize repository with a DID (e.g., `did:plc:alice`)
3. Create posts, edit profile, and manage your local data
4. Regular backups are recommended!

## Notes

- All data is stored locally in IndexedDB
- Private keys are stored in browser WebCrypto
- No inbound networking - fully local-first
- Publish feature exports snapshot for manual upload to remote PDS
