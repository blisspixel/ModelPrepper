# Install an official native release, or an explicitly verified offline archive.
[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'ModelPrepper\bin'),
    [string]$ArchivePath,
    [string]$Sha256,
    [switch]$NoPath
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Use install.sh on Linux or macOS.' }
$architecture = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $architecture = $env:PROCESSOR_ARCHITEW6432 }
if ($architecture -ne 'AMD64') { throw 'This installer supports native Windows x64. Other architectures are not qualified.' }
if ([string]::IsNullOrWhiteSpace($InstallDir)) { throw 'InstallDir cannot be empty.' }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$work = Join-Path ([IO.Path]::GetTempPath()) ('modelprepper-install-' + [guid]::NewGuid())
$candidate = $null
$backup = $null
New-Item -ItemType Directory -Path $work | Out-Null
$originalTls = [Net.ServicePointManager]::SecurityProtocol
try {
    [Net.ServicePointManager]::SecurityProtocol = $originalTls -bor [Net.SecurityProtocolType]::Tls12
    if ($ArchivePath) {
        if ($Version -eq 'latest' -or !$Sha256) { throw 'Offline installation requires -Version and -Sha256 from trusted release checksums.' }
    } else {
        if ($Sha256) { throw '-Sha256 is only supported with -ArchivePath.' }
        if ($Version -eq 'latest') {
            try {
                $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/blisspixel/ModelPrepper/releases/latest' -TimeoutSec 60
                $Version = $release.tag_name
            } catch { throw 'No stable release could be resolved. Build from source or specify a published version.' }
        }
    }
    if ($Version -cnotmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$') { throw 'Version must be a release tag such as v0.1.0.' }
    $asset = "modelprepper-$Version-x86_64-pc-windows-msvc.zip"
    if (!$ArchivePath) {
        $base = "https://github.com/blisspixel/ModelPrepper/releases/download/$Version"
        $manifest = Join-Path $work 'SHA256SUMS'
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $manifest -TimeoutSec 120
        $matchesFound = @(Get-Content -LiteralPath $manifest | Where-Object {
            $fields = $_ -split '\s+'
            $fields.Count -eq 2 -and $fields[1] -ceq $asset
        })
        if ($matchesFound.Count -ne 1 -or $matchesFound[0] -cnotmatch ('^[0-9a-f]{64}  ' + [regex]::Escape($asset) + '$')) { throw 'Release checksum entry is missing, invalid, or duplicated.' }
        $Sha256 = $matchesFound[0].Substring(0, 64)
        $ArchivePath = Join-Path $work $asset
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $ArchivePath -TimeoutSec 120
    }
    if ($Sha256 -cnotmatch '^[0-9a-f]{64}$') { throw 'Sha256 must be 64 lowercase hexadecimal characters.' }
    $hashStream = [IO.File]::OpenRead([IO.Path]::GetFullPath($ArchivePath))
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $actualHash = [BitConverter]::ToString($hasher.ComputeHash($hashStream)).Replace('-', '').ToLowerInvariant() }
    finally { $hashStream.Dispose(); $hasher.Dispose() }
    if ($actualHash -cne $Sha256) { throw 'Archive checksum mismatch. Existing installation was retained.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead([IO.Path]::GetFullPath($ArchivePath))
    try {
        $executable = Join-Path $work 'modelprepper.exe'
        $names = @($zip.Entries | ForEach-Object { $_.FullName } | Sort-Object)
        if ($zip.Entries.Count -ne 2 -or ($names -join ',') -cne 'LICENSE,modelprepper.exe') { throw 'Archive must contain exactly modelprepper.exe and LICENSE.' }
        foreach ($entry in $zip.Entries) {
            $kind = ($entry.ExternalAttributes -shr 16) -band 0xF000
            if ($kind -ne 0 -and $kind -ne 0x8000) { throw 'Archive entries must be regular files.' }
            if ($entry.Length -gt 134217728) { throw 'Archive entry exceeds the 128 MiB installer limit.' }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $work $entry.FullName), $false)
        }
    } finally { $zip.Dispose() }
    $actualVersion = & $executable --version
    if ($LASTEXITCODE -ne 0 -or $actualVersion -cne "modelprepper $($Version.Substring(1))") { throw 'Binary version does not match release tag.' }
    $walk = $InstallDir
    while ($walk) {
        if (Test-Path -LiteralPath $walk) {
            if ((Get-Item -Force -LiteralPath $walk).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Install directory cannot have symlink or reparse-point ancestors.' }
        }
        $parent = [IO.Directory]::GetParent($walk)
        if (!$parent) { break }
        $walk = $parent.FullName
    }
    $destination = Join-Path $InstallDir 'modelprepper.exe'
    if ((Test-Path -LiteralPath $destination) -and ((Get-Item -Force -LiteralPath $destination).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Existing executable cannot be a symlink or reparse point.' }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $licenseDestination = Join-Path $InstallDir 'modelprepper.LICENSE'
    if ((Test-Path -LiteralPath $licenseDestination) -and ((Get-Item -Force -LiteralPath $licenseDestination).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Existing license cannot be a symlink or reparse point.' }
    Copy-Item -LiteralPath (Join-Path $work 'LICENSE') -Destination $licenseDestination -Force
    $candidate = Join-Path $InstallDir ('.modelprepper-' + [guid]::NewGuid() + '.exe')
    Copy-Item -LiteralPath $executable -Destination $candidate
    if (Test-Path -LiteralPath $destination) {
        $backup = Join-Path $InstallDir ('.modelprepper-backup-' + [guid]::NewGuid() + '.exe')
        [IO.File]::Replace($candidate, $destination, $backup)
    }
    else { [IO.File]::Move($candidate, $destination) }
    $candidate = $null
    if (!$NoPath) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $parts = @($userPath -split ';' | Where-Object { $_ })
        if ($parts -notcontains $InstallDir) {
            [Environment]::SetEnvironmentVariable('Path', (($parts + $InstallDir) -join ';'), 'User')
        }
        if (@($env:Path -split ';') -notcontains $InstallDir) { $env:Path += ';' + $InstallDir }
    }
    Write-Output "Installed ModelPrepper $($Version.Substring(1)) at $destination"
    Write-Output 'Next: modelprepper config init --output config.local.toml --catalog catalog --volume models'
    if (!$NoPath) { Write-Output 'Open a new terminal to use the updated user PATH.' }
} finally {
    [Net.ServicePointManager]::SecurityProtocol = $originalTls
    if ($candidate -and (Test-Path -LiteralPath $candidate)) { Remove-Item -LiteralPath $candidate }
    if ($backup -and (Test-Path -LiteralPath $backup)) { Remove-Item -LiteralPath $backup }
    if (Test-Path -LiteralPath $work) {
        $resolvedWork = [IO.Path]::GetFullPath($work)
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
        if (!$resolvedWork.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
            [IO.Path]::GetFileName($resolvedWork) -notmatch '^modelprepper-install-[0-9a-f-]{36}$' -or
            ((Get-Item -Force -LiteralPath $resolvedWork).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing to clean an unexpected temporary directory.'
        }
        Remove-Item -LiteralPath $resolvedWork -Recurse -Force
    }
}
