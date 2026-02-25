#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Generate release notes with breo, open $EDITOR for review, and publish.

Usage:
    uv run scripts/release_notes.py              # use version from Cargo.toml
    uv run scripts/release_notes.py v0.1.0       # target a specific release
"""

import os
import re
import subprocess
import sys
from pathlib import Path

NOTES_FILE = Path("/tmp/breo-release-notes.md")


def get_version() -> str:
    cargo = Path(__file__).parent.parent / "Cargo.toml"
    match = re.search(r'^version\s*=\s*"(.+?)"', cargo.read_text(), re.MULTILINE)
    if not match:
        print("Error: could not find version in Cargo.toml", file=sys.stderr)
        sys.exit(1)
    return match.group(1)


def get_git_log(tag: str) -> str:
    """Get commit log since the previous tag up to this tag (or HEAD)."""
    # Check if the tag exists; if so, diff from the tag before it
    ref = tag
    result = subprocess.run(
        ["git", "rev-parse", "--verify", f"refs/tags/{tag}"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        ref = "HEAD"

    try:
        prev_tag = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0", f"{ref}^"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        log_range = f"{prev_tag}..{ref}"
    except subprocess.CalledProcessError:
        log_range = ref

    result = subprocess.run(
        ["git", "log", log_range, "--pretty=format:%s (%h)"],
        capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


def generate(tag: str) -> None:
    """Generate release notes with breo and write to /tmp."""
    git_log = get_git_log(tag)
    if not git_log:
        print("No commits found for this release.")
        sys.exit(1)

    print(f"Commits for {tag}:\n{git_log}\n")
    print("Generating release notes with breo...")

    prompt = (
        f"Generate concise GitHub release notes for {tag} of breo. "
        f"Use markdown with sections like Features, Fixes, Changes as needed. "
        f"Only include sections that apply. Be concise, no preamble.\n\n"
        f"Commits:\n{git_log}"
    )
    result = subprocess.run(
        ["breo", "--no-sandbox", prompt],
        capture_output=True, text=True,
    )
    if result.returncode != 0 or not result.stdout.strip():
        print("Warning: breo failed to generate notes, using git log", file=sys.stderr)
        notes = f"## Changes\n\n{git_log}"
    else:
        notes = result.stdout.strip()

    NOTES_FILE.write_text(notes)
    print(f"Release notes written to {NOTES_FILE}")


def edit(path: Path) -> None:
    """Open the notes file in $EDITOR for review."""
    editor = os.environ.get("EDITOR", "vi")
    subprocess.run([editor, str(path)], check=True)


def confirm() -> bool:
    """Ask the user to confirm before publishing."""
    try:
        answer = input("Publish these release notes? [y/N] ").strip().lower()
    except (EOFError, KeyboardInterrupt):
        print()
        return False
    return answer in ("y", "yes")


def publish(tag: str) -> None:
    """Publish the notes file to the GitHub release."""
    subprocess.run(
        ["gh", "release", "edit", tag, "--notes-file", str(NOTES_FILE)],
        check=True,
    )
    print(f"Release notes updated for {tag}")


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    tag = args[0] if args else f"v{get_version()}"

    generate(tag)
    edit(NOTES_FILE)

    if confirm():
        publish(tag)
    else:
        print(f"Cancelled. Notes saved at {NOTES_FILE}")


if __name__ == "__main__":
    main()
