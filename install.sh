#!/usr/bin/env sh
set -eu

REPO="plash3r/recontrol-lang"
INSTALL_DIR="${RCL_INSTALL_DIR:-$HOME/.local/bin}"
ASSET="rcl-linux-x86_64"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"

printf '%s\n' "Installing Recontrol Lang..."
mkdir -p "$INSTALL_DIR"
curl -fsSL "$URL" -o "$INSTALL_DIR/rcl"
chmod +x "$INSTALL_DIR/rcl"

printf '%s\n' "Installed rcl to $INSTALL_DIR/rcl"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf '%s\n' "Add $INSTALL_DIR to PATH, for example:" "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

"$INSTALL_DIR/rcl" --version
