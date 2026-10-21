param(
    [ValidateSet('Debug','Release','RelWithDebInfo')][string]$Configuration = 'Release',
    [switch]$RunTests,
    [int]$Parallel = 6
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'iPhoneMirror'
$verifiedSource = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $root
$expected = $verifiedSource.Commit
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsInfo = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json | ConvertFrom-Json) | Select-Object -First 1
if (-not $vsInfo) { throw 'Install Visual Studio C++ Build Tools and the Windows SDK.' }
$vs = $vsInfo.installationPath
$generator = switch (([version]$vsInfo.installationVersion).Major) {
    18 { 'Visual Studio 18 2026' }
    17 { 'Visual Studio 17 2022' }
    default { throw 'Visual Studio 2022 or 2026 C++ tools are required.' }
}
$cmake = Join-Path $vs 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'
$ctest = Join-Path $vs 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/ctest.exe'
$build = Join-Path $root 'build/native'
$runtime = Join-Path $root 'runtime'
New-Item -ItemType Directory -Force -Path $build,$runtime,(Join-Path $runtime 'Wireless') | Out-Null
& $cmake -S $source -B $build -G $generator -A x64 -DIPHONEMIRROR_BUILD_TESTS=ON -DIPHONEMIRROR_BUILD_DANGEROUS_USB_TOOLS=OFF
if ($LASTEXITCODE -ne 0) { throw 'CMake configure failed.' }
$targets = @('iPhoneMirror.Core','iPhoneMirror.WirelessHost','iPhoneMirror.UsbConfigurationSwitch')
if ($RunTests) {
    $targets += @('iPhoneMirror.Core.Tests','iPhoneMirror.QtUsbTransport.Tests','iPhoneMirror.LibUsb0Activation.Tests','iPhoneMirror.OutputModeState.Tests','iPhoneMirror.UsbConfigurationRestorePolicy.Tests','iPhoneMirror.UsbConfigurationSwitchIdentity.Tests','iPhoneMirror.UsbDeviceReconnect.Tests','iPhoneMirror.IpcWriter.Tests','iPhoneMirror.DnsSdRegistrationPolicy.Tests','iPhoneMirror.WirelessHost.Smoke','iPhoneMirror.WirelessHost.PreflightSmoke')
}
& $cmake --build $build --config $Configuration --target @targets --parallel $Parallel
if ($LASTEXITCODE -ne 0) { throw 'Native build failed.' }
$coreOutput = Join-Path $build "src/Core/$Configuration"
$wirelessOutput = Join-Path $build "src/WirelessHost/$Configuration"
Copy-Item -LiteralPath (Join-Path $coreOutput 'iPhoneMirror.Core.dll'),(Join-Path $coreOutput 'libusb-1.0.dll'),(Join-Path $coreOutput 'iPhoneMirror.UsbConfigurationSwitch.exe') -Destination $runtime -Force
Copy-Item -LiteralPath (Join-Path $source 'src/DriverInstaller/Assets/libusb-win32-1.2.6.0/amd64/libusb0.dll') -Destination $runtime -Force
foreach ($file in @('iPhoneMirror.WirelessHost.exe','airplay2dll.dll','avcodec-58.dll','avutil-56.dll','swresample-3.dll','swscale-5.dll','dnssd.dll')) {
    Copy-Item -LiteralPath (Join-Path $wirelessOutput $file) -Destination (Join-Path $runtime 'Wireless') -Force
}
if ($RunTests) {
    & $ctest --test-dir $build -C $Configuration --output-on-failure -E 'WirelessReceiverProtocolSmoke|WirelessReceiverMediaRouteSmoke|UxPlayHostSmoke|MediaSourceActivateTests|VirtualCameraComponentTests'
    if ($LASTEXITCODE -ne 0) { throw 'Native tests failed.' }
}
Write-Output "Native runtime copied to $runtime (ABI 18, upstream $expected)."
