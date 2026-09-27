//! Which shell runs a command, and the exact command lines Core starts for each run mode.
//! "Default" follows Windows Terminal's default profile, which is what the user gets when they
//! open a terminal; without one, Windows PowerShell (the Windows default) is used.
use super::jsonc::{self, Value};
use core_engine::search::ShellKind;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, PoisonError,
    },
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interpreter {
    Cmd,
    WindowsPowerShell,
    PowerShell,
    Wsl { distribution: Option<String> },
    GitBash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedShell {
    pub shell: Interpreter,
    pub program: PathBuf,
    /// Shown in the footer, such as `PowerShell 7 (terminal default)`.
    pub label: String,
}

/// A program and its full command line, ready for `CreateProcessW` or `ShellExecuteExW`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    /// Arguments only, without the program.
    pub arguments: String,
    /// Extra environment variables for the child.
    pub environment: Vec<(&'static str, String)>,
    /// Where the shell writes the directory the command finished in. Output cannot carry it:
    /// a console shows every character written to it.
    pub directory_report: PathBuf,
}

impl Invocation {
    pub fn command_line(&self) -> String {
        format!(
            "{} {}",
            quote(&self.program.to_string_lossy()),
            self.arguments
        )
    }
}

/// The shell for `preference`, reused from the last Enter while Windows Terminal's settings
/// are unchanged and the program is still there.
pub fn resolve(preference: ShellKind) -> Result<ResolvedShell, String> {
    static CACHE: Mutex<ShellCache> = Mutex::new(ShellCache::new());
    CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .resolve(preference, settings_stamp(), installed, resolve_uncached)
}

/// When each Windows Terminal settings file was last modified; `None` for a missing one.
type SettingsStamp = Vec<Option<SystemTime>>;

fn settings_stamp() -> SettingsStamp {
    terminal_settings_paths()
        .iter()
        .map(|path| {
            std::fs::metadata(path)
                .and_then(|file| file.modified())
                .ok()
        })
        .collect()
}

/// Shells resolved before, so each command does not re-read and parse Windows Terminal's
/// settings and probe for programs.
struct ShellCache {
    entries: Vec<(ShellKind, SettingsStamp, ResolvedShell)>,
}

impl ShellCache {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The cached shell while `stamp` matches and its program `exists`; otherwise `resolve`
    /// runs again. Failures are not kept, so a shell installed later is found.
    fn resolve(
        &mut self,
        preference: ShellKind,
        stamp: SettingsStamp,
        exists: impl Fn(&Path) -> bool,
        resolve: impl FnOnce(ShellKind) -> Result<ResolvedShell, String>,
    ) -> Result<ResolvedShell, String> {
        let cached = self.entries.iter().find(|(kind, ..)| *kind == preference);
        if let Some((_, cached_stamp, shell)) = cached {
            if *cached_stamp == stamp && exists(&shell.program) {
                return Ok(shell.clone());
            }
        }
        self.entries.retain(|(kind, ..)| *kind != preference);
        let resolved = resolve(preference)?;
        self.entries.push((preference, stamp, resolved.clone()));
        Ok(resolved)
    }
}

fn resolve_uncached(preference: ShellKind) -> Result<ResolvedShell, String> {
    let concrete = match preference {
        ShellKind::Default => {
            return Ok(terminal_default().unwrap_or_else(|| ResolvedShell {
                label: "Windows PowerShell (Windows default)".into(),
                ..windows_powershell()
            }))
        }
        ShellKind::Cmd => Interpreter::Cmd,
        ShellKind::WindowsPowerShell => Interpreter::WindowsPowerShell,
        ShellKind::PowerShell => Interpreter::PowerShell,
        ShellKind::Wsl => Interpreter::Wsl { distribution: None },
        ShellKind::GitBash => Interpreter::GitBash,
    };
    let program =
        locate(&concrete).ok_or_else(|| format!("{} is not installed", preference.label()))?;
    Ok(ResolvedShell {
        label: preference.label().into(),
        shell: concrete,
        program,
    })
}

fn windows_powershell() -> ResolvedShell {
    ResolvedShell {
        shell: Interpreter::WindowsPowerShell,
        program: system32().join(r"WindowsPowerShell\v1.0\powershell.exe"),
        label: "Windows PowerShell".into(),
    }
}

fn system32() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32")
}

