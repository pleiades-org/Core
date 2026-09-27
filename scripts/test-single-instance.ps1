param([switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
Assert-InteractiveMode $Interactive 'The single-instance check activates a real, non-dry-run Core window.'
$executable = Join-Path $projectRoot 'target\release\core-v2.exe'
if (Get-Process core-v2 -ErrorAction SilentlyContinue | Where-Object Path -EQ $executable) {
    throw 'Close Core v2 before the single-instance test. Existing processes will not be modified.'
}
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
$primary = Start-Process -FilePath $executable -ArgumentList '--start-hidden' -WindowStyle Hidden -PassThru
$secondary = $null
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        if ($primary.HasExited) { throw 'Primary Core instance exited.' }
        $window = [WindowsHarness]::FindWindow($primary.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Primary Core window did not initialize.' }
    $secondary = Start-Process -FilePath $executable -ArgumentList '--start-hidden' -WindowStyle Hidden -PassThru
    if (-not $secondary.WaitForExit(5000) -or $secondary.ExitCode -ne 0) { throw 'Second instance did not exit successfully.' }
    $timer.Restart()
    while (-not [WindowsHarness]::IsWindowVisible($window) -and $timer.Elapsed.TotalSeconds -lt 5) { Start-Sleep -Milliseconds 10 }
    if (-not [WindowsHarness]::IsWindowVisible($window)) { throw 'Second launch did not show the existing window.' }
    [WindowsHarness]::Escape($window)
    $timer.Restart()
    while ([WindowsHarness]::IsWindowVisible($window) -and $timer.Elapsed.TotalSeconds -lt 5) { Start-Sleep -Milliseconds 10 }
    if ([WindowsHarness]::IsWindowVisible($window)) { throw 'Escape did not hide the visible window.' }
    [pscustomobject]@{Date=(Get-Date -Format o); Passed=$true; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash; Scenarios=@('second instance exits','existing window shown','visible window hides with Escape')} | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\single-instance.json"
    'Single-instance and visible hide checks passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $primary.WaitForExit(5000)) { Stop-Process -Id $primary.Id }
    if ($secondary) {
        if (-not $secondary.HasExited) { Stop-Process -Id $secondary.Id }
        $secondary.Dispose()
    }
    $primary.Dispose()
}
