//! Commands that leave Core: a terminal that stays open, an administrator terminal, and
//! Windows Run-dialog targets.
use super::{
    environment,
    shells::{interactive_arguments, quote, ResolvedShell},
};
use std::path::{Path, PathBuf};
use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_CANCELLED, HWND},
        System::{Environment::ExpandEnvironmentStringsW, Threading::*},
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Windows Terminal's app execution alias, when it is installed.
fn windows_terminal() -> Option<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\WindowsApps\wt.exe");
    std::fs::symlink_metadata(&path).is_ok().then_some(path)
}

/// Windows Terminal splits its own command line at `;`, so a command's semicolons are escaped.
fn escape_for_windows_terminal(arguments: &str) -> String {
    arguments.replace(';', "\\;")
}

/// Opens a new terminal window in `directory` running `command`, then leaves the shell open.
pub fn open_terminal(
    resolved: &ResolvedShell,
    command: &str,
    directory: &Path,
) -> Result<(), String> {
    let shell_arguments = interactive_arguments(resolved, command, directory);
    let (program, command_line, flags) = match windows_terminal() {
        Some(terminal) => (
            terminal.clone(),
            format!(
                "{} -w new -d {} {} {}",
                quote(&terminal.to_string_lossy()),
                quote(&directory.to_string_lossy()),
                quote(&resolved.program.to_string_lossy()),
                escape_for_windows_terminal(&shell_arguments)
            ),
            PROCESS_CREATION_FLAGS(0),
        ),
        None => (
            resolved.program.clone(),
            format!(
                "{} {shell_arguments}",
                quote(&resolved.program.to_string_lossy())
            ),
            CREATE_NEW_CONSOLE,
        ),
    };
    let environment_block = environment::fresh_block(&[]);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    let program = wide(&program.to_string_lossy());
    let mut command_line = wide(&command_line);
    let directory = wide(&directory.to_string_lossy());
    let flags = if environment_block.is_some() {
        flags | CREATE_UNICODE_ENVIRONMENT
    } else {
        flags
    };
    unsafe {
        CreateProcessW(
            PCWSTR(program.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            environment_block
                .as_ref()
                .map(|block| block.as_ptr().cast()),
            PCWSTR(directory.as_ptr()),
            &startup,
            &mut information,
        )
    }
    .map_err(|error| format!("Could not open a terminal: {error}"))?;
    unsafe {
        let _ = CloseHandle(information.hThread);
        let _ = CloseHandle(information.hProcess);
    }
    Ok(())
}

/// Opens the shell in `directory` as administrator after the Windows consent prompt.
pub fn run_elevated(
    window: HWND,
    resolved: &ResolvedShell,
    command: &str,
    directory: &Path,
) -> Result<(), String> {
    shell_execute(
        window,
        w!("runas"),
        &resolved.program.to_string_lossy(),
        &interactive_arguments(resolved, command, directory),
        directory,
    )
}

/// Opens `target` the way the Windows Run dialog does: environment variables are expanded, an
/// existing path or URI opens as a whole, and otherwise the first word is the program.
pub fn open_run_target(window: HWND, target: &str, elevated: bool) -> Result<(), String> {
    let expanded = expand_environment(target.trim());
    let (file, parameters) = if std::path::Path::new(&expanded).exists()
        || expanded.contains(':') && !expanded.contains(' ')
    {
        (expanded.clone(), String::new())
    } else {
        split_program(&expanded)
    };
    shell_execute(
        window,
        if elevated { w!("runas") } else { w!("open") },
        &file,
        &parameters,
        &environment::home(),
    )
}

fn split_program(text: &str) -> (String, String) {
    if let Some(quoted) = text.strip_prefix('"') {
        if let Some(end) = quoted.find('"') {
            return (
                quoted[..end].to_owned(),
                quoted[end + 1..].trim().to_owned(),
            );
        }
    }
    match text.split_once(' ') {
        Some((program, rest)) => (program.to_owned(), rest.trim().to_owned()),
        None => (text.to_owned(), String::new()),
    }
}

fn shell_execute(
    window: HWND,
    verb: PCWSTR,
    file: &str,
    parameters: &str,
    directory: &Path,
) -> Result<(), String> {
    let file = wide(file);
    let parameters = wide(parameters);
    let directory = wide(&directory.to_string_lossy());
    let mut information = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        hwnd: window,
        lpVerb: verb,
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: PCWSTR(directory.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    match unsafe { ShellExecuteExW(&mut information) } {
        Ok(()) => {
            if !information.hProcess.is_invalid() && !information.hProcess.0.is_null() {
                unsafe {
                    let _ = CloseHandle(information.hProcess);
                }
            }
            Ok(())
        }
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => {
            Err("The administrator request was cancelled".into())
        }
        Err(error) => Err(format!("Windows could not open it: {error}")),
    }
}

fn expand_environment(text: &str) -> String {
    let source = wide(text);
    let mut buffer = vec![0_u16; 32_768];
    let length =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut buffer)) } as usize;
    if length == 0 || length > buffer.len() {
        return text.to_owned();
    }
    String::from_utf16_lossy(&buffer[..length - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_targets_split_programs_from_arguments() {
        assert_eq!(
            split_program("notepad notes.txt"),
            ("notepad".into(), "notes.txt".into())
        );
        assert_eq!(
            split_program("\"C:\\Program Files\\App\\app.exe\" --flag"),
            ("C:\\Program Files\\App\\app.exe".into(), "--flag".into())
        );
        assert_eq!(split_program("calc"), ("calc".into(), String::new()));
    }

    #[test]
    fn windows_terminal_semicolons_are_escaped() {
        assert_eq!(
            escape_for_windows_terminal("/k echo a; echo b"),
            "/k echo a\\; echo b"
        );
        assert!(expand_environment("%USERPROFILE%").len() > 3);
        assert_eq!(expand_environment("no variables"), "no variables");
    }
}
