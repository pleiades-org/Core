use std::{
    fs,
    io::{Read, Write},
    os::windows::ffi::OsStrExt,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};
use windows::{
    core::PCWSTR,
    Win32::Storage::FileSystem::{
        MoveFileExW, FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING,
        MOVEFILE_WRITE_THROUGH,
    },
};

pub(super) const MAX_EXECUTABLE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct FileTransaction {
    originals: Vec<(PathBuf, Option<Vec<u8>>)>,
    committed: bool,
}

impl FileTransaction {
    pub fn new() -> Self {
        Self {
            originals: Vec::new(),
            committed: false,
        }
    }

    pub fn replace(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.remember(path)?;
        atomic_write(path, bytes)
    }

    pub fn remove(&mut self, path: &Path) -> Result<(), String> {
        self.remember(path)?;
        remove_file(path)
    }

    fn remember(&mut self, path: &Path) -> Result<(), String> {
        ensure_plain_path(path)?;
        let original = match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Some(read_limited(path, MAX_EXECUTABLE_BYTES)?),
            Ok(_) => return Err(format!("Cannot safely replace {}.", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
        };
        self.originals.push((path.to_owned(), original));
        Ok(())
    }

    pub fn commit(mut self) {
        self.committed = true;
    }
}

pub(super) fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if bytes.len() as u64 > maximum {
        return Err(format!("{} is too large to change safely.", path.display()));
    }
    Ok(bytes)
}

impl Drop for FileTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, original) in self.originals.iter().rev() {
            let result = match original {
                Some(bytes) => atomic_write(path, bytes),
                None => remove_file(path),
            };
            if let Err(error) = result {
                eprintln!(
                    "Could not restore {} after setup failed: {error}",
                    path.display()
                );
            }
        }
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    ensure_plain_path(path)?;
    let parent = path
        .parent()
        .ok_or("The installation file has no parent directory.")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!("installing-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Could not prepare {}: {error}", path.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
        drop(file);
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| format!("Could not replace {}: {error}", path.display()))
    })();
    if temporary.exists() {
        if let Err(error) = remove_file(&temporary) {
            eprintln!("Could not remove the setup temporary file: {error}");
        }
    }
    result
}

pub(super) fn remove_file(path: &Path) -> Result<(), String> {
    ensure_plain_path(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove {}: {error}", path.display())),
    }
}

/// Never follow a junction or symbolic link while replacing or removing installer files.
pub(super) fn ensure_plain_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 => {
                return Err(format!(
                    "Setup cannot change a redirected folder or file: {}",
                    ancestor.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect {}: {error}", ancestor.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "core-installer-files-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            for entry in fs::read_dir(&self.0).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
            fs::remove_dir(&self.0).unwrap();
        }
    }

    #[test]
    fn failed_transaction_restores_replaced_and_removed_files_and_removes_new_files() {
        let directory = TestDirectory::new();
        let replaced = directory.0.join("replaced.exe");
        let removed = directory.0.join("removed.exe");
        let created = directory.0.join("created.exe");
        fs::write(&replaced, b"original executable").unwrap();
        fs::write(&removed, b"original uninstaller").unwrap();
        {
            let mut transaction = FileTransaction::new();
            transaction.replace(&replaced, b"replacement").unwrap();
            transaction.remove(&removed).unwrap();
            transaction.replace(&created, b"new file").unwrap();
        }
        assert_eq!(fs::read(replaced).unwrap(), b"original executable");
        assert_eq!(fs::read(removed).unwrap(), b"original uninstaller");
        assert!(!created.exists());
    }

    #[test]
    fn committed_transaction_keeps_the_replacement() {
        let directory = TestDirectory::new();
        let executable = directory.0.join("core.exe");
        fs::write(&executable, b"original").unwrap();
        let mut transaction = FileTransaction::new();
        transaction.replace(&executable, b"replacement").unwrap();
        transaction.commit();
        assert_eq!(fs::read(executable).unwrap(), b"replacement");
    }

    #[test]
    fn bounded_reads_accept_the_limit_and_reject_an_extra_byte() {
        let directory = TestDirectory::new();
        let file = directory.0.join("record.txt");
        fs::write(&file, b"four").unwrap();
        assert_eq!(read_limited(&file, 4).unwrap(), b"four");
        assert!(read_limited(&file, 3).is_err());
    }

    #[test]
    fn temporary_collision_preserves_the_other_file() {
        let directory = TestDirectory::new();
        let executable = directory.0.join("core.exe");
        let temporary = executable.with_extension(format!("installing-{}", std::process::id()));
        fs::write(&executable, b"original").unwrap();
        fs::write(&temporary, b"unrelated temporary file").unwrap();
        assert!(atomic_write(&executable, b"replacement").is_err());
        assert_eq!(fs::read(executable).unwrap(), b"original");
        assert_eq!(fs::read(temporary).unwrap(), b"unrelated temporary file");
    }
}
