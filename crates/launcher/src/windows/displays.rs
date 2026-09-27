use super::{settings::DisplayChoice, window_placement::ScreenArea};
use windows::Win32::{Foundation::*, Graphics::Gdi::*};

pub struct Display {
    pub choice: DisplayChoice,
    pub label: String,
    pub screen: ScreenArea,
}

pub fn connected() -> windows::core::Result<Vec<Display>> {
    let mut displays = Vec::<Display>::new();
    if !unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM((&mut displays as *mut Vec<Display>) as isize),
        )
    }
    .as_bool()
    {
        return Err(windows::core::Error::from_win32());
    }
    displays.sort_by_key(|display| display.choice.to_string());
    Ok(displays)
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    context: LPARAM,
) -> windows::core::BOOL {
    let mut information = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut information.monitorInfo) }.as_bool() {
        return false.into();
    }
    let name = String::from_utf16_lossy(
        &information.szDevice[..information
            .szDevice
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(32)],
    );
    let dimensions = information.monitorInfo.rcMonitor;
    let primary = if information.monitorInfo.dwFlags & 1 != 0 {
        " · primary"
    } else {
        ""
    };
    let display = Display {
        choice: DisplayChoice::Device(information.szDevice),
        label: format!(
            "{} · {}×{}{primary}",
            name.trim_start_matches(r"\\.\"),
            dimensions.right - dimensions.left,
            dimensions.bottom - dimensions.top
        ),
        screen: ScreenArea {
            monitor: dimensions,
            work: information.monitorInfo.rcWork,
        },
    };
    unsafe { &mut *(context.0 as *mut Vec<Display>) }.push(display);
    true.into()
}

pub fn screen_area(choice: DisplayChoice, reference: HWND) -> windows::core::Result<ScreenArea> {
    if choice != DisplayChoice::Active {
        if let Some(display) = connected()?
            .into_iter()
            .find(|display| display.choice == choice)
        {
            return Ok(display.screen);
        }
    }
    // A disconnected chosen screen falls back to the active one without losing the saved choice.
    super::window_placement::screen_area(reference)
}
