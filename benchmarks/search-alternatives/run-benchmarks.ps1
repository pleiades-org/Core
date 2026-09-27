$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --offline --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    New-Item -ItemType Directory -Force -Path results | Out-Null
    foreach ($runNumber in 1..3) {
        & '.\target\release\core-search-alternatives.exe' $runNumber |
            Set-Content -LiteralPath "results\run-$runNumber.csv" -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Benchmark run $runNumber failed." }
        Write-Output "Completed search experiment run $runNumber"
    }
    $allRows = foreach ($runNumber in 1..3) { Import-Csv "results\run-$runNumber.csv" }
    $summary = foreach ($group in ($allRows | Group-Object -Property kind, case)) {
        $rows = $group.Group
        $timings = @($rows | ForEach-Object { [double]$_.median_ns } | Sort-Object)
        $tails = @($rows | ForEach-Object { [double]$_.p95_ns } | Sort-Object)
        $retained = @($rows | ForEach-Object { [double]$_.retained_bytes } | Sort-Object)
        $peak = @($rows | ForEach-Object { [double]$_.peak_bytes } | Sort-Object)
        [pscustomobject]@{
            Kind = $rows[0].kind; Case = $rows[0].case; SamplesPerRun = $rows[0].samples
            MedianNs = $timings[1]; MinRunMedianNs = $timings[0]; MaxRunMedianNs = $timings[2]
            MedianRunP95Ns = $tails[1]; AllocationCalls = $rows[0].allocation_calls
            RequestedBytes = $rows[0].requested_bytes; RetainedBytes = $retained[1]; PeakBytes = $peak[1]
        }
    }
    $summary | Export-Csv -LiteralPath 'results\summary.csv' -NoTypeInformation -Encoding utf8
    [pscustomobject]@{
        Date = (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK')
        Processor = (Get-ItemProperty -LiteralPath 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name ProcessorNameString).ProcessorNameString.Trim()
        LogicalProcessors = [Environment]::ProcessorCount
        OperatingSystem = [Environment]::OSVersion.VersionString; Rust = (& rustc -V)
        Build = 'release; opt-level=3; thin LTO; codegen-units=1; no target-cpu=native'
        Runs = 3; Clock = 'std::time::Instant; one query per sample, except explicitly named 13-query typing sequences'
        Corpus = 'Deterministic synthetic names; 2k, 50k, 150k; no filesystem reads or personal data'
        Limitations = 'Warm caches; desktop not isolated or affinity pinned; allocator counts requested heap bytes, not RSS/private bytes/GPU/allocator overhead; no end-to-end UI or cold disk measurements'
    } | ConvertTo-Json | Set-Content -LiteralPath 'results\environment.json' -Encoding utf8
    Get-ChildItem src -File -Recurse | Get-FileHash -Algorithm SHA256 | Select-Object Path, Hash |
        Export-Csv -LiteralPath 'results\harness-hashes.csv' -NoTypeInformation -Encoding utf8
} finally { Pop-Location }
