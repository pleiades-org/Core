//! `/command` runs: the result rows give way to Core's terminal, Enter runs, Ctrl+Enter
//! opens a terminal and Ctrl+Shift+Enter runs as administrator. Keys typed into the terminal
//! reach the running command. Esc stops it, and ↑ and ↓ in the search box step through the
//! command history like a shell prompt.
use super::LauncherState;
use crate::windows::{
    commands::{self, CommandRun, Outcome},
    execute_action::NativeAction,
};
use core_engine::search::{parse_query, CommandKind, ParsedQuery, RunMode, ShellKind};
use std::{
    path::{Component, Path, PathBuf, Prefix},
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};

/// Delays taking output that arrives sooner than [`OUTPUT_INTERVAL`] after the last batch.
pub const COMMAND_OUTPUT_TIMER: usize = 43;
/// The shortest time between output batches. Each costs a parse, a layout check and a
/// repaint, so a fast command's small reads are gathered into fewer, larger batches.
const OUTPUT_INTERVAL: Duration = Duration::from_millis(16);

pub struct CommandSession {
    run: CommandRun,
    started: Instant,
    finished: Option<(Outcome, Duration)>,
    /// When output was last taken from the run.
    last_output: Option<Instant>,
    /// [`COMMAND_OUTPUT_TIMER`] is set to take the output later.
    output_timer: bool,
}

/// Progress through the history while ↑ and ↓ are pressed.
pub struct Recall {
    /// The command typed before the first ↑; only entries starting with it are recalled.
    typed: String,
    /// Position among those entries; `None` shows `typed` again.
    index: Option<usize>,
    /// The query as recall left it. Any other text means the person typed, ending the recall.
    shown: String,
}

/// Which way to run a shell command, from the modifiers held with Enter.
pub fn run_mode(control: bool, shift: bool) -> RunMode {
    match (control, shift) {
        (true, true) => RunMode::Elevated,
        (true, false) => RunMode::Terminal,
        (false, _) => RunMode::Capture,
    }
}

