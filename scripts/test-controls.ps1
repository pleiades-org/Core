param([string]$Executable = '', [string]$Label = 'controls-checks', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CaptureWindow.cs", "$PSScriptRoot\SettingsHarness.cs", "$PSScriptRoot\ControlsHarness.cs", "$PSScriptRoot\DismissalHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\controls-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @('--dry-run')) -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
$window = [IntPtr]::Zero
try {
    [WindowsHarness]::WaitFor([Func[bool]] { $script:window = [WindowsHarness]::FindWindow($process.Id); return $window -ne [IntPtr]::Zero }, 'Core did not initialize')
    [ControlsHarness]::VerifyLayout($window, $testDirectory)
    # Hover moves the real mouse pointer and captures the visible screen.
    if ($Interactive) { [ControlsHarness]::VerifyHover($window, $testDirectory, $process.Handle) }
    [pscustomobject]@{ Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash; Scenarios=@('power visible on fresh launch before typing', 'constrained DPI layout keeps footer, icons and all results reachable', 'power popup contained in available client area', 'position box previews', 'power visible after settings and reopen') + $(if ($Interactive) { @('all four power icons repaint on hover', 'hover resets on leave and hide', 'hover does not leak GDI resources') } else { @() }); TestDirectory=$testDirectory } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    "Controls checks passed. Captures: $testDirectory"
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core did not shut down.' }
    $process.Dispose()
}
