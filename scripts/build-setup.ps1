Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RepositoryRoot {
    return (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
}

function Assert-PathInsideRepository {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,

        [Parameter(Mandatory = $true)]
        [string]$TargetPath
    )

    $repositoryFullPath = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\')
    $targetFullPath = [System.IO.Path]::GetFullPath($TargetPath).TrimEnd('\')

    if (-not $targetFullPath.StartsWith($repositoryFullPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify path outside the repository: $targetFullPath"
    }
}

function Remove-DirectoryInsideRepository {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,

        [Parameter(Mandatory = $true)]
        [string]$DirectoryPath
    )

    if (-not (Test-Path -LiteralPath $DirectoryPath)) {
        return
    }

    Assert-PathInsideRepository -RepositoryRoot $RepositoryRoot -TargetPath $DirectoryPath
    Remove-Item -LiteralPath $DirectoryPath -Recurse -Force
}

function Invoke-CargoReleaseBuild {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot
    )

    Push-Location $RepositoryRoot
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --release failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }
}

function Get-IExpressPath {
    $iexpressCommand = Get-Command iexpress.exe -ErrorAction SilentlyContinue
    if ($null -eq $iexpressCommand) {
        throw 'iexpress.exe was not found. IExpress ships with Windows and is required to build CoreLauncherSetup.exe.'
    }

    return $iexpressCommand.Source
}

function Copy-SetupPayload {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,

        [Parameter(Mandatory = $true)]
        [string]$StagingDirectory
    )

    $releaseExecutablePath = Join-Path $RepositoryRoot 'target\release\core.exe'
    $installScriptPath = Join-Path $RepositoryRoot 'installer\install-core-launcher.ps1'
    $uninstallScriptPath = Join-Path $RepositoryRoot 'installer\uninstall-core-launcher.ps1'
    $appIconPath = Join-Path $RepositoryRoot 'assets\app_icon.ico'

    if (-not (Test-Path -LiteralPath $releaseExecutablePath)) {
        throw "Release executable was not found: $releaseExecutablePath"
    }

    New-Item -ItemType Directory -Force -Path $StagingDirectory | Out-Null
    Copy-Item -LiteralPath $releaseExecutablePath -Destination (Join-Path $StagingDirectory 'core.exe') -Force
    Copy-Item -LiteralPath $installScriptPath -Destination (Join-Path $StagingDirectory 'install-core-launcher.ps1') -Force
    Copy-Item -LiteralPath $uninstallScriptPath -Destination (Join-Path $StagingDirectory 'uninstall-core-launcher.ps1') -Force
    Copy-Item -LiteralPath $appIconPath -Destination (Join-Path $StagingDirectory 'app_icon.ico') -Force

    $vbsRunnerPath = Join-Path $StagingDirectory 'run-installer.vbs'
    $vbsContent = @"
Set WshShell = CreateObject("WScript.Shell")
WshShell.Run "powershell.exe -WindowStyle Hidden -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ""install-core-launcher.ps1""", 0, True
"@
    Set-Content -LiteralPath $vbsRunnerPath -Value $vbsContent -Encoding ASCII
}