fn environment_path(variable: &str, rest: &str) -> Option<PathBuf> {
    std::env::var_os(variable).map(|root| PathBuf::from(root).join(rest))
}

/// Absolute path of an installed shell.
fn locate(shell: &Interpreter) -> Option<PathBuf> {
    let candidates: Vec<Option<PathBuf>> = match shell {
        Interpreter::Cmd => vec![
            std::env::var_os("ComSpec").map(PathBuf::from),
            Some(system32().join("cmd.exe")),
        ],
        Interpreter::WindowsPowerShell => vec![Some(windows_powershell().program)],
        Interpreter::PowerShell => vec![
            environment_path("ProgramFiles", r"PowerShell\7\pwsh.exe"),
            environment_path("ProgramFiles", r"PowerShell\7-preview\pwsh.exe"),
            environment_path("LOCALAPPDATA", r"Microsoft\WindowsApps\pwsh.exe"),
        ],
        Interpreter::Wsl { .. } => vec![Some(system32().join("wsl.exe"))],
        Interpreter::GitBash => vec![
            environment_path("ProgramFiles", r"Git\bin\bash.exe"),
            environment_path("LOCALAPPDATA", r"Programs\Git\bin\bash.exe"),
        ],
    };
    candidates
        .into_iter()
        .flatten()
        .find(|path| installed(path))
}

/// App execution aliases (WindowsApps) are reparse points; `exists` follows them poorly, so
/// metadata is checked without following links too.
fn installed(path: &Path) -> bool {
    path.exists() || std::fs::symlink_metadata(path).is_ok()
}

fn terminal_settings_paths() -> Vec<PathBuf> {
    [
        r"Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json",
        r"Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
        r"Microsoft\Windows Terminal\settings.json",
    ]
    .into_iter()
    .filter_map(|relative| environment_path("LOCALAPPDATA", relative))
    .collect()
}

/// Windows Terminal's default profile, when it is a shell Core knows how to drive.
fn terminal_default() -> Option<ResolvedShell> {
    let settings = terminal_settings_paths()
        .into_iter()
        .filter_map(|path| std::fs::read_to_string(path).ok());
    default_shell(settings, expand_environment, locate)
}

/// The default profile's shell in the first of `settings` that parses: stable Windows
/// Terminal before Preview. A default Core cannot drive gives `None`, so Windows PowerShell is
/// used as documented, rather than another installation's default.
fn default_shell(
    settings: impl IntoIterator<Item = String>,
    expand: impl Fn(&str) -> String,
    locate: impl Fn(&Interpreter) -> Option<PathBuf>,
) -> Option<ResolvedShell> {
    let settings = settings
        .into_iter()
        .find_map(|text| jsonc::parse(text.trim_start_matches('\u{feff}')).ok())?;
    let profile = default_profile(&settings)?;
    let (shell, program) = classify_profile(profile, expand)?;
    let program = program.or_else(|| locate(&shell))?;
    Some(ResolvedShell {
        label: format!("{} (terminal default)", shell_label(&shell)),
        shell,
        program,
    })
}

fn shell_label(shell: &Interpreter) -> String {
    match shell {
        Interpreter::Cmd => "Command Prompt".into(),
        Interpreter::WindowsPowerShell => "Windows PowerShell".into(),
        Interpreter::PowerShell => "PowerShell 7".into(),
        Interpreter::Wsl {
            distribution: Some(name),
        } => format!("WSL {name}"),
        Interpreter::Wsl { distribution: None } => "WSL".into(),
        Interpreter::GitBash => "Git Bash".into(),
    }
}

fn default_profile(settings: &Value) -> Option<&Value> {
    let wanted = settings.get("defaultProfile")?.as_str()?;
    let profiles = settings.get("profiles")?;
    let list = profiles
        .get("list")
        .and_then(Value::as_array)
        .or_else(|| profiles.as_array())?;
    // Older settings name the default profile instead of using its GUID.
    list.iter().find(|profile| {
        ["guid", "name"].iter().any(|key| {
            profile
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(wanted))
        })
    })
}

