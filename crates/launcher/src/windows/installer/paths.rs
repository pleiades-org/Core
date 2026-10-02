use std::{
    fs,
    path::{Path, PathBuf},
};
use windows::{
    core::GUID,
    Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs, FOLDERID_RoamingAppData,
            SHGetKnownFolderPath, KF_FLAG_DEFAULT,
        },
    },
};

pub(super) const PRODUCT_MARKER: &str = "product=Pleiades.Core\n";
pub(super) const STARTUP_VALUE: &str = "PleiadesCoreV2";
pub(super) const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PleiadesCore";
pub(super) const STARTUP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const MAX_MARKER_BYTES: u64 = 4096;

#[derive(Clone, Debug)]
pub(super) struct InstallPaths {
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub uninstaller: PathBuf,
    pub marker: PathBuf,
    pub start_menu: PathBuf,
    pub desktop: PathBuf,
    pub settings: PathBuf,
    pub uninstall_key: String,
    pub startup_key: String,
    pub isolated: bool,
}

impl InstallPaths {
    pub fn new(test_root: Option<&Path>) -> Result<Self, String> {
        if let Some(root) = test_root {
            return Self::isolated(root);
        }
        Ok(Self::from_roots(
            known_folder(&FOLDERID_LocalAppData)?.join("Programs/Pleiades/Core"),
            known_folder(&FOLDERID_Programs)?.join("Pleiades/Core.lnk"),
            known_folder(&FOLDERID_Desktop)?.join("Core.lnk"),
            known_folder(&FOLDERID_RoamingAppData)?.join("Pleiades/Core/v2/appearance.ini"),
        ))
    }

    fn from_roots(
        directory: PathBuf,
        start_menu: PathBuf,
        desktop: PathBuf,
        settings: PathBuf,
    ) -> Self {
        // Shell links return native separators; use the same spelling for ownership checks.
        let directory: PathBuf = directory.components().collect();
        Self {
            executable: directory.join("core-v2.exe"),
            uninstaller: directory.join("uninstall.exe"),
            marker: directory.join("core-installation.txt"),
            directory,
            start_menu: start_menu.components().collect(),
            desktop: desktop.components().collect(),
            settings: settings.components().collect(),
            uninstall_key: UNINSTALL_KEY.into(),
            startup_key: STARTUP_KEY.into(),
            isolated: false,
        }
    }

    fn isolated(root: &Path) -> Result<Self, String> {
        if !root.is_absolute() {
            return Err("Installer test directories must be absolute paths.".into());
        }
        let resolved_root = fs::canonicalize(root)
            .map_err(|error| format!("Could not resolve the installer test directory: {error}"))?;
        let temporary_root =
            fs::canonicalize(std::env::temp_dir()).map_err(|error| error.to_string())?;
        let name = resolved_root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid installer test directory.")?;
        let token = name.strip_prefix("core-installer-test-").filter(|token| !token.is_empty()
            && token.len() <= 80 && token.chars().all(|character| character.is_ascii_alphanumeric() || character == '-'))
            .ok_or("Installer tests require a directory named core-installer-test-<id> inside Windows Temp.")?;
        if resolved_root.parent() != Some(temporary_root.as_path()) {
            return Err(
                "Installer test directories must be direct children of Windows Temp.".into(),
            );
        }
        let mut paths = Self::from_roots(
            root.join("install"),
            root.join("start-menu/Core.lnk"),
            root.join("desktop/Core.lnk"),
            root.join("settings/appearance.ini"),
        );
        paths.uninstall_key = format!(r"Software\Pleiades\Core\InstallerTests\{token}\Uninstall");
        paths.startup_key = format!(r"Software\Pleiades\Core\InstallerTests\{token}\Run");
        paths.isolated = true;
        Ok(paths)
    }

    pub fn installed(&self) -> Result<bool, String> {
        match self.read_marker()? {
            Some(marker) if marker.starts_with(PRODUCT_MARKER) => Ok(true),
            Some(_) => {
                Err("The installation folder contains an unrecognized installation record.".into())
            }
            None => Ok(false),
        }
    }

    pub fn desktop_enabled(&self) -> Result<bool, String> {
        if !self.installed()? {
            return Ok(false);
        }
        Ok(self
            .read_marker()?
            .is_some_and(|text| text.lines().any(|line| line == "desktop=true")))
    }

    pub fn version(&self) -> Result<Option<crate::windows::updates::Version>, String> {
        let Some(marker) = self.read_marker()? else {
            return Ok(None);
        };
        marker
            .lines()
            .find_map(|line| line.strip_prefix("version="))
            .map(|version| {
                version
                    .parse()
                    .map_err(|error| format!("Core's installed version is invalid: {error}"))
            })
            .transpose()
    }

    fn read_marker(&self) -> Result<Option<String>, String> {
        match fs::symlink_metadata(&self.marker) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Could not inspect Core's installation record: {error}"
                ))
            }
            Ok(_) => {}
        }
        super::files::ensure_plain_path(&self.marker)?;
        let bytes = super::files::read_limited(&self.marker, MAX_MARKER_BYTES)?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| format!("Core's installation record is invalid: {error}"))
    }
}

pub(super) fn known_folder(identifier: &GUID) -> Result<PathBuf, String> {
    let path = unsafe { SHGetKnownFolderPath(identifier, KF_FLAG_DEFAULT, None) }
        .map_err(|error| format!("Windows could not locate an installation folder: {error}"))?;
    let text = unsafe { path.to_string() };
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    text.map(PathBuf::from).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_test_roots_cannot_target_real_user_directories() {
        assert!(InstallPaths::new(Some(&std::env::temp_dir())).is_err());
        assert!(InstallPaths::new(Some(&std::env::current_dir().unwrap())).is_err());
    }

    #[test]
    fn installation_paths_use_native_separators_for_shell_shortcut_ownership() {
        let paths = InstallPaths::from_roots(
            PathBuf::from(r"C:\Users\Test\AppData\Local\Programs/Pleiades/Core"),
            PathBuf::from(r"C:\Users\Test\Start Menu/Pleiades/Core.lnk"),
            PathBuf::from(r"C:\Users\Test\Desktop/Core.lnk"),
            PathBuf::from(r"C:\Users\Test\AppData\Roaming/Pleiades/Core/v2/appearance.ini"),
        );
        assert_eq!(
            paths.executable.to_string_lossy(),
            r"C:\Users\Test\AppData\Local\Programs\Pleiades\Core\core-v2.exe"
        );
        for path in [
            paths.directory,
            paths.start_menu,
            paths.desktop,
            paths.settings,
        ] {
            assert!(!path.to_string_lossy().contains('/'));
        }
    }
}
