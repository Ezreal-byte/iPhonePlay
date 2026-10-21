param(
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$')][string]$Version = '0.1.1',
    [switch]$SkipInstaller,
    [switch]$SkipSource,
    [string]$InnoCompiler
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$dist = Join-Path $root 'dist'
$name = "iPhonePlay-$Version-windows-x64"
$payload = Join-Path $dist $name
$upstreamCommit = '31acd8a78e7038d46c7e1a6ee987b9035cf36dde'
$cargoVersion = [regex]::Match((Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw -Encoding utf8), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if ($Version -ne $cargoVersion) { throw "Package version $Version does not match Cargo.toml version $cargoVersion." }
if (-not $SkipInstaller) {
    if (-not $InnoCompiler) {
        $candidates = @((Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'), (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe'))
        $InnoCompiler = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
    if (-not $InnoCompiler) { throw 'Inno Setup 6 compiler not found. Supply -InnoCompiler or install Inno Setup 6.' }
    $innoDirectory = Split-Path -Parent $InnoCompiler
    $language = @((Join-Path $innoDirectory 'Languages/ChineseSimplified.isl'), (Join-Path $innoDirectory 'ChineseSimplified.isl')) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $language) { throw 'ChineseSimplified.isl is missing from the Inno Setup compiler installation.' }
}
$null = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $root
$executable = Join-Path $root 'target/release/iPhonePlay.exe'
if (-not (Test-Path -LiteralPath $executable)) { throw 'Build target/release/iPhonePlay.exe first with scripts/build.ps1.' }
$required = @('iPhoneMirror.Core.dll','iPhoneMirror.UsbConfigurationSwitch.exe','iPhoneMirror.Driver.exe','libusb-1.0.dll','libusb0.dll','msvcp140.dll','vcruntime140.dll','vcruntime140_1.dll','Wireless/iPhoneMirror.WirelessHost.exe','Wireless/airplay2dll.dll','Wireless/avcodec-58.dll','Wireless/avutil-56.dll','Wireless/swresample-3.dll','Wireless/swscale-5.dll','Wireless/dnssd.dll','Wireless/msvcp140.dll','Wireless/vcruntime140.dll','Wireless/vcruntime140_1.dll','tools/ffmpeg/ffmpeg.exe','tools/ffmpeg/LICENSE.txt','tools/ffmpeg/SOURCE.txt')
foreach ($entry in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $root "runtime/$entry") -PathType Leaf)) { throw "Missing release dependency: runtime/$entry" }
}
$ffmpegManifest = Import-PowerShellDataFile (Join-Path $root 'runtime/tools/ffmpeg/runtime-manifest.psd1')
foreach ($entry in $ffmpegManifest.Files.GetEnumerator()) {
    $hash = (Get-FileHash -LiteralPath (Join-Path $root "runtime/tools/ffmpeg/$($entry.Key)") -Algorithm SHA256).Hash
    if ($hash -ne $entry.Value) { throw "FFmpeg dependency hash mismatch: $($entry.Key)" }
}
New-Item -ItemType Directory -Force -Path $dist | Out-Null
if (Test-Path -LiteralPath $payload) {
    # Only this generated version directory may be removed. Never follow a
    # caller-provided path outside the resolved distribution directory.
    $resolved = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $payload).Path)
    $expected = [IO.Path]::GetFullPath((Join-Path $dist $name))
    if ($resolved -ne $expected -or -not $resolved.StartsWith($dist + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Unsafe package directory: $resolved" }
    if ((Get-Item -LiteralPath $payload).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing to replace a reparse-point package directory.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $payload,(Join-Path $payload 'licenses') | Out-Null
if (-not $SkipInstaller) {
    Copy-Item -LiteralPath (Join-Path $innoDirectory 'license.txt') -Destination (Join-Path $payload 'licenses/Inno-Setup-LICENSE.txt') -Force
}
Copy-Item -LiteralPath $executable -Destination (Join-Path $payload 'iPhonePlay.exe')
Copy-Item -Path (Join-Path $root 'runtime/*') -Destination $payload -Recurse -Force
foreach ($file in @('LICENSE','README.md','THIRD_PARTY_NOTICES.md','UPSTREAM.md','design-qa.md')) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $file))) { throw "Missing release documentation: $file" }
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $payload -Force
}
if (Test-Path -LiteralPath (Join-Path $root 'validation')) { Copy-Item -LiteralPath (Join-Path $root 'validation') -Destination $payload -Recurse -Force }
$upstreamLicenses = Join-Path $payload 'licenses/upstream'
New-Item -ItemType Directory -Force -Path $upstreamLicenses | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'iPhoneMirror/LICENSE') -Destination (Join-Path $upstreamLicenses 'iPhoneMirror-GPL-3.0.txt')
Copy-Item -LiteralPath (Join-Path $root 'iPhoneMirror/THIRD_PARTY_NOTICES.md') -Destination (Join-Path $upstreamLicenses 'iPhoneMirror-THIRD_PARTY_NOTICES.md')
Copy-Item -LiteralPath (Join-Path $root 'iPhoneMirror/third_party/libusb/COPYING') -Destination (Join-Path $upstreamLicenses 'libusb-LGPL-2.1.txt')
Copy-Item -LiteralPath (Join-Path $root 'iPhoneMirror/third_party/libusb-win32/COPYING_LGPL.txt') -Destination (Join-Path $upstreamLicenses 'libusb-win32-import-LGPL.txt')
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $root 'iPhoneMirror/third_party/airplay-server') -File | Where-Object Name -Match '^(LICENSE|NOTICE|SOURCE)') {
    Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $upstreamLicenses ("AirPlayServer-" + $file.Name))
}
$driverLicenseRoot = Join-Path $root 'iPhoneMirror/src/DriverInstaller/Assets/libusb-win32-1.2.6.0'
foreach ($file in @('COPYING_GPL.txt','COPYING_LGPL.txt','AUTHORS.txt','README.txt')) {
    Copy-Item -LiteralPath (Join-Path $driverLicenseRoot $file) -Destination (Join-Path $upstreamLicenses ("libusb-win32-driver-" + $file))
}

