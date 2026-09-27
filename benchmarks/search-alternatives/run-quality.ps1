$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force -Path target, results\quality | Out-Null
    rustc --edition 2021 -C opt-level=3 -C lto=thin -C codegen-units=1 launcher-quality.rs -o target\launcher-quality.exe
    if ($LASTEXITCODE -ne 0) { throw 'Quality experiment build failed.' }
    rustc --edition 2021 -C opt-level=3 --test launcher-quality.rs -o target\launcher-quality-tests.exe
    if ($LASTEXITCODE -ne 0) { throw 'Quality tests build failed.' }
    & '.\target\launcher-quality-tests.exe'
    if ($LASTEXITCODE -ne 0) { throw 'Quality checks failed.' }
    & '.\target\launcher-quality.exe' quality | Set-Content -LiteralPath results\quality\intent-results.csv -Encoding utf8
    foreach ($runNumber in 1..3) {
        & '.\target\launcher-quality.exe' $runNumber | Set-Content -LiteralPath "results\quality\run-$runNumber.csv" -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Quality timing run $runNumber failed." }
    }
    $allRows = foreach ($runNumber in 1..3) { Import-Csv "results\quality\run-$runNumber.csv" }
    $summary = foreach ($group in ($allRows | Group-Object mode, query)) {
        $medians = @($group.Group | ForEach-Object { [double]$_.median_ns } | Sort-Object)
        $tails = @($group.Group | ForEach-Object { [double]$_.p95_ns } | Sort-Object)
        [pscustomobject]@{
            Mode = $group.Group[0].mode; Query = $group.Group[0].query
            MedianNs = $medians[1]; MinRunMedianNs = $medians[0]; MaxRunMedianNs = $medians[2]; MedianRunP95Ns = $tails[1]
        }
    }
    $summary | Export-Csv -LiteralPath results\quality\summary.csv -NoTypeInformation -Encoding utf8
    Get-FileHash -LiteralPath launcher-quality.rs -Algorithm SHA256 | Select-Object Path, Hash | ConvertTo-Json |
        Set-Content -LiteralPath results\quality\source-hash.json -Encoding utf8
} finally { Pop-Location }
