pub const MAX_QUERY_BYTES: usize = 4096;
const MAX_COMMAND_BYTES: usize = 16;
const COMMAND_NAMES: &[&str] = &[
    "app",
    "apps",
    "calc",
    "calculator",
    "math",
    "time",
    "tz",
    "web",
    "power",
    "quicklink",
    "quicklinks",
    "link",
    "links",
    "taskbar",
    "tb",
    "run",
    "cmd",
    "ps",
    "powershell",
    "pwsh",
    "wsl",
    "bash",
    "shell",
    "terminal",
    "update",
    "media",
    "music",
];

use super::ShellKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Applications,
    Calculator,
    Time,
    Web,
    Quicklinks,
    Power,
    Taskbar,
    Update,
    /// Media controls: `@media`, `@music`.
    Media,
    /// A shell command: `/ipconfig`, `@pwsh Get-Process`.
    Shell(ShellKind),
    /// A Windows Run-dialog target: `@run notepad`.
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    TooLong,
    ControlCharacter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedQuery<'query> {
    Search(&'query str),
    Command {
        kind: CommandKind,
        payload: &'query str,
    },
    CommandHints(&'query str),
    UnknownCommand(&'query str),
    Invalid(QueryError),
}

/// Parse only the command prefix. The payload keeps its original spelling and spacing.
pub fn parse_query(query: &str) -> ParsedQuery<'_> {
    if query.len() > MAX_QUERY_BYTES {
        return ParsedQuery::Invalid(QueryError::TooLong);
    }
    if query
        .chars()
        .any(|character| character.is_control() && !character.is_whitespace())
    {
        return ParsedQuery::Invalid(QueryError::ControlCharacter);
    }
    let query = query.trim();
    // `/` starts a command prompt; everything after it is passed to the shell unchanged.
    if let Some(payload) = query.strip_prefix('/') {
        return ParsedQuery::Command {
            kind: CommandKind::Shell(ShellKind::Default),
            payload: payload.trim_start(),
        };
    }
    if let Some(payload) = query.strip_prefix('>') {
        return ParsedQuery::Command {
            kind: CommandKind::Quicklinks,
            payload: payload.trim_start(),
        };
    }
    let Some(scoped) = query.strip_prefix('@') else {
        return ParsedQuery::Search(query);
    };
    let (command, payload) = scoped
        .split_once(char::is_whitespace)
        .unwrap_or((scoped, ""));
    parse_command(command, payload.trim_start())
}

fn parse_command<'query>(command: &'query str, payload: &'query str) -> ParsedQuery<'query> {
    if command.len() > MAX_COMMAND_BYTES || !command.is_ascii() {
        return ParsedQuery::UnknownCommand(command);
    }
    let mut bytes = [0; MAX_COMMAND_BYTES];
    for (destination, original) in bytes.iter_mut().zip(command.bytes()) {
        *destination = original.to_ascii_lowercase();
    }
    // Every byte came from checked ASCII above, so this conversion cannot fail.
    let normalized = std::str::from_utf8(&bytes[..command.len()]).expect("ASCII command");
    let kind = match normalized {
        "app" | "apps" => CommandKind::Applications,
        "calc" | "calculator" | "math" => CommandKind::Calculator,
        "time" | "tz" => CommandKind::Time,
        "web" => CommandKind::Web,
        "power" => CommandKind::Power,
        "taskbar" | "tb" => CommandKind::Taskbar,
        "run" => CommandKind::Run,
        "update" => CommandKind::Update,
        "media" | "music" => CommandKind::Media,
        "shell" | "terminal" => CommandKind::Shell(ShellKind::Default),
        "cmd" => CommandKind::Shell(ShellKind::Cmd),
        "ps" | "powershell" => CommandKind::Shell(ShellKind::WindowsPowerShell),
        "pwsh" => CommandKind::Shell(ShellKind::PowerShell),
        "wsl" => CommandKind::Shell(ShellKind::Wsl),
        "bash" => CommandKind::Shell(ShellKind::GitBash),
        "quicklink" | "quicklinks" | "link" | "links" => CommandKind::Quicklinks,
        prefix
            if payload.is_empty() && COMMAND_NAMES.iter().any(|name| name.starts_with(prefix)) =>
        {
            return ParsedQuery::CommandHints(command)
        }
        _ => return ParsedQuery::UnknownCommand(command),
    };
    ParsedQuery::Command { kind, payload }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_borrow_the_original_payload_and_require_a_boundary() {
        assert_eq!(
            parse_query(" @CaLc  2 + 2 "),
            ParsedQuery::Command {
                kind: CommandKind::Calculator,
                payload: "2 + 2"
            }
        );
        assert_eq!(
            parse_query("@web Rust & Windows"),
            ParsedQuery::Command {
                kind: CommandKind::Web,
                payload: "Rust & Windows"
            }
        );
        assert_eq!(
            parse_query("@calc2+2"),
            ParsedQuery::UnknownCommand("calc2+2")
        );
        assert_eq!(
            parse_query("person@example.com"),
            ParsedQuery::Search("person@example.com")
        );
        assert_eq!(
            parse_query("2 + 2 @calc"),
            ParsedQuery::Search("2 + 2 @calc")
        );
    }

    #[test]
    fn a_slash_starts_a_command_prompt_with_the_rest_unchanged() {
        assert_eq!(
            parse_query("/ipconfig /all"),
            ParsedQuery::Command {
                kind: CommandKind::Shell(ShellKind::Default),
                payload: "ipconfig /all"
            }
        );
        assert_eq!(
            parse_query(" /  git  log "),
            ParsedQuery::Command {
                kind: CommandKind::Shell(ShellKind::Default),
                payload: "git  log"
            }
        );
        assert_eq!(
            parse_query("@pwsh Get-Process"),
            ParsedQuery::Command {
                kind: CommandKind::Shell(ShellKind::PowerShell),
                payload: "Get-Process"
            }
        );
        assert_eq!(
            parse_query("@run notepad"),
            ParsedQuery::Command {
                kind: CommandKind::Run,
                payload: "notepad"
            }
        );
        assert_eq!(parse_query("10 / 2"), ParsedQuery::Search("10 / 2"));
    }

    #[test]
    fn distinguishes_hints_unknown_commands_and_invalid_input() {
        assert_eq!(parse_query("@"), ParsedQuery::CommandHints(""));
        assert_eq!(parse_query("@cal"), ParsedQuery::CommandHints("cal"));
        assert_eq!(
            parse_query("@cal value"),
            ParsedQuery::UnknownCommand("cal")
        );
        assert_eq!(
            parse_query("@missing"),
            ParsedQuery::UnknownCommand("missing")
        );
        assert_eq!(
            parse_query("bad\0input"),
            ParsedQuery::Invalid(QueryError::ControlCharacter)
        );
        assert_eq!(
            parse_query(&"a".repeat(MAX_QUERY_BYTES + 1)),
            ParsedQuery::Invalid(QueryError::TooLong)
        );
        assert!(matches!(
            parse_query(&"a".repeat(MAX_QUERY_BYTES)),
            ParsedQuery::Search(_)
        ));
    }
}
