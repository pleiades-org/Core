use super::{
    files::{read_limited, FileTransaction, MAX_EXECUTABLE_BYTES},
    paths::{InstallPaths, PRODUCT_MARKER, STARTUP_VALUE},
    registry::{self, RegistryKey, RegistryTransaction, RegistryValue},
    shortcuts,
};
use std::{fs, path::Path};
use windows::{core::w, Win32::UI::WindowsAndMessaging::FindWindowW};

pub(super) fn install(paths: &InstallPaths, source: &Path, desktop: bool) -> Result<(), String> {
    ensure_idle(paths)?;
    let previous_desktop = validate_installation(paths, desktop)?;
    let maximum_setup_bytes = MAX_EXECUTABLE_BYTES + super::payload::SETUP_MARKER.len() as u64;
    let bytes = super::payload::application_bytes(read_limited(source, maximum_setup_bytes)?);
    if bytes.len() as u64 > MAX_EXECUTABLE_BYTES {
        return Err("Core's application exceeds the supported executable size.".into());
    }
    let mut files = FileTransaction::new();
    write_installation_files(paths, &bytes, desktop, previous_desktop, &mut files)?;
    let mut registration = RegistryTransaction::new();
    register_uninstaller(paths, bytes.len(), &mut registration)?;
    migrate_startup(paths, &mut registration)?;
    registration.commit();
    files.commit();
    Ok(())
}

fn validate_installation(paths: &InstallPaths, desktop: bool) -> Result<bool, String> {
    let installed = paths.installed()?;
    let previous_desktop = paths.desktop_enabled()?;
    let current = env!("CARGO_PKG_VERSION")
        .parse::<crate::windows::updates::Version>()
        .map_err(|error| error.to_string())?;
    if paths
        .version()?
        .is_some_and(|installed| installed > current)
    {
        return Err("A newer version of Core is installed. Use its installer to repair it.".into());
    }
    if !installed && (paths.executable.exists() || paths.uninstaller.exists()) {
        return Err(
            "Core's installation folder contains files this installer does not own.".into(),
        );
    }
    let mut shortcuts_to_change = vec![&paths.start_menu];
    if desktop || previous_desktop {
        shortcuts_to_change.push(&paths.desktop);
    }
    for shortcut in shortcuts_to_change {
        if shortcut.exists() && !shortcuts::belongs_to(shortcut, &paths.executable)? {
            return Err(format!("The shortcut {} belongs to another application; move or rename it before installing.", shortcut.display()));
        }
    }
    Ok(previous_desktop)
}

fn write_installation_files(
    paths: &InstallPaths,
    bytes: &[u8],
    desktop: bool,
    previous_desktop: bool,
    files: &mut FileTransaction,
) -> Result<(), String> {
    files.replace(&paths.executable, bytes)?;
    files.replace(&paths.uninstaller, bytes)?;
    let shortcut = shortcuts::create_bytes(&paths.executable, &paths.directory)
        .map_err(|error| format!("Could not create Core's shortcut: {error}"))?;
    files.replace(&paths.start_menu, &shortcut)?;
    if desktop {
        files.replace(&paths.desktop, &shortcut)?;
    } else if previous_desktop {
        files.remove(&paths.desktop)?;
    }
    let marker = format!(
        "{PRODUCT_MARKER}version={}\ndesktop={desktop}\n",
        env!("CARGO_PKG_VERSION")
    );
    files.replace(&paths.marker, marker.as_bytes())?;
    Ok(())
}

fn migrate_startup(
    paths: &InstallPaths,
    registration: &mut RegistryTransaction,
) -> Result<(), String> {
    let startup = RegistryKey::create(&paths.startup_key)?;
    if startup.read(STARTUP_VALUE)?.is_some() {
        registration.write(
            &paths.startup_key,
            STARTUP_VALUE,
            Some(&RegistryValue::text(&startup_command(paths))),
        )?;
    }
    Ok(())
}

