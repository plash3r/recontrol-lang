#!/usr/bin/env sh
set -eu

REPO="${RCL_REPO:-plash3r/recontrol-lang}"
INSTALL_DIR="${RCL_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${RCL_VERSION:-latest}"
BINARY_ASSET="rcl-linux-x86_64"
RUNTIME_ASSET="rcl-runtime-linux-x86_64.a"
TOKEN="${RCL_GITHUB_TOKEN:-${GH_TOKEN:-}}"

if [ "$VERSION" = "latest" ]; then
  BASE_URL="https://github.com/$REPO/releases/latest/download"
else
  case "$VERSION" in
    v*) TAG="$VERSION" ;;
    *) TAG="v$VERSION" ;;
  esac
  BASE_URL="https://github.com/$REPO/releases/download/$TAG"
fi

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t rcl)"
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM

download() {
  url="$1"
  output="$2"
  if [ -n "$TOKEN" ]; then
    curl -fsSL -H "Authorization: Bearer $TOKEN" "$url" -o "$output"
  else
    curl -fsSL "$url" -o "$output"
  fi
}

printf '%s\n' "Installing Recontrol Lang..."
download "$BASE_URL/$BINARY_ASSET" "$TMP_DIR/$BINARY_ASSET"
download "$BASE_URL/$RUNTIME_ASSET" "$TMP_DIR/$RUNTIME_ASSET"

if download "$BASE_URL/SHA256SUMS" "$TMP_DIR/SHA256SUMS" 2>/dev/null; then
  if command -v sha256sum >/dev/null 2>&1; then
    (
      cd "$TMP_DIR"
      grep "  $BINARY_ASSET\$" SHA256SUMS > selected-sums
      grep "  $RUNTIME_ASSET\$" SHA256SUMS >> selected-sums
      sha256sum -c selected-sums
    )
  else
    printf '%s\n' "Warning: sha256sum is unavailable; checksum verification skipped." >&2
  fi
else
  printf '%s\n' "Warning: SHA256SUMS is unavailable; checksum verification skipped." >&2
fi

mkdir -p "$INSTALL_DIR"
cp "$TMP_DIR/$BINARY_ASSET" "$INSTALL_DIR/rcl"
cp "$TMP_DIR/$RUNTIME_ASSET" "$INSTALL_DIR/librcl_runtime.a"
chmod +x "$INSTALL_DIR/rcl"

printf '%s\n' "Installed rcl to $INSTALL_DIR/rcl"
printf '%s\n' "Installed runtime to $INSTALL_DIR/librcl_runtime.a"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf '%s\n' "Add $INSTALL_DIR to PATH, for example:" "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

"$INSTALL_DIR/rcl" --version
