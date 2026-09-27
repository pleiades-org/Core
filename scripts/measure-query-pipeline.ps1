param([string]$Executable = '', [string]$Label = 'query-pipeline')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
$process = Start-Process -FilePath $Executable -ArgumentList '--start-hidden','--dry-run' -WindowStyle Hidden -PassThru -RedirectStandardError "$projectRoot\docs\measurements\$Label-stderr.txt"
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        if ($process.HasExited) { throw 'Launcher exited during pipeline measurement.' }
        $window = [WindowsHarness]::FindWindow($process.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Launcher window was not created.' }
    [void][WindowsHarness]::MeasureQueries($window, 100)
    $process.Refresh()
    $before = [pscustomobject]@{PrivateBytes=$process.PrivateMemorySize64; Handles=$process.HandleCount; Threads=$process.Threads.Count}
    $durations = [WindowsHarness]::MeasureQueries($window, 1000) | Sort-Object
    $process.Refresh()
    $after = [pscustomobject]@{PrivateBytes=$process.PrivateMemorySize64; Handles=$process.HandleCount; Threads=$process.Threads.Count}
    [pscustomobject]@{
        Date = (Get-Date -Format o)
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        Samples = $durations.Count
        P50Milliseconds = $durations[499]
        P95Milliseconds = $durations[949]
        P99Milliseconds = $durations[989]
        MaximumMilliseconds = $durations[999]
        Before = $before
        After = $after
        Limitations = 'WM_SETTEXT through worker completion and hidden LISTBOX text observation. Calculator queries; 2 ms polling interval. Includes harness overhead; excludes keyboard hardware, visible paint and compositor latency. External actions disabled.'
    } | ConvertTo-Json -Depth 4 | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Completed 1,000 changing query pipeline checks.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Launcher failed to shut down.' }
    $process.Dispose()
}
