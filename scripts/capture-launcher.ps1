param([string]$Executable = '', [string]$Query = '@calc 25% of 80', [string]$Label = 'launcher-preview', [ValidateRange(100,10000)][int]$SettleMilliseconds = 400)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
Add-Type -Path "$PSScriptRoot\CaptureWindow.cs"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
$process = Start-Process -FilePath $Executable -ArgumentList '--dry-run' -WindowStyle Hidden -PassThru -RedirectStandardError "$projectRoot\docs\measurements\capture-stderr.txt"
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        $window = [WindowsHarness]::FindWindow($process.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Launcher window was not created.' }
    [WindowsHarness]::Query($window, $Query)
    Start-Sleep -Milliseconds $SettleMilliseconds
    [CaptureWindow]::Save($window, "$projectRoot\docs\measurements\$Label.bmp")
    $rows = for ($index = 0; $index -lt [WindowsHarness]::Count($window); $index++) { [WindowsHarness]::Row($window, $index) }
    [pscustomobject]@{Query=$Query; Rows=@($rows); Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash} | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id }
    $process.Dispose()
}
