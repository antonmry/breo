#!/bin/bash
cd /home/runner/work/pds-wasm/pds-wasm

# Download all core files from main branch via GitHub raw URLs
BASE_URL="https://raw.githubusercontent.com/antonmry/pds-wasm/main/crates/core"

# Create example directory
mkdir -p crates/core/examples

# Download files using curl with proper error handling
for file in "Cargo.toml" "README.md"; do
  curl -f -sS -L "${BASE_URL}/${file}" -o "crates/core/${file}" 2>&1 || echo "Failed to download ${file}"
done

for file in "traits.rs" "types.rs" "repo.rs" "snapshot.rs" "automerge_wrapper.rs"; do
  curl -f -sS -L "${BASE_URL}/src/${file}" -o "crates/core/src/${file}" 2>&1 || echo "Failed to download src/${file}"
done

# Check what we got
ls -lh crates/core/
ls -lh crates/core/src/
