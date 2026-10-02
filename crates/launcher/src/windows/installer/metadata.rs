use super::{
    files::{atomic_write, read_limited},
    paths::InstallPaths,
    registry::{RegistryKey, RegistryValue},
};

/// Keeps Windows' installed-app version aligned with Core after a signed update.
pub(super) fn refresh(paths: &InstallPaths) -> Result<(), String> {
    if !paths.installed()? {
        return Ok(());
    }
    let key = RegistryKey::create(&paths.uninstall_key)?;
    let current = env!("CARGO_PKG_VERSION");
    if key
        .read("DisplayVersion")?
        .and_then(|value| value.as_text())
        .as_deref()
        == Some(current)
    {
        return Ok(());
    }
    let marker = String::from_utf8(read_limited(&paths.marker, 4096)?)
        .map_err(|error| format!("Invalid Core installation record: {error}"))?;
    let updated = marker
        .lines()
        .map(|line| {
            if line.starts_with("version=") {
                format!("version={current}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    atomic_write(&paths.marker, updated.as_bytes())?;
    key.write("DisplayVersion", Some(&RegistryValue::text(current)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Writes an isolated temporary HKCU installer key; run explicitly"]
    fn refreshed_version_preserves_installation_options_after_an_update() {
        let root = std::env::temp_dir().join(format!(
            "core-installer-test-metadata-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let paths = InstallPaths::new(Some(&root)).unwrap();
        std::fs::create_dir(&paths.directory).unwrap();
        std::fs::write(
            &paths.marker,
            "product=Pleiades.Core\nversion=0.0.1\ndesktop=true\n",
        )
        .unwrap();
        let key = RegistryKey::create(&paths.uninstall_key).unwrap();
        key.write("DisplayVersion", Some(&RegistryValue::text("0.0.1")))
            .unwrap();

        refresh(&paths).unwrap();

        let registered_version = key.read("DisplayVersion").unwrap().unwrap().as_text();
        let marker = std::fs::read_to_string(&paths.marker).unwrap();
        drop(key);
        super::super::registry::remove_key(&paths.uninstall_key).unwrap();
        super::super::registry::remove_key(&paths.startup_key.replace("\\Run", "")).unwrap();
        std::fs::remove_file(&paths.marker).unwrap();
        std::fs::remove_dir(&paths.directory).unwrap();
        std::fs::remove_dir(root).unwrap();
        assert_eq!(
            registered_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert!(marker.contains(&format!("version={}\n", env!("CARGO_PKG_VERSION"))));
        assert!(marker.contains("desktop=true\n"));
    }
}
