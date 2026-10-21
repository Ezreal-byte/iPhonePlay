param(
    [Parameter(Mandatory)][string]$Destination,
    [string]$ProjectRoot = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($ProjectRoot)
$destinationPath = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $destinationPath) {
    if (@(Get-ChildItem -LiteralPath $destinationPath -Force).Count) { throw 'Source export destination must be empty.' }
}
$upstream = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $root
New-Item -ItemType Directory -Force -Path $destinationPath | Out-Null
foreach ($entry in @('src','assets','scripts','installer','validation','design','Cargo.toml','Cargo.lock','rust-toolchain.toml','build.rs','README.md','LICENSE','THIRD_PARTY_NOTICES.md','UPSTREAM.md','design-qa.md','.gitignore')) {
    $path = Join-Path $root $entry
    if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $destinationPath -Recurse -Force }
}
$nativeSource = Join-Path $destinationPath 'iPhoneMirror'
New-Item -ItemType Directory -Force -Path $nativeSource | Out-Null
if ($upstream.Mode -eq 'git') {
    $archive = Join-Path $destinationPath 'upstream-export.tmp.zip'
    & git -C $upstream.SourceRoot archive --format=zip "--output=$archive" $upstream.Commit
    if ($LASTEXITCODE -ne 0) { throw 'Unable to export the pinned upstream source.' }
    Expand-Archive -LiteralPath $archive -DestinationPath $nativeSource
    Remove-Item -LiteralPath $archive -Force
    $manifest = [ordered]@{
        schema_version = 1
        repository = 'https://github.com/RayrenSX/iPhoneMirror'
        commit = $upstream.Commit
        files = @(Get-ChildItem -LiteralPath $nativeSource -Recurse -File -Force | Sort-Object FullName | ForEach-Object {
            [ordered]@{ path = [IO.Path]::GetRelativePath($nativeSource, $_.FullName).Replace('\','/'); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
        })
    }
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $nativeSource 'iphoneplay-source-manifest.json') -Encoding utf8NoBOM
} else {
    # Repack a source ZIP without Git or local build outputs. The verified
    # manifest is the allowlist and remains unchanged across repackaging.
    $manifest = Get-Content -LiteralPath $upstream.ManifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($entry in $manifest.files) {
        $target = Join-Path $nativeSource $entry.path
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath (Join-Path $upstream.SourceRoot $entry.path) -Destination $target
    }
    Copy-Item -LiteralPath $upstream.ManifestPath -Destination (Join-Path $nativeSource 'iphoneplay-source-manifest.json')
}
$verified = & (Join-Path $PSScriptRoot 'verify-upstream.ps1') -ProjectRoot $destinationPath
@('# iPhonePlay source archive', '', 'This folder is directly buildable after installing the documented toolchains. No Git metadata or second extraction step is required.', '', 'Run ./scripts/verify-upstream.ps1, then ./scripts/build.ps1. The iPhoneMirror/iphoneplay-source-manifest.json records the fixed upstream commit and SHA-256 of every archived upstream file. Build and media preparation scripts verify those bytes before use. Local generated build, obj, bin and work directories are not considered upstream source and are not included when repackaging.', '', 'See README.md and UPSTREAM.md for Rust, MSVC, .NET and FFmpeg prerequisites and source/license records.') | Set-Content -LiteralPath (Join-Path $destinationPath 'SOURCE-ARCHIVE.md') -Encoding utf8NoBOM
Write-Output "Source exported to $destinationPath ($($verified.FileCount) verified upstream files; commit $($verified.Commit))."
