#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

fn main() {
    if std::env::args().any(|argument| argument == "--version") {
        println!("Core {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    #[cfg(windows)]
    match windows::installer::dispatch() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Core setup failed: {error}");
            windows::show_fatal_error(&error);
            std::process::exit(1);
        }
    }
    #[cfg(windows)]
    if let Err(error) = windows::run() {
        eprintln!("Core could not start: {error}");
        if std::env::args().any(|argument| {
            argument == "--dry-run"
                || argument.starts_with("--update-")
                || argument == "--after-update"
        }) {
            std::process::exit(1);
        }
        windows::show_fatal_error(&error.to_string());
    }
    #[cfg(not(windows))]
    eprintln!("Core's launcher shell currently requires Windows.");
}
