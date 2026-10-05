mod activation;
mod app_aliases;
mod app_icon;
mod application_icon;
mod button_hover;
mod commands;
mod corner_fringe;
mod diagnostics;
mod discover_applications;
mod displays;
mod exchange_rates;
mod execute_action;
mod favicon;
mod foreground_observer;
mod http;
mod icon_worker;
pub mod installer;
mod launcher_state;
mod local_cache;
mod media;
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
mod spotify;
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

/// A test thread's stay in Windows' runtime, which it leaves again when dropped. Declare it
/// first in a test, so everything obtained from the runtime is released before it.
///
/// The first test to enter also starts a thread that stays inside for as long as the test
/// process runs. Without it each test left the runtime as it ended, and whenever no thread was
/// inside, Windows unloaded classes the `windows` crate still held on to: the next test to use
/// one died with an access violation. Core itself is always inside on its window's thread, so
/// only tests were affected.
#[cfg(test)]
pub struct TestRuntime(bool);

#[cfg(test)]
impl TestRuntime {
    pub fn enter() -> Self {
        use ::windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        static KEPT: std::sync::Once = std::sync::Once::new();
        KEPT.call_once(|| {
            let (entered, inside) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
                let _ = entered.send(());
                loop {
                    std::thread::park();
                }
            });
            // Entered before this test goes on, so there is never a moment with no one inside.
            let _ = inside.recv();
        });
        Self(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok())
    }
}

#[cfg(test)]
impl Drop for TestRuntime {
    fn drop(&mut self) {
        if self.0 {
            unsafe { ::windows::Win32::System::WinRT::RoUninitialize() };
        }
    }
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
