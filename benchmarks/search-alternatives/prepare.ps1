$ErrorActionPreference = 'Stop'
$referenceRoot = Join-Path $PSScriptRoot '..\reference\Search'
$fixtureRoot = Join-Path $PSScriptRoot 'src\fixtures'
New-Item -ItemType Directory -Force -Path $fixtureRoot | Out-Null
$manifest = foreach ($sourceName in @('arena.rs', 'index.rs', 'search.rs', 'walker.rs', 'engine.rs')) {
    $sourcePath = Join-Path $referenceRoot "src\$sourceName"
    [pscustomobject]@{ File = $sourceName; SHA256 = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash }
}
Copy-Item -LiteralPath (Join-Path $referenceRoot 'src\arena.rs') -Destination (Join-Path $fixtureRoot 'arena.rs')
Copy-Item -LiteralPath (Join-Path $referenceRoot 'src\index.rs') -Destination (Join-Path $fixtureRoot 'index.rs')
$searchSource = Get-Content -LiteralPath (Join-Path $referenceRoot 'src\search.rs') -Raw
$boundary = $searchSource.IndexOf('#[derive(Debug, Clone, PartialEq, Eq)]')
if ($boundary -lt 0) { throw 'Search extraction boundary changed; inspect the source.' }
$filenameSource = $searchSource.Substring(0, $boundary).Replace('use rayon::prelude::*;', '').Replace('use memchr::memmem::Finder;', '')
Set-Content -LiteralPath (Join-Path $fixtureRoot 'filename.rs') -Value $filenameSource -Encoding utf8
$walkerSource = Get-Content -LiteralPath (Join-Path $referenceRoot 'src\walker.rs') -Raw
$deletionBoundary = $walkerSource.IndexOf('pub fn rebuild_subtree(')
if ($deletionBoundary -lt 0) { throw 'Deletion extraction boundary changed.' }
$deletionImports = @'
use crate::arena::IndexArena;
use std::path::{Path, PathBuf};
fn collect_paths(_: &Path) -> Vec<PathBuf> { panic!("fixture only tests a deleted, nonexistent directory") }

'@
Set-Content -LiteralPath (Join-Path $fixtureRoot 'deletion.rs') -Value ($deletionImports + $walkerSource.Substring($deletionBoundary)) -Encoding utf8
[pscustomobject]@{
    Repository = 'https://github.com/pleiades-org/Search'
    Commit = (& git -c "safe.directory=$((Resolve-Path $referenceRoot).Path.Replace('\', '/'))" -C $referenceRoot rev-parse HEAD)
    Sources = $manifest
    Adaptation = 'arena/index copied verbatim; filename search prefix unchanged except removal of two unused content-search imports. Deletion fixture preserves rebuild_subtree/path_within, with a panic stub for collection: tests only take the nonexistent-directory branch. No scanner/watch/cache/MFT is run.'
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'source-manifest.json') -Encoding utf8
