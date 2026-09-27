param([string]$Executable = '', [string]$Label = 'windows-integration', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
if (-not $Executable) { $Executable = Join-Path $projectRoot 'target\release\core-v2.exe' }
$process = Start-Process -FilePath $executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @('--start-hidden','--dry-run')) -WindowStyle Hidden -PassThru -RedirectStandardError "$projectRoot\docs\measurements\integration-stderr.txt"
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        if ($process.HasExited) { throw "Launcher exited: $(Get-Content "$projectRoot\docs\measurements\integration-stderr.txt" -Raw)" }
        $window = [WindowsHarness]::FindWindow($process.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Launcher controls did not become ready.' }
    [WindowsHarness]::Verify($window)
    [pscustomobject]@{Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash; Scenarios=@('real app discovery and exact query with dry-run acceptance','calculator result and dry-run action','division error clears results','command completion','web payload','100 rapid edits plus immediate Enter','unknown command rejects stale action','implicit and explicit time conversion with date and immediate Enter','DST gap and overlap reject stale actions','invalid date and unknown time zone','search completion while tray menu is open','Escape hides'); SideEffects='Disabled by --dry-run'} | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Windows integration scenarios passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Launcher failed to shut down cleanly.' }
    $process.Dispose()
}
