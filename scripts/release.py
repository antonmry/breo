#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Trigger a GitHub release using the version from Cargo.toml."""

import re
import subprocess
import sys
from pathlib import Path


def get_version() -> str:
    cargo = Path(__file__).parent.parent / "Cargo.toml"
    match = re.search(r'^version\s*=\s*"(.+?)"', cargo.read_text(), re.MULTILINE)
    if not match:
        print("Error: could not find version in Cargo.toml", file=sys.stderr)
        sys.exit(1)
    return match.group(1)


def main() -> None:
    version = get_version()
    tag = f"v{version}"
    print(f"Releasing {tag}")

    subprocess.run(
        ["gh", "workflow", "run", "release.yml", "-f", f"version={tag}"],
        check=True,
    )
    print(f"Workflow triggered. Check status: gh run list --workflow=release.yml")


if __name__ == "__main__":
    main()
