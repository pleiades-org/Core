param([string]$LegacyRoot = 'C:\Users\Robert\code\Rust\core')
$ErrorActionPreference = 'Stop'
$fixtureDirectory = Join-Path $PSScriptRoot 'src\fixtures'
New-Item -ItemType Directory -Force -Path $fixtureDirectory | Out-Null
$routerPath = Join-Path $LegacyRoot 'src\command_router.rs'
$filesPath = Join-Path $LegacyRoot 'src\file_index.rs'
$searchPath = Join-Path $LegacyRoot 'src\search_text.rs'
$routerLines = Get-Content -LiteralPath $routerPath
$fileLines = Get-Content -LiteralPath $filesPath
if ($routerLines[602] -ne 'struct ScopedQuery {' -or $fileLines[317] -notmatch '^pub fn scope_from_tag') {
    throw 'Legacy source changed; review fixture extraction ranges before running.'
}
$fileScope = @'
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSearchScope {
    AllFiles,
    Content,
    Videos,
    Images,
    Extension(String),
}
'@
$wrappers = @'
pub fn describe(query: &str) -> Option<(u8, String)> {
    let parsed = parse_scoped_query(query)?;
    std::hint::black_box(&parsed);
    let command = match parsed.scope {
        QueryScope::Applications => 1,
        QueryScope::Calculator => 2,
        QueryScope::Web => 3,
        QueryScope::Quicklinks => 4,
        _ => 255,
    };
    Some((command, parsed.search_text))
}
'@
$fixtureText = @(
    '// Exact legacy functions/types, extracted by prepare.ps1; wrapper below is benchmark-only.'
    '#![allow(dead_code)]'
    $fileScope
    ($fileLines[317..347] -join "`n")
    ($fileLines[599..604] -join "`n")
    ($routerLines[601..774] -join "`n")
    $wrappers
) -join "`n`n"
Set-Content -LiteralPath (Join-Path $fixtureDirectory 'legacy_router.rs') -Value $fixtureText -Encoding utf8
Copy-Item -LiteralPath $searchPath -Destination (Join-Path $fixtureDirectory 'legacy_search_text.rs')
$manifest = foreach ($sourcePath in @($routerPath, $filesPath, $searchPath)) {
    [pscustomobject]@{ Source=$sourcePath; SHA256=(Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash }
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'source-manifest.json') -Encoding utf8
