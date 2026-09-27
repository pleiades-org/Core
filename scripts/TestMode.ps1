# Dot-sourced by the test scripts: . "$PSScriptRoot\TestMode.ps1"
#
# Background mode (the default) starts Core with --test-background, so it never takes the
# foreground and click-away dismissal is off: you can keep working while tests run. Checks that
# need the real foreground, mouse pointer or keyboard only run with -Interactive; start those
# when you can leave the computer alone for a few minutes.

function Get-TestModeArguments {
    param([bool]$Interactive)
    if ($Interactive) { @() } else { @('--test-background') }
}

function Assert-InteractiveMode {
    param([bool]$Interactive, [string]$Reason)
    if (-not $Interactive) {
        throw "$Reason Rerun with -Interactive when you can leave the keyboard and mouse alone."
    }
}