/// Splits a shell query into the prefix that selects the shell (`/`, `@cmd `) and the command.
fn shell_query(query: &str) -> Option<(String, &str)> {
    let ParsedQuery::Command {
        kind: CommandKind::Shell(_),
        payload,
    } = parse_query(query)
    else {
        return None;
    };
    // The payload always runs to the end of the trimmed query.
    let trimmed = query.trim();
    let mut prefix = trimmed.strip_suffix(payload).unwrap_or(trimmed).to_owned();
    if !prefix.ends_with(['/', ' ']) {
        prefix.push(' ');
    }
    Some((prefix, payload))
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

impl LauncherState {
    /// `Default` in a query means the shell chosen in Settings (itself possibly "Default").
    fn shell_for(&self, shell: ShellKind) -> ShellKind {
        match shell {
            ShellKind::Default => self.settings.saved.preferences.shell,
            explicit => explicit,
        }
    }

    fn remember_command(&mut self, command: &str) {
        if let Err(error) = self.history.record(command) {
            eprintln!("{error}");
        }
    }

    /// Terminal and administrator runs leave Core, so they become a native action.
    pub(super) fn command_action(
        &mut self,
        command: Arc<str>,
        shell: ShellKind,
        mode: RunMode,
    ) -> Result<NativeAction, String> {
        let resolved = commands::resolve(self.shell_for(shell))?;
        self.remember_command(&command);
        let directory = self.command_directory();
        Ok(match mode {
            RunMode::Elevated => NativeAction::RunElevated {
                resolved,
                command,
                directory,
            },
            RunMode::Terminal | RunMode::Capture => NativeAction::OpenTerminal {
                resolved,
                command,
                directory,
            },
        })
    }

    /// Where the next command starts: where the last one finished, as after `cd` in a
    /// terminal. Home again if that folder has since gone.
    fn command_directory(&mut self) -> PathBuf {
        if !usable_directory(&self.working_directory, Path::is_dir) {
            self.working_directory = commands::home();
        }
        self.working_directory.clone()
    }

    /// Starts a run in Core's terminal, replacing any previous run. The keyboard moves to the
    /// terminal, so the command can be answered as it runs.
    pub(super) fn run_captured(&mut self, command: Arc<str>, shell: ShellKind) {
        let Some(view) = self.view.clone() else {
            return;
        };
        // Dropping the previous session stops it if it is still running.
        self.command = None;
        let resolved = match commands::resolve(self.shell_for(shell)) {
            Ok(resolved) => resolved,
            Err(error) => {
                view.set_footer(&error);
                return;
            }
        };
        self.remember_command(&command);
        let invocation = commands::capture_invocation(&resolved, &command);
        let directory = self.command_directory();
        let header = |directory: &Path| {
            format!(
                "{}> {command}",
                display_directory(directory, &commands::home())
            )
        };
        let (columns, rows) = view.start_console(&header(&directory));
        match CommandRun::start(self.window, &invocation, &directory, columns, rows) {
            Ok(run) => {
                // The folder had gone, so the command started at home.
                if run.directory() != directory {
                    self.working_directory = run.directory().to_path_buf();
                    view.start_console(&header(run.directory()));
                }
                view.attach_console(run.input());
                self.command = Some(CommandSession {
                    run,
                    started: Instant::now(),
                    finished: None,
                    last_output: None,
                    output_timer: false,
                });
                self.sync_terminal_view();
                view.relayout();
                view.set_footer(&self.command_status().unwrap_or_default());
            }
            Err(error) => view.set_footer(&error),
        }
    }

    /// The run has output. Output arriving within [`OUTPUT_INTERVAL`] of the last batch waits
    /// for a timer; the run keeps gathering it meanwhile.
    pub fn receive_command_output(&mut self) {
        let window = self.window;
        let Some(session) = self.command.as_mut() else {
            return;
        };
        if session.output_timer {
            return;
        }
        let delay = output_delay(session.last_output, Instant::now());
        if !delay.is_zero() {
            let milliseconds = delay.as_millis().try_into().unwrap_or(u32::MAX);
            session.output_timer =
                unsafe { SetTimer(Some(window), COMMAND_OUTPUT_TIMER, milliseconds, None) } != 0;
            // Without a timer the output is taken now rather than left waiting.
            if session.output_timer {
                return;
            }
        }
        self.take_command_output();
    }

    /// [`COMMAND_OUTPUT_TIMER`] fired: the delayed output is taken.
    pub fn command_output_timer(&mut self) {
        if let Err(error) = unsafe { KillTimer(Some(self.window), COMMAND_OUTPUT_TIMER) } {
            eprintln!("Could not stop the command output timer: {error}");
        }
        let Some(session) = self.command.as_mut() else {
            return;
        };
        session.output_timer = false;
        self.take_command_output();
    }

    fn take_command_output(&mut self) {
        let (Some(view), Some(session)) = (self.view.clone(), self.command.as_mut()) else {
            return;
        };
        session.last_output = Some(Instant::now());
        let mut update = session.run.take_update();
        view.feed_console(&update.output);
        session.run.recycle(std::mem::take(&mut update.output));
        // The run checked that the folder exists, away from the UI thread.
        if let Some(directory) = update.directory {
            self.working_directory = directory;
        }
        let Some(session) = self.command.as_mut() else {
            return;
        };
        if session.finished.is_none() {
            if let Some(outcome) = update.outcome {
                session.finished = Some((outcome, session.started.elapsed()));
                view.finish_console();
            }
        }
        if view.output_visible() {
            view.set_footer(&self.command_status().unwrap_or_default());
        }
    }

    /// Esc stops a running command before it hides Core.
    pub fn stop_command(&mut self) -> bool {
        let stopped = self
            .command
            .as_ref()
            .is_some_and(|session| session.run.is_running() && session.run.stop());
        if stopped {
            if let Some(view) = &self.view {
                view.set_footer("Stopping…");
            }
        }
        stopped
    }

    pub fn clear_command_history(&mut self) -> Result<(), String> {
        self.history.clear()?;
        self.queue_search();
        Ok(())
    }

    /// A shell query replaces the rows with the output of the latest run, if there is one.
    /// Applied at the view's next layout.
    pub(super) fn sync_terminal_view(&self) {
        let Some(view) = &self.view else {
            return;
        };
        // An alias may stand for a shell command; its output then shows like any other.
        let terminal = shell_query(&self.acted_query()).is_some();
        view.set_terminal(terminal, self.command.is_some() && !view.settings_open());
    }

    /// ↑ (`older`) and ↓ in a shell query step through recent commands, newest first.
    /// Returns false when the query is not a shell command, so the keys keep their usual role.
    pub fn recall_history(&mut self, older: bool) -> bool {
        let Some(view) = self.view.clone() else {
            return false;
        };
        let query = view.query();
        let Some((prefix, payload)) = shell_query(&query) else {
            return false;
        };
        let recall = match self.recall.take() {
            Some(recall) if recall.shown == query => recall,
            _ => Recall {
                typed: payload.to_owned(),
                index: None,
                shown: String::new(),
            },
        };
        let entries = self.history.entries();
        let matches: Vec<&Arc<str>> = entries
            .iter()
            .filter(|entry| starts_with_ignore_case(entry, &recall.typed))
            .collect();
        let index = match (recall.index, older) {
            (None, true) if matches.is_empty() => None,
            (None, true) => Some(0),
            (Some(index), true) => Some((index + 1).min(matches.len().saturating_sub(1))),
            (None | Some(0), false) => None,
            (Some(index), false) => Some(index - 1),
        };
        let command = index
            .and_then(|index| matches.get(index))
            .map_or(recall.typed.as_str(), |entry| entry);
        let text = format!("{prefix}{command}");
        if let Err(error) = view.set_query(&text) {
            view.set_footer(&format!("Could not recall the command: {error}"));
            return true;
        }
        self.recall = Some(Recall {
            index,
            shown: text,
            ..recall
        });
        self.queue_search();
        true
    }

    /// Typing ends a history recall; the next ↑ starts from the newest match again. A typed
    /// `/` at the start switches the box to a command prompt.
    pub fn query_edited(&mut self) {
        self.typed_since_show = true;
        if let Some(view) = &self.view {
            if let Err(error) = view.absorb_command_prefix() {
                view.set_footer(&format!("Could not start a command: {error}"));
            }
        }
        let query = self.view.as_ref().map(|view| view.query());
        if self
            .recall
            .as_ref()
            .is_some_and(|recall| Some(&recall.shown) != query.as_ref())
        {
            self.recall = None;
        }
        self.queue_search();
    }

    /// Backspace in the search box. In an empty command prompt it returns to search and
    /// returns true; otherwise the key edits the text as usual.
    pub fn leave_command_mode(&mut self, window: HWND) -> bool {
        let Some(view) = self.view.clone() else {
            return false;
        };
        if !view.is_input(window) || !view.leave_empty_command_mode() {
            return false;
        }
        self.recall = None;
        self.queue_search();
        true
    }

    /// After a command runs, the box keeps only its shell prefix (nothing, for `/`), ready for
    /// the next command.
    pub(super) fn clear_command_input(&mut self) {
        let Some(view) = self.view.clone() else {
            return;
        };
        let Some((prefix, _)) = shell_query(&view.query()) else {
            return;
        };
        if let Err(error) = view.set_query(&prefix) {
            view.set_footer(&format!("Could not clear the command: {error}"));
            return;
        }
        self.recall = None;
        self.queue_search();
    }

    /// Footer text describing the run, while the output panel is visible.
    pub(super) fn command_status(&self) -> Option<String> {
        let session = self.command.as_ref()?;
        let full_screen = self
            .view
            .as_ref()
            .is_some_and(|view| view.console_takes_escape());
        Some(match &session.finished {
            // A full-screen program keeps Esc for itself.
            None if full_screen => "Esc in search to stop".into(),
            None => "Esc to stop".into(),
            Some((Outcome::Exited(0), elapsed)) => {
                format!("Done in {}", duration(*elapsed))
            }
            Some((Outcome::Exited(code), elapsed)) => {
                format!("Exit code {code} after {}", duration(*elapsed))
            }
            Some((Outcome::Stopped, elapsed)) => {
                format!("Stopped after {}", duration(*elapsed))
            }
        })
    }
}

/// Whether a command can start in `directory`. Network and WSL folders (`\\server\share`,
/// `\\wsl.localhost\…`) are not checked, because touching an offline share or an idle WSL
/// distribution blocks for seconds; starting the command falls back to home if one has gone.
fn usable_directory(directory: &Path, is_dir: impl FnOnce(&Path) -> bool) -> bool {
    let unc = matches!(
        directory.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    );
    unc || is_dir(directory)
}

/// A directory as a prompt shows it: `~` for the home folder and anything inside it.
fn display_directory(directory: &Path, home: &Path) -> String {
    match directory.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".into(),
        Ok(relative) => format!("~\\{}", relative.display()),
        Err(_) => directory.display().to_string(),
    }
}

