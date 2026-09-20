#!/usr/bin/env sh
set -eu

REPO="plash3r/recontrol-lang"
INSTALL_DIR="${RCL_INSTALL_DIR:-$HOME/.local/bin}"
ASSET="rcl-linux-x86_64"
RUNTIME_ASSET="librcl_runtime.a"
TOOLCHAIN_ASSET="rcl-toolchain-linux-x86_64.tar.gz"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"

printf '%s\n' "Installing Recontrol Lang..."
mkdir -p "$INSTALL_DIR"
curl -fsSL "$URL" -o "$INSTALL_DIR/rcl"
curl -fsSL "https://github.com/$REPO/releases/latest/download/$RUNTIME_ASSET" -o "$INSTALL_DIR/$RUNTIME_ASSET"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://github.com/$REPO/releases/latest/download/$TOOLCHAIN_ASSET" -o "$tmp/toolchain.tar.gz"
tar -xzf "$tmp/toolchain.tar.gz" -C "$INSTALL_DIR"
chmod +x "$INSTALL_DIR/rcl" "$INSTALL_DIR/rcl-toolchain/bin/llc" "$INSTALL_DIR/rcl-toolchain/bin/ld.lld"

printf '%s\n' "Installed rcl to $INSTALL_DIR/rcl"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf '%s\n' "Add $INSTALL_DIR to PATH, for example:" "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

"$INSTALL_DIR/rcl" --version