/// A shell and, when the profile names one, its executable.
fn classify_profile(
    profile: &Value,
    expand: impl Fn(&str) -> String,
) -> Option<(Interpreter, Option<PathBuf>)> {
    if let Some(command_line) = profile.get("commandline").and_then(Value::as_str) {
        let expanded = expand(command_line);
        let (program, rest) = split_program(&expanded);
        let name = Path::new(&program)
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        let path = Path::new(&program)
            .is_absolute()
            .then(|| PathBuf::from(&program));
        let shell = match name.trim_end_matches(".exe") {
            "cmd" => Interpreter::Cmd,
            "powershell" => Interpreter::WindowsPowerShell,
            "pwsh" => Interpreter::PowerShell,
            "wsl" => Interpreter::Wsl {
                distribution: distribution_argument(rest),
            },
            "bash" if program.to_lowercase().contains("git") => Interpreter::GitBash,
            _ => return None,
        };
        return Some((shell, path));
    }
    let shell = match profile.get("source").and_then(Value::as_str)? {
        "Windows.Terminal.PowershellCore" => Interpreter::PowerShell,
        "Windows.Terminal.Wsl" => Interpreter::Wsl {
            distribution: profile
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        "Git" => Interpreter::GitBash,
        _ => return None,
    };
    Some((shell, None))
}

/// `"C:\Program Files\x.exe" -a` or `x.exe -a` → program and the remaining arguments.
fn split_program(command_line: &str) -> (String, &str) {
    let command_line = command_line.trim();
    if let Some(quoted) = command_line.strip_prefix('"') {
        if let Some(end) = quoted.find('"') {
            return (quoted[..end].to_owned(), quoted[end + 1..].trim_start());
        }
    }
    // Unquoted paths may contain spaces (`C:\Program Files\Git\bin\bash.exe -i`): the
    // program ends at the first `.exe` followed by a space or the end.
    let lower = command_line.to_ascii_lowercase();
    let exe_end = lower
        .match_indices(".exe")
        .map(|(index, _)| index + 4)
        .find(|&end| {
            command_line[end..].is_empty() || command_line[end..].starts_with(char::is_whitespace)
        });
    if let Some(end) = exe_end {
        return (
            command_line[..end].to_owned(),
            command_line[end..].trim_start(),
        );
    }
    match command_line.split_once(char::is_whitespace) {
        Some((program, rest)) => (program.to_owned(), rest.trim_start()),
        None => (command_line.to_owned(), ""),
    }
}

fn distribution_argument(arguments: &str) -> Option<String> {
    let mut words = arguments.split_whitespace();
    while let Some(word) = words.next() {
        if matches!(word, "-d" | "--distribution") {
            return words.next().map(|name| name.trim_matches('"').to_owned());
        }
    }
    None
}

fn expand_environment(text: &str) -> String {
    use windows::{core::PCWSTR, Win32::System::Environment::ExpandEnvironmentStringsW};
    let source: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let mut buffer = vec![0_u16; 4_096];
    let length =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut buffer)) } as usize;
    if length == 0 || length > buffer.len() {
        return text.to_owned();
    }
    String::from_utf16_lossy(&buffer[..length - 1])
}

/// Names the file a captured run writes its final directory to, so the next command starts
/// there, as `cd` would leave a terminal.
const DIRECTORY_REPORT_VARIABLE: &str = "CORE_DIRECTORY_FILE";

/// A new file name in the temporary folder for one run's directory report.
fn directory_report_path() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "core-v2-directory-{}-{}.txt",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Carries the command to [`POWERSHELL_CAPTURE`], which removes it before running the command.
const POWERSHELL_COMMAND_VARIABLE: &str = "CORE_COMMAND";

