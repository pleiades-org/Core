#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    core_launcher::ui::gpui_app::run();
}
