Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Remove-LauncherShortcut {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ShortcutPath
    )

    if (Test-Path -LiteralPath $ShortcutPath) {
        Remove-Item -LiteralPath $ShortcutPath -Force
    }
}

function Stop-RunningInstalledLauncher {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstalledExecutablePath
    )

    $installedExecutableFullPath = [System.IO.Path]::GetFullPath($InstalledExecutablePath)

    Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
        $processPath = $null
        try {
            $processPath = $_.Path
        } catch {
            $processPath = $null
        }

        if (-not [string]::IsNullOrWhiteSpace($processPath)) {
            $processFullPath = [System.IO.Path]::GetFullPath($processPath)
            if ($processFullPath -ieq $installedExecutableFullPath) {
                Stop-Process -Id $_.Id -Force
            }
        }
    }
}

function Uninstall-CoreLauncher {
    $localAppDataPath = [Environment]::GetEnvironmentVariable('LOCALAPPDATA')
    $roamingAppDataPath = [Environment]::GetEnvironmentVariable('APPDATA')

    if ([string]::IsNullOrWhiteSpace($localAppDataPath) -or [string]::IsNullOrWhiteSpace($roamingAppDataPath)) {
        throw 'Required Windows profile paths are not available.'
    }

    $installDirectory = Join-Path $localAppDataPath 'Programs\Core Launcher'
    $installedExecutablePath = Join-Path $installDirectory 'Core Launcher.exe'
    $startMenuShortcutPath = Join-Path $roamingAppDataPath 'Microsoft\Windows\Start Menu\Programs\Core Launcher.lnk'
    $startupShortcutPath = Join-Path $roamingAppDataPath 'Microsoft\Windows\Start Menu\Programs\Startup\Core Launcher.lnk'

    Stop-RunningInstalledLauncher -InstalledExecutablePath $installedExecutablePath
    Remove-LauncherShortcut -ShortcutPath $startMenuShortcutPath
    Remove-LauncherShortcut -ShortcutPath $startupShortcutPath

    if (Test-Path -LiteralPath $installDirectory) {
        Remove-Item -LiteralPath $installDirectory -Recurse -Force
    }
}

Uninstall-CoreLauncher
