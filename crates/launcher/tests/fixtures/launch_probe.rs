#![windows_subsystem = "windows"]

use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executable = env::current_exe()?;
    let folder = executable.parent().ok_or("Probe has no parent directory")?;
    let report = format!(
        "{}\n{}",
        env::current_dir()?.display(),
        fs::read_to_string("data/launch-marker.txt").unwrap_or_else(|error| error.to_string()),
    );
    fs::write(folder.join("launch-report.tmp"), report)?;
    fs::rename(
        folder.join("launch-report.tmp"),
        folder.join("launch-report.txt"),
    )?;
    Ok(())
}
