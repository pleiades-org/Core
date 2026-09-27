$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --offline --locked
    if ($LASTEXITCODE -ne 0) { throw 'Follow-up build failed.' }
    New-Item -ItemType Directory -Force -Path results\followup | Out-Null
    foreach ($runNumber in 1..3) {
        & '.\target\release\core-search-alternatives.exe' followup $runNumber |
            Set-Content -LiteralPath "results\followup\run-$runNumber.csv" -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Follow-up run $runNumber failed." }
    }
    $allRows = foreach ($runNumber in 1..3) { Import-Csv "results\followup\run-$runNumber.csv" }
    $summary = foreach ($group in ($allRows | Group-Object -Property kind, case)) {
        $rows = $group.Group
        $medians = @($rows | ForEach-Object { [double]$_.median_ns } | Sort-Object)
        $tails = @($rows | ForEach-Object { [double]$_.p95_ns } | Sort-Object)
        [pscustomobject]@{
            Kind = $rows[0].kind; Case = $rows[0].case
            MedianNs = $medians[1]; MinRunMedianNs = $medians[0]; MaxRunMedianNs = $medians[2]
            MedianRunP95Ns = $tails[1]; RetainedBytes = $rows[0].retained_bytes; PeakBytes = $rows[0].peak_bytes
        }
    }
    $summary | Export-Csv -LiteralPath results\followup\summary.csv -NoTypeInformation -Encoding utf8
    Get-ChildItem src -File -Recurse | Get-FileHash -Algorithm SHA256 | Select-Object Path, Hash |
        Export-Csv -LiteralPath results\followup\harness-hashes.csv -NoTypeInformation -Encoding utf8
} finally { Pop-Location }
