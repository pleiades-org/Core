param([string]$Executable = '', [string]$Label = 'shortcut-checks', [switch]$RegistrationOnly, [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
# Injected keystrokes go to the foreground window, so background runs check registration only.
$RegistrationOnly = [switch]($RegistrationOnly -or -not $Interactive)
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CaptureWindow.cs", "$PSScriptRoot\SettingsHarness.cs", "$PSScriptRoot\ExtensionsHarness.cs", "$PSScriptRoot\ShortcutHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\shortcut-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'settings.ini'
Set-Content -LiteralPath $settingsFile -Value "version=1`nbackground=#000000`nposition=Center`nshortcut=Ctrl+Shift+F11`ndisplay=Active`nstartup=false`n"
$process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @(@('--start-hidden','--dry-run','--test-shortcut','--reduced-motion','--settings-file', ('"' + $settingsFile + '"')))) -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
$window = [IntPtr]::Zero
try {
    [WindowsHarness]::WaitFor([Func[bool]] { $script:window = [WindowsHarness]::FindWindow($process.Id); return $window -ne [IntPtr]::Zero }, 'Core did not initialize')
    [SettingsHarness]::Flush($window)
    if ($RegistrationOnly) { [ShortcutHarness]::VerifyRegistration($window) }
    else { [ShortcutHarness]::Verify($window) }
    $scenarios = if ($RegistrationOnly) { @('custom global hotkey registered','Windows-key mode releases previous shortcut','switching back restores registration','conflict retains previous working binding') } else { @('custom global hotkey opens Core','left and right Windows-key taps toggle Core','Windows combinations preserve delivery','batched key input preserves delivery','no stuck modifiers','switching Windows mode back to ordinary hotkey','conflict rejects save and keeps working binding') }
    [pscustomobject]@{ Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; PhysicalKeyboardChecked=(-not $RegistrationOnly); Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash; Scenarios=$scenarios; TestDirectory=$testDirectory; Limitations='Physical input is tested only when PhysicalKeyboardChecked is true. Registration-only mode injects no input. Elevated apps and secure desktop not covered. External actions and startup writes disabled.' } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Global shortcuts, Windows-key handling and conflict recovery passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Shortcut test Core did not shut down.' }
    $process.Dispose()
}
