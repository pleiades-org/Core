param(
    [Parameter(Mandatory)][string]$Executable,
    [string]$SigningKeyPath = $env:CORE_RELEASE_SIGNING_KEY,
    [string]$Label = 'release-updates',
    [switch]$Interactive
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\ReleaseSigning.ps1"
if (-not $SigningKeyPath) { throw 'Updater handoff tests require the external release key via -SigningKeyPath.' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$version = (& $Executable --version | Out-String).Trim().Replace('Core ', '')
Add-Type -Path "$PSScriptRoot\WindowsHarness.cs"
$root = Join-Path "$projectRoot\target\update-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null

function Invoke-HandoffScenario([string]$Name, [bool]$ExpectRollback, [bool]$Tamper, [bool]$ManualStart = $false) {
    $directory = Join-Path $root $Name
    New-Item -ItemType Directory -Path $directory | Out-Null
    $current = Join-Path $directory 'core-v2.exe'
    $previous = Join-Path $directory 'core-v2.previous.exe'
    $helper = Join-Path $directory 'core-v2.updater.exe'
    $pending = Join-Path $directory 'core-v2.pending-update.txt'
    $settings = Join-Path $directory 'settings.ini'
    Copy-Item -LiteralPath $Executable -Destination $current
    # PE overlays are ignored by the loader; make the old copy distinguishable by its hash.
    $stream = [IO.File]::Open($current, [IO.FileMode]::Append)
    try { $stream.Write([Text.Encoding]::ASCII.GetBytes('previous test build')) } finally { $stream.Dispose() }
    $oldHash = (Get-FileHash -LiteralPath $current).Hash
    Copy-Item -LiteralPath $current -Destination $helper
    $arguments = @('--dry-run', '--test-background', '--test-stay-open', '--reduced-motion', '--settings-file', ('"' + $settings + '"'))
    $old = $null
    $guard = $null
    $armed = $null
    try {
        $old = Start-Process -FilePath $current -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardError "$directory\old.log"
        [WindowsHarness]::WaitFor([Func[bool]] { [WindowsHarness]::FindWindow($old.Id) -ne [IntPtr]::Zero }, 'Old test Core did not start')
        $oldWindow = [WindowsHarness]::FindWindow($old.Id)
        # Exercise the actual Windows running-image rename used by apply_staged.
        Move-Item -LiteralPath $current -Destination $previous
        Copy-Item -LiteralPath $Executable -Destination $current
        Write-CoreUpdateManifest -Executable $current -Version $version -SigningKeyPath $SigningKeyPath -PublicKeyPath "$projectRoot\crates\launcher\assets\release-key.bin" -OutputPath $pending
        $newHash = (Get-FileHash -LiteralPath $current).Hash
        if ($Tamper) { [IO.File]::AppendAllText($current, 'tampered after signing') }
        $token = 'Local\Pleiades.Core.Update.' + [Guid]::NewGuid().ToString('N')
        $armed = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, "$token.armed")
        $nextArguments = $arguments + @('--test-update-handoff')
        if ($ExpectRollback -and -not $Tamper) { $nextArguments += '--test-update-no-ready' }
        if (-not $ManualStart) {
            $guardArguments = @('--update-guard', $old.Id, $token) + $nextArguments
            $guard = Start-Process -FilePath $helper -ArgumentList $guardArguments -WindowStyle Hidden -PassThru -RedirectStandardError "$directory\guard.log"
            if (-not $armed.WaitOne(3000)) { throw "${Name}: helper did not arm." }
            if ($guard.HasExited) { throw "${Name}: helper exited before the old process." }
        }
        [WindowsHarness]::Close($oldWindow)
        if (-not $old.WaitForExit(5000)) { throw "${Name}: old process failed to exit." }
        if ($ManualStart) {
            $replacement = Start-Process -FilePath $current -ArgumentList $nextArguments -WindowStyle Hidden -PassThru -RedirectStandardError "$directory\startup.log"
            $replacement.Dispose()
            $deadline = [DateTime]::UtcNow.AddSeconds(15)
            while ((Test-Path -LiteralPath $pending) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        } else {
            if (-not $guard.WaitForExit(12000)) { throw "${Name}: helper did not finish." }
            if ($guard.ExitCode -ne 0) { throw "${Name}: helper failed; see $directory\guard.log" }
        }
        $expected = if ($ExpectRollback) { $oldHash } else { $newHash }
        if ((Get-FileHash -LiteralPath $current).Hash -ne $expected) { throw "${Name}: incorrect executable after handoff." }
        if (Test-Path -LiteralPath $pending) { throw "${Name}: pending marker was not cleared." }
        if (-not $ExpectRollback -and (Get-FileHash -LiteralPath $previous).Hash -ne $oldHash) { throw 'Known-good backup was not retained.' }
        [WindowsHarness]::WaitFor([Func[bool]] {
            $running = @(Get-Process core-v2 -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $current })
            return @($running | Where-Object { [WindowsHarness]::IsWindowVisible([WindowsHarness]::FindWindow($_.Id)) }).Count -eq 1
        }, "${Name}: replacement did not show its window")
    } finally {
        # Every process is matched against this scenario's absolute paths; never touch installed Core.
        foreach ($process in @(Get-Process core-v2,core-v2.updater -ErrorAction SilentlyContinue | Where-Object { $_.Path -in @($current, $previous, $helper) })) {
            $window = [WindowsHarness]::FindWindow($process.Id)
            if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
            if (-not $process.WaitForExit(3000)) { Stop-Process -Id $process.Id }
            $process.Dispose()
        }
        if ($armed) { $armed.Dispose() }
        if ($guard) { $guard.Dispose() }
        if ($old) { $old.Dispose() }
    }
}

function Test-HiddenStartup {
    $token = 'Local\Pleiades.Core.Update.' + [Guid]::NewGuid().ToString('N')
    $ready = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, $token)
    $probe = $null
    try {
        $probe = Start-Process -FilePath $Executable -ArgumentList @('--dry-run', '--probe-hidden', '--update-probe', $token) -WindowStyle Hidden -PassThru
        if (-not $probe.WaitForExit(5000)) { throw 'Hidden startup probe did not exit within five seconds.' }
        if ($probe.ExitCode -ne 0 -or -not $ready.WaitOne(0)) { throw 'Hidden startup probe did not acknowledge successful startup.' }
    } finally {
        if ($probe) {
            if (-not $probe.HasExited) { Stop-Process -Id $probe.Id }
            $probe.Dispose()
        }
        $ready.Dispose()
    }
}

Test-HiddenStartup
Invoke-HandoffScenario 'success' $false $false
Invoke-HandoffScenario 'startup-timeout' $true $false
Invoke-HandoffScenario 'tampered-before-restart' $true $true
Invoke-HandoffScenario 'ordinary-startup' $false $false $true
Invoke-HandoffScenario 'ordinary-startup-timeout' $true $false $true
Invoke-HandoffScenario 'ordinary-startup-tampered' $true $true $true
[pscustomobject]@{
    Date = (Get-Date -Format o); Passed = $true; Sha256 = (Get-FileHash -LiteralPath $Executable).Hash
    Scenarios = @('Hidden startup probe', 'Running executable renamed', 'Old process exits before replacement startup', 'Visible-window acknowledgment', 'Previous executable retained', 'Five-second startup rollback', 'Tampered executable rollback', 'Ordinary next-start success', 'Ordinary next-start timeout rollback', 'Ordinary next-start tampering rollback')
    TestDirectory = $root; SideEffects = 'Isolated copies, background dry-run windows; no installed Core process is stopped and no network requests are made.'
} | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
'Update handoff, verification and rollback checks passed.'
