[CmdletBinding()]
param(
    # Quiet mode for unattended installs: no app launch after install.
    [switch]$Silent
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RequiredEnvironmentPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$VariableName
    )

    $environmentPath = [Environment]::GetEnvironmentVariable($VariableName)
    if ([string]::IsNullOrWhiteSpace($environmentPath)) {
        throw "Required environment variable '$VariableName' is not available."
    }

    return $environmentPath
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

function New-LauncherShortcut {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ShortcutPath,

        [Parameter(Mandatory = $true)]
        [string]$TargetPath,

        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory,

        [Parameter(Mandatory = $true)]
        [string]$Description,

        [Parameter(Mandatory = $true)]
        [string]$IconPath
    )

    $shortcutDirectory = Split-Path -Parent $ShortcutPath
    New-Item -ItemType Directory -Force -Path $shortcutDirectory | Out-Null

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($ShortcutPath)
    $shortcut.TargetPath = $TargetPath
    $shortcut.WorkingDirectory = $WorkingDirectory
    $shortcut.Description = $Description
    $shortcut.IconLocation = $IconPath
    $shortcut.Save()
}

function Install-CoreLauncher {
    param(
        [switch]$Silent
    )

    $localAppDataPath = Get-RequiredEnvironmentPath -VariableName 'LOCALAPPDATA'
    $roamingAppDataPath = Get-RequiredEnvironmentPath -VariableName 'APPDATA'
    $sourceExecutablePath = Join-Path $PSScriptRoot 'core.exe'
    $sourceIconPath = Join-Path $PSScriptRoot 'app_icon.ico'

    if (-not (Test-Path -LiteralPath $sourceExecutablePath)) {
        throw "The setup package is missing core.exe."
    }

    $installDirectory = Join-Path $localAppDataPath 'Programs\Core Launcher'
    $installedExecutablePath = Join-Path $installDirectory 'Core Launcher.exe'
    $installedIconPath = Join-Path $installDirectory 'Core Launcher.ico'
    $startMenuDirectory = Join-Path $roamingAppDataPath 'Microsoft\Windows\Start Menu\Programs'
    $startupDirectory = Join-Path $roamingAppDataPath 'Microsoft\Windows\Start Menu\Programs\Startup'
    $startMenuShortcutPath = Join-Path $startMenuDirectory 'Core Launcher.lnk'
    $startupShortcutPath = Join-Path $startupDirectory 'Core Launcher.lnk'

    New-Item -ItemType Directory -Force -Path $installDirectory | Out-Null
    Stop-RunningInstalledLauncher -InstalledExecutablePath $installedExecutablePath
    Copy-Item -LiteralPath $sourceExecutablePath -Destination $installedExecutablePath -Force
    $shortcutIconPath = $installedExecutablePath
    if (Test-Path -LiteralPath $sourceIconPath) {
        Copy-Item -LiteralPath $sourceIconPath -Destination $installedIconPath -Force
        $shortcutIconPath = $installedIconPath
    }

    New-LauncherShortcut `
        -ShortcutPath $startMenuShortcutPath `
        -TargetPath $installedExecutablePath `
        -WorkingDirectory $installDirectory `
        -Description 'Core Launcher' `
        -IconPath $shortcutIconPath

    New-LauncherShortcut `
        -ShortcutPath $startupShortcutPath `
        -TargetPath $installedExecutablePath `
        -WorkingDirectory $installDirectory `
        -Description 'Start Core Launcher when Windows signs in' `
        -IconPath $shortcutIconPath

    if (-not $Silent) {
        Start-Process -FilePath $installedExecutablePath -WorkingDirectory $installDirectory
    }
}

Install-CoreLauncher -Silent:$Silent
