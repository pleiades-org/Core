//! Test helpers for isolated filesystem-backed tests.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static TEST_DATA_DIR_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Environment variable that overrides the launcher data root (see [`crate::paths`]).
pub const TEST_DATA_DIR_ENV: &str = "CORE_TEST_DATA_DIR";

/// Returns the test data directory when a test override or `CORE_TEST_DATA_DIR` is set.
pub fn test_data_dir() -> Option<PathBuf> {
    if let Ok(guard) = TEST_DATA_DIR_OVERRIDE.lock() {
        if let Some(path) = guard.as_ref() {
            return Some(path.clone());
        }
    }
    std::env::var_os(TEST_DATA_DIR_ENV).map(PathBuf::from)
}

/// Runs `callback` with the data root pointed at a fresh temporary directory.
pub fn with_test_data_dir<F, R>(callback: F) -> R
where
    F: FnOnce(&Path) -> R,
{
    static TEST_RUNNER_MUTEX: Mutex<()> = Mutex::new(());
    let _runner_guard = TEST_RUNNER_MUTEX.lock().expect("test runner lock");

    let temp_dir = std::env::temp_dir().join(format!(
        "core-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp test data dir");

    {
        let mut guard = TEST_DATA_DIR_OVERRIDE
            .lock()
            .expect("test data dir lock");
        *guard = Some(temp_dir.clone());
    }

    let result = callback(&temp_dir);

    {
        let mut guard = TEST_DATA_DIR_OVERRIDE
            .lock()
            .expect("test data dir lock");
        *guard = None;
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}