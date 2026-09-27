param(
    [int]$DurationSeconds = 60,
    [string]$Label = 'native-catalog-hidden',
    [string]$Executable = '',
    [string[]]$Arguments = @('--start-hidden'),
    [switch]$WarmWindow,
    [string]$WarmQuery = ''
)
$ErrorActionPreference = 'Stop'
if ($DurationSeconds -lt 10) { throw 'Measure at least ten seconds.' }
if ($Label -notmatch '^[a-z0-9-]+$') { throw 'Use a simple lowercase measurement label.' }
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $Executable) { $Executable = Join-Path $projectRoot 'target\release\core-v2.exe' }
$outputDirectory = Join-Path $projectRoot 'docs\measurements'
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
$process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WindowStyle Hidden -PassThru -RedirectStandardError "$outputDirectory\$Label-stderr.txt"
$window = [IntPtr]::Zero
try {
    Start-Sleep -Seconds 10
    if ($process.HasExited) { throw 'Launcher exited during warm-up. Check the measurement stderr file.' }
    $window = [WindowsHarness]::FindWindow($process.Id)
    if ($WarmWindow) {
        if ($window -eq [IntPtr]::Zero) { throw 'Warm-up requires a native Core window.' }
        [WindowsHarness]::WarmWindow($window)
    }
    if ($WarmQuery) {
        if ($window -eq [IntPtr]::Zero) { throw 'Query warm-up requires a native Core window.' }
        [WindowsHarness]::Query($window, $WarmQuery)
        [WindowsHarness]::Show($window)
        [WindowsHarness]::WaitFor([Func[bool]]{ [WindowsHarness]::Count($window) -gt 0 }, 'Warm-up query returned no rows')
        Start-Sleep -Seconds 3
        [WindowsHarness]::Escape($window)
        [WindowsHarness]::WaitFor([Func[bool]]{ -not [WindowsHarness]::IsWindowVisible($window) }, 'Warm-up hide did not finish')
    }
    $process.Refresh()
    $initialCpuSeconds = $process.TotalProcessorTime.TotalSeconds
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $samples = [Collections.Generic.List[object]]::new()
    while ($timer.Elapsed.TotalSeconds -lt $DurationSeconds) {
        Start-Sleep -Seconds 1
        $process.Refresh()
        if ($process.HasExited) { throw 'Launcher exited during measurement.' }
        $samples.Add([pscustomobject]@{
            ElapsedSeconds = $timer.Elapsed.TotalSeconds
            PrivateBytes = $process.PrivateMemorySize64
            WorkingSetBytes = $process.WorkingSet64
            Handles = $process.HandleCount
            Threads = $process.Threads.Count
            CpuSeconds = $process.TotalProcessorTime.TotalSeconds - $initialCpuSeconds
        })
    }
    $last = $samples[$samples.Count - 1]
    [pscustomobject]@{
        Date = (Get-Date -Format o)
        Label = $Label
        Executable = $Executable
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        WarmupSeconds = 10
        WarmedWithShowHide = [bool]$WarmWindow
        WarmQuery = $WarmQuery
        DurationSeconds = $last.ElapsedSeconds
        CpuPercentOneLogicalCore = 100 * $last.CpuSeconds / $last.ElapsedSeconds
        FinalPrivateBytes = $last.PrivateBytes
        FinalWorkingSetBytes = $last.WorkingSetBytes
        Samples = $samples
        Limitations = 'Private bytes and CPU of the process; GPU memory and frame latency are not captured. Actual Start Menu catalog, not the full planned release dataset.'
    } | ConvertTo-Json -Depth 5 | Set-Content "$outputDirectory\$Label.json"
    "Saved $outputDirectory\$Label.json"
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id }
    $process.Dispose()
}
