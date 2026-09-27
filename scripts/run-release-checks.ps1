param(
    [string]$Executable = '',
    # Adds checks that use the real foreground, mouse pointer and screen: click-away dismissal and
    # pointer hover. Leave the computer alone while they run.
    [switch]$Interactive,
    # With -Interactive, also injects physical keystrokes for shortcuts and quicklink tabbing.
    [switch]$PhysicalKeyboard
)
# Runs every release validation script against one executable. By default nothing takes the
# foreground, moves the pointer or types, so you can keep working while it runs.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $Executable) { $Executable = Join-Path $projectRoot 'target\styling\release\core-v2.exe' }
if ($PhysicalKeyboard -and -not $Interactive) { throw '-PhysicalKeyboard requires -Interactive.' }
# Each script loads its harness with Add-Type, so each needs a fresh PowerShell 7 process.
$pwsh = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
if (-not $pwsh) { $pwsh = Join-Path $env:USERPROFILE '.dotnet\tools\pwsh.exe' }
if (-not (Test-Path -LiteralPath $pwsh)) { throw 'PowerShell 7 (pwsh) is required: dotnet tool install --global PowerShell' }

$mode = if ($Interactive) { @('-Interactive') } else { @() }
$runs = @(
    @{ Script = 'test-settings.ps1'; Label = 'release-settings'; Extra = @() },
    @{ Script = 'test-controls.ps1'; Label = 'release-controls'; Extra = @() },
    @{ Script = 'test-extensions.ps1'; Label = 'release-extensions'; Extra = @() },
    @{ Script = 'test-shortcuts.ps1'; Label = 'release-shortcuts'; Extra = $(if ($PhysicalKeyboard) { @() } else { @('-RegistrationOnly') }) },
    @{ Script = 'test-windows.ps1'; Label = 'release-integration'; Extra = @() },
    @{ Script = 'test-styling.ps1'; Label = 'release-styling'; Extra = @() },
    @{ Script = 'test-quicklinks.ps1'; Label = 'release-quicklinks'; Extra = $(if ($PhysicalKeyboard) { @() } else { @('-SkipKeyboard') }) },
    @{ Script = 'test-commands.ps1'; Label = 'release-commands'; Extra = @() }
)
if ($Interactive) {
    $runs += @{ Script = 'test-dismissal.ps1'; Label = 'release-dismissal'; Extra = @() }
}
$failed = 0
foreach ($run in $runs) {
    $arguments = @('-NoProfile', '-File', (Join-Path $PSScriptRoot $run.Script), '-Executable', $Executable, '-Label', $run.Label) + $mode + $run.Extra
    $output = & $pwsh @arguments 2>&1 | Out-String
    $lines = $output.Trim() -split "`n"
    if ($LASTEXITCODE -eq 0) {
        "PASS  $($run.Script): $($lines[-1].Trim())"
    } else {
        $failed++
        "FAIL  $($run.Script)"
        $lines | Select-Object -Last 12 | ForEach-Object { "      $_" }
    }
}
if (-not $Interactive) { 'SKIP  test-dismissal.ps1 and pointer hover (need -Interactive)' }
if ($failed -gt 0) { throw "$failed release check(s) failed." }
