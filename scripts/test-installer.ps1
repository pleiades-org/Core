param(
    [Parameter(Mandatory)][string]$Executable,
    [string]$PayloadExecutable = '',
    [switch]$AutomaticSetup,
    [string]$Label = 'release-installer',
    [switch]$Interactive
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$Executable = (Resolve-Path -LiteralPath $Executable).Path
if (-not $PayloadExecutable) { $PayloadExecutable = $Executable }
$payloadHash = (Get-FileHash -LiteralPath $PayloadExecutable).Hash
if (-not [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($Executable)).Contains('level="asInvoker"')) {
    throw 'Setup does not declare current-user execution in its embedded Windows manifest.'
}
Add-Type -Path @("$PSScriptRoot/WindowsHarness.cs", "$PSScriptRoot/InstallerHarness.cs", "$PSScriptRoot/CaptureWindow.cs")
$testToken = [Guid]::NewGuid().ToString('N')
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "core-installer-test-$testToken"
New-Item -ItemType Directory -Path $testRoot | Out-Null
$installDirectory = Join-Path $testRoot 'install'
$registration = "HKCU:\Software\Pleiades\Core\InstallerTests\$testToken\Uninstall"
$startup = "HKCU:\Software\Pleiades\Core\InstallerTests\$testToken\Run"
$settingsFile = Join-Path $testRoot 'settings/appearance.ini'
New-Item -ItemType Directory -Path (Split-Path $settingsFile) | Out-Null
$settingsText = "version=1`nbackground=#000000`nposition=Center`n"
[IO.File]::WriteAllText($settingsFile, $settingsText)
$activeProcess = $null
$activeWindow = [IntPtr]::Zero
$temporaryUninstallerDirectory = $null

function Start-Setup([string]$Operation, [string]$Program = $Executable) {
    $script:activeProcess = Start-Process -FilePath $Program -ArgumentList @($Operation, '--test-background', '--installer-test-root', ('"' + $testRoot + '"')) -WindowStyle Hidden -PassThru -RedirectStandardError "$testRoot/$($Operation.TrimStart('-'))-$([Guid]::NewGuid().ToString('N')).log"
    [WindowsHarness]::WaitFor([Func[bool]] { $script:activeWindow = [InstallerHarness]::FindForDirectory($installDirectory); return $activeWindow -ne [IntPtr]::Zero }, 'Setup did not open')
    $windowProcessId = [InstallerHarness]::ProcessId($activeWindow)
    if ($windowProcessId -ne $activeProcess.Id) {
        if (-not $activeProcess.WaitForExit(5000) -or $activeProcess.ExitCode -ne 0) { throw 'The installed uninstaller did not hand off successfully.' }
        $activeProcess.Dispose()
        $script:activeProcess = Get-Process -Id $windowProcessId
        $activeProcess.Handle | Out-Null
        $script:temporaryUninstallerDirectory = Split-Path $activeProcess.Path -Parent
    }
}

function Finish-Setup {
    $processHandle = $activeProcess.Handle
    [InstallerHarness]::Close($activeWindow)
    if (-not $activeProcess.WaitForExit(5000) -or [InstallerHarness]::ExitCode($processHandle) -ne 0) { throw 'Setup did not exit successfully.' }
    $activeProcess.Dispose()
    $script:activeProcess = $null
    $script:activeWindow = [IntPtr]::Zero
    if ($temporaryUninstallerDirectory) {
        [WindowsHarness]::WaitFor([Func[bool]] { return -not (Test-Path -LiteralPath $temporaryUninstallerDirectory) }, 'The temporary uninstaller was not cleaned after exit')
        $script:temporaryUninstallerDirectory = $null
    }
}

function Install-Core([bool]$Desktop = $false) {
    $operation = if ($AutomaticSetup) { '' } else { '--install' }
    Start-Setup $operation
    if ($Desktop) { [InstallerHarness]::Click($activeWindow, [InstallerHarness]::DesktopId) }
    [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
    try {
        [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::StatusId) -eq 'Core is installed.' }, 'Installation did not complete')
    } catch {
        throw ('Installation did not complete: ' + [InstallerHarness]::Text($activeWindow, [InstallerHarness]::StatusId))
    }
    Finish-Setup
}

