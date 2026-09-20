#!/usr/bin/env bash
set -euo pipefail

REPO="plash3r/recontrol-lang"
echo "Finding the latest Recontrol Lang VS Code release..."

asset_url="$(
  curl -fsSL -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases?per_page=20" |
  python3 -c '
import json, sys
releases = json.load(sys.stdin)
for release in releases:
    if release.get("tag_name", "").startswith("vscode-v"):
        for asset in release.get("assets", []):
            if asset.get("name", "").endswith(".vsix"):
                print(asset["browser_download_url"])
                raise SystemExit
raise SystemExit("No VS Code extension release with a VSIX asset exists yet.")
'
)"

tmp="$(mktemp --suffix=.vsix)"
trap 'rm -f "$tmp"' EXIT
echo "Downloading VSIX..."
curl -fL "$asset_url" -o "$tmp"

if command -v code >/dev/null 2>&1; then
  code --install-extension "$tmp" --force
  echo "Recontrol Lang extension installed successfully."
else
  echo "VS Code 'code' command was not found."
  echo "VSIX downloaded to: $tmp"
  echo "Install it in VS Code with Extensions -> ... -> Install from VSIX..."
  trap - EXIT
fi
