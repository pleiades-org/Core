$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --offline --locked --bin line-search
    if ($LASTEXITCODE -ne 0) { throw 'Line search build failed.' }
    New-Item -ItemType Directory -Force -Path results\line-search | Out-Null
    $sourcePaths = @(rg --files 'C:\Users\Robert\code\Rust\core\src' '..\reference\Search\src' -g '*.rs' | ForEach-Object { (Resolve-Path -LiteralPath $_).Path } | Sort-Object)
    $sourcePaths | Set-Content -LiteralPath results\line-search\corpus-manifest.txt -Encoding utf8
    $sourcePaths | Get-FileHash -Algorithm SHA256 | Select-Object Path, Hash |
        Export-Csv -LiteralPath results\line-search\corpus-hashes.csv -NoTypeInformation -Encoding utf8
    foreach ($runNumber in 1..3) {
        & '.\target\release\line-search.exe' $runNumber 'results\line-search\corpus-manifest.txt' |
            Set-Content -LiteralPath "results\line-search\run-$runNumber.csv" -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Line search run $runNumber failed." }
        Write-Output "Line search run $runNumber complete"
    }
    $allRows = foreach ($runNumber in 1..3) { Import-Csv "results\line-search\run-$runNumber.csv" }
    $summary = foreach ($group in ($allRows | Group-Object -Property kind, case)) {
        $rows = $group.Group
        $medians = @($rows | ForEach-Object { [double]$_.median_ns } | Sort-Object)
        $tails = @($rows | ForEach-Object { [double]$_.p95_ns } | Sort-Object)
        $retained = @($rows | ForEach-Object { [double]$_.retained_bytes } | Sort-Object)
        $peaks = @($rows | ForEach-Object { [double]$_.peak_bytes } | Sort-Object)
        [pscustomobject]@{
            Kind = $rows[0].kind; Case = $rows[0].case; SamplesPerRun = $rows[0].samples
            MedianNs = $medians[1]; MinRunMedianNs = $medians[0]; MaxRunMedianNs = $medians[2]
            MedianRunP95Ns = $tails[1]; RetainedBytes = $retained[1]; PeakBytes = $peaks[1]
        }
    }
    $summary | Export-Csv -LiteralPath results\line-search\summary.csv -NoTypeInformation -Encoding utf8
    [pscustomobject]@{
        Date = (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK')
        Processor = (Get-ItemProperty -LiteralPath 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name ProcessorNameString).ProcessorNameString.Trim()
        OperatingSystem = [Environment]::OSVersion.VersionString; Rust = (& rustc -V)
        Build = 'release opt-level=3; thin LTO; codegen-units=1; no target-cpu=native'
        Runs = 3; Samples = 101; Warmups = 32
        Contract = 'case-sensitive UTF-8 byte literals; one result per matching line; deterministic document/line order; top12 and all separately; CRLF canonicalized to LF before timing'
        Limitations = 'All content/indexes resident; no disk I/O, process startup, update cost, CPU affinity, parallel workers, regex, fuzzy or Unicode case folding; no whole-process memory measurement'
    } | ConvertTo-Json | Set-Content -LiteralPath results\line-search\environment.json -Encoding utf8
    Get-Item src\bin\line-search.rs, src\measurement.rs, src\line_search\*.rs | Get-FileHash -Algorithm SHA256 | Select-Object Path, Hash |
        Export-Csv -LiteralPath results\line-search\harness-hashes.csv -NoTypeInformation -Encoding utf8
} finally { Pop-Location }