try {
    $initialOperation = if ($AutomaticSetup) { '' } else { '--install' }
    Start-Setup $initialOperation
    [InstallerHarness]::VerifyAppearance($activeWindow, "$testRoot/installer-black.bmp", 0)
    [InstallerHarness]::EnterOnControl($activeWindow, [InstallerHarness]::CancelId)
    if (-not $activeProcess.WaitForExit(5000) -or $activeProcess.ExitCode -ne 0) { throw 'Enter on Cancel did not cancel setup.' }
    $activeProcess.Dispose()
    $script:activeProcess = $null
    $script:activeWindow = [IntPtr]::Zero
    Start-Setup '--install'
    Finish-Setup
    [IO.File]::WriteAllText($settingsFile, $settingsText.Replace('#000000', '#ffffff'))
    Start-Setup '--install'
    [InstallerHarness]::VerifyAppearance($activeWindow, "$testRoot/installer-white.bmp", 0xffffff)
    Finish-Setup
    [IO.File]::WriteAllText($settingsFile, $settingsText)
    if (Test-Path -LiteralPath $installDirectory) { throw 'Cancelling setup changed the installation.' }
    if (Test-Path -LiteralPath $registration) { throw 'Cancelling setup registered an application.' }

    New-Item -Path $startup -Force | Out-Null
    New-ItemProperty -Path $startup -Name PleiadesCoreV2 -Value '"C:\Old portable Core\core-v2.exe" --start-hidden' -PropertyType String | Out-Null
    Install-Core $true
    $installedExecutable = Join-Path $installDirectory 'core-v2.exe'
    $uninstaller = Join-Path $installDirectory 'uninstall.exe'
    if ((Get-FileHash -LiteralPath $installedExecutable).Hash -ne $payloadHash) { throw 'Installed executable does not match the setup payload.' }
    if (-not (Test-Path -LiteralPath $uninstaller)) { throw 'Uninstaller is missing.' }
    if (-not (Test-Path -LiteralPath "$testRoot/start-menu/Core.lnk") -or -not (Test-Path -LiteralPath "$testRoot/desktop/Core.lnk")) { throw 'Install shortcuts are missing.' }
    $metadata = Get-ItemProperty -LiteralPath $registration
    if ($metadata.DisplayName -ne 'Core' -or $metadata.UninstallString -ne ('"' + $uninstaller + '" --uninstall')) { throw 'Windows uninstall registration is incorrect.' }
    if ((Get-ItemProperty -LiteralPath $startup).PleiadesCoreV2 -ne ('"' + $installedExecutable + '" --start-hidden')) { throw 'Existing startup registration was not migrated.' }
    if ([IO.File]::ReadAllText($settingsFile) -ne $settingsText) { throw 'Installation changed existing preferences.' }

    # Reinstallation repairs owned files and preserves the desktop choice shown in setup.
    Install-Core
    if (-not (Test-Path -LiteralPath "$testRoot/desktop/Core.lnk")) { throw 'Repair forgot the existing desktop shortcut.' }
    $installationMarker = Join-Path $installDirectory 'core-installation.txt'
    $originalMarker = [IO.File]::ReadAllText($installationMarker)
    $newerMarker = $originalMarker -replace 'version=[^\n]+', 'version=999.0.0'
    [IO.File]::WriteAllText($installationMarker, $newerMarker)
    Start-Setup '--install'
    [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
    [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::StatusId).StartsWith('A newer version of Core is installed.') }, 'Older setup did not reject a downgrade')
    Finish-Setup
    if ([IO.File]::ReadAllText($installationMarker) -ne $newerMarker -or (Get-FileHash -LiteralPath $installedExecutable).Hash -ne $payloadHash) { throw 'Rejected downgrade changed the installation.' }
    [IO.File]::WriteAllText($installationMarker, $originalMarker)
    $originalBytes = [Text.Encoding]::UTF8.GetBytes('Keep the original file if repair fails')
    [IO.File]::WriteAllBytes($installedExecutable, $originalBytes)
    $lockedUninstaller = [IO.File]::Open($uninstaller, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    try {
        Start-Setup '--install'
        [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
        [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::PrimaryId) -eq 'Retry' }, 'Failed repair did not offer Retry')
        Finish-Setup
        if ([IO.File]::ReadAllText($installedExecutable) -ne [Text.Encoding]::UTF8.GetString($originalBytes)) { throw 'Failed repair did not restore the original executable.' }
    } finally { $lockedUninstaller.Dispose() }
    Install-Core
    [IO.File]::WriteAllText("$installDirectory/user-file.txt", 'keep this file')

    Start-Setup '' $uninstaller
    [CaptureWindow]::Save($activeWindow, "$testRoot/uninstaller-black.bmp")
    [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
    [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::StatusId) -eq 'Core has been uninstalled.' }, 'Uninstallation did not complete')
    Finish-Setup
    foreach ($file in @($installedExecutable, $uninstaller, "$testRoot/start-menu/Core.lnk", "$testRoot/desktop/Core.lnk")) {
        if (Test-Path -LiteralPath $file) { throw "Uninstallation left $file behind." }
    }
    if (Test-Path -LiteralPath $registration) { throw 'Uninstallation left its Windows entry.' }
    if ((Get-ItemProperty -LiteralPath $startup).PSObject.Properties.Name -contains 'PleiadesCoreV2') { throw 'Uninstallation left startup enabled.' }
    if ([IO.File]::ReadAllText($settingsFile) -ne $settingsText -or [IO.File]::ReadAllText("$installDirectory/user-file.txt") -ne 'keep this file') { throw 'Uninstallation removed user data.' }

    # Unselected or unverifiable shortcuts belong to their owner, not to setup.
    [IO.File]::WriteAllText("$testRoot/desktop/Core.lnk", 'foreign desktop shortcut')
    Install-Core
    if ([IO.File]::ReadAllText("$testRoot/desktop/Core.lnk") -ne 'foreign desktop shortcut') { throw 'Install replaced an unselected foreign desktop shortcut.' }
    [IO.File]::WriteAllText("$testRoot/start-menu/Core.lnk", 'unverifiable shortcut')
    Start-Setup '--uninstall' $installedExecutable
    [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
    [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::StatusId) -eq 'Core has been uninstalled.' }, 'Uninstall with unrelated shortcuts failed')
    Finish-Setup
    if ([IO.File]::ReadAllText("$testRoot/desktop/Core.lnk") -ne 'foreign desktop shortcut' -or [IO.File]::ReadAllText("$testRoot/start-menu/Core.lnk") -ne 'unverifiable shortcut') { throw 'Uninstall removed an unrelated shortcut.' }
    Remove-Item -LiteralPath "$testRoot/desktop/Core.lnk", "$testRoot/start-menu/Core.lnk"

    $redirectTarget = Join-Path $testRoot 'redirect-sentinel'
    New-Item -ItemType Directory -Path $redirectTarget | Out-Null
    [IO.File]::WriteAllText("$redirectTarget/sentinel.txt", 'keep redirected file')
    $redirectedDesktop = Join-Path $testRoot 'desktop'
    Remove-Item -LiteralPath $redirectedDesktop
    New-Item -ItemType Junction -Path $redirectedDesktop -Target $redirectTarget | Out-Null
    try {
        Start-Setup '--install'
        [InstallerHarness]::Click($activeWindow, [InstallerHarness]::DesktopId)
        [InstallerHarness]::Click($activeWindow, [InstallerHarness]::PrimaryId)
        [WindowsHarness]::WaitFor([Func[bool]] { return [InstallerHarness]::Text($activeWindow, [InstallerHarness]::PrimaryId) -eq 'Retry' }, 'Redirected shortcut path was not rejected')
        Finish-Setup
        if ([IO.File]::ReadAllText("$redirectTarget/sentinel.txt") -ne 'keep redirected file' -or (Test-Path -LiteralPath "$redirectTarget/Core.lnk") -or (Test-Path -LiteralPath $installedExecutable)) { throw 'Rejected redirected install changed files.' }
    } finally { [IO.Directory]::Delete($redirectedDesktop) }

    [pscustomobject]@{
        Date = (Get-Date -Format o); Passed = $true; Sha256 = (Get-FileHash -LiteralPath $Executable).Hash
        PayloadSha256 = $payloadHash; AutomaticSetup = [bool]$AutomaticSetup
        Scenarios = @('Cancel makes no changes', 'Current-user installation', 'Start Menu and optional desktop shortcuts', 'Installed binary matches package', 'Windows uninstall entry', 'Existing startup path migrated', 'Repair preserves desktop preference', 'Older setup rejects a downgrade', 'Failed repair restores original files', 'Installed uninstall and app executables relocate before removal', 'Temporary uninstall executable cleaned after exit', 'Uninstall removes owned files and registration', 'Preferences and user-added files retained', 'Foreign and unverifiable shortcuts preserved', 'Redirected install path rejected')
        TestDirectory = $testRoot; SideEffects = 'Only isolated files in Windows Temp and a unique HKCU InstallerTests key; installed Core and real startup settings untouched.'
    } | ConvertTo-Json | Set-Content -LiteralPath "$projectRoot/docs/measurements/$Label.json"
    "Installer checks passed. Captures: $testRoot"
} finally {
    if ($activeProcess -and -not $activeProcess.HasExited) {
        if ($activeWindow -ne [IntPtr]::Zero) { [InstallerHarness]::Close($activeWindow) }
        if (-not $activeProcess.WaitForExit(5000)) { Stop-Process -Id $activeProcess.Id }
        $activeProcess.Dispose()
    }
    $testRegistryKey = "HKCU:\Software\Pleiades\Core\InstallerTests\$testToken"
    if (Test-Path -LiteralPath $testRegistryKey) { Remove-Item -LiteralPath $testRegistryKey -Recurse -Force }
}
