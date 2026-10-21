param([Parameter(Mandatory)][string]$ArchiveRoot)
$ErrorActionPreference = 'Stop'
$verify = Join-Path $PSScriptRoot 'verify-upstream.ps1'
$root = [IO.Path]::GetFullPath($ArchiveRoot)
$info = & $verify -ProjectRoot $root
if ($info.Mode -ne 'archive') { throw 'This test must run against an isolated exported archive, never the upstream Git checkout.' }
$manifestPath = $info.ManifestPath
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$readme = Join-Path $info.SourceRoot 'README.md'
$readmeBytes = [IO.File]::ReadAllBytes($readme)
$passed = 1
function Assert-Rejected([string]$Expected) {
    try { $null = & $verify -ProjectRoot $root }
    catch {
        if ($_.Exception.Message -like "*$Expected*") { return }
        throw
    }
    throw "Expected source validation rejection: $Expected"
}
try {
    [IO.File]::AppendAllText($readme, "`ncorrupted-source-test`n", [Text.UTF8Encoding]::new($false))
    Assert-Rejected 'SHA-256 mismatch'
    $passed++
} finally { [IO.File]::WriteAllBytes($readme, $readmeBytes) }
try {
    Remove-Item -LiteralPath $readme
    Assert-Rejected 'source file is missing'
    $passed++
} finally { [IO.File]::WriteAllBytes($readme, $readmeBytes) }
try {
    $manifest = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json
    $manifest.commit = '0000000000000000000000000000000000000000'
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    Assert-Rejected 'incorrect pinned commit'
    $passed++
} finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
try {
    $manifest = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json
    $manifest.files[0].path = '../outside-the-source'
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    Assert-Rejected 'Invalid or duplicate upstream manifest path'
    $passed++
} finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
try {
    $manifest = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json
    $manifest.files[1].path = $manifest.files[0].path
    $manifest.files[1].bytes = $manifest.files[0].bytes
    $manifest.files[1].sha256 = $manifest.files[0].sha256
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    Assert-Rejected 'Invalid or duplicate upstream manifest path'
    $passed++
} finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
$null = & $verify -ProjectRoot $root
$passed++
Write-Output "Source archive validation: $passed/7 checks passed; all modified test files restored."
