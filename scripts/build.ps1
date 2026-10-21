param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [switch]$SkipNative,
    [switch]$SkipDriver,
    [switch]$SkipMedia,
    [switch]$SkipChecks
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $null = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $root
    if (-not $SkipNative) { & "$PSScriptRoot/build-native.ps1" -Configuration $Configuration -RunTests:(-not $SkipChecks) }
    if (-not $SkipMedia) { & "$PSScriptRoot/prepare-ffmpeg.ps1" }
    if (-not $SkipDriver) {
        $env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
        $env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
        $dotnet = Join-Path $root 'build/tools/dotnet/dotnet.exe'
        $installed = Get-Command dotnet -ErrorAction SilentlyContinue
        if ($installed -and ((& $installed.Source --list-sdks) -match '^10\.0\.3')) { $dotnet = $installed.Source }
        if (-not (Test-Path -LiteralPath $dotnet)) {
            $toolsDirectory = Join-Path $root 'build/tools'
            New-Item -ItemType Directory -Force -Path $toolsDirectory | Out-Null
            $installer = Join-Path $toolsDirectory 'dotnet-install.ps1'
            Invoke-WebRequest -Uri 'https://dot.net/v1/dotnet-install.ps1' -OutFile $installer -ConnectionTimeoutSeconds 30
            & $installer -Version '10.0.301' -InstallDir (Join-Path $toolsDirectory 'dotnet') -AzureFeed 'https://dotnetcli.blob.core.windows.net/dotnet' -NoPath
            if ($LASTEXITCODE -ne 0) { throw '.NET SDK preparation failed.' }
        }
        # This is the standalone driver manager, not the original WPF main app.
        & $dotnet publish (Join-Path $root 'iPhoneMirror/src/DriverInstaller/iPhoneMirror.DriverInstaller.csproj') -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:PublishTrimmed=false -o (Join-Path $root 'build/driver')
        if ($LASTEXITCODE -ne 0) { throw 'Standalone driver manager publish failed.' }
        Copy-Item -LiteralPath (Join-Path $root 'build/driver/iPhoneMirror.Driver.exe') -Destination (Join-Path $root 'runtime/iPhoneMirror.Driver.exe') -Force
        $driverLicenses = Join-Path $root 'runtime/licenses/driver'
        New-Item -ItemType Directory -Force -Path $driverLicenses | Out-Null
        if (Test-Path -LiteralPath (Join-Path $root 'build/driver/licenses')) {
            Copy-Item -Path (Join-Path $root 'build/driver/licenses/*') -Destination $driverLicenses -Recurse -Force
        }
        foreach ($file in @('LICENSE.txt','ThirdPartyNotices.txt')) {
            $dotnetNotice = Join-Path (Split-Path -Parent $dotnet) $file
            if (Test-Path -LiteralPath $dotnetNotice) { Copy-Item -LiteralPath $dotnetNotice -Destination (Join-Path $driverLicenses ("dotnet-" + $file)) -Force }
        }
    }
    & (Join-Path $root 'iPhoneMirror/scripts/prepare_vc_runtime.ps1') -DestinationDirectory (Join-Path $root 'runtime') -AdditionalDestinationDirectories @((Join-Path $root 'runtime/Wireless'))
    if (-not $SkipChecks) {
        & cargo fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { throw 'Rust formatting check failed.' }
        & cargo clippy --locked --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Rust Clippy check failed.' }
        & cargo test --locked --lib
        if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    }
    $arguments = @('build', '--locked', '--bin', 'iPhonePlay')
    if ($Configuration -eq 'Release') { $arguments += '--release' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    $profile = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
    # Upstream USB core locates this helper next to the process executable.
    Copy-Item -LiteralPath (Join-Path $root 'runtime/iPhoneMirror.UsbConfigurationSwitch.exe') -Destination (Join-Path $root "target/$profile/iPhoneMirror.UsbConfigurationSwitch.exe") -Force
    Write-Output "Built $(Join-Path $root "target/$profile/iPhonePlay.exe")"
} finally { Pop-Location }
