//! Shell commands typed after `/` (or `@cmd`, `@ps`, `@pwsh`, `@wsl`, `@bash`). Search only
//! describes what would run; the launcher runs it after Enter.
use super::{Action, ResultKind, SearchBatch, SearchResult};
use std::sync::Arc;

/// Which shell runs a command. `Default` follows the user's terminal default profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShellKind {
    #[default]
    Default,
    Cmd,
    WindowsPowerShell,
    PowerShell,
    Wsl,
    GitBash,
}

impl ShellKind {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Cmd,
        Self::WindowsPowerShell,
        Self::PowerShell,
        Self::Wsl,
        Self::GitBash,
    ];

    /// Stable identifier used in settings files.
    pub fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Cmd => "cmd",
            Self::WindowsPowerShell => "powershell",
            Self::PowerShell => "pwsh",
            Self::Wsl => "wsl",
            Self::GitBash => "gitbash",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default shell",
            Self::Cmd => "Command Prompt",
            Self::WindowsPowerShell => "Windows PowerShell",
            Self::PowerShell => "PowerShell 7",
            Self::Wsl => "WSL",
            Self::GitBash => "Git Bash",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|shell| shell.id().eq_ignore_ascii_case(id.trim()))
    }
}

/// Where a command's output goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Hidden process; output streams into Core's output panel.
    Capture,
    /// A terminal window that stays open after the command.
    Terminal,
    /// A terminal window with administrator rights (UAC prompt).
    Elevated,
}

const MESSAGE: &str =
    "Enter runs · Ctrl+Enter opens a terminal · Ctrl+Shift+Enter as administrator";
const EMPTY_MESSAGE: &str = "Commands run in your shell · ↑ recalls recent commands";

/// The three ways to run `payload`; the launcher shows them as keys rather than rows.
pub(super) fn terminal_results(payload: &str, shell: ShellKind) -> SearchBatch {
    let command = payload.trim();
    if command.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: EMPTY_MESSAGE,
        };
    }
    let command_text: Arc<str> = command.into();
    let run = |mode: RunMode, title: Arc<str>, description: String, id: &str| SearchResult {
        kind: ResultKind::Terminal,
        id: id.into(),
        title,
        description: description.into(),
        action: Action::RunCommand {
            command: command_text.clone(),
            shell,
            mode,
        },
    };
    let results = vec![
        run(
            RunMode::Capture,
            command_text.clone(),
            format!("Run in {} · output appears below", shell.label()),
            "terminal-run",
        ),
        run(
            RunMode::Terminal,
            "Open in terminal".into(),
            format!("{command} · {}", shell.label()),
            "terminal-open",
        ),
        run(
            RunMode::Elevated,
            "Run as administrator".into(),
            format!("{command} · opens a terminal after the Windows prompt"),
            "terminal-admin",
        ),
    ];
    SearchBatch {
        results,
        message: MESSAGE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_offers_run_terminal_and_administrator() {
        let batch = terminal_results("  git status ", ShellKind::Default);
        let modes: Vec<_> = batch
            .results
            .iter()
            .map(|result| match &result.action {
                Action::RunCommand {
                    command,
                    shell: ShellKind::Default,
                    mode,
                } if &**command == "git status" => *mode,
                other => panic!("unexpected action {other:?}"),
            })
            .collect();
        assert_eq!(
            modes,
            [RunMode::Capture, RunMode::Terminal, RunMode::Elevated]
        );
        assert_eq!(batch.message, MESSAGE);
    }

    #[test]
    fn an_empty_prompt_has_nothing_to_run() {
        let batch = terminal_results("   ", ShellKind::PowerShell);
        assert!(batch.results.is_empty());
        assert_eq!(batch.message, EMPTY_MESSAGE);
    }

    #[test]
    fn shell_identifiers_round_trip() {
        for shell in ShellKind::ALL {
            assert_eq!(ShellKind::parse(shell.id()), Some(shell));
        }
        assert_eq!(ShellKind::parse("fish"), None);
    }
}
