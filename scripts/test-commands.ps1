param([string]$Executable = '', [string]$Label = 'commands-checks', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CommandsHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\commands-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'appearance.ini'
Set-Content -LiteralPath $settingsFile -Value "version=1`nbackground=#000000`nposition=Center`n" -NoNewline
# Hidden command runs only: terminal and administrator rows stay dry-run in every mode.
$arguments = @(Get-TestModeArguments $Interactive) + @('--start-hidden','--dry-run','--test-commands','--reduced-motion','--settings-file', ('"' + $settingsFile + '"'))
$process = Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
$window = [IntPtr]::Zero
try {
    [WindowsHarness]::WaitFor([Func[bool]] { $script:window = [WindowsHarness]::FindWindow($process.Id); return $window -ne [IntPtr]::Zero }, 'Core did not initialize')
    [CommandsHarness]::Verify($window, $testDirectory)
    [pscustomobject]@{ Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash; Scenarios=@('typing / enters command mode with the / hidden; result rows hidden; output streams into Core''s terminal with no PowerShell CLIXML','the command is cleared after running and command mode stays','Backspace in the empty prompt returns to search','@cmd keeps its prefix after running','cd carries over to the next command, shown in the terminal prompt','keys typed into the output reach the running command: a line answering set /p (not saved to history) and a single key answering choice','typing after a command finishes goes to the search box','a full-screen program (edit, when installed) keeps Esc in the output; Esc in the search box stops it and restores the output','@cmd exit codes reported','Esc stops a running command and keeps Core open','Up and Down recall history newest first, filtered by typed text; saved newest first','output hidden outside / mode','@run and bare Run-dialog URIs offer open and administrator rows','dry run never opens Run-dialog targets'); TestDirectory=$testDirectory; SideEffects='Dry run with --test-commands: hidden echo, exit, cd, set /p, choice, ping and edit commands only; no terminal, administrator prompt or Run-dialog target is opened.' } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Command prompt, output, stop, history and Run-dialog checks passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core did not exit cleanly.' }
    $process.Dispose()
}
