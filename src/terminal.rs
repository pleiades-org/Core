use std::{
    collections::HashSet,
    env,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellKind {
    PowerShell,
    CommandPrompt,
    Posix,
    Wsl,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellProfile {
    pub display_name: String,
    pub executable_path: PathBuf,
    pub kind: ShellKind,
}

impl ShellProfile {
    pub fn subtitle(&self) -> String {
        self.executable_path.display().to_string()
    }

    pub fn preference_key(&self) -> String {
        format!(
            "{}|{}",
            self.display_name.to_lowercase(),
            self.executable_path.to_string_lossy().to_lowercase()
        )
    }

    fn command_arguments(&self, command_text: &str) -> Vec<String> {
        match self.kind {
            ShellKind::PowerShell => vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                command_text.to_string(),
            ],
            ShellKind::CommandPrompt => vec!["/C".to_string(), command_text.to_string()],
            ShellKind::Posix => vec!["-lc".to_string(), command_text.to_string()],
            ShellKind::Wsl => vec![
                "--exec".to_string(),
                "bash".to_string(),
                "-lc".to_string(),
                command_text.to_string(),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOutputKind {
    Command,
    StandardOutput,
    StandardError,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalProcessEvent {
    Output {
        kind: TerminalOutputKind,
        text: String,
    },
    Completed {
        exit_code: Option<i32>,
    },
    Failed(String),
}

pub fn search_terminal_scope(search_text: &str) -> Vec<crate::command::CommandResult> {
    use crate::command::{CommandCategory, CommandResult};

    if search_text.trim().is_empty() {
        return vec![CommandResult::informational(
            "Terminal",
            "Type a command after @cmd to run it in the launcher",
        )];
    }

    vec![CommandResult::copyable_feature(
        format!("Run in terminal: {}", search_text.trim()),
        "Press Enter to choose a shell and run this command",
        search_text.trim().to_string(),
        CommandCategory::BuiltIn,
        84,
    )]
}

pub fn parse_command_scope(query: &str) -> Option<String> {
    let trimmed_query = query.trim_start();
    let mut command_words = Vec::new();
    let mut found_command_scope = false;

    for word in trimmed_query.split_whitespace() {
        let normalized_scope_tag = word
            .trim_matches(|character: char| matches!(character, ',' | ';' | ':' | '(' | ')'))
            .strip_prefix('@')
            .map(|scope_tag| scope_tag.to_lowercase());

        if normalized_scope_tag
            .as_deref()
            .is_some_and(is_command_scope_tag)
        {
            found_command_scope = true;
            continue;
        }

        command_words.push(word);
    }

    found_command_scope.then(|| command_words.join(" ").trim().to_string())
}

fn is_command_scope_tag(scope_tag: &str) -> bool {
    matches!(scope_tag, "cmd" | "terminal" | "term" | "shell")
}

pub fn detect_shell_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();
    let mut seen_profile_keys = HashSet::new();

    for candidate in shell_candidates() {
        let Some(executable_path) = resolve_shell_executable(&candidate) else {
            continue;
        };

        let profile_key = format!(
            "{}:{}",
            candidate.display_name.to_lowercase(),
            executable_path.to_string_lossy().to_lowercase()
        );
        if seen_profile_keys.insert(profile_key) {
            profiles.push(ShellProfile {
                display_name: candidate.display_name.to_string(),
                executable_path,
                kind: candidate.kind,
            });
        }
    }

    profiles
}

pub fn spawn_terminal_command(
    shell_profile: ShellProfile,
    command_text: String,
    working_directory: PathBuf,
) -> Receiver<TerminalProcessEvent> {
    let (event_sender, event_receiver) = mpsc::channel();

    thread::spawn(move || {
        if command_text.trim().is_empty() {
            let _ = event_sender.send(TerminalProcessEvent::Failed(
                "No command was provided.".to_string(),
            ));
            return;
        }

        let _ = event_sender.send(TerminalProcessEvent::Output {
            kind: TerminalOutputKind::Command,
            text: format!(
                "{} {}> {}",
                shell_profile.display_name,
                working_directory.display(),
                command_text.trim()
            ),
        });

        let mut command = Command::new(&shell_profile.executable_path);
        command
            .args(shell_profile.command_arguments(&command_text))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.current_dir(valid_terminal_directory(&working_directory));

        configure_no_console_window(&mut command);

        let mut child_process = match command.spawn() {
            Ok(child_process) => child_process,
            Err(error) => {
                let _ = event_sender.send(TerminalProcessEvent::Failed(format!(
                    "Could not start {}: {error}",
                    shell_profile.display_name
                )));
                return;
            }
        };

        let stdout_reader = child_process.stdout.take().map(|stdout| {
            spawn_output_reader(
                stdout,
                TerminalOutputKind::StandardOutput,
                event_sender.clone(),
            )
        });
        let stderr_reader = child_process.stderr.take().map(|stderr| {
            spawn_output_reader(
                stderr,
                TerminalOutputKind::StandardError,
                event_sender.clone(),
            )
        });

        match child_process.wait() {
            Ok(exit_status) => {
                if let Some(reader_handle) = stdout_reader {
                    let _ = reader_handle.join();
                }
                if let Some(reader_handle) = stderr_reader {
                    let _ = reader_handle.join();
                }

                let _ = event_sender.send(TerminalProcessEvent::Completed {
                    exit_code: exit_status.code(),
                });
            }
            Err(error) => {
                let _ = event_sender.send(TerminalProcessEvent::Failed(format!(
                    "Command wait failed: {error}"
                )));
            }
        }
    });

    event_receiver
}

pub fn default_terminal_directory() -> PathBuf {
    home_directory().unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn parse_directory_change_target(command_text: &str) -> Option<String> {
    let trimmed_command = command_text.trim();
    let (command_name, target_text) = trimmed_command
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed_command, ""));
    if !matches!(command_name.to_lowercase().as_str(), "cd" | "chdir") {
        return None;
    }

    let target_text = target_text.trim();
    let target_text = target_text
        .strip_prefix("/d ")
        .or_else(|| target_text.strip_prefix("/D "))
        .unwrap_or(target_text)
        .trim();

    Some(unquote_path_text(target_text))
}

pub fn resolve_directory_change_target(current_directory: &Path, target_text: &str) -> PathBuf {
    let trimmed_target = target_text.trim();
    if trimmed_target.is_empty() || trimmed_target == "~" {
        return default_terminal_directory();
    }

    let expanded_target = expand_home_alias(trimmed_target);
    let expanded_target = expand_windows_environment_variables(&expanded_target);
    let target_path = PathBuf::from(expanded_target);

    if target_path.is_absolute() {
        target_path
    } else {
        current_directory.join(target_path)
    }
}

fn spawn_output_reader(
    output_stream: impl Read + Send + 'static,
    output_kind: TerminalOutputKind,
    event_sender: Sender<TerminalProcessEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(output_stream);
        let mut output_line = String::new();

        loop {
            output_line.clear();
            match reader.read_line(&mut output_line) {
                Ok(0) => break,
                Ok(_) => {
                    let normalized_line = output_line.trim_end_matches(['\r', '\n']).to_string();
                    let _ = event_sender.send(TerminalProcessEvent::Output {
                        kind: output_kind,
                        text: normalized_line,
                    });
                }
                Err(error) => {
                    let _ = event_sender.send(TerminalProcessEvent::Failed(format!(
                        "Output read failed: {error}"
                    )));
                    break;
                }
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn configure_no_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_no_console_window(_command: &mut Command) {}

struct ShellCandidate {
    display_name: &'static str,
    executable_name: &'static str,
    kind: ShellKind,
    fallback_paths: Vec<PathBuf>,
    resolver: ShellResolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellResolver {
    PathThenFallback,
    GitBash,
}

fn resolve_shell_executable(candidate: &ShellCandidate) -> Option<PathBuf> {
    match candidate.resolver {
        ShellResolver::GitBash => find_git_bash_executable(),
        ShellResolver::PathThenFallback => find_executable(candidate.executable_name)
            .or_else(|| find_existing_path(&candidate.fallback_paths)),
    }
}

fn find_git_bash_executable() -> Option<PathBuf> {
    for candidate_path in git_bash_candidate_paths() {
        if candidate_path.is_file() {
            return Some(candidate_path);
        }
    }

    if let Some(git_path) = find_executable("git") {
        if let Some(bash_path) = bash_from_git_executable(&git_path) {
            return Some(bash_path);
        }
    }

    find_executable("bash").filter(|bash_path| is_git_bash_path(bash_path))
}

fn git_bash_candidate_paths() -> Vec<PathBuf> {
    let mut candidate_paths = env_paths(&[
        ("ProgramFiles", "Git\\bin\\bash.exe"),
        ("ProgramFiles", "Git\\usr\\bin\\bash.exe"),
        ("ProgramFiles(x86)", "Git\\bin\\bash.exe"),
        ("ProgramFiles(x86)", "Git\\usr\\bin\\bash.exe"),
        ("LOCALAPPDATA", "Programs\\Git\\bin\\bash.exe"),
        ("LOCALAPPDATA", "Programs\\Git\\usr\\bin\\bash.exe"),
    ]);

    if let Some(install_root) = git_install_root_from_registry() {
        candidate_paths.push(install_root.join("bin").join("bash.exe"));
        candidate_paths.push(install_root.join("usr").join("bin").join("bash.exe"));
    }

    candidate_paths
}

fn bash_from_git_executable(git_path: &Path) -> Option<PathBuf> {
    let install_root = git_path.parent()?.parent()?;
    for relative_path in ["bin/bash.exe", "usr/bin/bash.exe"] {
        let bash_path = install_root.join(relative_path);
        if bash_path.is_file() {
            return Some(bash_path);
        }
    }

    None
}

fn is_git_bash_path(path: &Path) -> bool {
    let normalized_path = path.to_string_lossy().replace('/', "\\").to_lowercase();
    normalized_path.contains("\\git\\") && !normalized_path.contains("\\windowsapps\\")
}

#[cfg(target_os = "windows")]
fn git_install_root_from_registry() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_SZ,
    };

    const INSTALL_PATH_VALUE: &str = "InstallPath";
    const GIT_FOR_WINDOWS_KEY: &str = "SOFTWARE\\GitForWindows";

    unsafe {
        let mut registry_key = HKEY::default();
        let key_path = wide_string(GIT_FOR_WINDOWS_KEY);
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_path.as_ptr()),
            Some(0),
            KEY_READ,
            &mut registry_key,
        )
        .is_err()
        {
            return None;
        }

        let value_name = wide_string(INSTALL_PATH_VALUE);
        let mut value_type = REG_SZ;
        let mut value_bytes = vec![0u16; 260];
        let mut value_bytes_len = (value_bytes.len() * 2) as u32;

        let query_result = RegQueryValueExW(
            registry_key,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut value_type as *mut _),
            Some(value_bytes.as_mut_ptr().cast()),
            Some(&mut value_bytes_len),
        );
        let _ = RegCloseKey(registry_key);

        if query_result.is_err() || value_type != REG_SZ {
            return None;
        }

        let value_length = (value_bytes_len as usize / 2).saturating_sub(1);
        let install_path = OsString::from_wide(&value_bytes[..value_length]);
        let install_root = PathBuf::from(install_path);
        install_root.is_dir().then_some(install_root)
    }
}

#[cfg(target_os = "windows")]
fn wide_string(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(target_os = "windows"))]
fn git_install_root_from_registry() -> Option<PathBuf> {
    None
}

fn shell_candidates() -> Vec<ShellCandidate> {
    vec![
        ShellCandidate {
            display_name: "PowerShell 7",
            executable_name: "pwsh",
            kind: ShellKind::PowerShell,
            fallback_paths: env_paths(&[
                ("ProgramFiles", "PowerShell\\7\\pwsh.exe"),
                ("LOCALAPPDATA", "Microsoft\\WindowsApps\\pwsh.exe"),
            ]),
            resolver: ShellResolver::PathThenFallback,
        },
        ShellCandidate {
            display_name: "Windows PowerShell",
            executable_name: "powershell",
            kind: ShellKind::PowerShell,
            fallback_paths: env_paths(&[(
                "SystemRoot",
                "System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            )]),
            resolver: ShellResolver::PathThenFallback,
        },
        ShellCandidate {
            display_name: "Command Prompt",
            executable_name: "cmd",
            kind: ShellKind::CommandPrompt,
            fallback_paths: env_paths(&[("SystemRoot", "System32\\cmd.exe")]),
            resolver: ShellResolver::PathThenFallback,
        },
        ShellCandidate {
            display_name: "Git Bash",
            executable_name: "bash",
            kind: ShellKind::Posix,
            fallback_paths: git_bash_candidate_paths(),
            resolver: ShellResolver::GitBash,
        },
        ShellCandidate {
            display_name: "Zsh",
            executable_name: "zsh",
            kind: ShellKind::Posix,
            fallback_paths: Vec::new(),
            resolver: ShellResolver::PathThenFallback,
        },
        ShellCandidate {
            display_name: "Fish",
            executable_name: "fish",
            kind: ShellKind::Posix,
            fallback_paths: Vec::new(),
            resolver: ShellResolver::PathThenFallback,
        },
        ShellCandidate {
            display_name: "WSL Bash",
            executable_name: "wsl",
            kind: ShellKind::Wsl,
            fallback_paths: env_paths(&[("SystemRoot", "System32\\wsl.exe")]),
            resolver: ShellResolver::PathThenFallback,
        },
    ]
}

fn env_paths(paths: &[(&str, &str)]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter_map(|(environment_variable, relative_path)| {
            env::var_os(environment_variable)
                .map(PathBuf::from)
                .map(|base_path| base_path.join(relative_path))
        })
        .collect()
}

fn find_existing_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.is_file()).cloned()
}

fn find_executable(executable_name: &str) -> Option<PathBuf> {
    let executable_path = Path::new(executable_name);
    if executable_path.is_absolute() && executable_path.is_file() {
        return Some(executable_path.to_path_buf());
    }

    let path_variable = env::var_os("PATH")?;
    let executable_extensions = executable_extensions();

    for search_path in env::split_paths(&path_variable) {
        let direct_candidate = search_path.join(executable_name);
        if direct_candidate.is_file() {
            return Some(direct_candidate);
        }

        if Path::new(executable_name).extension().is_some() {
            continue;
        }

        for extension in &executable_extensions {
            let extension = extension.trim_start_matches('.');
            let candidate_path = search_path.join(format!("{executable_name}.{extension}"));
            if candidate_path.is_file() {
                return Some(candidate_path);
            }
        }
    }

    None
}

fn home_directory() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn valid_terminal_directory(requested_directory: &Path) -> PathBuf {
    if requested_directory.is_dir() {
        requested_directory.to_path_buf()
    } else {
        default_terminal_directory()
    }
}

fn unquote_path_text(path_text: &str) -> String {
    let trimmed_path_text = path_text.trim();
    if trimmed_path_text.len() >= 2 {
        let first_character = trimmed_path_text.chars().next();
        let last_character = trimmed_path_text.chars().last();
        if matches!(
            (first_character, last_character),
            (Some('"'), Some('"')) | (Some('\''), Some('\''))
        ) {
            return trimmed_path_text[1..trimmed_path_text.len() - 1].to_string();
        }
    }

    trimmed_path_text.to_string()
}

fn expand_home_alias(path_text: &str) -> String {
    if path_text == "~" {
        return default_terminal_directory().display().to_string();
    }

    if let Some(relative_path) = path_text
        .strip_prefix("~/")
        .or_else(|| path_text.strip_prefix("~\\"))
    {
        return default_terminal_directory()
            .join(relative_path)
            .display()
            .to_string();
    }

    path_text.to_string()
}

fn expand_windows_environment_variables(path_text: &str) -> String {
    let mut expanded_text = String::new();
    let mut remaining_text = path_text;

    while let Some(start_index) = remaining_text.find('%') {
        expanded_text.push_str(&remaining_text[..start_index]);
        let after_start = &remaining_text[start_index + 1..];
        let Some(end_index) = after_start.find('%') else {
            expanded_text.push_str(&remaining_text[start_index..]);
            return expanded_text;
        };

        let variable_name = &after_start[..end_index];
        if let Ok(variable_value) = env::var(variable_name) {
            expanded_text.push_str(&variable_value);
        } else {
            expanded_text.push('%');
            expanded_text.push_str(variable_name);
            expanded_text.push('%');
        }
        remaining_text = &after_start[end_index + 1..];
    }

    expanded_text.push_str(remaining_text);
    expanded_text
}

fn executable_extensions() -> Vec<String> {
    env::var("PATHEXT")
        .map(|path_extensions| {
            path_extensions
                .split(';')
                .filter_map(|extension| {
                    let trimmed_extension = extension.trim();
                    (!trimmed_extension.is_empty()).then(|| trimmed_extension.to_string())
                })
                .collect()
        })
        .unwrap_or_else(|_| vec!["exe".to_string(), "cmd".to_string(), "bat".to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_scope_with_command_text() {
        assert_eq!(
            parse_command_scope("@cmd cargo test"),
            Some("cargo test".to_string())
        );
        assert_eq!(
            parse_command_scope("cargo test @cmd"),
            Some("cargo test".to_string())
        );
    }

    #[test]
    fn ignores_non_command_scope() {
        assert_eq!(parse_command_scope("@app steam"), None);
        assert_eq!(parse_command_scope("cmd cargo test"), None);
        assert_eq!(parse_command_scope("@command deploy"), None);
    }

    #[test]
    fn parses_directory_change_target() {
        assert_eq!(
            parse_directory_change_target("cd coding"),
            Some("coding".to_string())
        );
        assert_eq!(
            parse_directory_change_target("cd /d D:\\anime"),
            Some("D:\\anime".to_string())
        );
        assert_eq!(
            parse_directory_change_target("chdir \"My Videos\""),
            Some("My Videos".to_string())
        );
        assert_eq!(parse_directory_change_target("dir"), None);
    }

    #[test]
    fn recognizes_git_bash_install_paths() {
        assert!(is_git_bash_path(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(!is_git_bash_path(Path::new(
            r"C:\Users\Robert\AppData\Local\Microsoft\WindowsApps\bash.exe"
        )));
    }

    #[test]
    fn derives_git_bash_from_git_executable() {
        let bash_path = bash_from_git_executable(Path::new(
            r"C:\Program Files\Git\cmd\git.exe",
        ))
        .expect("expected bash next to git.exe");

        assert!(bash_path.ends_with("bash.exe"));
        assert!(bash_path.starts_with(r"C:\Program Files\Git"));
    }

    #[test]
    fn detects_git_bash_profile() {
        let profiles = detect_shell_profiles();
        assert!(
            profiles
                .iter()
                .any(|profile| profile.display_name == "Git Bash"),
            "expected Git Bash in {:?}",
            profiles
                .iter()
                .map(|profile| (&profile.display_name, &profile.executable_path))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolves_relative_directory_change_target() {
        let current_directory = Path::new("C:\\Users\\Robert");

        assert_eq!(
            resolve_directory_change_target(current_directory, "coding"),
            PathBuf::from("C:\\Users\\Robert").join("coding")
        );
    }
}
