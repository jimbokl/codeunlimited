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
BASE_URL="${CODEUNLIMITED_DOWNLOAD_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
URL="${BASE_URL%/}/$ASSET"
TMP_DIR="$(mktemp -d)"
DOWNLOAD="$TMP_DIR/$ASSET"
SUM_FILE="$TMP_DIR/$ASSET.sha256"
STAGED="$DEST/.codeunlimited.install.$$"
cleanup() {
  rm -f "$DOWNLOAD" "$SUM_FILE" "$STAGED"
  rmdir "$TMP_DIR" 2>/dev/null || true
}
trap cleanup EXIT

echo "Downloading $ASSET ..."
curl -fsSL "$URL" -o "$DOWNLOAD"
if ! curl -fsSL "$URL.sha256" -o "$SUM_FILE"; then
  echo "Checksum download failed - preserving the existing installation." >&2
  exit 1
fi

SUM="$(awk 'NR == 1 { print $1 }' "$SUM_FILE")"
case "$SUM" in
  ""|*[!0-9a-fA-F]*)
    echo "Malformed checksum - preserving the existing installation." >&2
    exit 1
    ;;
esac
if [ "${#SUM}" -ne 64 ]; then
  echo "Malformed checksum - preserving the existing installation." >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$DOWNLOAD" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL="$(shasum -a 256 "$DOWNLOAD" | awk '{print $1}')"
else
  echo "No SHA-256 tool found - preserving the existing installation." >&2
  exit 1
fi
SUM_LOWER="$(printf '%s' "$SUM" | tr '[:upper:]' '[:lower:]')"
ACTUAL_LOWER="$(printf '%s' "$ACTUAL" | tr '[:upper:]' '[:lower:]')"
if [ "$ACTUAL_LOWER" != "$SUM_LOWER" ]; then
  echo "Checksum mismatch - preserving the existing installation." >&2
  exit 1
fi

chmod 755 "$DOWNLOAD"
if ! "$DOWNLOAD" --version >/dev/null; then
  echo "Downloaded binary failed its smoke test - preserving the existing installation." >&2
  exit 1
fi
install -m 755 "$DOWNLOAD" "$STAGED"
mv -f "$STAGED" "$DEST/codeunlimited"
echo "Installed: $DEST/codeunlimited"
"$DEST/codeunlimited" --version
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "Note: add it to PATH ->  export PATH=\"$DEST:\$PATH\"" ;;
esac
echo "Next: codeunlimited audit"
