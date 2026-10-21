[CmdletBinding()]
param([string]$Destination = (Join-Path $PSScriptRoot '..\runtime\tools\ffmpeg'))
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$upstream = Join-Path $root 'iPhoneMirror'
$null = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $root
$stage = Join-Path $upstream 'work\iphoneplay-ffmpeg'
# The upstream recipe verifies the archive and executable against the pinned SHA-256 manifest.
& (Join-Path $upstream 'scripts\prepare_ffmpeg.ps1') -Destination $stage | Out-Host
$destinationPath = [IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Force -Path $destinationPath | Out-Null
foreach ($file in @('ffmpeg.exe', 'LICENSE.txt', 'README.txt', 'SOURCE.txt')) {
    Copy-Item -LiteralPath (Join-Path $stage $file) -Destination (Join-Path $destinationPath $file) -Force
}
Copy-Item -LiteralPath (Join-Path $upstream 'scripts\ffmpeg-runtime-manifest.psd1') -Destination (Join-Path $destinationPath 'runtime-manifest.psd1') -Force
Write-Output (Join-Path $destinationPath 'ffmpeg.exe')
