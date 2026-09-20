# Recontrol Lang installer for Windows PowerShell
$ErrorActionPreference = "Stop"

$repo = if ($env:RCL_REPO) { $env:RCL_REPO } else { "plash3r/recontrol-lang" }
$version = if ($env:RCL_VERSION) { $env:RCL_VERSION } else { "latest" }
$installDir = if ($env:RCL_INSTALL_DIR) { $env:RCL_INSTALL_DIR } else { Join-Path $HOME ".rcl\bin" }
$token = if ($env:RCL_GITHUB_TOKEN) { $env:RCL_GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
$asset = "rcl-windows-x86_64.exe"
$target = Join-Path $installDir "rcl.exe"

$headers = @{ "Accept" = "application/vnd.github+json" }
if ($token) { $headers["Authorization"] = "Bearer $token" }

if ($version -ne "latest" -and -not $version.StartsWith("v")) {
    $version = "v$version"
}

$api = "https://api.github.com/repos/$repo/releases"
$releaseUrl = if ($version -eq "latest") { "$api/latest" } else { "$api/tags/$version" }

try {
    $release = Invoke-RestMethod -Uri $releaseUrl -Headers $headers
} catch {
    throw "Could not access release '$version'. If the repository is private, set RCL_GITHUB_TOKEN or GH_TOKEN."
}

$releaseTag = $release.tag_name
if (-not $releaseTag) { throw "GitHub returned an invalid release response." }

$url = "https://github.com/$repo/releases/download/$releaseTag/$asset"
$checksumUrl = "https://github.com/$repo/releases/download/$releaseTag/SHA256SUMS"

$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("rcl-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
$tempFile = Join-Path $tempDir $asset
$checksumFile = Join-Path $tempDir "SHA256SUMS"

try {
    Write-Host "Installing Recontrol Lang $releaseTag ($asset)..."
    Invoke-WebRequest -Uri $url -Headers $headers -OutFile $tempFile

    $checksumOk = $false
    try {
        Invoke-WebRequest -Uri $checksumUrl -Headers $headers -OutFile $checksumFile
        $line = Get-Content $checksumFile | Where-Object { $_ -match "\s+$([regex]::Escape($asset))$" } | Select-Object -First 1
        if (-not $line) { throw "Checksum for $asset is missing from SHA256SUMS." }

        $expected = ($line -split "\s+")[0].ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 -Path $tempFile).Hash.ToLowerInvariant()
        if ($expected -ne $actual) { throw "SHA-256 verification failed." }

        $checksumOk = $true
        Write-Host "Checksum verified."
    } catch {
        Write-Warning "SHA256SUMS is not available; skipping checksum verification."
    }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Move-Item -Force $tempFile $target

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $installDir) {
        $newPath = (($pathEntries + $installDir) | Where-Object { $_ -and $_.Trim() }) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    }

    Write-Host "Installed: $target"
    & $target --version
    Write-Host ""
    Write-Host "Restart your terminal if 'rcl' is not yet on PATH."
}
finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