# Retain exact licenses from the Cargo.lock-resolved dependency sources.
Push-Location $root
try { $metadata = (& cargo metadata --locked --format-version 1 | ConvertFrom-Json) } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Unable to obtain locked Rust dependency metadata.' }
$rustNotices = @('# Rust dependency licenses', '', 'Generated from the Cargo.lock dependency graph. License texts are in the adjacent crate directories.', '')
foreach ($package in $metadata.packages | Where-Object { $_.name -ne 'iphoneplay' } | Sort-Object name,version) {
    $rustNotices += "- $($package.name) $($package.version): $($package.license); $($package.repository)"
    $sourceDirectory = Split-Path -Parent $package.manifest_path
    $files = @(Get-ChildItem -LiteralPath $sourceDirectory -Recurse -File | Where-Object {
        $_.Name -match '(LICENSE|LICENCE|COPYING|NOTICE|^UNLICENSE|^OFL|^UFL)' -or $_.FullName -match '[\\/]fonts[\\/].*\.txt$'
    })
    if ($files.Count -gt 0) {
        $destination = Join-Path $payload "licenses/rust/$($package.name)-$($package.version)"
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        foreach ($file in $files) {
            $target = Join-Path $destination ([IO.Path]::GetRelativePath($sourceDirectory, $file.FullName))
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $target
        }
    }
}
$egui = $metadata.packages | Where-Object { $_.name -eq 'egui' } | Select-Object -First 1
if ($egui) {
    # These egui crate packages reference parent-repository licenses that are
    # absent from the crates.io tarball. Prefer the crate's VCS revision;
    # some published revisions are not public, so use its exact release tag.
    $vcs = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $egui.manifest_path) '.cargo_vcs_info.json') -Encoding utf8 | ConvertFrom-Json
    $eguiNotices = Join-Path $payload 'licenses/rust/egui-shared'
    New-Item -ItemType Directory -Force -Path $eguiNotices | Out-Null
    foreach ($licenseName in @('LICENSE-APACHE','LICENSE-MIT')) {
        $licenseDestination = Join-Path $eguiNotices $licenseName
        try {
            Invoke-WebRequest -Uri "https://raw.githubusercontent.com/emilk/egui/$($vcs.git.sha1)/$licenseName" -OutFile $licenseDestination -ConnectionTimeoutSeconds 30
        } catch {
            if ($_.Exception.Response.StatusCode -ne [Net.HttpStatusCode]::NotFound) { throw }
            Invoke-WebRequest -Uri "https://raw.githubusercontent.com/emilk/egui/$($egui.version)/$licenseName" -OutFile $licenseDestination -ConnectionTimeoutSeconds 30
        }
    }
}
$rustNotices | Set-Content -LiteralPath (Join-Path $payload 'licenses/RUST-DEPENDENCIES.md') -Encoding utf8NoBOM

$forbidden = @(Get-ChildItem -LiteralPath $payload -Recurse -File | Where-Object { $_.Name -match '^(iPhoneMirror\.exe|iUsbBridge\.exe|python.*\.dll|usb_touch_bridge\.py)$' -or $_.FullName -match '[\\/]DDI[\\/]' })
if ($forbidden.Count) { throw "Out-of-scope runtime detected: $($forbidden[0].FullName)" }
$manifest = [ordered]@{
    product = 'iPhonePlay'; version = $Version; platform = 'windows-x64'; abi = 18
    upstream_commit = $upstreamCommit; rust_toolchain = '1.96.0'; created_utc = [DateTime]::UtcNow.ToString('o')
    files = @(Get-ChildItem -LiteralPath $payload -Recurse -File | ForEach-Object {
        [ordered]@{ path = [IO.Path]::GetRelativePath($payload, $_.FullName).Replace('\','/'); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $payload 'manifest.json') -Encoding utf8NoBOM
$portable = Join-Path $dist "$name.zip"
Compress-Archive -LiteralPath $payload -DestinationPath $portable -CompressionLevel Optimal -Force

if (-not $SkipSource) {
    $sourceStage = Join-Path $root ('build/source-' + [Guid]::NewGuid().ToString('N'))
    & (Join-Path $PSScriptRoot 'export-source.ps1') -Destination $sourceStage -ProjectRoot $root
    Compress-Archive -Path (Join-Path $sourceStage '*') -DestinationPath (Join-Path $dist "iPhonePlay-$Version-source.zip") -CompressionLevel Optimal -Force
}

if (-not $SkipInstaller) {
    & $InnoCompiler "/DPayloadDir=$payload" "/DOutputDir=$dist" "/DAppVersion=$Version" "/DChineseLanguageFile=$language" (Join-Path $root 'installer/iPhonePlay.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }
}
Get-ChildItem -LiteralPath $dist -File | Where-Object { $_.Name -like "iPhonePlay-$Version*" -and $_.Extension -in @('.zip','.exe') } | ForEach-Object {
    "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
} | Set-Content -LiteralPath (Join-Path $dist "iPhonePlay-$Version-SHA256SUMS.txt") -Encoding utf8NoBOM
Write-Output "Release artifacts staged at $dist. No installation or upload has been performed."
