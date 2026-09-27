param([string]$ValidationRecord = 'styling-checks.json')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$candidate = Join-Path $projectRoot 'target\styling\release\core-v2.exe'
$destination = Join-Path $projectRoot 'target\release\core-v2.exe'
$expectedHash = (Get-Content (Join-Path "$projectRoot\docs\measurements" $ValidationRecord) -Raw | ConvertFrom-Json).Sha256
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $candidate).Hash -ne $expectedHash) { throw 'Candidate differs from the validated styling build.' }
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
if (Test-Path -LiteralPath $destination) {
    $backupDirectory = Join-Path $projectRoot 'target\backups'
    New-Item -ItemType Directory -Path $backupDirectory -Force | Out-Null
    $backupPath = Join-Path $backupDirectory "core-v2-$(Get-Date -Format yyyyMMdd-HHmmssfff).exe"
    Copy-Item -LiteralPath $destination -Destination $backupPath
}
$running = @(Get-Process core-v2 -ErrorAction SilentlyContinue | Where-Object Path -EQ $destination)
foreach ($process in $running) {
    $window = [WindowsHarness]::FindWindow($process.Id)
    if ($window -eq [IntPtr]::Zero) { throw 'Running Core has no controllable launcher window; it has not been stopped.' }
    [WindowsHarness]::Close($window)
    if (-not $process.WaitForExit(5000)) { throw 'Core did not exit cleanly; the executable has not been replaced.' }
    $process.Dispose()
}
Copy-Item -LiteralPath $candidate -Destination $destination -Force
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash -ne $expectedHash) { throw 'Published executable hash mismatch.' }
$updated = Start-Process -FilePath $destination -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru
try {
    $window = [IntPtr]::Zero
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt 5) {
        if ($updated.HasExited) { throw 'Updated Core exited during startup.' }
        $window = [WindowsHarness]::FindWindow($updated.Id)
        if ($window -ne [IntPtr]::Zero -and [WindowsHarness]::IsWindowVisible($window) -and [WindowsHarness]::Opacity($window) -eq 255) { break }
        Start-Sleep -Milliseconds 20
    }
    if ($window -eq [IntPtr]::Zero -or -not [WindowsHarness]::IsWindowVisible($window) -or [WindowsHarness]::Opacity($window) -ne 255) { throw 'Updated Core did not finish its entrance transition.' }
    [pscustomobject]@{Executable=$destination; ProcessId=$updated.Id; Sha256=$expectedHash; Visible=$true} | ConvertTo-Json
} finally { $updated.Dispose() }
