#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(windows)]
    if let Err(error) = windows::run() {
        eprintln!("Core could not start: {error}");
        if std::env::args().any(|argument| argument == "--dry-run") {
            std::process::exit(1);
        }
        windows::show_fatal_error(&error.to_string());
    }
    #[cfg(not(windows))]
    eprintln!("Core's launcher shell currently requires Windows.");
}
