$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
cargo test --manifest-path $manifest --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Calculator candidate checks failed.' }
cargo build --release --manifest-path $manifest --offline --locked
if ($LASTEXITCODE -ne 0) { throw 'Calculator benchmark build failed.' }
$executable = Join-Path $PSScriptRoot 'target\release\core-calculator-benchmarks.exe'
$measurements = Join-Path $projectRoot 'docs\measurements'
for ($run = 1; $run -le 3; $run++) {
    $arguments = @()
    if ($run -eq 2) { $arguments = @('--reverse') }
    & $executable @arguments 2> (Join-Path $measurements "calculator-alternatives-$run-scratch.txt") |
        Set-Content -LiteralPath (Join-Path $measurements "calculator-alternatives-$run.csv")
    if ($LASTEXITCODE -ne 0) { throw "Calculator benchmark run $run failed." }
}
$sourceFiles = @(
    (Join-Path $projectRoot 'crates\engine\src\calculator\evaluate.rs'),
    (Join-Path $projectRoot 'crates\engine\src\calculator\math_function.rs')
) + @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'src') -Filter '*.rs' | Select-Object -ExpandProperty FullName)
$sourceHashes = @($sourceFiles | ForEach-Object { Get-FileHash -Algorithm SHA256 -LiteralPath $_ } | Select-Object Path,Hash)
[pscustomobject]@{
    Date=(Get-Date -Format o)
    Processor=$env:PROCESSOR_IDENTIFIER
    Rust=(rustc -Vv | Out-String).Trim()
    BenchmarkSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash
    Profile='opt-level=3, thin LTO, one codegen unit'
    Method='Three processes, middle run reverses candidate order. 31 batches of 30000 operations per case. Median and p95 describe batch averages, not individual-query latency. Allocations sampled separately. Persistent scratch reused.'
    Sources=$sourceHashes
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $measurements 'calculator-alternatives-environment.json')
'Three calculator comparison runs saved.'