/// Runs `$env:CORE_COMMAND` as if typed at a prompt, with UTF-8 output and no progress bars.
/// The exit code is the last native program's, else 1 when the last statement failed.
const POWERSHELL_CAPTURE: &str = concat!(
    "$ProgressPreference = 'SilentlyContinue'; ",
    "[Console]::InputEncoding = [Text.Encoding]::UTF8; ",
    "[Console]::OutputEncoding = [Text.Encoding]::UTF8; ",
    "$OutputEncoding = [Text.Encoding]::UTF8; ",
    "$coreDirectoryFile = $env:CORE_DIRECTORY_FILE; Remove-Item Env:CORE_DIRECTORY_FILE; ",
    // Parsing the command alone first makes syntax errors point at what was typed.
    "try { $null = [ScriptBlock]::Create($env:CORE_COMMAND) } ",
    "catch { [Console]::Error.WriteLine($_.Exception.GetBaseException().Message); exit 1 }; ",
    // A trailing line reads `$?` straight after the command's last statement.
    "$coreSucceeded = $true; ",
    "$coreCommand = [ScriptBlock]::Create($env:CORE_COMMAND + [Environment]::NewLine + '$coreSucceeded = $?'); ",
    "Remove-Item Env:CORE_COMMAND; ",
    ". $coreCommand; ",
    // Registry and other provider locations cannot be a process's working directory.
    "if ($PWD.Provider.Name -eq 'FileSystem') { ",
    "[IO.File]::WriteAllText($coreDirectoryFile, $PWD.ProviderPath) }; ",
    "if ($LASTEXITCODE) { exit $LASTEXITCODE } elseif (-not $coreSucceeded) { exit 1 }",
);

/// A bash command followed by its directory report, keeping the command's exit status.
/// `directory` prints the working directory as a Windows path.
fn bash_capture(command: &str, directory: &str) -> String {
    format!(
        "{command}\n__core_status=$?; {directory} > \"${DIRECTORY_REPORT_VARIABLE}\" 2>/dev/null; exit $__core_status"
    )
}

/// A run shown in Core's own terminal. PowerShell skips profiles for speed and predictable
/// output; every shell is told to use UTF-8, and reports its final directory.
pub fn capture_invocation(resolved: &ResolvedShell, command: &str) -> Invocation {
    let arguments = match &resolved.shell {
        // `/s /c "…"` passes the command verbatim; chcp makes console tools use UTF-8, which
        // also makes the directory report UTF-8. `call` delays `%errorlevel%` until the command
        // has run, and the saved error level becomes the exit code, since `cd` would otherwise
        // report success.
        Interpreter::Cmd => format!(
            "/d /s /c \"chcp 65001>nul & {command} & call set CORE_EXIT=%^errorlevel% \
             & cd>\"%{DIRECTORY_REPORT_VARIABLE}%\" & call exit %^CORE_EXIT%\""
        ),
        // `-Command`, because `-EncodedCommand` writes errors and progress as CLIXML. The
        // command itself travels in an environment variable, verbatim.
        Interpreter::WindowsPowerShell | Interpreter::PowerShell => {
            format!("-NoLogo -NoProfile -Command {}", quote(POWERSHELL_CAPTURE))
        }
        Interpreter::Wsl { distribution } => format!(
            "{}--exec bash -lc {}",
            wsl_distribution(distribution),
            quote(&bash_capture(command, "wslpath -w \"$PWD\""))
        ),
        Interpreter::GitBash => format!("-lc {}", quote(&bash_capture(command, "pwd -W"))),
    };
    let directory_report = directory_report_path();
    let mut environment = vec![(
        DIRECTORY_REPORT_VARIABLE,
        directory_report.to_string_lossy().into_owned(),
    )];
    match resolved.shell {
        // WSLENV shares the variable with Linux, translated to a Linux path.
        Interpreter::Wsl { .. } => environment.extend([
            ("WSL_UTF8", "1".to_owned()),
            ("WSLENV", shared_with_wsl(std::env::var("WSLENV").ok())),
        ]),
        Interpreter::WindowsPowerShell | Interpreter::PowerShell => {
            environment.push((POWERSHELL_COMMAND_VARIABLE, command.to_owned()));
        }
        Interpreter::Cmd | Interpreter::GitBash => {}
    }
    Invocation {
        program: resolved.program.clone(),
        arguments,
        environment,
        directory_report,
    }
}

/// The user's own WSLENV list with the directory report added as a translated path.
fn shared_with_wsl(existing: Option<String>) -> String {
    let report = format!("{DIRECTORY_REPORT_VARIABLE}/p");
    match existing.filter(|list| !list.is_empty()) {
        Some(list) => format!("{list}:{report}"),
        None => report,
    }
}

