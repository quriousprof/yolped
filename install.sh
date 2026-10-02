#!/bin/sh
set -e

REPO="quriousprof/yolped"
BINARY="yolped"

# ── Detect OS and architecture ───────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)         ASSET="yolped-linux-amd64" ;;
      aarch64|arm64)  ASSET="yolped-linux-arm64" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  ASSET="yolped-darwin-amd64" ;;
      arm64)   ASSET="yolped-darwin-arm64" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    echo "On Windows, install via: cargo install yolped" >&2
    exit 1
    ;;
esac

URL="https://github.com/$REPO/releases/latest/download/$ASSET"

# ── Pick install directory ───────────────────────────────────────────────────
if [ -w /usr/local/bin ]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

# ── Download ─────────────────────────────────────────────────────────────────
printf "Downloading %s...\n" "$ASSET"
curl -fsSL "$URL" -o "$INSTALL_DIR/$BINARY"
chmod +x "$INSTALL_DIR/$BINARY"

printf "Installed to %s/%s\n" "$INSTALL_DIR" "$BINARY"

# Warn if install dir isn't in PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf "\nNote: add %s to your PATH:\n  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR" "$INSTALL_DIR" ;;
esac
