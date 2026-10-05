param(
    [string]$Candidate = '',
    [string]$SigningKeyPath = $env:CORE_RELEASE_SIGNING_KEY,
    # Package from a background-only run (scripts\run-release-checks.ps1 without -Interactive).
    # Click-away dismissal and pointer hover are then untested; release-package.json records it.
    [switch]$SkipInteractive
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot\ReleaseSigning.ps1"
if (-not $SigningKeyPath) { throw 'Set CORE_RELEASE_SIGNING_KEY or pass -SigningKeyPath with the external PKCS#8 release key.' }
$metadata = cargo metadata --no-deps --format-version 1 --manifest-path "$projectRoot\Cargo.toml" | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Could not read release version from Cargo.' }
$version = ($metadata.packages | Where-Object name -eq 'core-launcher-v2').version
if (-not $Candidate) { $Candidate = Join-Path $projectRoot 'target\styling\release\core-v2.exe' }
if ((& $Candidate --version | Out-String).Trim() -ne "Core $version") { throw 'Candidate version does not match Cargo.toml.' }
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Candidate).Hash
$required = @('release-controls','release-extensions','release-settings','release-shortcuts','release-integration','release-styling','release-quicklinks','release-commands','release-updates','release-installer')
if (-not $SkipInteractive) { $required += 'release-dismissal' }
foreach ($name in $required) {
    $record = Get-Content -LiteralPath (Join-Path "$projectRoot\docs\measurements" "$name.json") -Raw | ConvertFrom-Json
    if (-not $record.Passed -or $record.Sha256 -ne $hash) { throw "The candidate does not match a passing $name validation record." }
    # Controls hover only runs interactively; a background record is accepted only when skipping.
    if ($name -eq 'release-controls' -and -not $SkipInteractive -and $record.Interactive -eq $false) {
        throw 'release-controls ran without pointer hover. Rerun scripts\run-release-checks.ps1 -Interactive, or package with -SkipInteractive.'
    }
}
$packageDirectory = Join-Path $projectRoot "dist\Core-$version-windows-x64"
New-Item -ItemType Directory -Path $packageDirectory -Force | Out-Null
Copy-Item -LiteralPath $Candidate -Destination "$packageDirectory\core-v2.exe" -Force
$setupName = "Core-Setup-$version.exe"
$setupExecutable = Join-Path $packageDirectory $setupName
Copy-Item -LiteralPath $Candidate -Destination $setupExecutable -Force
$markerBytes = [IO.File]::ReadAllBytes("$projectRoot/crates/launcher/src/windows/installer/setup-marker.txt")
$setupStream = [IO.File]::Open($setupExecutable, [IO.FileMode]::Append, [IO.FileAccess]::Write)
try { $setupStream.Write($markerBytes, 0, $markerBytes.Length) } finally { $setupStream.Dispose() }
Write-CoreUpdateManifest -Executable "$packageDirectory\core-v2.exe" -Version $version -SigningKeyPath $SigningKeyPath -PublicKeyPath "$projectRoot\crates\launcher\assets\release-key.bin" -OutputPath "$packageDirectory\core-update.txt"
$releaseNotes = "$projectRoot\docs\RELEASE_$version.md"
if (-not (Test-Path -LiteralPath $releaseNotes)) { throw "Write docs\RELEASE_$version.md before packaging." }
Copy-Item -LiteralPath $releaseNotes -Destination "$packageDirectory\RELEASE_NOTES.md" -Force
@'
Core v2 — Windows x64

