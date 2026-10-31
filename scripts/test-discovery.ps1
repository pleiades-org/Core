param([string]$Executable = '', [string]$Label = 'application-discovery', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = Join-Path $projectRoot 'target\debug\core-v2.exe' }
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
$testDirectory = Join-Path "$projectRoot\target\discovery-tests" ([Guid]::NewGuid().ToString('N'))
$appData = Join-Path $testDirectory 'AppData'
$programData = Join-Path $testDirectory 'ProgramData'
$startMenu = Join-Path $appData 'Microsoft\Windows\Start Menu\Programs'
New-Item -ItemType Directory -Path $startMenu, $programData -Force | Out-Null
$existingName = 'CoreDiscoveryExistingFixture'
$installedName = 'CoreDiscoveryInstalledFixture'
$installedPath = Join-Path $startMenu "$installedName.url"
$shortcutContents = "[InternetShortcut]`r`nURL=https://example.invalid/`r`n"
Set-Content -LiteralPath (Join-Path $startMenu "$existingName.url") -Value $shortcutContents
$process = $null
$window = [IntPtr]::Zero

function Wait-ForFirstRow {
    param([string]$Expected, [int]$TimeoutSeconds = 15)
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    while ($elapsed.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        if ($process.HasExited) { throw 'Core exited during application discovery checks.' }
        if ($Expected -eq '' -and [WindowsHarness]::Count($window) -eq 0) { return }
        if ($Expected -ne '' -and [WindowsHarness]::First($window) -eq $Expected) { return }
        Start-Sleep -Milliseconds 20
    }
    throw "Expected first row '$Expected'; got '$([WindowsHarness]::First($window))'. Evidence: $testDirectory"
}

try {
    # Only the child sees the fixture Start Menu; the account's shortcuts and settings stay intact.
    $process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @('--dry-run', '--start-hidden', '--reduced-motion')) -Environment @{ APPDATA = $appData; ProgramData = $programData } -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
    [WindowsHarness]::WaitFor([Func[bool]] {
        $script:window = [WindowsHarness]::FindWindow($process.Id)
        return $script:window -ne [IntPtr]::Zero
    }, 'Core did not initialize')

    [WindowsHarness]::Query($window, "@app $existingName")
    Wait-ForFirstRow $existingName
    [WindowsHarness]::Query($window, "@app $installedName")
    Wait-ForFirstRow ''

    Set-Content -LiteralPath $installedPath -Value $shortcutContents
    [WindowsHarness]::Show($window)
    Wait-ForFirstRow $installedName

    if (-not [WindowsHarness]::PostMessageW($window, 0x0111, [IntPtr]105, [IntPtr]::Zero)) {
        throw 'Could not open the power menu.'
    }
    [WindowsHarness]::WaitFor([Func[bool]] {
        [WindowsHarness]::IsWindowVisible([WindowsHarness]::GetDlgItem($window, 106))
    }, 'The power menu did not open')

    # A real timer refresh must update the current query while Core stays open.
    Remove-Item -LiteralPath $installedPath
    Wait-ForFirstRow '' 45
    if (-not [WindowsHarness]::IsWindowVisible([WindowsHarness]::GetDlgItem($window, 106))) {
        throw 'Background application discovery dismissed the power menu.'
    }
    if (-not [WindowsHarness]::PostMessageW($window, 0x0111, [IntPtr]105, [IntPtr]::Zero)) {
        throw 'Could not close the power menu.'
    }
    [WindowsHarness]::WaitFor([Func[bool]] {
        -not [WindowsHarness]::IsWindowVisible([WindowsHarness]::GetDlgItem($window, 106))
    }, 'The power menu did not close')
    [WindowsHarness]::Escape($window)
    [WindowsHarness]::WaitFor([Func[bool]] { -not [WindowsHarness]::IsWindowVisible($window) }, 'Core did not hide')
    Set-Content -LiteralPath $installedPath -Value $shortcutContents
    [WindowsHarness]::Show($window)
    Wait-ForFirstRow $installedName
    [WindowsHarness]::Enter($window)
    [WindowsHarness]::WaitFor([Func[bool]] {
        [WindowsHarness]::Text([WindowsHarness]::GetDlgItem($window, 102)).Contains('Verified action')
    }, 'The refreshed application did not have a launch action')

    $record = [pscustomobject]@{
        Date = Get-Date -Format o
        Passed = $true
        Interactive = [bool]$Interactive
        Executable = $Executable
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash
        TestDirectory = $testDirectory
        Scenarios = @('discover initial Start Menu entry', 'find newly installed app on open without restarting', 'remove uninstalled app on visible timer without editing query', 'preserve open power menu during background discovery', 'find reinstalled app on reopen', 'accept refreshed launch target in dry run')
    }
    $recordDirectory = Join-Path $projectRoot 'docs\measurements'
    New-Item -ItemType Directory -Path $recordDirectory -Force | Out-Null
    $record | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $recordDirectory "$Label.json")
    'Application discovery install, refresh and removal checks passed.'
} finally {
    if ($null -ne $process) {
        if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
        if (-not $process.WaitForExit(5000)) {
            Stop-Process -Id $process.Id
            throw 'Core failed to shut down cleanly.'
        }
        $process.Dispose()
    }
}
