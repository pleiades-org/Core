param([string]$Executable = '', [string]$Label = 'release-quicklinks', [switch]$SkipKeyboard, [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
# Injected keystrokes go to the foreground window, so background runs skip them.
$SkipKeyboard = [switch]($SkipKeyboard -or -not $Interactive)
if (-not $Executable) { $Executable = "$projectRoot\target\styling\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CaptureWindow.cs", "$PSScriptRoot\SettingsHarness.cs", "$PSScriptRoot\QuicklinksHarness.cs", "$PSScriptRoot\DismissalHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\quicklinks-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'settings.ini'
$process = $null
$window = [IntPtr]::Zero
$startCounter = 0
function Start-TestCore {
    $script:startCounter++
    $script:process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @(@('--dry-run','--test-stay-open','--reduced-motion','--settings-file', ('"' + $settingsFile + '"')))) -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr-$startCounter.txt"
    [WindowsHarness]::WaitFor([Func[bool]] { $script:window = [WindowsHarness]::FindWindow($script:process.Id); return $script:window -ne [IntPtr]::Zero }, 'Core did not initialize')
    [SettingsHarness]::Flush($window)
}
function Stop-TestCore {
    [WindowsHarness]::Close($window)
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core failed to exit.' }
    $process.Dispose()
    $script:process = $null
    $script:window = [IntPtr]::Zero
}
try {
    Start-TestCore
    [QuicklinksHarness]::Verify($window, $testDirectory, $process.Handle, -not $SkipKeyboard)
    Stop-TestCore
    $saved = Get-Content -LiteralPath $settingsFile -Raw
    if (-not $saved.Contains('steam://rungameid/1') -or $saved.Contains('STEAM:')) { throw 'App-link scheme was not normalized and persisted.' }
    if (($saved -split "`n" | Where-Object { $_.StartsWith('quicklink=') }).Count -ne 30 -or $saved.Contains('incomplete') -or $saved.Contains('javascript:') -or $saved.Contains('Project Files')) { throw 'Persisted quicklinks do not match completed rows.' }
    Start-TestCore
    [QuicklinksHarness]::Query($window, '> Core Documentation', 'Core Documentation')
    [QuicklinksHarness]::Query($window, '> Quicklink 31', 'Quicklink 31')
    [QuicklinksHarness]::Query($window, '> Quicklink 04', 'Quicklink 04')
    [SettingsHarness]::Open($window)
    [SettingsHarness]::Click($window, 232)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 301)) -ne 'Core Documentation') { throw 'Quicklink rows did not survive restart.' }
    (Get-Item -LiteralPath $settingsFile).IsReadOnly = $true
    [QuicklinksHarness]::SetField($window, 301, 'Updated Documentation')
    [WindowsHarness]::WaitFor([Func[bool]] { [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 222)).Contains('Could not replace settings') }, 'Failed quicklink save was not reported')
    if ((Get-Content -LiteralPath $settingsFile -Raw) -ne $saved) { throw 'Failed quicklink save damaged original data.' }
    (Get-Item -LiteralPath $settingsFile).IsReadOnly = $false
    [SettingsHarness]::Click($window, 220)
    [SettingsHarness]::WaitSaved($window)
    [SettingsHarness]::Done($window)
    [QuicklinksHarness]::Query($window, '> Updated Documentation', 'Updated Documentation')
    [pscustomobject]@{ Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; PhysicalKeyboardChecked=(-not $SkipKeyboard); Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash; Scenarios=@('Link and Name table adds blank row after completion','scrolling beyond visible rows','31 rows without GDI growth','Unicode and website normalization','unsupported target and duplicate name rejection','rename and remove persisted','normal and scoped search','dry-run activation','partial row not persisted','process restart restores table and search','failed replacement preserves links and retry recovers'); TestDirectory=$testDirectory; SideEffects='Isolated settings file and dry-run activation. Click-away suppressed only for table data checks by --test-stay-open; tested separately.' } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Quicklink table, search, restart and failure checks passed.'
} finally {
    if (Test-Path -LiteralPath $settingsFile) { (Get-Item -LiteralPath $settingsFile).IsReadOnly = $false }
    if ($null -ne $process) { Stop-TestCore }
}
