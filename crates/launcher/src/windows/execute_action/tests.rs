use super::*;
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn only_absolute_executables_get_an_installation_directory() {
    for path in [
        r"D:\Games\My Game\game.exe",
        r"D:\Games\My Game\game.EXE",
        "D:/Games/My Game/game.exe",
        r"\\server\share\game.exe",
    ] {
        let path = Path::new(path);
        assert_eq!(executable_directory(path), path.parent());
    }
    for path in [
        r"C:\Start Menu\Game.lnk",
        r"C:\Start Menu\Game.url",
        "https://example.com/game.exe",
        "steam://rungameid/3669870",
        "spotify:track:abc",
        "com.epicgames.launcher://apps/fn?action=launch",
        "shell:AppsFolder\\Example!App",
        "game.exe",
        r"\Games\game.exe",
        "C:game.exe",
        r"C:\Documents\file.txt",
        r"C:\Games",
    ] {
        assert_eq!(executable_directory(Path::new(path)), None, "{path}");
    }
}

#[test]
fn unregistered_app_link_schemes_name_the_missing_app() {
    assert_eq!(
        require_registered_scheme("core-no-such-scheme-12345"),
        Err("No app is registered for core-no-such-scheme-12345: links".into())
    );
    for scheme in BUILT_IN_SCHEMES {
        assert_eq!(require_registered_scheme(scheme), Ok(()), "{scheme}");
    }
}

/// Exercise ShellExecute itself: dry-run UI tests do not start the selected target.
#[test]
#[ignore = "compiles and launches a harmless native probe; run explicitly with --ignored"]
fn executable_launches_find_relative_data_for_applications_and_quicklinks() {
    let folder = ProbeFolder::new();
    let executable = folder.0.join("Launch probe.EXE");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/launch_probe.rs");
    let compilation = Command::new("rustc")
        .arg("--edition=2021")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("Could not run rustc to build launch probe");
    assert!(
        compilation.status.success(),
        "{}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    fs::create_dir(folder.0.join("data")).unwrap();
    fs::write(
        folder.0.join("data/launch-marker.txt"),
        "relative data found",
    )
    .unwrap();
    assert_ne!(std::env::current_dir().unwrap(), folder.0);

    for action in [
        NativeAction::OpenApplication(executable.clone()),
        NativeAction::OpenQuicklink(executable.to_str().unwrap().into()),
    ] {
        action
            .execute(HWND::default())
            .expect("Could not launch probe");
        let report_path = folder.0.join("launch-report.txt");
        wait_for("Launch probe did not finish", || report_path.exists());
        let report = fs::read_to_string(&report_path).unwrap();
        assert_eq!(
            report,
            format!("{}\nrelative data found", folder.0.display())
        );
        fs::remove_file(report_path).unwrap();
    }
}

fn wait_for(message: &str, mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(started.elapsed() < Duration::from_secs(10), "{message}");
        thread::sleep(Duration::from_millis(10));
    }
}

struct ProbeFolder(PathBuf);

impl ProbeFolder {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "Core launch probe é-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&folder).unwrap();
        Self(folder)
    }
}

impl Drop for ProbeFolder {
    fn drop(&mut self) {
        // Only remove this test's uniquely named direct child of the temporary directory.
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let started = Instant::now();
        while let Err(error) = fs::remove_dir_all(&self.0) {
            if started.elapsed() >= Duration::from_secs(10) {
                eprintln!(
                    "Could not remove launch probe {}: {error}",
                    self.0.display()
                );
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