/// A visible shell that stays open after the command, with the user's profile loaded. It moves
/// to `directory` itself, because administrator shells ignore the directory they are given.
pub fn interactive_arguments(resolved: &ResolvedShell, command: &str, directory: &Path) -> String {
    let directory = directory.to_string_lossy();
    match &resolved.shell {
        Interpreter::Cmd => format!("/k cd /d \"{directory}\" & {command}"),
        Interpreter::WindowsPowerShell | Interpreter::PowerShell => {
            format!(
                "-NoLogo -NoExit -EncodedCommand {}",
                encode_powershell(&format!(
                    "Set-Location -LiteralPath {}; {command}",
                    single_quoted_powershell(&directory)
                ))
            )
        }
        Interpreter::Wsl { distribution } => format!(
            "{}--cd {} --exec bash -lc {}",
            wsl_distribution(distribution),
            quote(&directory),
            quote(&format!("{command}; exec \"${{SHELL:-bash}}\" -l"))
        ),
        Interpreter::GitBash => format!(
            "-lc {}",
            quote(&format!(
                "cd {} && {command}; exec bash -l",
                single_quoted_bash(&directory.replace('\\', "/"))
            ))
        ),
    }
}

fn single_quoted_powershell(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn single_quoted_bash(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// A directory reported by a shell, as a Windows path. Git Bash reports `C:/Users`.
pub fn reported_directory(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let path = PathBuf::from(text.replace('/', "\\"));
    path.is_absolute().then_some(path)
}

fn wsl_distribution(distribution: &Option<String>) -> String {
    distribution
        .as_ref()
        .map(|name| format!("-d {} ", quote(name)))
        .unwrap_or_default()
}

/// One argument quoted for the Microsoft C runtime's command-line parser.
pub fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '\n', '"']) {
        return argument.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(character);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// PowerShell's `-EncodedCommand`: Base64 of UTF-16LE, which avoids every quoting rule.
fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(json: &str) -> Value {
        jsonc::parse(json).unwrap()
    }

    #[test]
    fn terminal_profiles_map_to_shells() {
        let expand = |text: &str| text.replace("%SystemRoot%", r"C:\Windows");
        let cases = [
            (
                r#"{"commandline": "%SystemRoot%\\System32\\cmd.exe"}"#,
                Interpreter::Cmd,
            ),
            (
                r#"{"commandline": "powershell.exe -NoLogo"}"#,
                Interpreter::WindowsPowerShell,
            ),
            (
                r#"{"commandline": "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -nologo"}"#,
                Interpreter::PowerShell,
            ),
            (
                r#"{"commandline": "wsl.exe -d Ubuntu-22.04"}"#,
                Interpreter::Wsl {
                    distribution: Some("Ubuntu-22.04".into()),
                },
            ),
            (
                r#"{"commandline": "C:\\Program Files\\Git\\bin\\bash.exe -i -l"}"#,
                Interpreter::GitBash,
            ),
            (
                r#"{"source": "Windows.Terminal.PowershellCore"}"#,
                Interpreter::PowerShell,
            ),
            (
                r#"{"source": "Windows.Terminal.Wsl", "name": "Debian"}"#,
                Interpreter::Wsl {
                    distribution: Some("Debian".into()),
                },
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(
                classify_profile(&profile(json), expand).map(|(shell, _)| shell),
                Some(expected),
                "{json}"
            );
        }
        let (_, program) = classify_profile(
            &profile(
                r#"{"commandline": "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -nologo"}"#,
            ),
            expand,
        )
        .unwrap();
        assert_eq!(
            program,
            Some(PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"))
        );
        for unsupported in [
            r#"{"commandline": "nu.exe"}"#,
            r#"{"source": "Windows.Terminal.Azure"}"#,
            "{}",
        ] {
            assert_eq!(
                classify_profile(&profile(unsupported), expand),
                None,
                "{unsupported}"
            );
        }
    }

    #[test]
    fn the_default_profile_is_found_by_guid_or_name() {
        let settings = profile(
            r#"{"defaultProfile": "{B}", "profiles": {"list": [
                {"guid": "{a}", "commandline": "cmd.exe"}, {"guid": "{b}", "commandline": "pwsh.exe"}]}}"#,
        );
        assert_eq!(
            default_profile(&settings)
                .and_then(|p| p.get("commandline"))
                .and_then(Value::as_str),
            Some("pwsh.exe")
        );
        let legacy = profile(
            r#"{"defaultProfile": "Command Prompt", "profiles": [{"name": "Command Prompt", "commandline": "cmd.exe"}]}"#,
        );
        assert!(default_profile(&legacy).is_some());
    }

    #[test]
    fn cached_shells_are_reused_until_the_settings_or_program_change() {
        use std::{cell::Cell, time::Duration};
        let resolves = Cell::new(0);
        let resolve = |preference: ShellKind| {
            resolves.set(resolves.get() + 1);
            Ok(ResolvedShell {
                shell: Interpreter::Cmd,
                program: PathBuf::from(format!(r"C:\shells\{preference:?}-{}.exe", resolves.get())),
                label: String::new(),
            })
        };
        let exists = |_: &Path| true;
        let stamp = vec![Some(SystemTime::UNIX_EPOCH), None];
        let mut cache = ShellCache::new();
        let first = cache.resolve(ShellKind::Default, stamp.clone(), exists, resolve);
        let again = cache.resolve(ShellKind::Default, stamp.clone(), exists, resolve);
        assert_eq!(first, again);
        assert_eq!(resolves.get(), 1, "unchanged settings reuse the shell");
        cache
            .resolve(ShellKind::Cmd, stamp.clone(), exists, resolve)
            .unwrap();
        assert_eq!(resolves.get(), 2, "each preference is cached on its own");

        let edited = vec![Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)), None];
        let changed = cache.resolve(ShellKind::Default, edited.clone(), exists, resolve);
        assert_ne!(changed, first);
        assert_eq!(resolves.get(), 3, "edited settings are read again");
        let created = vec![
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
            Some(SystemTime::UNIX_EPOCH),
        ];
        cache
            .resolve(ShellKind::Default, created.clone(), exists, resolve)
            .unwrap();
        assert_eq!(resolves.get(), 4, "a new settings file is read");

        cache
            .resolve(ShellKind::Default, created.clone(), |_| false, resolve)
            .unwrap();
        assert_eq!(
            resolves.get(),
            5,
            "an uninstalled program is looked for again"
        );

        let failed = cache.resolve(ShellKind::GitBash, created.clone(), exists, |_| {
            Err("Git Bash is not installed".into())
        });
        assert!(failed.is_err());
        cache
            .resolve(ShellKind::GitBash, created, exists, resolve)
            .unwrap();
        assert_eq!(resolves.get(), 6, "failures are not cached");
    }

    #[test]
    fn only_the_first_readable_terminal_settings_choose_the_default() {
        let expand = |text: &str| text.to_owned();
        let locate =
            |shell: &Interpreter| Some(PathBuf::from(format!(r"C:\located\{shell:?}.exe")));
        let settings = |commandline: &str| {
            format!(
                r#"{{"defaultProfile": "{{d}}", "profiles": {{"list": [{{"guid": "{{d}}", "commandline": "{commandline}"}}]}}}}"#
            )
        };
        let preview_pwsh = settings("pwsh.exe");
        let found = default_shell([settings("cmd.exe"), preview_pwsh.clone()], expand, locate)
            .expect("stable's default");
        assert_eq!(found.shell, Interpreter::Cmd);
        assert_eq!(found.label, "Command Prompt (terminal default)");
        // Stable's default cannot be driven: Windows PowerShell, not Preview's default.
        assert_eq!(
            default_shell([settings("nu.exe"), preview_pwsh.clone()], expand, locate),
            None
        );
        // Settings that cannot be parsed count as not installed.
        let found = default_shell(["{ broken".to_owned(), preview_pwsh], expand, locate)
            .expect("Preview's default");
        assert_eq!(found.shell, Interpreter::PowerShell);
        assert_eq!(found.program, PathBuf::from(r"C:\located\PowerShell.exe"));
        assert_eq!(default_shell(Vec::new(), expand, locate), None);
    }

    #[test]
    fn arguments_are_quoted_for_the_c_runtime() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"C:\path with space\"), r#""C:\path with space\\""#);
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn powershell_commands_are_base64_utf16() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(encode_powershell("ls"), "bABzAA==");
    }

    #[test]
    fn capture_invocations_request_utf8_output() {
        let cmd = ResolvedShell {
            shell: Interpreter::Cmd,
            program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            label: String::new(),
        };
        let invocation = capture_invocation(&cmd, "echo a & echo \"b\"");
        assert_eq!(
            invocation.arguments,
            "/d /s /c \"chcp 65001>nul & echo a & echo \"b\" & call set CORE_EXIT=%^errorlevel% \
             & cd>\"%CORE_DIRECTORY_FILE%\" & call exit %^CORE_EXIT%\""
        );
        let report = invocation.directory_report.to_string_lossy().into_owned();
        assert_eq!(
            invocation.environment,
            [(DIRECTORY_REPORT_VARIABLE, report)]
        );
        assert_ne!(
            capture_invocation(&cmd, "dir").directory_report,
            invocation.directory_report,
            "every run reports to its own file"
        );
        let wsl = ResolvedShell {
            shell: Interpreter::Wsl {
                distribution: Some("Ubuntu".into()),
            },
            program: PathBuf::from("wsl.exe"),
            label: String::new(),
        };
        let invocation = capture_invocation(&wsl, "ls -la ~");
        assert!(
            invocation
                .arguments
                .starts_with("-d Ubuntu --exec bash -lc \"ls -la ~\n__core_status=$?;"),
            "{}",
            invocation.arguments
        );
        assert!(invocation
            .arguments
            .contains("wslpath -w \\\"$PWD\\\" > \\\"$CORE_DIRECTORY_FILE\\\""));
        assert!(invocation
            .environment
            .contains(&("WSL_UTF8", "1".to_owned())));
        assert_eq!(shared_with_wsl(None), "CORE_DIRECTORY_FILE/p");
        assert_eq!(
            shared_with_wsl(Some("USERPROFILE/p".into())),
            "USERPROFILE/p:CORE_DIRECTORY_FILE/p"
        );
        let powershell = ResolvedShell {
            shell: Interpreter::WindowsPowerShell,
            program: PathBuf::from("powershell.exe"),
            label: String::new(),
        };
        let invocation = capture_invocation(&powershell, "echo \"a b\"");
        assert!(invocation
            .arguments
            .starts_with("-NoLogo -NoProfile -Command \""));
        assert!(!invocation.arguments.contains("a b"));
        assert!(invocation
            .environment
            .contains(&(POWERSHELL_COMMAND_VARIABLE, "echo \"a b\"".to_owned())));
        assert!(POWERSHELL_CAPTURE.contains(&format!("$env:{DIRECTORY_REPORT_VARIABLE}")));
        assert!(POWERSHELL_CAPTURE.contains(&format!("$env:{POWERSHELL_COMMAND_VARIABLE}")));
        assert!(!POWERSHELL_CAPTURE.contains('"'));
        let pictures = Path::new(r"C:\Users\Me\My Pictures");
        assert_eq!(
            interactive_arguments(&cmd, "dir", pictures),
            "/k cd /d \"C:\\Users\\Me\\My Pictures\" & dir"
        );
        let git_bash = ResolvedShell {
            shell: Interpreter::GitBash,
            program: PathBuf::from("bash.exe"),
            label: String::new(),
        };
        assert_eq!(
            interactive_arguments(&git_bash, "ls", Path::new(r"C:\it's")),
            "-lc \"cd 'C:/it'\\''s' && ls; exec bash -l\""
        );
    }

    #[test]
    fn reported_directories_become_windows_paths() {
        assert_eq!(
            reported_directory("C:/Users/Me/Pictures"),
            Some(PathBuf::from(r"C:\Users\Me\Pictures"))
        );
        assert_eq!(
            reported_directory(r"\\wsl.localhost\Ubuntu\home\me"),
            Some(PathBuf::from(r"\\wsl.localhost\Ubuntu\home\me"))
        );
        assert_eq!(reported_directory(""), None);
        assert_eq!(reported_directory("relative"), None);
    }
}
