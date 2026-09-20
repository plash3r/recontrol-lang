$ErrorActionPreference = "Stop"

$repo = "plash3r/recontrol-lang"
$api = "https://api.github.com/repos/$repo/releases?per_page=20"

Write-Host "Finding the latest Recontrol Lang VS Code release..."
$releases = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "recontrol-lang-installer" }
$release = $releases | Where-Object { $_.tag_name -like "vscode-v*" } | Select-Object -First 1

if (-not $release) { throw "No VS Code extension release exists yet." }

$asset = $release.assets | Where-Object { $_.name -like "recontrol-lang-*.vsix" } | Select-Object -First 1
if (-not $asset) { throw "The selected release does not contain a VSIX asset." }

$tmp = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name)..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -UseBasicParsing

if (Get-Command code -ErrorAction SilentlyContinue) {
  & code --install-extension $tmp --force
  if ($LASTEXITCODE -ne 0) { throw "VS Code failed to install the extension." }
  Write-Host "Recontrol Lang extension installed successfully."
} else {
  Write-Host "VS Code 'code' command was not found."
  Write-Host "The VSIX was downloaded to: $tmp"
  Write-Host "Install it in VS Code with Extensions -> ... -> Install from VSIX..."
}
