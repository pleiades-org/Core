mod activation;
mod app_aliases;
mod app_icon;
mod application_icon;
mod button_hover;
mod commands;
mod diagnostics;
mod discover_applications;
mod displays;
mod exchange_rates;
mod execute_action;
mod favicon;
mod foreground_observer;
mod http;
mod icon_worker;
mod launcher_state;
mod local_cache;
mod motion;
mod packaged_applications;
mod painting;
mod power;
mod power_menu;
mod recent_applications;
mod recent_list;
mod search_layout;
mod search_worker;
mod settings;
mod shell;
mod taskbar;
mod theme;
mod time_converter;
mod tray;
mod updates;
mod user_assist;
mod view;
mod window_placement;

pub use shell::{run, show_fatal_error};

/// Tests that count process-wide USER/GDI handles, or create threads that allocate them,
/// hold this lock so parallel tests cannot skew the counts.
#[cfg(test)]
pub static GUI_RESOURCE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