pub(super) fn uninstall(paths: &InstallPaths) -> Result<(), String> {
    ensure_idle(paths)?;
    if !paths.installed()? {
        return Err("Core's installed files were not found. No files were removed.".into());
    }
    let mut files = FileTransaction::new();
    remove_owned_files(paths, &mut files)?;
    let mut registration = RegistryTransaction::new();
    let startup = RegistryKey::create(&paths.startup_key)?;
    if startup
        .read(STARTUP_VALUE)?
        .and_then(|value| value.as_text())
        .is_some_and(|command| command.eq_ignore_ascii_case(&startup_command(paths)))
    {
        registration.write(&paths.startup_key, STARTUP_VALUE, None)?;
    }
    files.remove(&paths.marker)?;
    registry::remove_key(&paths.uninstall_key)?;
    registration.commit();
    files.commit();
    // Keep user-added files and all preferences; remove only directories that are empty.
    remove_empty_directory(&paths.directory);
    if let Some(directory) = paths.start_menu.parent() {
        remove_empty_directory(directory);
    }
    Ok(())
}

fn remove_owned_files(paths: &InstallPaths, files: &mut FileTransaction) -> Result<(), String> {
    for shortcut in [&paths.start_menu, &paths.desktop] {
        match shortcuts::belongs_to(shortcut, &paths.executable) {
            Ok(true) => files.remove(shortcut)?,
            Ok(false) => {}
            Err(error) => eprintln!(
                "Keeping an unverifiable shortcut at {}: {error}",
                shortcut.display()
            ),
        }
    }
    for name in [
        "core-v2.exe",
        "uninstall.exe",
        "core-v2.previous.exe",
        "core-v2.updater.exe",
        "core-v2.failed.exe",
        "core-v2.update.exe",
        "core-v2.update.txt",
        "core-v2.pending-update.txt",
    ] {
        files.remove(&paths.directory.join(name))?;
    }
    Ok(())
}

fn register_uninstaller(
    paths: &InstallPaths,
    executable_bytes: usize,
    registration: &mut RegistryTransaction,
) -> Result<(), String> {
    let values = [
        ("DisplayName", RegistryValue::text("Core")),
        (
            "DisplayVersion",
            RegistryValue::text(env!("CARGO_PKG_VERSION")),
        ),
        ("Publisher", RegistryValue::text("Pleiades")),
        (
            "InstallLocation",
            RegistryValue::text(&paths.directory.to_string_lossy()),
        ),
        (
            "DisplayIcon",
            RegistryValue::text(&format!("\"{}\",0", paths.executable.display())),
        ),
        (
            "UninstallString",
            RegistryValue::text(&format!("\"{}\" --uninstall", paths.uninstaller.display())),
        ),
        (
            "URLInfoAbout",
            RegistryValue::text("https://github.com/pleiades-org/Core"),
        ),
        ("NoModify", RegistryValue::number(1)),
        ("NoRepair", RegistryValue::number(1)),
        (
            "EstimatedSize",
            RegistryValue::number((executable_bytes * 2).div_ceil(1024) as u32),
        ),
    ];
    for (name, value) in values {
        registration.write(&paths.uninstall_key, name, Some(&value))?;
    }
    Ok(())
}

fn startup_command(paths: &InstallPaths) -> String {
    format!("\"{}\" --start-hidden", paths.executable.display())
}

fn ensure_idle(paths: &InstallPaths) -> Result<(), String> {
    if !paths.isolated && unsafe { FindWindowW(w!("Pleiades.Core.V2"), None) }.is_ok() {
        return Err("Core is running. Exit Core from its tray menu, then choose Retry.".into());
    }
    Ok(())
}

fn remove_empty_directory(path: &Path) {
    match fs::remove_dir(path) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            // A nonempty directory contains user files, so leave it in place.
        }
        Err(error) => eprintln!(
            "Core was removed, but its empty directory at {} could not be cleaned: {error}",
            path.display()
        ),
    }
}
