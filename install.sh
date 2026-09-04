#!/bin/sh
# codeunlimited installer (Linux x86_64 / macOS arm64)
#   curl -fsSL https://raw.githubusercontent.com/jimbokl/codeunlimited/main/install.sh | sh
set -eu

REPO="jimbokl/codeunlimited"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ASSET="codeunlimited-linux-x86_64" ;;
  Darwin-arm64)  ASSET="codeunlimited-macos-arm64" ;;
  *) echo "No prebuilt binary for $(uname -s)/$(uname -m) - use: cargo install codeunlimited" >&2; exit 1 ;;
esac

DEST="${CODEUNLIMITED_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"
TMP="$(mktemp)"
echo "Downloading $ASSET ..."
curl -fsSL "$URL" -o "$TMP"

# Verify checksum when the release ships one and a hasher is available.
SUM_URL="$URL.sha256"
if SUM="$(curl -fsSL "$SUM_URL" 2>/dev/null | awk '{print $1}')" && [ -n "$SUM" ]; then
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMP" | awk '{print $1}')"
  else
    ACTUAL=""
  fi
  if [ -n "$ACTUAL" ] && [ "$ACTUAL" != "$SUM" ]; then
    echo "Checksum mismatch - aborting." >&2; rm -f "$TMP"; exit 1
  fi
fi

install -m 755 "$TMP" "$DEST/codeunlimited"
rm -f "$TMP"
echo "Installed: $DEST/codeunlimited"
"$DEST/codeunlimited" --version || true
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "Note: add it to PATH ->  export PATH=\"$DEST:\$PATH\"" ;;
esac
echo "Next: codeunlimited audit"
