#!/usr/bin/env bash
# Install or use a cached undox binary.
#
# Usage:
#   ./docs/scripts/install-undox.sh [version]
#
# The binary is cached at .undox/bin/undox. Set UNDOX_CACHE_DIR to override.
# In CI, point this at a directory covered by actions/cache.

set -euo pipefail

VERSION="${1:-0.1.8}"
CACHE_DIR="${UNDOX_CACHE_DIR:-$(git rev-parse --show-toplevel)/.undox}"
BIN_DIR="${CACHE_DIR}/bin"
BIN="${BIN_DIR}/undox"

# If the cached binary matches the requested version, reuse it.
if [[ -x "$BIN" ]]; then
  CACHED_VERSION=$("$BIN" --version 2>/dev/null | awk '{print $2}' || echo "")
  if [[ "$CACHED_VERSION" == "$VERSION" ]]; then
    echo "undox $VERSION already cached at $BIN"
    echo "$BIN_DIR"
    exit 0
  fi
fi

mkdir -p "$BIN_DIR"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="unknown-linux-gnu" ;;
  Darwin) PLATFORM="apple-darwin" ;;
  *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64)  TARGET="${ARCH}-${PLATFORM}" ;;
  aarch64|arm64) TARGET="aarch64-${PLATFORM}" ;;
  *)       echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARBALL="undox-${TARGET}.tar.gz"
URL="https://github.com/undox-rs/undox/releases/download/v${VERSION}/${TARBALL}"

echo "Downloading undox v${VERSION} for ${TARGET}..."
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "${TMPDIR}/${TARBALL}"
tar -xzf "${TMPDIR}/${TARBALL}" -C "$TMPDIR"

# The tarball extracts a binary named 'undox'
if [[ -f "${TMPDIR}/undox" ]]; then
  mv "${TMPDIR}/undox" "$BIN"
else
  # Some releases nest inside a directory
  EXTRACTED=$(find "$TMPDIR" -name undox -type f | head -1)
  if [[ -z "$EXTRACTED" ]]; then
    echo "Could not find undox binary in release archive" >&2
    exit 1
  fi
  mv "$EXTRACTED" "$BIN"
fi

chmod +x "$BIN"
echo "Installed undox v${VERSION} to $BIN"
echo "$BIN_DIR"
