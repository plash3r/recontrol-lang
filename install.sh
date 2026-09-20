#!/usr/bin/env sh
set -eu

REPO="${RCL_REPO:-plash3r/recontrol-lang}"
INSTALL_DIR="${RCL_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${RCL_VERSION:-latest}"
TOKEN="${RCL_GITHUB_TOKEN:-${GH_TOKEN:-}}"

die() {
  printf '%s\n' "rcl installer: $*" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || die "curl is required."
command -v uname >/dev/null 2>&1 || die "uname is required."

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS:$ARCH" in
  Linux:x86_64|Linux:amd64)
    ASSET="rcl-linux-x86_64"
    TARGET="rcl"
    ;;
  *)
    die "Unsupported platform: $OS/$ARCH. Currently supported: Linux x86_64."
    ;;
esac

API="https://api.github.com/repos/$REPO"
AUTH_HEADER=""
if [ -n "$TOKEN" ]; then
  AUTH_HEADER="Authorization: Bearer $TOKEN"
fi

api_get() {
  if [ -n "$AUTH_HEADER" ]; then
    curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/vnd.github+json" "$1"
  else
    curl -fsSL -H "Accept: application/vnd.github+json" "$1"
  fi
}

if [ "$VERSION" = "latest" ]; then
  RELEASE_JSON="$(api_get "$API/releases/latest")" ||
    die "Could not access the latest release. If the repository is private, set RCL_GITHUB_TOKEN (or GH_TOKEN)."
else
  case "$VERSION" in
    v*) ;;
    *) VERSION="v$VERSION" ;;
  esac
  RELEASE_JSON="$(api_get "$API/releases/tags/$VERSION")" ||
    die "Could not access release $VERSION. If the repository is private, set RCL_GITHUB_TOKEN (or GH_TOKEN)."
fi

# Keep this installer dependency-free apart from POSIX tools + curl.
RELEASE_TAG="$(printf '%s' "$RELEASE_JSON" | sed -n 's/.*"tag_name":[[:space:]]*"\\([^"]*\\)".*/\\1/p' | head -n 1)"
[ -n "$RELEASE_TAG" ] || die "GitHub returned an invalid release response."

URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/$ASSET"
CHECKSUM_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/SHA256SUMS"

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t rcl)"
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM
TMP_FILE="$TMP_DIR/$ASSET"

printf '%s\n' "Installing Recontrol Lang $RELEASE_TAG ($ASSET)..."

if [ -n "$AUTH_HEADER" ]; then
  curl -fL --retry 3 --retry-delay 1 -H "$AUTH_HEADER" -o "$TMP_FILE" "$URL" ||
    die "Failed to download $ASSET."
else
  curl -fL --retry 3 --retry-delay 1 -o "$TMP_FILE" "$URL" ||
    die "Failed to download $ASSET."
fi

# Verify the release checksum when SHA256SUMS is available.
CHECKSUM_FILE="$TMP_DIR/SHA256SUMS"
if { [ -n "$AUTH_HEADER" ] && curl -fLsS -H "$AUTH_HEADER" -o "$CHECKSUM_FILE" "$CHECKSUM_URL"; } ||
   { [ -z "$AUTH_HEADER" ] && curl -fLsS -o "$CHECKSUM_FILE" "$CHECKSUM_URL"; }; then
  EXPECTED="$(grep "  $ASSET$" "$CHECKSUM_FILE" | awk '{print $1}' | head -n 1)"
  [ -n "$EXPECTED" ] || die "Checksum for $ASSET is missing from SHA256SUMS."

  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP_FILE" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')"
  else
    die "No SHA-256 utility found (sha256sum or shasum)."
  fi

  [ "$EXPECTED" = "$ACTUAL" ] || die "SHA-256 verification failed."
  printf '%s\n' "Checksum verified."
else
  printf '%s\n' "Warning: SHA256SUMS is not available; skipping checksum verification." >&2
fi

mkdir -p "$INSTALL_DIR"
chmod 755 "$TMP_FILE"
mv "$TMP_FILE" "$INSTALL_DIR/$TARGET"
chmod 755 "$INSTALL_DIR/$TARGET"

printf '%s\n' "Installed: $INSTALL_DIR/$TARGET"
"$INSTALL_DIR/$TARGET" --version || true

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '%s\n' "" "Add $INSTALL_DIR to PATH if needed:" "  export PATH=\"\$INSTALL_DIR:\$PATH\"" >&2
    ;;
esac
