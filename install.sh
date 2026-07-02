#!/usr/bin/env bash
# Installs agent-token-usage into ~/.local/bin and adds an `atu` alias.
# Usage: curl -fsSL https://raw.githubusercontent.com/DLYZZT/agent-token-usage/main/install.sh | bash

set -euo pipefail

REPO="DLYZZT/agent-token-usage"
BIN_NAME="agent-token-usage"
ALIAS_NAME="atu"
INSTALL_DIR="${HOME}/.local/bin"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux) platform="unknown-linux-gnu" ;;
  Darwin) platform="apple-darwin" ;;
  *)
    echo "Unsupported OS: $os" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64|amd64) cpu="x86_64" ;;
  arm64|aarch64) cpu="aarch64" ;;
  *)
    echo "Unsupported architecture: $arch" >&2
    exit 1
    ;;
esac

target="${cpu}-${platform}"
asset="${BIN_NAME}-${target}"

latest_tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"

if [ -z "$latest_tag" ]; then
  echo "Could not determine latest release tag for ${REPO}" >&2
  exit 1
fi

download_url="https://github.com/${REPO}/releases/download/${latest_tag}/${asset}"

echo "Installing ${BIN_NAME} ${latest_tag} (${target})..."

mkdir -p "$INSTALL_DIR"
tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

curl -fsSL "$download_url" -o "$tmp_file"
chmod +x "$tmp_file"
mv "$tmp_file" "${INSTALL_DIR}/${BIN_NAME}"
trap - EXIT

ln -sf "${INSTALL_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${ALIAS_NAME}"

echo "Installed to ${INSTALL_DIR}/${BIN_NAME} (alias: ${ALIAS_NAME})"

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "${INSTALL_DIR} is not on your PATH. Add this to your shell profile:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
