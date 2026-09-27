//! Running commands from Core: `/command` runs in Core's own terminal, and the same command can
//! open in a terminal window or as administrator. `@run` opens anything the Windows Run dialog
//! accepts.
mod capture;
mod environment;
mod jsonc;
mod launch;
mod shells;
pub mod terminal;

pub use capture::{CommandRun, Outcome, TerminalInput, COMMAND_OUTPUT};
pub use environment::home;
pub use launch::{open_run_target, open_terminal, run_elevated};
pub use shells::{capture_invocation, resolve, ResolvedShell};
