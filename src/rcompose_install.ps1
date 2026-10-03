# Installs the latest rcompose release for RC Desktop.
#
# Same behavior as https://ricardoborges.github.io/rcompose/install.ps1
# (per-user folder, user PATH entry, no admin rights), plus a SHA-256 check
# against the checksum published with the release.
#
# Environment: RCOMPOSE_INSTALL_DIR (set by RC Desktop), RCOMPOSE_VERSION (optional tag).

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'ricardoborges/rcompose'

$osArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$arch = switch ($osArch) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { throw "Unsupported architecture: $osArch" }
}

$asset = "rcompose-$arch-pc-windows-msvc.zip"
$version = $env:RCOMPOSE_VERSION
if ($version) {
    $base = "https://github.com/$repo/releases/download/$version"
} else {
    $version = 'latest'
    $base = "https://github.com/$repo/releases/latest/download"
}

$installDir = $env:RCOMPOSE_INSTALL_DIR
if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'Programs\rcompose' }

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rcompose-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Output "Downloading rcompose ($version, $arch)..."
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip -UseBasicParsing

    Write-Output "Verifying checksum..."
    $sumFile = Join-Path $tmp "$asset.sha256"
    Invoke-WebRequest -Uri "$base/$asset.sha256" -OutFile $sumFile -UseBasicParsing
    $expected = ((Get-Content -Path $sumFile -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw "Checksum mismatch for $asset (expected $expected, got $actual)" }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Get-ChildItem -Path $tmp -Filter 'rcompose.exe' -Recurse | Select-Object -First 1
    if (-not $exe) { throw "rcompose.exe not found in $asset" }

    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Copy-Item -Path $exe.FullName -Destination (Join-Path $installDir 'rcompose.exe') -Force
} finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "Installed rcompose to $installDir"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$entries = @($userPath -split ';' | Where-Object { $_ })
if ($entries -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', (($entries + $installDir) -join ';'), 'User')
    Write-Output "Added $installDir to your user PATH."
}
