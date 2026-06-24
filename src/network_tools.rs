use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    search_text::normalize_search_text,
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;
const PUBLIC_IP_API: &str = "https://api.ipify.org";

pub fn search_network(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);

    if normalized_search_text.is_empty() {
        return network_catalog();
    }

    execute_network_query(&normalized_search_text)
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(query);
    execute_network_query(&normalized)
}

fn network_catalog() -> Vec<CommandResult> {
    vec![
        local_ip_result(86),
        public_ip_fetch_result(85),
        hint_result("Ping host", "ping google.com", "ping ", 84),
        flush_dns_result(83),
    ]
}

fn hint_result(title: &str, subtitle: &str, copy_text: &str, confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(title, subtitle, copy_text, CommandCategory::Network, confidence)
}

fn execute_network_query(normalized: &str) -> Vec<CommandResult> {
    if normalized == "ip" || normalized == "local ip" || normalized == "my ip" {
        return vec![local_ip_result(90)];
    }

    if normalized == "public ip" || normalized == "external ip" {
        return vec![public_ip_fetch_result(90)];
    }

    if let Some(host) = normalized.strip_prefix("ping ") {
        if !host.is_empty() {
            return vec![ping_host_result(host, 90)];
        }
    }

    if normalized == "flush dns" || normalized == "flushdns" || normalized == "dns flush" {
        return vec![flush_dns_result(90)];
    }

    if !normalized.contains(' ') {
        return vec![
            ping_host_result(normalized, 82),
            hint_result(
                &format!("Ping {normalized}"),
                &format!("ping {normalized}"),
                &format!("ping {normalized}"),
                80,
            ),
        ];
    }

    Vec::new()
}

fn local_ip_result(confidence: u8) -> CommandResult {
    match read_local_ip() {
        Some(ip) => CommandResult::copyable_feature(
            "Local IP address",
            "IPv4 from active network adapter",
            ip.clone(),
            CommandCategory::Network,
            confidence,
        ),
        None => CommandResult::informational("Local IP", "Could not determine local IP address"),
    }
}

fn public_ip_fetch_result(confidence: u8) -> CommandResult {
    match ureq::get(PUBLIC_IP_API).call() {
        Ok(response) => {
            if !(200..300).contains(&response.status()) {
                return CommandResult::informational(
                    "Public IP",
                    &format!("IP service returned status {}", response.status()),
                );
            }
            let ip = response.into_string().unwrap_or_default().trim().to_string();
            if ip.is_empty() {
                return CommandResult::informational("Public IP", "Empty response from IP service");
            }
            CommandResult::copyable_feature(
                "Public IP address",
                "Fetched from api.ipify.org",
                ip,
                CommandCategory::Network,
                confidence,
            )
        }
        Err(error) => {
            CommandResult::informational("Public IP", &format!("Network error: {error}"))
        }
    }
}

fn ping_host_result(host: &str, confidence: u8) -> CommandResult {
    CommandResult {
        title: format!("Ping {host}"),
        subtitle: "Run ping -n 4".to_string(),
        copy_text: format!("ping {host}"),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Network,
        action: open_terminal_action("ping", &["-n", "4", host]),
        confidence,
    }
}

fn flush_dns_result(confidence: u8) -> CommandResult {
    CommandResult {
        title: "Flush DNS cache".to_string(),
        subtitle: "Run ipconfig /flushdns".to_string(),
        copy_text: "ipconfig /flushdns".to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Network,
        action: CommandAction::RunProgram {
            program: "ipconfig".to_string(),
            arguments: vec!["/flushdns".to_string()],
        },
        confidence,
    }
}

fn read_local_ip() -> Option<String> {
    let script = r#"
Get-NetIPAddress -AddressFamily IPv4 |
    Where-Object { $_.IPAddress -notlike '127.*' -and $_.PrefixOrigin -ne 'WellKnown' } |
    Sort-Object InterfaceMetric |
    Select-Object -ExpandProperty IPAddress -First 1
"#;

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-Command",
        script,
    ]);
    command.stdin(Stdio::null());
    configure_no_console_window(&mut command);

    let child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout?;
    let ip = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .next()?
        .trim()
        .to_string();

    if ip.is_empty() {
        None
    } else {
        Some(ip)
    }
}

fn open_terminal_action(program: &str, arguments: &[&str]) -> CommandAction {
    let command_line = std::iter::once(program.to_string())
        .chain(arguments.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>()
        .join(" ");

    CommandAction::RunProgram {
        program: "cmd.exe".to_string(),
        arguments: vec![
            "/c".to_string(),
            "start".to_string(),
            "cmd".to_string(),
            "/k".to_string(),
            command_line,
        ],
    }
}

#[cfg(target_os = "windows")]
fn configure_no_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_no_console_window(_command: &mut Command) {}

pub fn parse_ping_query(normalized: &str) -> Option<&str> {
    normalized.strip_prefix("ping ").map(str::trim).filter(|host| !host.is_empty())
}

pub fn parse_network_inline(normalized: &str) -> Option<&'static str> {
    match normalized {
        "ip" | "local ip" | "my ip" => Some("local_ip"),
        "public ip" | "external ip" => Some("public_ip"),
        "flush dns" | "flushdns" | "dns flush" => Some("flush_dns"),
        _ => parse_ping_query(normalized).map(|_| "ping"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ping_inline_query() {
        assert_eq!(parse_ping_query("ping github.com"), Some("github.com"));
        assert_eq!(parse_ping_query("ip"), None);
    }

    #[test]
    fn parses_network_inline_triggers() {
        assert_eq!(parse_network_inline("public ip"), Some("public_ip"));
        assert_eq!(parse_network_inline("flush dns"), Some("flush_dns"));
        assert_eq!(parse_network_inline("ping google.com"), Some("ping"));
    }

    #[test]
    fn inline_ip_triggers_local_ip() {
        let results = search_inline("ip");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, CommandCategory::Network);
    }
}