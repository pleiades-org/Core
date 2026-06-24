use crate::{
    command::{CommandCategory, CommandResult, FeatureAction, WindowManagementCommand},
    search_text::normalize_search_text,
};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

const WINDOW_POSITION_FILE_NAME: &str = "window_position.toml";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct SavedWindowRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

pub fn search_window_commands(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    if normalized_search_text.is_empty() {
        return window_command_results();
    }

    window_command_results()
        .into_iter()
        .filter(|result| {
            normalize_search_text(&result.title).contains(&normalized_search_text)
                || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
        })
        .collect()
}

pub fn execute_window_command(
    command: &WindowManagementCommand,
    target_window_handle: Option<isize>,
) -> io::Result<()> {
    platform::execute_window_command(command, target_window_handle)
}

pub fn active_window_handle() -> Option<isize> {
    platform::active_window_handle()
}

fn window_command_results() -> Vec<CommandResult> {
    [
        (
            "Snap window left",
            "Resize active window to the left half",
            WindowManagementCommand::LeftHalf,
            94,
        ),
        (
            "Snap window right",
            "Resize active window to the right half",
            WindowManagementCommand::RightHalf,
            94,
        ),
        (
            "Snap window top",
            "Resize active window to the top half",
            WindowManagementCommand::TopHalf,
            90,
        ),
        (
            "Snap window bottom",
            "Resize active window to the bottom half",
            WindowManagementCommand::BottomHalf,
            90,
        ),
        (
            "Snap window top left",
            "Resize active window to the top-left quarter",
            WindowManagementCommand::TopLeftQuarter,
            88,
        ),
        (
            "Snap window top right",
            "Resize active window to the top-right quarter",
            WindowManagementCommand::TopRightQuarter,
            88,
        ),
        (
            "Snap window bottom left",
            "Resize active window to the bottom-left quarter",
            WindowManagementCommand::BottomLeftQuarter,
            88,
        ),
        (
            "Snap window bottom right",
            "Resize active window to the bottom-right quarter",
            WindowManagementCommand::BottomRightQuarter,
            88,
        ),
        (
            "Move window left third",
            "Resize active window to the left third",
            WindowManagementCommand::LeftThird,
            84,
        ),
        (
            "Move window center third",
            "Resize active window to the center third",
            WindowManagementCommand::CenterThird,
            84,
        ),
        (
            "Move window right third",
            "Resize active window to the right third",
            WindowManagementCommand::RightThird,
            84,
        ),
        (
            "Maximize window",
            "Maximize active window",
            WindowManagementCommand::Maximize,
            86,
        ),
        (
            "Center window",
            "Center active window without resizing",
            WindowManagementCommand::Center,
            82,
        ),
        (
            "Move window to next display",
            "Move active window to the next monitor",
            WindowManagementCommand::MoveToNextDisplay,
            80,
        ),
        (
            "Move window to previous display",
            "Move active window to the previous monitor",
            WindowManagementCommand::MoveToPreviousDisplay,
            80,
        ),
        (
            "Restore previous window position",
            "Restore the last position changed by Core Launcher",
            WindowManagementCommand::RestorePreviousPosition,
            78,
        ),
    ]
    .into_iter()
    .map(|(title, subtitle, window_command, confidence)| {
        CommandResult::feature(
            title,
            subtitle,
            CommandCategory::WindowManagement,
            FeatureAction::WindowManagement(window_command),
            confidence,
        )
    })
    .collect()
}

fn save_previous_window_rect(rect: SavedWindowRect) -> io::Result<()> {
    let position_path = window_position_file_path();
    if let Some(position_directory) = position_path.parent() {
        fs::create_dir_all(position_directory)?;
    }

    let position_text = toml::to_string_pretty(&rect).unwrap_or_default();
    fs::write(position_path, position_text)
}

fn load_previous_window_rect() -> Option<SavedWindowRect> {
    fs::read_to_string(window_position_file_path())
        .ok()
        .and_then(|position_text| toml::from_str(&position_text).ok())
}

