use super::paths::known_folder;
use std::{
    ffi::OsString,
    fs,
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::{System::Threading::CREATE_NO_WINDOW, UI::Shell::FOLDERID_System};

const DIRECTORY_PREFIX: &str = "Pleiades-Core-Uninstall-";
const CLEANUP_SCRIPT: &str = include_str!("cleanup.ps1");

pub(super) fn launch_uninstaller(executable: &Path, arguments: &[OsString]) -> Result<(), String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "{DIRECTORY_PREFIX}{}-{timestamp}",
        std::process::id()
    ));
    fs::create_dir(&directory)
        .map_err(|error| format!("Could not prepare the uninstaller: {error}"))?;
    let temporary = directory.join("uninstall.exe");
    let outcome = fs::copy(executable, &temporary)
        .map_err(|error| format!("Could not prepare Core's uninstaller: {error}"))
        .and_then(|_| {
            Command::new(&temporary)
                .args(arguments)
                .creation_flags(CREATE_NO_WINDOW.0)
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("Could not start Core's uninstaller: {error}"))
        });
    if outcome.is_err() {
        if temporary.exists() {
            if let Err(error) = fs::remove_file(&temporary) {
                eprintln!("Could not remove the failed temporary uninstaller: {error}");
            }
        }
        if let Err(error) = fs::remove_dir(&directory) {
            eprintln!("Could not clean the failed uninstaller directory: {error}");
        }
    }
    outcome
}

pub(super) fn cleanup(executable: &Path) {
    let Some(directory) = executable.parent() else {
        return;
    };
    if directory.parent() != Some(std::env::temp_dir().as_path())
        || !directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(DIRECTORY_PREFIX))
        || executable
            .file_name()
            .is_none_or(|name| name != "uninstall.exe")
    {
        return;
    }
    if let Err(error) = schedule_cleanup(directory) {
        eprintln!("Core was removed, but Windows could not clean its temporary uninstaller at {}: {error}", directory.display());
    }
}

fn schedule_cleanup(directory: &Path) -> Result<(), String> {
    // Windows keeps a running image mapped. A separate process waits for exit
    // before removing only our temporary executable and its empty directory.
    let powershell = known_folder(&FOLDERID_System)?.join("WindowsPowerShell/v1.0/powershell.exe");
    Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            CLEANUP_SCRIPT,
        ])
        .env("CORE_SETUP_CLEANUP_DIRECTORY", directory)
        .env("CORE_SETUP_CLEANUP_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|error| format!("Could not start temporary-file cleanup: {error}"))?;
    Ok(())
}
