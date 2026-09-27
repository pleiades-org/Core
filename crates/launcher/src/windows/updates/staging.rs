use super::{crypto, manifest::Manifest, UpdateError, MAX_EXECUTABLE_BYTES};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Installation {
    pub executable: PathBuf,
    pub staged: PathBuf,
    pub manifest: PathBuf,
    pub previous: PathBuf,
    pub pending: PathBuf,
    pub helper: PathBuf,
}

impl Installation {
    pub fn writable(&self) -> Result<bool, UpdateError> {
        if fs::metadata(&self.executable)
            .map_err(UpdateError::io)?
            .permissions()
            .readonly()
        {
            return Ok(false);
        }
        let probe = self
            .executable
            .with_extension(format!("{}.write-test", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(file) => {
                drop(file);
                remove_if_present(&probe)?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
            Err(error) => Err(UpdateError::io(error)),
        }
    }
    pub fn new(executable: PathBuf) -> Self {
        Self {
            staged: executable.with_extension("update.exe"),
            manifest: executable.with_extension("update.txt"),
            previous: executable.with_extension("previous.exe"),
            pending: executable.with_extension("pending-update.txt"),
            helper: executable.with_extension("updater.exe"),
            executable,
        }
    }

    pub fn stage(&self, manifest: &Manifest, bytes: &[u8]) -> Result<(), UpdateError> {
        check_hash(bytes, manifest)?;
        atomic_write(&self.staged, bytes)?;
        atomic_write(&self.manifest, manifest.text.as_bytes())
    }

    pub fn verified_stage(&self) -> Result<Manifest, UpdateError> {
        let manifest = Manifest::verify(&read_bounded(&self.manifest, super::MAX_MANIFEST_BYTES)?)?;
        check_hash(
            &read_bounded(&self.staged, MAX_EXECUTABLE_BYTES)?,
            &manifest,
        )?;
        Ok(manifest)
    }

    /// No network work. If the second rename fails, put the old executable back immediately.
    pub fn apply(&self) -> Result<(), UpdateError> {
        let manifest = self.verified_stage()?;
        atomic_write(&self.pending, manifest.text.as_bytes())?;
        if let Err(error) = self.swap() {
            remove_if_present(&self.pending)?;
            return Err(error);
        }
        // Retain the signed pending record until a new process confirms a visible window.
        Ok(())
    }

    fn swap(&self) -> Result<(), UpdateError> {
        remove_if_present(&self.previous)?;
        fs::rename(&self.executable, &self.previous).map_err(UpdateError::io)?;
        if let Err(error) = fs::rename(&self.staged, &self.executable) {
            fs::rename(&self.previous, &self.executable).map_err(|restore| {
                UpdateError::Io(format!(
                    "Install failed: {error}; restoring the original also failed: {restore}"
                ))
            })?;
            return Err(UpdateError::io(error));
        }
        Ok(())
    }

    pub fn rollback(&self) -> Result<(), UpdateError> {
        if !self.previous.is_file() {
            return Err(UpdateError::Io("Previous executable is missing".into()));
        }
        let failed = self.executable.with_extension("failed.exe");
        remove_if_present(&failed)?;
        match fs::rename(&self.executable, &failed) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(UpdateError::io(error)),
        }
        if let Err(error) = fs::rename(&self.previous, &self.executable) {
            fs::rename(&failed, &self.executable).map_err(UpdateError::io)?;
            return Err(UpdateError::io(error));
        }
        remove_if_present(&self.pending)?;
        remove_if_present(&self.manifest)?;
        Ok(())
    }

    pub fn confirm(&self) -> Result<(), UpdateError> {
        remove_if_present(&self.pending)?;
        remove_if_present(&self.manifest)
    }
}

pub fn check_hash(bytes: &[u8], manifest: &Manifest) -> Result<(), UpdateError> {
    if bytes.is_empty()
        || bytes.len() > MAX_EXECUTABLE_BYTES
        || crypto::sha256(bytes)? != manifest.digest
    {
        return Err(UpdateError::HashMismatch);
    }
    Ok(())
}

pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(UpdateError::io)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(UpdateError::io)?;
    if bytes.len() > limit {
        return Err(UpdateError::HashMismatch);
    }
    Ok(bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(UpdateError::io)?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(UpdateError::io);
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            eprintln!("Could not remove update temporary file: {error}");
        }
    }
    result
}

pub fn remove_if_present(path: &Path) -> Result<(), UpdateError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(UpdateError::io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        installation: Installation,
        folder: PathBuf,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let folder =
                std::env::temp_dir().join(format!("core-update-{name}-{}", std::process::id()));
            fs::create_dir_all(&folder).unwrap();
            let installation = Installation::new(folder.join("core-v2.exe"));
            fs::write(&installation.executable, b"original").unwrap();
            Self {
                installation,
                folder,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.folder).unwrap();
        }
    }

    #[test]
    fn installation_retains_backup_and_rolls_back_failed_startup() {
        let fixture = Fixture::new("rollback");
        let installation = &fixture.installation;
        fs::write(&installation.staged, b"replacement").unwrap();
        installation.swap().unwrap();
        assert_eq!(fs::read(&installation.executable).unwrap(), b"replacement");
        assert_eq!(fs::read(&installation.previous).unwrap(), b"original");
        installation.rollback().unwrap();
        assert_eq!(fs::read(&installation.executable).unwrap(), b"original");
    }

    #[test]
    fn a_missing_stage_at_swap_time_restores_the_original_path() {
        let fixture = Fixture::new("missing-stage");
        assert!(fixture.installation.swap().is_err());
        assert_eq!(
            fs::read(&fixture.installation.executable).unwrap(),
            b"original"
        );
    }

    #[test]
    fn rollback_restores_the_backup_even_if_the_replacement_disappeared() {
        let fixture = Fixture::new("missing-replacement");
        fs::rename(
            &fixture.installation.executable,
            &fixture.installation.previous,
        )
        .unwrap();
        fixture.installation.rollback().unwrap();
        assert_eq!(
            fs::read(&fixture.installation.executable).unwrap(),
            b"original"
        );
    }

    #[test]
    fn unverified_stages_cannot_replace_the_executable() {
        let fixture = Fixture::new("unsigned");
        fs::write(&fixture.installation.staged, b"replacement").unwrap();
        fs::write(&fixture.installation.manifest, b"version=9.8.7\n").unwrap();
        assert!(fixture.installation.apply().is_err());
        assert_eq!(
            fs::read(&fixture.installation.executable).unwrap(),
            b"original"
        );
        assert!(!fixture.installation.previous.exists());
    }

    #[test]
    fn a_read_only_executable_falls_back_to_notification() {
        let fixture = Fixture::new("readonly");
        let path = &fixture.installation.executable;
        let original = fs::metadata(path).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(path, readonly).unwrap();
        let writable = fixture.installation.writable();
        fs::set_permissions(path, original).unwrap();
        assert!(!writable.unwrap());
    }
}