fn window_position_file_path() -> PathBuf {
    crate::paths::data_file(WINDOW_POSITION_FILE_NAME)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        load_previous_window_rect, save_previous_window_rect, SavedWindowRect,
        WindowManagementCommand,
    };
    use std::io;
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::{HWND, LPARAM, RECT},
            Graphics::Gdi::{
                EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR,
                MONITORINFO, MONITOR_DEFAULTTONEAREST,
            },
            UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowRect, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
                SWP_NOZORDER, SW_MAXIMIZE, SW_RESTORE,
            },
        },
    };

    #[derive(Clone, Copy)]
    struct WorkArea {
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    }

    pub fn active_window_handle() -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.0.is_null()).then_some(hwnd.0 as isize)
    }

    pub fn execute_window_command(
        command: &WindowManagementCommand,
        target_window_handle: Option<isize>,
    ) -> io::Result<()> {
        let hwnd = target_window_handle
            .map(|handle| HWND(handle as *mut std::ffi::c_void))
            .or_else(|| active_window_handle().map(|handle| HWND(handle as *mut std::ffi::c_void)))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No active window found."))?;

        match command {
            WindowManagementCommand::RestorePreviousPosition => restore_previous_window_rect(hwnd),
            WindowManagementCommand::Maximize => {
                save_current_window_rect(hwnd)?;
                unsafe {
                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                }
                Ok(())
            }
            WindowManagementCommand::Center => {
                let work_area = work_area_for_window(hwnd)?;
                let current_rect = current_window_rect(hwnd)?;
                save_previous_window_rect(current_rect)?;
                let width = current_rect.right - current_rect.left;
                let height = current_rect.bottom - current_rect.top;
                let left = work_area.left + (work_area.width - width) / 2;
                let top = work_area.top + (work_area.height - height) / 2;
                set_window_rect(hwnd, left, top, width, height)
            }
            WindowManagementCommand::MoveToNextDisplay => move_to_adjacent_display(hwnd, 1),
            WindowManagementCommand::MoveToPreviousDisplay => move_to_adjacent_display(hwnd, -1),
            _ => {
                let work_area = work_area_for_window(hwnd)?;
                let target_rect = target_rect_for_command(command, work_area);
                save_current_window_rect(hwnd)?;
                set_window_rect(
                    hwnd,
                    target_rect.left,
                    target_rect.top,
                    target_rect.right - target_rect.left,
                    target_rect.bottom - target_rect.top,
                )
            }
        }
    }

    fn target_rect_for_command(
        command: &WindowManagementCommand,
        work_area: WorkArea,
    ) -> SavedWindowRect {
        let half_width = work_area.width / 2;
        let half_height = work_area.height / 2;
        let third_width = work_area.width / 3;

        match command {
            WindowManagementCommand::LeftHalf => {
                rect(work_area.left, work_area.top, half_width, work_area.height)
            }
            WindowManagementCommand::RightHalf => rect(
                work_area.left + half_width,
                work_area.top,
                work_area.width - half_width,
                work_area.height,
            ),
            WindowManagementCommand::TopHalf => {
                rect(work_area.left, work_area.top, work_area.width, half_height)
            }
            WindowManagementCommand::BottomHalf => rect(
                work_area.left,
                work_area.top + half_height,
                work_area.width,
                work_area.height - half_height,
            ),
            WindowManagementCommand::TopLeftQuarter => {
                rect(work_area.left, work_area.top, half_width, half_height)
            }
            WindowManagementCommand::TopRightQuarter => rect(
                work_area.left + half_width,
                work_area.top,
                work_area.width - half_width,
                half_height,
            ),
            WindowManagementCommand::BottomLeftQuarter => rect(
                work_area.left,
                work_area.top + half_height,
                half_width,
                work_area.height - half_height,
            ),
            WindowManagementCommand::BottomRightQuarter => rect(
                work_area.left + half_width,
                work_area.top + half_height,
                work_area.width - half_width,
                work_area.height - half_height,
            ),
            WindowManagementCommand::LeftThird => {
                rect(work_area.left, work_area.top, third_width, work_area.height)
            }
            WindowManagementCommand::CenterThird => rect(
                work_area.left + third_width,
                work_area.top,
                third_width,
                work_area.height,
            ),
            WindowManagementCommand::RightThird => rect(
                work_area.left + third_width * 2,
                work_area.top,
                work_area.width - third_width * 2,
                work_area.height,
            ),
            _ => rect(
                work_area.left,
                work_area.top,
                work_area.width,
                work_area.height,
            ),
        }
    }

    fn move_to_adjacent_display(hwnd: HWND, direction: isize) -> io::Result<()> {
        let monitors = available_monitor_work_areas();
        if monitors.len() <= 1 {
            return Ok(());
        }

        let current_work_area = work_area_for_window(hwnd)?;
        let current_index = monitors
            .iter()
            .position(|monitor| {
                monitor.left == current_work_area.left && monitor.top == current_work_area.top
            })
            .unwrap_or(0);
        let next_index = (current_index as isize + direction).rem_euclid(monitors.len() as isize);
        let target_work_area = monitors[next_index as usize];
        let current_rect = current_window_rect(hwnd)?;
        save_previous_window_rect(current_rect)?;

        let width = current_rect.right - current_rect.left;
        let height = current_rect.bottom - current_rect.top;
        let offset_x = current_rect.left - current_work_area.left;
        let offset_y = current_rect.top - current_work_area.top;
        let left = target_work_area.left + offset_x.min(target_work_area.width - width);
        let top = target_work_area.top + offset_y.min(target_work_area.height - height);

        set_window_rect(hwnd, left, top, width, height)
    }

    fn restore_previous_window_rect(hwnd: HWND) -> io::Result<()> {
        let previous_rect = load_previous_window_rect()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No saved window position."))?;
        set_window_rect(
            hwnd,
            previous_rect.left,
            previous_rect.top,
            previous_rect.right - previous_rect.left,
            previous_rect.bottom - previous_rect.top,
        )
    }

    fn save_current_window_rect(hwnd: HWND) -> io::Result<()> {
        save_previous_window_rect(current_window_rect(hwnd)?)
    }

    fn current_window_rect(hwnd: HWND) -> io::Result<SavedWindowRect> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }
            .map_err(|error| io::Error::other(error.to_string()))?;

        Ok(SavedWindowRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    fn work_area_for_window(hwnd: HWND) -> io::Result<WorkArea> {
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        monitor_work_area(monitor)
    }

    fn monitor_work_area(monitor: HMONITOR) -> io::Result<WorkArea> {
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
            return Err(io::Error::last_os_error());
        }

        let rect = monitor_info.rcWork;
        Ok(WorkArea {
            left: rect.left,
            top: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }

    fn available_monitor_work_areas() -> Vec<WorkArea> {
        let mut monitors = Vec::<HMONITOR>::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(monitor_enum_proc),
                LPARAM(&mut monitors as *mut _ as isize),
            );
        }

        let mut work_areas = monitors
            .into_iter()
            .filter_map(|monitor| monitor_work_area(monitor).ok())
            .collect::<Vec<_>>();
        work_areas.sort_by_key(|work_area| (work_area.left, work_area.top));
        work_areas
    }

    unsafe extern "system" fn monitor_enum_proc(
        monitor: HMONITOR,
        _device_context: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = data.0 as *mut Vec<HMONITOR>;
        unsafe {
            (*monitors).push(monitor);
        }
        BOOL(1)
    }

    fn set_window_rect(hwnd: HWND, left: i32, top: i32, width: i32, height: i32) -> io::Result<()> {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            SetWindowPos(
                hwnd,
                None,
                left,
                top,
                width.max(1),
                height.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| io::Error::other(error.to_string()))
    }

    fn rect(left: i32, top: i32, width: i32, height: i32) -> SavedWindowRect {
        SavedWindowRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::command::WindowManagementCommand;
    use std::io;

    pub fn active_window_handle() -> Option<isize> {
        None
    }

    pub fn execute_window_command(
        _command: &WindowManagementCommand,
        _target_window_handle: Option<isize>,
    ) -> io::Result<()> {
        Ok(())
    }
}