/// How long to wait before taking output, so batches come at least [`OUTPUT_INTERVAL`] apart.
fn output_delay(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |last| {
        OUTPUT_INTERVAL.saturating_sub(now.saturating_duration_since(last))
    })
}

fn duration(elapsed: Duration) -> String {
    let milliseconds = elapsed.as_millis();
    if milliseconds < 1_000 {
        format!("{milliseconds} ms")
    } else if milliseconds < 60_000 {
        format!("{:.1} s", elapsed.as_secs_f64())
    } else {
        format!(
            "{} min {} s",
            milliseconds / 60_000,
            milliseconds % 60_000 / 1_000
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_choose_how_a_command_runs() {
        assert_eq!(run_mode(false, false), RunMode::Capture);
        assert_eq!(run_mode(false, true), RunMode::Capture);
        assert_eq!(run_mode(true, false), RunMode::Terminal);
        assert_eq!(run_mode(true, true), RunMode::Elevated);
    }

    #[test]
    fn shell_queries_split_into_prefix_and_command() {
        assert_eq!(shell_query("/ls -la"), Some(("/".into(), "ls -la")));
        assert_eq!(
            shell_query("  /  git status"),
            Some(("/  ".into(), "git status"))
        );
        assert_eq!(shell_query("/"), Some(("/".into(), "")));
        assert_eq!(shell_query("@cmd dir"), Some(("@cmd ".into(), "dir")));
        assert_eq!(shell_query("@cmd"), Some(("@cmd ".into(), "")));
        assert_eq!(shell_query("notepad"), None);
        assert_eq!(shell_query("@calc 1+1"), None);
    }

    #[test]
    fn recall_matches_ignore_case() {
        assert!(starts_with_ignore_case("Git status", "git"));
        assert!(starts_with_ignore_case("anything", ""));
        assert!(!starts_with_ignore_case("git", "git status"));
    }

    #[test]
    fn directories_show_home_as_a_tilde() {
        let home = Path::new(r"C:\Users\Me");
        assert_eq!(display_directory(home, home), "~");
        assert_eq!(
            display_directory(Path::new(r"C:\Users\Me\Pictures\2026"), home),
            r"~\Pictures\2026"
        );
        assert_eq!(display_directory(Path::new(r"D:\Games"), home), r"D:\Games");
        assert_eq!(
            display_directory(Path::new(r"C:\Users\Mel"), home),
            r"C:\Users\Mel"
        );
    }

    #[test]
    fn network_and_wsl_folders_are_not_checked_before_a_run() {
        let unchecked = |_: &Path| -> bool { panic!("a UNC folder was checked") };
        for folder in [
            r"\\wsl.localhost\Ubuntu\home\me",
            r"\\wsl$\Debian",
            r"\\nas\share\projects",
            r"\\?\UNC\nas\share",
        ] {
            assert!(usable_directory(Path::new(folder), unchecked), "{folder}");
        }
        assert!(usable_directory(Path::new(r"C:\Users\Me"), |_| true));
        assert!(!usable_directory(Path::new(r"C:\gone"), |_| false));
        assert!(!usable_directory(Path::new(r"\\?\C:\gone"), |_| false));
    }

    #[test]
    fn output_batches_come_at_least_an_interval_apart() {
        let now = Instant::now();
        assert_eq!(
            output_delay(None, now),
            Duration::ZERO,
            "the first batch is taken at once"
        );
        assert_eq!(
            output_delay(Some(now), now + Duration::from_millis(4)),
            OUTPUT_INTERVAL - Duration::from_millis(4)
        );
        assert_eq!(
            output_delay(Some(now), now + OUTPUT_INTERVAL),
            Duration::ZERO
        );
        assert_eq!(
            output_delay(Some(now), now + Duration::from_secs(1)),
            Duration::ZERO
        );
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(Duration::from_millis(42)), "42 ms");
        assert_eq!(duration(Duration::from_millis(1_250)), "1.2 s");
        assert_eq!(duration(Duration::from_secs(125)), "2 min 5 s");
    }
}
