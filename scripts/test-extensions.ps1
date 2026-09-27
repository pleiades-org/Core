param([string]$Executable = '', [string]$Label = 'extensions-checks', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\TestMode.ps1"
if (-not $Executable) { $Executable = "$projectRoot\target\release\core-v2.exe" }
Add-Type -Path @("$PSScriptRoot\WindowsHarness.cs", "$PSScriptRoot\CaptureWindow.cs", "$PSScriptRoot\SettingsHarness.cs", "$PSScriptRoot\ExtensionsHarness.cs")
$testDirectory = Join-Path "$projectRoot\target\extensions-tests" ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$settingsFile = Join-Path $testDirectory 'settings.ini'
$process = Start-Process -FilePath $Executable -ArgumentList (@(Get-TestModeArguments $Interactive) + @(@('--start-hidden','--dry-run','--test-stay-open','--reduced-motion','--settings-file', ('"' + $settingsFile + '"'),'--exchange-rates-file', ('"' + "$PSScriptRoot\fixtures\ecb-rates.xml" + '"')))) -WindowStyle Hidden -PassThru -RedirectStandardError "$testDirectory\stderr.txt"
$window = [IntPtr]::Zero
try {
    [WindowsHarness]::WaitFor([Func[bool]] { $script:window = [WindowsHarness]::FindWindow($process.Id); return $window -ne [IntPtr]::Zero }, 'Core did not initialize')
    [ExtensionsHarness]::Verify($window, "$projectRoot\docs\measurements")
    $saved = Get-Content -LiteralPath $settingsFile -Raw
    if ($saved -notmatch 'shortcut=Ctrl\+Shift\+F11' -or $saved -notmatch 'startup=true' -or $saved -notmatch 'display=\\\\\.\\DISPLAY') { throw 'Behaviour settings were not persisted correctly.' }
    [pscustomobject]@{ Date=(Get-Date -Format o); Passed=$true; Interactive=[bool]$Interactive; Sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Executable).Hash; Scenarios=@('relative dates and leap-year month end','smart conversions: currency with fixture ECB rates, download time, number bases, percentages, colours, pints versus Pacific time','unit conversions and math functions','invalid math clears pending action','large calculator card','settings and power icons in requested corners','inline power popup and keyboard navigation','Escape closes popup before launcher','all three power options default to Cancel','confirmed command reaches dry-run dispatch','installed Xbox discovered','shortcut validation','specific monitor selection','startup preference persisted without registry side effects','Windows-key preset'); TestDirectory=$testDirectory; SideEffects='External actions, startup registration and global shortcuts disabled by --dry-run. Separate shortcut tests are required.' } | ConvertTo-Json | Set-Content "$projectRoot\docs\measurements\$Label.json"
    'Calculator, power menu, Xbox and behaviour settings checks passed.'
} finally {
    if ($window -ne [IntPtr]::Zero) { [WindowsHarness]::Close($window) }
    if (-not $process.WaitForExit(5000)) { Stop-Process -Id $process.Id; throw 'Core did not shut down.' }
    $process.Dispose()
}
