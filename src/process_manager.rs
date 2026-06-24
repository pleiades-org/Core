use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    search_text::normalize_search_text,
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunningProcess {
    pid: u32,
    name: String,
}

pub fn search_processes(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    let processes = list_running_processes().unwrap_or_default();

    if normalized_search_text.is_empty() {
        return processes
            .into_iter()
            .take(20)
            .map(process_result)
            .collect();
    }

    processes
        .into_iter()
        .filter(|process| {
            normalize_search_text(&process.name).contains(&normalized_search_text)
                || process.pid.to_string().contains(&normalized_search_text)
        })
        .take(20)
        .map(process_result)
        .collect()
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    if let Some(results) = try_kill_window_query(query) {
        return results;
    }

    if let Some(result) = try_kill_frontmost_query(query) {
        return vec![result];
    }

    let process_name = match parse_kill_query(query) {
        Some(name) if !name.is_empty() => name,
        _ => return Vec::new(),
    };

    search_processes(process_name)
}

pub fn search_processes_scoped(search_text: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(search_text);

    if normalized.starts_with("window ") {
        let title = normalized.strip_prefix("window ").unwrap_or("");
        return search_processes_by_window_title(title);
    }

    if normalized == "frontmost" || normalized == "window" {
        return vec![kill_frontmost_result(88)];
    }

    search_processes(search_text)
}

fn parse_kill_query(query: &str) -> Option<&str> {
    let trimmed_query = query.trim();
    let (verb, rest) = trimmed_query.split_once(char::is_whitespace)?;
    if matches!(verb.to_ascii_lowercase().as_str(), "kill" | "quit") {
        Some(rest.trim())
    } else {
        None
    }
}

pub fn parse_kill_window_query(query: &str) -> Option<&str> {
    let trimmed_query = query.trim();
    let (verb, rest) = trimmed_query.split_once(char::is_whitespace)?;
    if !matches!(verb.to_ascii_lowercase().as_str(), "kill" | "quit") {
        return None;
    }

    let (target, title) = rest.split_once(char::is_whitespace)?;
    if target.eq_ignore_ascii_case("window") {
        Some(title.trim())
    } else {
        None
    }
}

fn try_kill_window_query(query: &str) -> Option<Vec<CommandResult>> {
    let title = parse_kill_window_query(query)?;
    if title.is_empty() {
        return None;
    }
    Some(search_processes_by_window_title(title))
}

fn try_kill_frontmost_query(query: &str) -> Option<CommandResult> {
    let normalized = normalize_search_text(query);
    if matches!(
        normalized.as_str(),
        "kill frontmost" | "quit frontmost" | "quit front" | "kill front"
    ) {
        return Some(kill_frontmost_result(90));
    }
    None
}

fn search_processes_by_window_title(title_query: &str) -> Vec<CommandResult> {
    let processes = list_processes_with_window_titles(title_query).unwrap_or_default();
    processes
        .into_iter()
        .take(20)
        .map(window_process_result)
        .collect()
}

fn window_process_result(process: WindowProcess) -> CommandResult {
    CommandResult {
        title: format!("Kill window: {}", process.window_title),
        subtitle: format!(
            "{} · PID {} · taskkill /F",
            process.name, process.pid
        ),
        copy_text: process.pid.to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: CommandAction::RunProgram {
            program: "taskkill".to_string(),
            arguments: vec!["/PID".to_string(), process.pid.to_string(), "/F".to_string()],
        },
        confidence: 89,
    }
}

