param([string]$ProjectRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
$expectedCommit = '31acd8a78e7038d46c7e1a6ee987b9035cf36dde'
$source = [IO.Path]::GetFullPath((Join-Path $ProjectRoot 'iPhoneMirror'))
if (-not (Test-Path -LiteralPath $source -PathType Container)) { throw "Missing pinned upstream source: $source" }

if (Test-Path -LiteralPath (Join-Path $source '.git')) {
    $actual = (& git -C $source rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -ne $expectedCommit) { throw "Native source must be pinned to $expectedCommit; found $actual" }
    & git -C $source diff --quiet HEAD --
    if ($LASTEXITCODE -ne 0) { throw 'Pinned upstream source has tracked changes. Review them before rebuilding the fixed baseline.' }
    return [pscustomobject]@{ Mode = 'git'; Commit = $expectedCommit; SourceRoot = $source; FileCount = @(& git -C $source ls-files).Count; ManifestPath = $null }
}

$manifestPath = Join-Path $source 'iphoneplay-source-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Upstream source has neither Git metadata nor iphoneplay-source-manifest.json. Use the complete source ZIP or the pinned checkout.' }
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.commit -ne $expectedCommit -or @($manifest.files).Count -eq 0) { throw 'Unsupported upstream source manifest or incorrect pinned commit.' }
$paths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $manifest.files) {
    $relative = [string]$entry.path
    if ([string]::IsNullOrWhiteSpace($relative) -or [IO.Path]::IsPathRooted($relative) -or $relative.Contains(':') -or $relative.Split([char[]]@('/', '\')) -contains '..' -or -not $paths.Add($relative.Replace('\','/'))) { throw "Invalid or duplicate upstream manifest path: $relative" }
    if ([string]$entry.sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw "Invalid upstream manifest SHA-256: $relative" }
    $file = [IO.Path]::GetFullPath((Join-Path $source $relative))
    if (-not $file.StartsWith($source + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Upstream manifest path escapes source directory: $relative" }
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Upstream source file is missing: $relative" }
    $item = Get-Item -LiteralPath $file
    if ($item.Length -ne [long]$entry.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Upstream source SHA-256 mismatch: $relative" }
}
foreach ($required in @('CMakeLists.txt','src/Core/include/iPhoneMirror/CoreApi.h','src/WirelessHost/WirelessHost.cpp','src/DriverInstaller/iPhoneMirror.DriverInstaller.csproj','scripts/ffmpeg-runtime-manifest.psd1')) {
    if (-not $paths.Contains($required)) { throw "Upstream manifest omits required source: $required" }
}
return [pscustomobject]@{ Mode = 'archive'; Commit = $expectedCommit; SourceRoot = $source; FileCount = $paths.Count; ManifestPath = $manifestPath }
