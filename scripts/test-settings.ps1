param([string]$Executable = '', [string]$Label = 'settings-checks', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CaptureWindow.cs", "$PSScriptRoot\SettingsHarness.cs", "$PSScriptRoot\ExtensionsHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\settings-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'appearance.ini'
$process = $null
$window = [IntPtr]::Zero
function Start-TestCore {
    $script:process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @(@('--start-hidden','--dry-run','--test-stay-open','--reduced-motion','--settings-file', ('"' + $settingsFile + '"')))) -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
    $script:window = [IntPtr]::Zero
    [WindowsHarness]::WaitFor([Func[bool]]{
        if ($script:process.HasExited) { throw 'Settings test app exited before creating its window.' }
        $script:window = [WindowsHarness]::FindWindow($script:process.Id)
        return $script:window -ne [IntPtr]::Zero
    }, 'Core window did not initialize')
}
function Stop-TestCore {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core did not exit cleanly.' }
    $process.Dispose()
    $script:process = $null
    $script:window = [IntPtr]::Zero
}
try {
    Start-TestCore
    [SettingsHarness]::Verify($window, "$projectRoot\docs\measurements", $process.Handle)
    $saved = Get-Content -LiteralPath $settingsFile -Raw
    if ($saved -notmatch 'background=#123456' -or $saved -notmatch 'position=Right') { throw 'Saved settings do not match the accepted changes.' }
    Stop-TestCore
    Start-TestCore
    [WindowsHarness]::Query($window, '@calc 7+8')
    [WindowsHarness]::WaitFor([Func[bool]]{[WindowsHarness]::First($window) -eq '15'}, 'Reloaded result did not finish')
    [WindowsHarness]::Show($window)
    [WindowsHarness]::WaitFor([Func[bool]]{[WindowsHarness]::IsWindowVisible($window)}, 'Reloaded Core did not show')
    [SettingsHarness]::CheckPosition($window, 4)
    [SettingsHarness]::Open($window)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 200)) -ne '#123456') { throw 'Background did not survive restart.' }
    # Every preference must persist before Done, including changes queued behind another write.
    [SettingsHarness]::Click($window, 231)
    [SettingsHarness]::CheckSidebar($window, $true)
    [ExtensionsHarness]::SetShortcut($window, 'Ctrl+Shift+F11')
    [SettingsHarness]::Choose($window, 242, 1)
    [SettingsHarness]::Click($window, 243)
    [SettingsHarness]::WaitSaved($window)
    if (-not [SettingsHarness]::IsOpen($window)) { throw 'Autosave closed settings.' }
    $displayLabel = [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 242))
    $saved = Get-Content -LiteralPath $settingsFile -Raw
    if ($saved -notmatch 'shortcut=Ctrl\+Shift\+F11' -or $saved -notmatch 'startup=true' -or $displayLabel.StartsWith('Active')) { throw 'Automatic behaviour save failed.' }
    Stop-TestCore
    Start-TestCore
    [WindowsHarness]::PostMessageW($window, 0x8007, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    [SettingsHarness]::Flush($window)
    [SettingsHarness]::CheckPosition($window, 4)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 200)) -ne '#123456') { throw 'Background was lost when saving behaviour.' }
    [SettingsHarness]::Click($window, 231)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 240)) -ne 'Ctrl+Shift+F11' -or
        [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 242)) -ne $displayLabel -or
        -not [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 243)).EndsWith('On')) { throw 'Behaviour preferences did not survive process restart.' }
    [SettingsHarness]::Click($window, 230)
    # A denied replace must preserve the previous settings file and explain the failure.
    (Get-Item -LiteralPath $settingsFile).IsReadOnly = $true
    [SettingsHarness]::SetColor($window, '#ABCDEF')
    [WindowsHarness]::WaitFor([Func[bool]]{[WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 222)).Contains('Could not replace settings')}, 'Denied save was not reported')
    if (-not [WindowsHarness]::IsWindowVisible([WindowsHarness]::GetDlgItem($window, 220))) { throw 'Failed autosave did not offer Retry.' }
    if ((Get-Content -LiteralPath $settingsFile -Raw) -ne $saved) { throw 'Failed save changed the original file.' }
    (Get-Item -LiteralPath $settingsFile).IsReadOnly = $false
    [SettingsHarness]::Click($window, 220)
    [SettingsHarness]::WaitSaved($window)
    if ((Get-Content -LiteralPath $settingsFile -Raw) -notmatch 'background=#ABCDEF') { throw 'Retry did not persist the latest draft.' }
    if ([WindowsHarness]::IsWindowVisible([WindowsHarness]::GetDlgItem($window, 220))) { throw 'Retry remained visible after recovery.' }
    # Dismiss and immediately reopen during the typing delay: the accepted value must survive.
    [SettingsHarness]::SetColor($window, '#456789')
    [WindowsHarness]::PostMessageW($window, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    [WindowsHarness]::PostMessageW($window, 0x8007, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    [SettingsHarness]::Flush($window)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 200)) -ne '#456789') { throw 'Rapid reopen reverted the pending edit.' }
    [WindowsHarness]::WaitFor([Func[bool]]{(Get-Content -LiteralPath $settingsFile -Raw) -match 'background=#456789'}, 'Dismissal lost the pending edit')
    [SettingsHarness]::Done($window)
    # Sliders: spacing is measured from the physical screen edge and floats every corner.
    [SettingsHarness]::Open($window)
    [SettingsHarness]::Click($window, 216)
    [SettingsHarness]::Slide($window, 251, 0)
    [SettingsHarness]::Slide($window, 254, 60)
    [SettingsHarness]::WaitSaved($window)
    $saved = Get-Content -LiteralPath $settingsFile -Raw
    if ($saved -notmatch 'corner_radius=0' -or $saved -notmatch 'edge_spacing=60') { throw 'Slider values were not saved.' }
    [SettingsHarness]::CheckSpacing($window, 60, $false)
    if ([WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 255)) -ne '60 px from the screen edge') { throw 'Spacing value label is wrong.' }
    [SettingsHarness]::Slide($window, 251, 24)
    [SettingsHarness]::WaitSaved($window)
    [SettingsHarness]::CheckSpacing($window, 60, $true)
    # Returning both sliders to their defaults omits them from the file again.
    [SettingsHarness]::Slide($window, 251, 16)
    [SettingsHarness]::Slide($window, 254, 0)
    [SettingsHarness]::WaitSaved($window)
    if ((Get-Content -LiteralPath $settingsFile -Raw) -match 'corner_radius|edge_spacing') { throw 'Default slider values were written.' }
    [SettingsHarness]::CheckPosition($window, 6)
    [SettingsHarness]::Done($window)
    # Close during the typing delay: Core must flush and wait for the latest valid edit.
    [SettingsHarness]::Open($window)
    [SettingsHarness]::SetColor($window, '#654321')
    Stop-TestCore
    if ((Get-Content -LiteralPath $settingsFile -Raw) -notmatch 'background=#654321') { throw 'Closing lost an accepted settings save.' }
    $corrupt = "version=999`nbackground=#000000`nposition=Center`n"
    Set-Content -LiteralPath $settingsFile -Value $corrupt -NoNewline
    Start-TestCore
    [WindowsHarness]::PostMessageW($window, 0x8007, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    [SettingsHarness]::Flush($window)
    [SettingsHarness]::Click($window, 220)
    if (-not [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 222)).Contains('preserved')) { throw 'Unsupported settings version was not explained.' }
    if ((Get-Content -LiteralPath $settingsFile -Raw) -ne $corrupt) { throw 'Unsupported settings were overwritten.' }
    Stop-TestCore
    [pscustomobject]@{
        Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        Scenarios=@('lazy settings controls','left sidebar with right-side category controls','settings entry points and Tab navigation','all seven anchors and attached corner masks','edge anchoring after result height changes','custom and light background pixels','automatic saving without closing settings','Done retains saved appearance','invalid color rejected and Escape retains last valid settings','30 rapid edits without GDI growth','all five preferences survive process restart','failed autosave preserves prior settings','Retry recovers after denied replacement','dismissal and immediate reopen retain pending edit','close flushes typing debounce','unsupported settings preserved','corner rounding and screen-edge spacing sliders save, place and clip','default slider values omitted from the file')
        Limitations='Queued native control commands with UI completion acknowledgements, region checks and own-window bitmaps. Physical mixed-DPI monitor switching and screen-reader acceptance remain manual. Isolated settings file; no real user preferences changed.'
        TestDirectory=$testDirectory
    } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Settings, placement, corner, persistence and failure scenarios passed.'
} finally {
    if (Test-Path -LiteralPath $settingsFile) { (Get-Item -LiteralPath $settingsFile).IsReadOnly = $false }
    if ($null -ne $process) { Stop-TestCore }
}
