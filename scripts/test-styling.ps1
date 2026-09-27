param([string]$Executable = '', [switch]$ReducedMotion, [string]$Label = 'styling-checks', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
$arguments = @('--start-hidden','--dry-run')
if ($ReducedMotion) { $arguments += '--reduced-motion' }
$process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @($arguments)) -WindowStyle Hidden -PassThru -RedirectStandardError "$projectRoot\docs\measurements\styling-stderr.txt"
$window = [IntPtr]::Zero
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $timer.Elapsed.TotalSeconds -lt 5) {
        if ($process.HasExited) { throw 'Styling test process exited during startup.' }
        $window = [WindowsHarness]::FindWindow($process.Id)
        Start-Sleep -Milliseconds 10
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Styling test window did not initialize.' }
    $coldGdi = [WindowsHarness]::GetGuiResources($process.Handle, 0)
    [WindowsHarness]::WarmWindow($window)
    $beforeGdi = [WindowsHarness]::GetGuiResources($process.Handle, 0)
    $animationsEnabled = [WindowsHarness]::VerifyStyling($window, [bool]$ReducedMotion)
    $afterGdi = [WindowsHarness]::GetGuiResources($process.Handle, 0)
    if ($afterGdi -gt $beforeGdi + 2) { throw "GDI resources grew unexpectedly: $beforeGdi to $afterGdi" }
    [pscustomobject]@{
        Date = (Get-Date -Format o)
        Passed = $true
        Interactive = [bool]$Interactive
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        EffectiveAnimationsEnabled = $animationsEnabled
        ColdGdiObjects = $coldGdi
        GdiObjectsBefore = $beforeGdi
        GdiObjectsAfter = $afterGdi
        Scenarios = @('DPI-scaled expanded calculator geometry','rounded window region','fade-in and fade-out opacity','Escape hides','20 interrupted/reversed animation cycles','dynamic five-result height','arrow-key selection')
        Limitations = 'Native control and opacity observations; not presented-frame timing. Per-process motion preferences allow both animation and reduced-motion paths to be tested without changing Windows settings.'
    } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Styling, transition reversal and GDI resource checks passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Styling test process did not shut down.' }
    $process.Dispose()
}
