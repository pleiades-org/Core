use std::io;

#[cfg(target_os = "windows")]
mod platform {
    use super::io;
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };

    const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;
    const STARTUP_SHORTCUT_NAME: &str = "Core Launcher.lnk";
    const INSTALLED_ICON_NAME: &str = "Core Launcher.ico";

    pub fn set_launch_at_startup(is_enabled: bool) -> io::Result<()> {
        let startup_shortcut_path = startup_shortcut_path()?;

        if is_enabled {
            create_startup_shortcut(&startup_shortcut_path)
        } else {
            remove_startup_shortcut(&startup_shortcut_path)
        }
    }

    fn startup_shortcut_path() -> io::Result<PathBuf> {
        let roaming_app_data_path = env::var_os("APPDATA").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "APPDATA is not available for Startup shortcut management.",
            )
        })?;

        Ok(PathBuf::from(roaming_app_data_path)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join(STARTUP_SHORTCUT_NAME))
    }

    fn create_startup_shortcut(shortcut_path: &Path) -> io::Result<()> {
        let executable_path = env::current_exe()?;
        let working_directory = executable_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let icon_path = launcher_icon_path(&working_directory).unwrap_or(executable_path.clone());

        if let Some(shortcut_directory) = shortcut_path.parent() {
            fs::create_dir_all(shortcut_directory)?;
        }

        let script = format!(
            "$shell = New-Object -ComObject WScript.Shell; \
             $shortcut = $shell.CreateShortcut({shortcut_path}); \
             $shortcut.TargetPath = {target_path}; \
             $shortcut.WorkingDirectory = {working_directory}; \
             $shortcut.Description = 'Start Core Launcher when Windows signs in'; \
             $shortcut.IconLocation = {icon_path}; \
             $shortcut.Save()",
            shortcut_path = powershell_single_quoted(&shortcut_path.display().to_string()),
            target_path = powershell_single_quoted(&executable_path.display().to_string()),
            working_directory = powershell_single_quoted(&working_directory.display().to_string()),
            icon_path = powershell_single_quoted(&icon_path.display().to_string()),
        );

        let status = powershell_command(&script).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                "PowerShell could not create the Startup shortcut.",
            ))
        }
    }

    fn remove_startup_shortcut(shortcut_path: &Path) -> io::Result<()> {
        if shortcut_path.exists() {
            fs::remove_file(shortcut_path)?;
        }

        Ok(())
    }

    fn launcher_icon_path(working_directory: &Path) -> Option<PathBuf> {
        let icon_path = working_directory.join(INSTALLED_ICON_NAME);
        icon_path.is_file().then_some(icon_path)
    }

    fn powershell_command(script: &str) -> Command {
        use std::os::windows::process::CommandExt;

        let mut command = Command::new("powershell");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
            ])
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(WINDOWS_CREATE_NO_WINDOW);
        command
    }

    fn powershell_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::io;

    pub fn set_launch_at_startup(_is_enabled: bool) -> io::Result<()> {
        Ok(())
    }
}

pub use platform::set_launch_at_startup;