function New-IExpressSedFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SedPath,

        [Parameter(Mandatory = $true)]
        [string]$StagingDirectory,

        [Parameter(Mandatory = $true)]
        [string]$SetupExecutablePath
    )

    $stagingDirectoryForSed = [System.IO.Path]::GetFullPath($StagingDirectory).TrimEnd('\') + '\'
    $setupExecutableFullPath = [System.IO.Path]::GetFullPath($SetupExecutablePath)

    $sedText = @"
[Version]
Class=IEXPRESS
SEDVersion=3

[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles

[Strings]
InstallPrompt=
DisplayLicense=
FinishMessage=Core Launcher was installed successfully.
TargetName=$setupExecutableFullPath
FriendlyName=Core Launcher Setup
AppLaunched=wscript.exe run-installer.vbs
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
FILE0="core.exe"
FILE1="install-core-launcher.ps1"
FILE2="uninstall-core-launcher.ps1"
FILE3="app_icon.ico"
FILE4="run-installer.vbs"

[SourceFiles]
SourceFiles0=$stagingDirectoryForSed

[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
%FILE3%=
%FILE4%=
"@

    Set-Content -LiteralPath $SedPath -Value $sedText -Encoding ASCII
}

function Invoke-IExpressBuild {
    param(
        [Parameter(Mandatory = $true)]
        [string]$IExpressPath,

        [Parameter(Mandatory = $true)]
        [string]$SedPath,

        [Parameter(Mandatory = $true)]
        [string]$SetupExecutablePath
    )

    $iexpressProcess = Start-Process `
        -FilePath $IExpressPath `
        -ArgumentList @('/N', '/Q', $SedPath) `
        -Wait `
        -PassThru `
        -WindowStyle Hidden
    $iexpressExitCode = $iexpressProcess.ExitCode

    if (-not (Test-Path -LiteralPath $SetupExecutablePath)) {
        if ($iexpressExitCode -ne 0) {
            throw "iexpress.exe failed with exit code $iexpressExitCode."
        }

        throw "IExpress completed but did not create the setup executable: $SetupExecutablePath"
    }

    $setupExecutable = Get-Item -LiteralPath $SetupExecutablePath
    if ($setupExecutable.Length -le 0) {
        throw "IExpress created an empty setup executable: $SetupExecutablePath"
    }

    if ($iexpressExitCode -ne 0) {
        Write-Warning "iexpress.exe returned exit code $iexpressExitCode after creating the setup executable."
    }
}

function Set-SetupExecutableIcon {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SetupExecutablePath,

        [Parameter(Mandatory = $true)]
        [string]$IconPath
    )

    if (-not (Test-Path -LiteralPath $IconPath)) {
        Write-Warning "App icon file not found: $IconPath"
        return
    }

    $iconUpdaterSource = @"
using System;
using System.IO;
using System.Runtime.InteropServices;

public static class SetupIconHelper {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr BeginUpdateResource(string pFileName, bool bDeleteExistingResources);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool UpdateResource(IntPtr hUpdate, IntPtr lpType, IntPtr lpName, ushort wLanguage, byte[] lpData, uint cbData);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool EndUpdateResource(IntPtr hUpdate, bool fDiscard);

    private static readonly IntPtr RT_ICON = (IntPtr)3;
    private static readonly IntPtr RT_GROUP_ICON = (IntPtr)14;

    public static bool ApplyIcon(string exePath, string icoPath) {
        try {
            if (!File.Exists(exePath) || !File.Exists(icoPath)) return false;

            byte[] icoBytes = File.ReadAllBytes(icoPath);
            if (icoBytes.Length < 6) return false;

            ushort count = BitConverter.ToUInt16(icoBytes, 4);
            if (count == 0) return false;

            IntPtr hUpdate = BeginUpdateResource(exePath, false);
            if (hUpdate == IntPtr.Zero) return false;

            int groupHeaderSize = 6 + (count * 14);
            byte[] groupBytes = new byte[groupHeaderSize];
            Array.Copy(icoBytes, 0, groupBytes, 0, 6);

            for (ushort i = 0; i < count; i++) {
                int icoEntryOffset = 6 + (i * 16);
                int groupEntryOffset = 6 + (i * 14);

                Array.Copy(icoBytes, icoEntryOffset, groupBytes, groupEntryOffset, 12);
                ushort iconId = (ushort)(i + 1);
                byte[] idBytes = BitConverter.GetBytes(iconId);
                Array.Copy(idBytes, 0, groupBytes, groupEntryOffset + 12, 2);

                uint bytesInRes = BitConverter.ToUInt32(icoBytes, icoEntryOffset + 8);
                uint imageOffset = BitConverter.ToUInt32(icoBytes, icoEntryOffset + 12);

                if (imageOffset + bytesInRes > icoBytes.Length) continue;

                byte[] iconImageData = new byte[bytesInRes];
                Array.Copy(icoBytes, (int)imageOffset, iconImageData, 0, (int)bytesInRes);

                UpdateResource(hUpdate, RT_ICON, (IntPtr)iconId, 0, iconImageData, (uint)iconImageData.Length);
            }

            UpdateResource(hUpdate, RT_GROUP_ICON, (IntPtr)1, 0, groupBytes, (uint)groupBytes.Length);
            return EndUpdateResource(hUpdate, false);
        } catch {
            return false;
        }
    }
}
"@
    Add-Type -TypeDefinition $iconUpdaterSource -Language CSharp
    $applied = [SetupIconHelper]::ApplyIcon($SetupExecutablePath, $IconPath)
    if ($applied) {
        Write-Host "Applied Core Launcher icon to setup executable."
    } else {
        Write-Warning "Could not update setup executable icon."
    }
}

function Build-CoreLauncherSetup {
    $repositoryRoot = Get-RepositoryRoot
    $distDirectory = Join-Path $repositoryRoot 'dist'
    $setupDirectory = Join-Path $distDirectory 'setup'
    $stagingDirectory = Join-Path $setupDirectory 'staging'
    $sedPath = Join-Path $setupDirectory 'core-launcher.sed'
    $setupExecutablePath = Join-Path $distDirectory 'CoreLauncherSetup.exe'
    $appIconPath = Join-Path $repositoryRoot 'assets\app_icon.ico'
    $iexpressPath = Get-IExpressPath

    Remove-DirectoryInsideRepository -RepositoryRoot $repositoryRoot -DirectoryPath $setupDirectory
    New-Item -ItemType Directory -Force -Path $setupDirectory | Out-Null
    New-Item -ItemType Directory -Force -Path $distDirectory | Out-Null

    if (Test-Path -LiteralPath $setupExecutablePath) {
        Assert-PathInsideRepository -RepositoryRoot $repositoryRoot -TargetPath $setupExecutablePath
        Remove-Item -LiteralPath $setupExecutablePath -Force
    }

    Invoke-CargoReleaseBuild -RepositoryRoot $repositoryRoot
    Copy-SetupPayload -RepositoryRoot $repositoryRoot -StagingDirectory $stagingDirectory
    New-IExpressSedFile -SedPath $sedPath -StagingDirectory $stagingDirectory -SetupExecutablePath $setupExecutablePath
    Invoke-IExpressBuild -IExpressPath $iexpressPath -SedPath $sedPath -SetupExecutablePath $setupExecutablePath
    Set-SetupExecutableIcon -SetupExecutablePath $setupExecutablePath -IconPath $appIconPath

    Write-Host "Created setup installer: $setupExecutablePath"
}

Build-CoreLauncherSetup
