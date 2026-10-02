$ErrorActionPreference = 'Stop'
$cleanupDirectory = [IO.Path]::GetFullPath($env:CORE_SETUP_CLEANUP_DIRECTORY)
$temporaryDirectory = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$cleanupParent = [IO.Path]::GetDirectoryName($cleanupDirectory).TrimEnd('\')
$cleanupName = [IO.Path]::GetFileName($cleanupDirectory)
if ($cleanupParent -ne $temporaryDirectory -or $cleanupName -notmatch '^Pleiades-Core-Uninstall-[0-9]+-[0-9]+$') {
    throw 'Refusing to clean a directory outside Core''s temporary uninstall directory.'
}
$cleanupProcessId = 0
if (-not [int]::TryParse($env:CORE_SETUP_CLEANUP_PID, [ref]$cleanupProcessId) -or $cleanupProcessId -le 0) {
    throw 'The temporary uninstaller process ID is invalid.'
}
try { $uninstallerProcess = [Diagnostics.Process]::GetProcessById($cleanupProcessId) }
catch [ArgumentException] { $uninstallerProcess = $null }
if ($uninstallerProcess) {
    try {
        if (-not $uninstallerProcess.WaitForExit(30000)) { throw 'The temporary uninstaller did not exit.' }
    } finally { $uninstallerProcess.Dispose() }
}
$cleanupExecutable = Join-Path $cleanupDirectory 'uninstall.exe'
foreach ($cleanupPath in @($cleanupDirectory, $cleanupExecutable)) {
    if (Test-Path -LiteralPath $cleanupPath) {
        $cleanupItem = Get-Item -LiteralPath $cleanupPath -Force
        if ($cleanupItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Refusing to clean a redirected uninstall path.'
        }
    }
}
if (Test-Path -LiteralPath $cleanupExecutable) { Remove-Item -LiteralPath $cleanupExecutable -Force }
if (Test-Path -LiteralPath $cleanupDirectory) { Remove-Item -LiteralPath $cleanupDirectory }
