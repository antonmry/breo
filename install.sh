#!/bin/sh
# Install script for breo
# Usage: curl -fsSL https://raw.githubusercontent.com/antonmry/breo/main/install.sh | bash

set -eu

REPO="antonmry/breo"
INSTALL_DIR="${BREO_INSTALL_DIR:-$HOME/.local/bin}"

# Detect platform
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux)  os_part="unknown-linux-gnu" ;;
        Darwin) os_part="apple-darwin" ;;
        *)
            echo "Error: unsupported OS: $os" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch_part="x86_64" ;;
        aarch64|arm64)  arch_part="aarch64" ;;
        *)
            echo "Error: unsupported architecture: $arch" >&2
            exit 1
            ;;
    esac

    echo "${arch_part}-${os_part}"
}

# Get latest version from GitHub API
get_latest_version() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
            sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | \
            sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'
    else
        echo "Error: curl or wget is required" >&2
        exit 1
    fi
}

# Download a file
download() {
    url="$1"
    output="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$output" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$output" "$url"
    else
        echo "Error: curl or wget is required" >&2
        exit 1
    fi
}

main() {
    target=$(detect_target)
    version="${BREO_VERSION:-$(get_latest_version)}"

    if [ -z "$version" ]; then
        echo "Error: could not determine latest version" >&2
        exit 1
    fi

    echo "Installing breo ${version} for ${target}..."

    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT

    base_url="https://github.com/${REPO}/releases/download/${version}"
    archive="breo-${target}.tar.gz"

    # Download archive and checksums
    download "${base_url}/${archive}" "${tmpdir}/${archive}"
    download "${base_url}/checksums.sha256" "${tmpdir}/checksums.sha256"

    # Verify checksum
    cd "$tmpdir"
    if command -v sha256sum >/dev/null 2>&1; then
        grep "  ${archive}\$" checksums.sha256 | sha256sum -c -
    elif command -v shasum >/dev/null 2>&1; then
        grep "  ${archive}\$" checksums.sha256 | shasum -a 256 -c -
    else
        echo "Error: sha256sum or shasum is required for checksum verification" >&2
        exit 1
    fi

    # Extract and install
    tar xzf "$archive"
    mkdir -p "$INSTALL_DIR"
    mv breo "$INSTALL_DIR/breo"
    chmod +x "$INSTALL_DIR/breo"

    echo "Installed breo to ${INSTALL_DIR}/breo"

    # Set up shell completions
    setup_completions

    # Check PATH
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            echo ""
            echo "Add ${INSTALL_DIR} to your PATH:"
            echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
            ;;
    esac

    echo ""
    echo "Done! Run 'breo --version' to verify."
}

setup_completions() {
    shell_name=$(basename "${SHELL:-}")

    case "$shell_name" in
        bash)
            rc_file="$HOME/.bashrc"
            line='eval "$(breo setup bash)"'
            ;;
        zsh)
            rc_file="$HOME/.zshrc"
            line='eval "$(breo setup zsh)"'
            ;;
        fish)
            rc_file="${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish"
            line='breo setup fish | source'
            ;;
        *)
            echo "Shell completions: run 'breo setup <shell>' manually for your shell."
            return
            ;;
    esac

    if [ -f "$rc_file" ] && grep -qF "$line" "$rc_file" 2>/dev/null; then
        echo "Shell completions already configured in ${rc_file}"
    else
        mkdir -p "$(dirname "$rc_file")"
        [ -f "$rc_file" ] || touch "$rc_file"
        echo "$line" >> "$rc_file"
        echo "Added shell completions to ${rc_file}"
    fi
}

main
