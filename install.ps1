# Recontrol Lang installer for Windows PowerShell
$ErrorActionPreference = "Stop"

$repo = if ($env:RCL_REPO) { $env:RCL_REPO } else { "plash3r/recontrol-lang" }
$version = if ($env:RCL_VERSION) { $env:RCL_VERSION } else { "latest" }
$installDir = if ($env:RCL_INSTALL_DIR) { $env:RCL_INSTALL_DIR } else { Join-Path $HOME ".rcl\bin" }
$token = if ($env:RCL_GITHUB_TOKEN) { $env:RCL_GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
$binaryAsset = "rcl-windows-x86_64.exe"
$runtimeAsset = "rcl-runtime-windows-x86_64.lib"
$binaryTarget = Join-Path $installDir "rcl.exe"
$runtimeTarget = Join-Path $installDir "rcl-runtime.lib"

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

$baseUrl = "https://github.com/$repo/releases/download/$releaseTag"
$tempDir = Join-Path ([IO.Path]::GetTempPath()) ("rcl-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
$binaryTemp = Join-Path $tempDir $binaryAsset
$runtimeTemp = Join-Path $tempDir $runtimeAsset
$checksumFile = Join-Path $tempDir "SHA256SUMS"

function Test-AssetChecksum {
    param(
        [string]$Asset,
        [string]$Path,
        [string[]]$Lines
    )
    $line = $Lines | Where-Object { $_ -match "\s+$([regex]::Escape($Asset))$" } | Select-Object -First 1
    if (-not $line) { throw "Checksum for $Asset is missing from SHA256SUMS." }

    $expected = ($line -split "\s+")[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw "SHA-256 verification failed for $Asset." }
}

try {
    Write-Host "Installing Recontrol Lang $releaseTag..."
    Invoke-WebRequest -Uri "$baseUrl/$binaryAsset" -Headers $headers -OutFile $binaryTemp
    Invoke-WebRequest -Uri "$baseUrl/$runtimeAsset" -Headers $headers -OutFile $runtimeTemp

    try {
        Invoke-WebRequest -Uri "$baseUrl/SHA256SUMS" -Headers $headers -OutFile $checksumFile
        $lines = Get-Content $checksumFile
        Test-AssetChecksum -Asset $binaryAsset -Path $binaryTemp -Lines $lines
        Test-AssetChecksum -Asset $runtimeAsset -Path $runtimeTemp -Lines $lines
        Write-Host "Checksums verified."
    } catch {
        Write-Warning "SHA256SUMS is not available or could not be verified; checksum verification skipped."
    }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Move-Item -Force $binaryTemp $binaryTarget
    Move-Item -Force $runtimeTemp $runtimeTarget

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $installDir) {
        $newPath = (($pathEntries + $installDir) | Where-Object { $_ -and $_.Trim() }) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    }

    Write-Host "Installed: $binaryTarget"
    Write-Host "Runtime:   $runtimeTarget"
    & $binaryTarget --version
    Write-Host ""
    Write-Host "Restart your terminal if 'rcl' is not yet on PATH."
}
finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
