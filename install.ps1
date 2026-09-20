$ErrorActionPreference = "Stop"

$repo = "plash3r/recontrol-lang"
$installDir = if ($env:RCL_INSTALL_DIR) { $env:RCL_INSTALL_DIR } else { Join-Path $HOME ".rcl\bin" }
$asset = "rcl-windows-x86_64.exe"
$runtimeAsset = "rcl_runtime.lib"
$toolchainAsset = "rcl-toolchain-windows-x86_64.zip"
$url = "https://github.com/$repo/releases/latest/download/$asset"
$target = Join-Path $installDir "rcl.exe"

Write-Host "Installing Recontrol Lang..."
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Invoke-WebRequest -Uri $url -OutFile $target
Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest/download/$runtimeAsset" -OutFile (Join-Path $installDir $runtimeAsset)
$tempZip = Join-Path $env:TEMP "rcl-toolchain.zip"
$tempDir = Join-Path $env:TEMP "rcl-toolchain-extract"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tempDir
Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest/download/$toolchainAsset" -OutFile $tempZip
Expand-Archive -Path $tempZip -DestinationPath $installDir -Force
Remove-Item -Force $tempZip

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $installDir } else { "$userPath;$installDir" }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
}

Write-Host "Installed rcl to $target"
Write-Host "Open a new terminal, then run: rcl --version"
& $target --version