Run Core-Setup-<version>.exe to install for your current Windows account.
No administrator prompt is needed. Setup adds Core to the Start Menu and
Windows Installed apps, with an optional desktop shortcut. Exit a running
Core from its tray menu before installing or uninstalling.
For portable use, extract the ZIP and run core-v2.exe instead.
Requires 64-bit Windows and the Microsoft Visual C++ v14 x64 runtime.
Ctrl+Alt+Space opens or hides Core by default. Escape or clicking outside hides it.
With nothing typed, Core shows your recently used apps as a grid: apps opened
from Core first, then apps Windows has seen you start (read locally from
Windows' own usage record). Arrow keys move; Enter or a click opens.

The top-right gear opens Settings, with categories in a left sidebar and their
controls on the right. Appearance controls the background, seven screen
positions illustrated with small screen boxes, corner rounding (0-32 px) and
screen edge spacing (0-200 px from the physical screen edge; 0 stays above the
taskbar automatically). Behaviour controls the shortcut,
display and Windows startup. Changes save automatically and survive restarting
Core; Done returns to search. A failed save shows an explanation and Retry save.
Choose "Use Windows key" to replace a bare Windows-key tap with Core. Type
taskbar or tb to reveal an auto-hidden Windows taskbar in that mode.

Settings > Quicklinks has Link and Name columns. Fill both to append a blank row;
scroll to add more and use the x button to remove a row. Completed valid entries
save automatically. Search a name normally, or type > / @quicklink for links only.
Links can be HTTP/HTTPS websites, app links such as steam://rungameid/1,
or absolute Windows file/folder paths.
Direct executable links start in the executable's own folder so relative data
files can be found. Windows handles Steam and other shortcuts as before.
Website icons come from Google's favicon service, falling back to the site;
local and private hosts are never sent to Google.

Type / to turn the search box into a command prompt for your shell (Windows
Terminal's default profile unless Settings > Behaviour chooses another). Enter
runs it in Core's own terminal and clears the prompt; Ctrl+Enter opens a
terminal window; Ctrl+Shift+Enter runs as administrator. Keys typed into Core's
terminal reach the running command as you press them: prompts, REPLs such as
python, and full-screen programs such as edit or vim work. Esc stops a running
command (in a full-screen program, press Esc in the search box). Up and Down
step through recent commands, and Backspace in the empty prompt returns to
search. cd carries over from one command to the next.
@run notepad, shell:startup and %temp% open like Windows Run (Win+R).

The bottom-right power icon opens Sleep, Restart and Power off. Commands such as
@power and restart work too. Each action has a separate confirmation; Cancel is
selected by default. Power icons highlight on hover. The footer remains visible
when Windows limits Core's size to fit the display.

The footer shows one hint for the selected action, such as Enter to copy or
Enter to open, with the local time centred in HH:MM format. The clock updates
each minute while search is visible; it stops while Core is hidden or Settings
is open. Errors and command completion status appear in place of the hint.

Type conversions and calculations directly; docs/SMART_CONVERSIONS.md in the
source repository lists every form. Currency uses European Central Bank reference
rates, downloaded in the background while Core is shown and cached for offline use.

Try these queries:
  time
  date
  100 usd to eur
  10 GB at 100 Mbps
  20% off 80
  tip 15% on 80 split 4
  255 to hex
  #ff8800
  unix now
  mortgage 250k at 4.5% for 25 years
  2 days from now
  next Friday
  3 hours ago
  2028-01-31 + 1 month
  10 kg to lb
  100c to f
  sqrt(81) + round(2.6)
  9pm et to uk
  /ipconfig
  xbox

Media: type @media or @music to control Spotify or any player Windows knows
(play, pause, next, previous), or type play, pause, next or now playing on
their own. Core prefers music apps over a paused browser tab. A now-playing bar
above the search box shows the track, its art and progress, with buttons.
Rest the pointer on the album art for a volume slider: it changes that
player's volume in Windows' volume mixer, not the whole PC's.
Settings > Music chooses Music apps first or Playing media first, preferred
and ignored apps, the bar, and media shortcuts that work while Core is open
or in every app. Shortcut boxes record keys: click one and press the keys.

Optional: Settings > Music > Spotify song search connects your own Spotify
Premium account. Type @song and a song or artist, then press Enter to play
the chosen song. The Spotify volume switch beside it makes the bar's slider
set Spotify's own volume. Off by default; docs/SPOTIFY.md in the source
repository has the one-time setup.

Keep the executable in a stable folder before enabling Start with Windows.
Startup runs at your Windows sign-in. Core's tray menu can exit the application.
Your preferences live in %APPDATA%\Pleiades\Core\v2\appearance.ini.
Diagnostics are written to core.log in the same folder, and command history to
command-history.txt (Settings > Behaviour > Clear command history deletes it).

Settings > Behaviour > Updates chooses Automatic (default), Notify or Off.
Automatic checks GitHub when Core is shown, at most once a day (one-hour retry
after errors), and downloads signed updates. Verified updates install on exit;
type @update and press Enter to check GitHub immediately (at most once a minute),
or restart to install a ready update. Notify only checks and opens the release page.
Off disables update checks and installation. Read-only folders use notify-only.
Core keeps core-v2.previous.exe and rolls back if an updated restart fails to
show its window within five seconds. Back up the external release-signing key.

Updates use signed manifests; the executable does not have Authenticode signing.
No administrator account, browser runtime or
network connection is required for local calculations and application search.
See RELEASE_NOTES.md for tested behavior, benchmarks and remaining limitations.
'@ | Set-Content -LiteralPath "$packageDirectory\README.txt" -Encoding utf8
$checksums = foreach ($name in @($setupName,'core-v2.exe','core-update.txt','README.txt','RELEASE_NOTES.md')) {
    $fileHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $packageDirectory $name)).Hash.ToLowerInvariant()
    "$fileHash  $name"
}
$checksums | Set-Content -LiteralPath "$packageDirectory\SHA256SUMS.txt" -Encoding ascii
$archive = Join-Path $projectRoot "dist\Core-$version-windows-x64.zip"
Compress-Archive -LiteralPath "$packageDirectory\core-v2.exe","$packageDirectory\core-update.txt","$packageDirectory\README.txt","$packageDirectory\RELEASE_NOTES.md","$packageDirectory\SHA256SUMS.txt" -DestinationPath $archive -Force -CompressionLevel Optimal
$opened = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $entry = $opened.GetEntry('core-v2.exe')
    if ($null -eq $entry) { throw 'Release archive is missing core-v2.exe.' }
    $stream = $entry.Open()
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $archivedHash = [Convert]::ToHexString($hasher.ComputeHash($stream)) }
    finally { $hasher.Dispose(); $stream.Dispose() }
    if ($archivedHash -ne $hash) { throw 'Archived executable hash mismatch.' }
} finally { $opened.Dispose() }
[pscustomobject]@{SetupExecutable=$setupExecutable; Executable="$packageDirectory\core-v2.exe"; Archive=$archive; InteractiveChecksSkipped=[bool]$SkipInteractive; ExecutableBytes=(Get-Item "$packageDirectory\core-v2.exe").Length; ArchiveBytes=(Get-Item $archive).Length; Sha256=$hash} | ConvertTo-Json | Tee-Object -FilePath "$projectRoot\docs\measurements\release-package.json"
