param([string]$Executable = '', [switch]$ReducedMotion, [string]$Label = 'click-away', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
Assert-InteractiveMode $Interactive 'Click-away dismissal switches the foreground and clicks with the real mouse.'
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\DismissalHarness.cs")
$arguments = @('--start-hidden','--dry-run')
if ($ReducedMotion) { $arguments += '--reduced-motion' }
$process = Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardError "$projectRoot\docs\measurements\$Label-stderr.txt"
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        if ($process.HasExited) { throw 'Core exited before the dismissal test.' }
        $window = [WindowsHarness]::FindWindow($process.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Core test window did not initialize.' }
    $timings = @([DismissalHarness]::Verify($window) | Sort-Object)
    [pscustomobject]@{
        Date = (Get-Date -Format o)
        Passed = $true
        Interactive = [bool]$Interactive
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        ReducedMotion = [bool]$ReducedMotion
        Samples = $timings.Count
        MedianDismissalMilliseconds = ($timings[5] + $timings[6]) / 2
        MaximumDismissalMilliseconds = $timings[-1]
        Scenarios = @('outside native-window activation hides Core','Core does not steal foreground after dismissal','unknown observer notification does not hide focused Core','result-control click retains visibility','query preserved across 12 reopen cycles','deactivation during entrance can be reversed')
        Limitations = 'Native foreground changes between test-owned windows. Temporarily attaches the test input queue or uses hit-checked cursor clicks when Windows refuses background activation. Timings include the hide animation and 2 ms observation polling; not hardware input or compositor timing. External actions disabled. Owned-popup behavior is not certified: a separate cross-process popup fixture attached input queues and produced unstable activation. Core currently has no owned dialog.'
    } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Click-away dismissal scenarios passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core test process did not exit cleanly.' }
    $process.Dispose()
}