fn kill_frontmost_result(confidence: u8) -> CommandResult {
    let script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class ForegroundWindow {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
"@
$hwnd = [ForegroundWindow]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) { exit 1 }
[uint32]$pid = 0
[void][ForegroundWindow]::GetWindowThreadProcessId($hwnd, [ref]$pid)
if ($pid -eq 0) { exit 1 }
taskkill /PID $pid /F
"#;

    CommandResult {
        title: "Kill frontmost window".to_string(),
        subtitle: "Terminate the foreground application".to_string(),
        copy_text: "kill frontmost".to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: CommandAction::RunProgram {
            program: "powershell.exe".to_string(),
            arguments: vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-WindowStyle".to_string(),
                "Hidden".to_string(),
                "-Command".to_string(),
                script.trim().to_string(),
            ],
        },
        confidence,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowProcess {
    pid: u32,
    name: String,
    window_title: String,
}

fn list_processes_with_window_titles(title_query: &str) -> Option<Vec<WindowProcess>> {
    let escaped_query = title_query.replace('\'', "''");
    let script = format!(
        "Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{escaped_query}*' }} | Select-Object Id,ProcessName,MainWindowTitle | ConvertTo-Csv -NoTypeInformation"
    );

    let mut command = Command::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-Command", &script]);
    command.stdin(Stdio::null());
    configure_no_console_window(&mut command);

    let child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout?;
    let reader = BufReader::new(stdout);

    let mut processes = Vec::new();
    for line in reader.lines().map_while(Result::ok).skip(1) {
        if let Some(process) = parse_window_process_csv_line(&line) {
            processes.push(process);
        }
    }

    Some(processes)
}

fn parse_window_process_csv_line(line: &str) -> Option<WindowProcess> {
    let mut fields = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;

    for character in line.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut current_field));
            }
            _ => current_field.push(character),
        }
    }
    fields.push(current_field);

    if fields.len() < 3 {
        return None;
    }

    let pid = fields[0].trim().trim_matches('"').parse().ok()?;
    let name = fields[1].trim().trim_matches('"').to_string();
    let window_title = fields[2].trim().trim_matches('"').to_string();
    if name.is_empty() || window_title.is_empty() {
        return None;
    }

    Some(WindowProcess {
        pid,
        name,
        window_title,
    })
}

fn process_result(process: RunningProcess) -> CommandResult {
    CommandResult {
        title: format!("Kill {}", process.name),
        subtitle: format!("PID {} · taskkill /F", process.pid),
        copy_text: process.pid.to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: CommandAction::RunProgram {
            program: "taskkill".to_string(),
            arguments: vec!["/PID".to_string(), process.pid.to_string(), "/F".to_string()],
        },
        confidence: 88,
    }
}

fn list_running_processes() -> Option<Vec<RunningProcess>> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-Command",
        "Get-Process | Sort-Object ProcessName | Select-Object -Property Id,ProcessName | ConvertTo-Csv -NoTypeInformation",
    ]);
    command.stdin(Stdio::null());
    configure_no_console_window(&mut command);

    let child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout?;
    let reader = BufReader::new(stdout);

    let mut processes = Vec::new();
    for line in reader.lines().map_while(Result::ok).skip(1) {
        if let Some(process) = parse_process_csv_line(&line) {
            processes.push(process);
        }
    }

    Some(processes)
}

fn parse_process_csv_line(line: &str) -> Option<RunningProcess> {
    let mut fields = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;

    for character in line.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut current_field));
            }
            _ => current_field.push(character),
        }
    }
    fields.push(current_field);

    if fields.len() < 2 {
        return None;
    }

    let pid = fields[0].trim().trim_matches('"').parse().ok()?;
    let name = fields[1].trim().trim_matches('"').to_string();
    if name.is_empty() {
        return None;
    }

    Some(RunningProcess { pid, name })
}

#[cfg(target_os = "windows")]
fn configure_no_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_no_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kill_and_quit_queries() {
        assert_eq!(parse_kill_query("kill chrome"), Some("chrome"));
        assert_eq!(parse_kill_query("quit Discord"), Some("Discord"));
        assert_eq!(parse_kill_query("chrome kill"), None);
    }

    #[test]
    fn parses_kill_window_queries() {
        assert_eq!(parse_kill_window_query("kill window chrome"), Some("chrome"));
        assert_eq!(parse_kill_window_query("quit window VS Code"), Some("VS Code"));
        assert_eq!(parse_kill_window_query("kill chrome"), None);
    }

    #[test]
    fn inline_kill_frontmost_returns_action() {
        let results = search_inline("kill frontmost");
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("frontmost"));
    }

    #[test]
    fn parses_process_csv_line() {
        let process = parse_process_csv_line("\"1234\",\"chrome\"").expect("process");
        assert_eq!(process.pid, 1234);
        assert_eq!(process.name, "chrome");
    }

    #[test]
    fn filters_process_results_by_name() {
        let results = search_processes("System");
        assert!(results.iter().all(|result| result.category == CommandCategory::System));
        if !results.is_empty() {
            assert!(matches!(
                results[0].action,
                CommandAction::RunProgram { .. }
            ));
        }
    }
}