[CmdletBinding()]
param(
    [switch]$Native,
    [switch]$Media,
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$logDirectory = Join-Path $root 'build\validation'
New-Item -ItemType Directory -Force -Path $logDirectory | Out-Null
$logPath = Join-Path $logDirectory ("rust-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
$cargo = (Get-Command cargo -ErrorAction Stop).Source
$offlineArguments = if ($Offline) { @('--offline') } else { @() }

function Invoke-CargoCheck {
    param([string]$Label, [string[]]$Arguments)
    $description = "[$(Get-Date -Format o)] $Label : cargo $($Arguments -join ' ')"
    Write-Host $description
    $description | Add-Content -LiteralPath $logPath -Encoding utf8NoBOM
    & $cargo @Arguments 2>&1 | Tee-Object -FilePath $logPath -Append | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$Label failed (exit $LASTEXITCODE). Log: $logPath" }
}

Push-Location $root
try {
    Invoke-CargoCheck 'Format' @('fmt', '--all', '--', '--check')
    Invoke-CargoCheck 'Clippy' (@('clippy', '--locked', '--all-targets') + $offlineArguments + @('--', '-D', 'warnings'))
    Invoke-CargoCheck 'Unit tests' (@('test', '--locked', '--all-targets') + $offlineArguments)

    # These are opt-in. Default validation never starts Bluetooth advertising,
    # an AirPlay receiver, FFmpeg, or asks the user to connect a phone.
    if ($Media) {
        $ffmpeg = Join-Path $root 'runtime\tools\ffmpeg\ffmpeg.exe'
        if (-not (Test-Path -LiteralPath $ffmpeg -PathType Leaf)) {
            throw 'FFmpeg runtime is missing. Run scripts/prepare-ffmpeg.ps1 before -Media.'
        }
        Invoke-CargoCheck 'FFmpeg synthetic integration and failure tests' (
            @('test', '--locked', '--lib', 'media::') + $offlineArguments +
            @('--', '--ignored', '--nocapture', '--test-threads=1'))
    }
    if ($Native) {
        if (-not (Test-Path -LiteralPath (Join-Path $root 'runtime\iPhoneMirror.Core.dll') -PathType Leaf)) {
            throw 'Native runtime is missing. Run scripts/build-native.ps1 before -Native.'
        }
        $nativeLogDirectory = Join-Path $env:LOCALAPPDATA 'iPhonePlay\logs'
        New-Item -ItemType Directory -Force -Path $nativeLogDirectory | Out-Null
        $previousNativeLog = $env:IPHONE_MIRROR_LOG_FILE
        try {
            $env:IPHONE_MIRROR_LOG_FILE = Join-Path $nativeLogDirectory 'native-smoke.log'
            Invoke-CargoCheck 'Real native receiver startup/shutdown smoke' (
                @('test', '--locked', '--lib', 'native::tests::native_receiver_start_stop_smoke') +
                $offlineArguments + @('--', '--ignored', '--nocapture', '--test-threads=1'))
        } finally {
            $env:IPHONE_MIRROR_LOG_FILE = $previousNativeLog
        }
    }
    Write-Output "Validation passed. Log: $logPath"
} finally {
    Pop-Location
}
