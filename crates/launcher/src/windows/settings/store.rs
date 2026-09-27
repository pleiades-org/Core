use super::SettingsDocument;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::mpsc,
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};

pub const SETTINGS_SAVED: u32 = WM_APP + 8;
const MAX_SETTINGS_BYTES: u64 = 4 * 1024 * 1024;

pub struct SettingsStore {
    pub saved: SettingsDocument,
    pub warning: Option<String>,
    path: Option<PathBuf>,
    pending: Option<mpsc::Receiver<Result<SettingsDocument, String>>>,
    isolated: bool,
}

impl SettingsStore {
    pub fn load(isolated: bool) -> Self {
        let mut store = Self {
            saved: SettingsDocument::default(),
            warning: None,
            path: None,
            pending: None,
            isolated,
        };
        match settings_path(isolated).and_then(|path| {
            let settings = path
                .as_deref()
                .map(read_settings)
                .transpose()?
                .unwrap_or_default();
            Ok((path, settings))
        }) {
            Ok((path, settings)) => {
                store.path = path;
                store.saved = settings;
            }
            Err(error) => {
                store.warning = Some(format!(
                    "{error} Existing settings were preserved; saving is disabled."
                ))
            }
        }
        store
    }

    /// The folder holding the settings file, for files that belong beside it.
    pub fn folder(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    pub fn is_saving(&self) -> bool {
        self.pending.is_some()
    }

    pub fn start_save(&mut self, window: HWND, settings: SettingsDocument) -> Result<(), String> {
        if let Some(warning) = &self.warning {
            return Err(warning.clone());
        }
        if self.is_saving() {
            return Err("Settings are already being saved.".into());
        }
        let path = self.path.clone();
        let startup_changed =
            !self.isolated && settings.preferences.startup != self.saved.preferences.startup;
        let address = window.0 as usize;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("core-settings-save".into())
            .spawn(move || {
                let outcome = (|| {
                    let mut startup = if startup_changed {
                        Some(super::startup::StartupChange::apply(
                            settings.preferences.startup,
                        )?)
                    } else {
                        None
                    };
                    if let Some(path) = path.as_deref() {
                        write_settings(path, &settings)?;
                    }
                    if let Some(startup) = startup.as_mut() {
                        startup.commit();
                    }
                    Ok(settings)
                })();
                if sender.send(outcome).is_ok() {
                    if let Err(error) = unsafe {
                        PostMessageW(
                            Some(HWND(address as *mut _)),
                            SETTINGS_SAVED,
                            WPARAM(0),
                            LPARAM(0),
                        )
                    } {
                        eprintln!("Could not deliver settings save result: {error}");
                    }
                }
            })
            .map_err(|error| format!("Could not start saving settings: {error}"))?;
        self.pending = Some(receiver);
        Ok(())
    }

    pub fn finish_save(&mut self) -> Option<Result<SettingsDocument, String>> {
        let outcome = match self.pending.as_ref()?.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The settings save worker stopped unexpectedly.".into())
            }
        };
        self.pending = None;
        if let Ok(settings) = &outcome {
            self.saved = settings.clone();
        }
        Some(outcome)
    }
}

fn settings_path(isolated: bool) -> Result<Option<PathBuf>, String> {
    let mut arguments = std::env::args_os();
    while let Some(argument) = arguments.next() {
        if argument == "--settings-file" {
            return arguments
                .next()
                .filter(|path| !path.is_empty())
                .map(|path| Some(PathBuf::from(path)))
                .ok_or_else(|| "--settings-file requires a file path.".into());
        }
    }
    if isolated {
        return Ok(None);
    }
    std::env::var_os("APPDATA")
        .map(|root| Some(PathBuf::from(root).join("Pleiades/Core/v2/appearance.ini")))
        .ok_or_else(|| "Windows did not provide an AppData folder.".into())
}

fn read_settings(path: &Path) -> Result<SettingsDocument, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SettingsDocument::default())
        }
        Err(error) => return Err(format!("Could not open settings: {error}")),
    };
    let mut text = String::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("Could not read settings: {error}"))?;
    if text.len() > MAX_SETTINGS_BYTES as usize {
        return Err("Settings exceed the supported size.".into());
    }
    // Notepad and other editors may save UTF-8 with a byte-order mark.
    SettingsDocument::decode(text.strip_prefix('\u{feff}').unwrap_or(&text))
}

fn write_settings(path: &Path, settings: &SettingsDocument) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create settings folder: {error}"))?;
    let temporary = parent.join(format!(".core-appearance-{}.tmp", std::process::id()));
    // Truncate rather than create_new: a crash can leave this file behind, and a later process
    // may reuse the PID. Only one writer per process exists, so overwriting is safe.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| format!("Could not create temporary settings file: {error}"))?;
    let written = file
        .write_all(settings.encode().as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    let result = written
        .map_err(|error| format!("Could not write settings: {error}"))
        .and_then(|_| {
            let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
            let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                MoveFileExW(
                    PCWSTR(source.as_ptr()),
                    PCWSTR(destination.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(|error| format!("Could not replace settings: {error}"))
        });
    if let Err(error) = result {
        return match fs::remove_file(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!(
                "{error}; temporary-file cleanup also failed: {cleanup}"
            )),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A per-test folder under the system temp directory, removed on drop.
    struct TestFolder(PathBuf);
    impl TestFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "core-store-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample() -> SettingsDocument {
        SettingsDocument::decode("version=1\nbackground=#123456\nposition=Right\nstartup=true")
            .unwrap()
    }

    #[test]
    fn written_settings_read_back_and_missing_files_use_defaults() {
        let folder = TestFolder::new("round-trip");
        let path = folder.0.join("nested/appearance.ini");
        assert_eq!(read_settings(&path), Ok(SettingsDocument::default()));
        write_settings(&path, &sample()).unwrap();
        assert_eq!(read_settings(&path), Ok(sample()));
    }

    #[test]
    fn a_byte_order_mark_from_an_editor_is_accepted() {
        let folder = TestFolder::new("bom");
        let path = folder.0.join("appearance.ini");
        fs::write(&path, format!("\u{feff}{}", sample().encode())).unwrap();
        assert_eq!(read_settings(&path), Ok(sample()));
    }

    #[test]
    fn a_temporary_file_left_by_a_crash_does_not_block_saving() {
        let folder = TestFolder::new("stale-temp");
        let path = folder.0.join("appearance.ini");
        let stale = folder
            .0
            .join(format!(".core-appearance-{}.tmp", std::process::id()));
        fs::write(&stale, "partial").unwrap();
        write_settings(&path, &sample()).unwrap();
        assert_eq!(read_settings(&path), Ok(sample()));
        assert!(
            !stale.exists(),
            "the temporary file becomes the settings file"
        );
    }

    #[test]
    fn oversized_settings_are_rejected_without_decoding() {
        let folder = TestFolder::new("oversized");
        let path = folder.0.join("appearance.ini");
        fs::write(&path, vec![b'#'; MAX_SETTINGS_BYTES as usize + 1]).unwrap();
        assert!(read_settings(&path).is_err());
    }
}
