use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::io::IntoRawHandle,
    path::PathBuf,
};
use windows::Win32::{
    Foundation::HANDLE,
    System::{
        Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE},
        SystemInformation::GetLocalTime,
        Threading::GetCurrentProcessId,
    },
};

/// Rotate before the log grows past this size; one previous log is kept as `core.log.old`.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// Release builds use the Windows subsystem, so stderr is detached and every `eprintln!`
/// diagnostic would be lost. Point stderr at `%APPDATA%\Pleiades\Core\v2\core.log` instead.
/// Test scripts redirect stderr themselves; an existing handle is left untouched.
pub fn redirect_stderr_to_log() {
    let existing = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if existing.is_ok_and(|handle| !handle.is_invalid() && !handle.0.is_null()) {
        return;
    }
    let Some(path) = log_path() else {
        return;
    };
    // There is nowhere to report a logging failure; Core keeps running without a log.
    let _ = open_log(&path).map(|file| unsafe {
        let _ = SetStdHandle(STD_ERROR_HANDLE, HANDLE(file.into_raw_handle()));
    });
}

fn log_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join("Pleiades/Core/v2/core.log"))
}

fn open_log(path: &PathBuf) -> std::io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_LOG_BYTES) {
        fs::rename(path, path.with_extension("log.old"))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let time = unsafe { GetLocalTime() };
    writeln!(
        file,
        "--- Core started {:04}-{:02}-{:02} {:02}:{:02}:{:02} · pid {} ---",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        unsafe { GetCurrentProcessId() }
    )?;
    Ok(file)
}
