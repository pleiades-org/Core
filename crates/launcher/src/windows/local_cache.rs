//! Downloaded caches live in Local AppData: with folder redirection, Roaming AppData can be a
//! network share, so every cache read would cross the network. Settings stay in Roaming AppData.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const CORE_FOLDER: &str = "Pleiades/Core/v2";

/// `%LOCALAPPDATA%\Pleiades\Core\v2\{name}`. A file or folder of that name left in Roaming
/// AppData by earlier versions is moved across first; anything that cannot move stays behind.
pub fn path(name: &str) -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join(CORE_FOLDER)
        .join(name);
    if let Some(roaming) = std::env::var_os("APPDATA") {
        let roaming = PathBuf::from(roaming).join(CORE_FOLDER).join(name);
        if roaming != local {
            move_to_local(&roaming, &local);
        }
    }
    Some(local)
}

/// Moves a file, or every file in a folder, keeping any copy already at the destination.
fn move_to_local(from: &Path, to: &Path) {
    // Absent after the first run: nothing to move.
    let Ok(metadata) = fs::symlink_metadata(from) else {
        return;
    };
    let moved = if metadata.is_dir() {
        move_folder(from, to)
    } else {
        move_file(from, to)
    };
    if let Err(error) = moved {
        eprintln!(
            "Could not move the cache {} to {}: {error}",
            from.display(),
            to.display()
        );
    }
}

fn move_folder(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let mut first_error = None;
    for entry in fs::read_dir(from)? {
        let moved = entry.and_then(|entry| move_file(&entry.path(), &to.join(entry.file_name())));
        if let Err(error) = moved {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => fs::remove_dir(from),
    }
}

fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    // Written by a newer run, so the Roaming copy is older.
    if fs::symlink_metadata(to).is_ok() {
        return fs::remove_file(from);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    // Renaming fails across volumes, as with a redirected Roaming folder; copy instead.
    fs::rename(from, to).or_else(|_| {
        if let Err(error) = fs::copy(from, to) {
            // A partial copy would look like a valid cache file; the original stays instead.
            let _ = fs::remove_file(to);
            return Err(error);
        }
        fs::remove_file(from)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roaming_caches_move_once_and_never_replace_newer_local_files() {
        let folder = std::env::temp_dir().join(format!("core-local-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let roaming = folder.join("Roaming");
        let local = folder.join("Local");
        fs::create_dir_all(roaming.join("favicons")).unwrap();
        fs::create_dir_all(local.join("favicons")).unwrap();
        fs::write(roaming.join("exchange-rates.xml"), "old rates").unwrap();
        fs::write(roaming.join("appearance.ini"), "settings").unwrap();
        fs::write(roaming.join("favicons/a.icon"), "a").unwrap();
        fs::write(roaming.join("favicons/b.icon"), "old b").unwrap();
        fs::write(local.join("favicons/b.icon"), "new b").unwrap();

        for _ in 0..2 {
            move_to_local(
                &roaming.join("exchange-rates.xml"),
                &local.join("exchange-rates.xml"),
            );
            move_to_local(&roaming.join("favicons"), &local.join("favicons"));
        }
        move_to_local(&roaming.join("absent"), &local.join("absent"));

        let read = |path: PathBuf| fs::read_to_string(path).unwrap();
        assert_eq!(read(local.join("exchange-rates.xml")), "old rates");
        assert_eq!(read(local.join("favicons/a.icon")), "a");
        assert_eq!(read(local.join("favicons/b.icon")), "new b");
        assert!(!roaming.join("exchange-rates.xml").exists());
        assert!(!roaming.join("favicons").exists());
        assert!(!local.join("absent").exists());
        // Settings are not caches and stay in Roaming AppData.
        assert_eq!(read(roaming.join("appearance.ini")), "settings");
        assert!(!local.join("appearance.ini").exists());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_missing_destination_folder_is_created() {
        let folder =
            std::env::temp_dir().join(format!("core-local-cache-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let from = folder.join("Roaming/exchange-rates.xml");
        let to = folder.join("Local/Pleiades/Core/v2/exchange-rates.xml");
        fs::create_dir_all(from.parent().unwrap()).unwrap();
        fs::write(&from, "rates").unwrap();
        move_to_local(&from, &to);
        assert_eq!(fs::read_to_string(&to).unwrap(), "rates");
        assert!(!from.exists());
        fs::remove_dir_all(&folder).unwrap();
    }
}
