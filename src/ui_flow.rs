//! UI flow entry points for the launcher.
//!
//! Graph / documentation anchors:
//! - `track_accept_result` — user confirms a search result
//! - `track_save_settings` — settings are persisted
//! - `track_open_settings` — settings panel is opened
//!
//! ```text
//! search input -> accept_result -> action_executor / built-in handlers
//! settings panel -> save_settings -> config.toml (+ credential store for secrets)
//! tray / command -> open_settings -> settings panel
//! ```

use crate::command::CommandResult;

/// UI flow entry point: user accepts a launcher search result.
pub fn track_accept_result(selected_result: &CommandResult) {
    let _ = selected_result;
}

/// UI flow entry point: launcher settings are saved to disk.
pub fn track_save_settings() {}

/// UI flow entry point: the settings panel is opened.
pub fn track_open_settings() {}

/// UI flow entry point: a command result action is executed.
pub fn track_execute_result(result: &CommandResult) {
    let _ = result;
}