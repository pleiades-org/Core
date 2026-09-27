$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --offline --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release benchmark build failed.' }
    New-Item -ItemType Directory -Force -Path results | Out-Null
    foreach ($runNumber in 1..3) {
        & '.\target\release\core-construct-benchmarks.exe' |
            Set-Content -LiteralPath "results\run-$runNumber.csv" -Encoding utf8
        if ($LASTEXITCODE -ne 0) { throw "Benchmark run $runNumber failed." }
        Write-Output "Completed benchmark run $runNumber"
    }
    $allRows = foreach ($runNumber in 1..3) { Import-Csv "results\run-$runNumber.csv" }
    $summary = foreach ($group in ($allRows | Group-Object -Property case)) {
        $medians = @($group.Group | ForEach-Object { [double]$_.median_batch_ns_per_op } | Sort-Object)
        $percentiles = @($group.Group | ForEach-Object { [double]$_.p95_batch_ns_per_op } | Sort-Object)
        [pscustomobject]@{
            Case = $group.Name
            MedianNanoseconds = $medians[1]
            MinimumRunMedianNanoseconds = $medians[0]
            MaximumRunMedianNanoseconds = $medians[2]
            MedianRunP95BatchNanoseconds = $percentiles[1]
            AllocationCallsPerOperation = $group.Group[0].allocation_calls_per_op
            CumulativeRequestedBytesPerOperation = $group.Group[0].cumulative_requested_bytes_per_op
        }
    }
    $summary | Export-Csv -LiteralPath 'results\summary.csv' -NoTypeInformation -Encoding utf8
    $processor = (Get-ItemProperty -LiteralPath 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name ProcessorNameString).ProcessorNameString.Trim()
    [pscustomobject]@{
        Date = (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK')
        Processor = $processor
        LogicalProcessors = [Environment]::ProcessorCount
        OperatingSystem = [Environment]::OSVersion.VersionString
        Rust = (& rustc -V)
        Build = 'release; opt-level=3; thin LTO; codegen-units=1; no target-cpu=native'
        Runs = 3
        BatchesPerCasePerRun = 31
        Clock = 'std::time::Instant; wall clock'
        Limitations = 'Not isolated or affinity-pinned; no process/GPU/RSS measurement; p95 values describe batch averages, not individual query latency.'
    } | ConvertTo-Json | Set-Content -LiteralPath 'results\environment.json' -Encoding utf8
} finally {
    Pop-Location
}
