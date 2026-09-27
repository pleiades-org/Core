use windows::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::Gdi::InvalidateRect,
        UI::{
            Controls::WM_MOUSELEAVE,
            Input::KeyboardAndMouse::{TrackMouseEvent, TME_CANCEL, TME_LEAVE, TRACKMOUSEEVENT},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const HOVER_SUBCLASS: usize = 1;

/// Track pointer entry/exit through native events; no polling or animation timer.
pub fn install(button: HWND) -> windows::core::Result<()> {
    unsafe { SetWindowSubclass(button, Some(button_proc), HOVER_SUBCLASS, 0).ok() }
}

pub fn is_hovered(button: HWND) -> bool {
    !unsafe { GetPropW(button, w!("Core.ButtonHovered")) }
        .0
        .is_null()
}

fn enter(button: HWND) -> windows::core::Result<()> {
    let mut tracking = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: button,
        ..Default::default()
    };
    unsafe {
        TrackMouseEvent(&mut tracking)?;
        SetPropW(
            button,
            w!("Core.ButtonHovered"),
            Some(HANDLE(std::ptr::without_provenance_mut(1))),
        )?;
        let _ = InvalidateRect(Some(button), None, false);
    }
    Ok(())
}

fn leave(button: HWND) -> windows::core::Result<()> {
    if !is_hovered(button) {
        return Ok(());
    }
    unsafe {
        RemovePropW(button, w!("Core.ButtonHovered"))?;
        let _ = InvalidateRect(Some(button), None, false);
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_CANCEL | TME_LEAVE,
            hwndTrack: button,
            ..Default::default()
        };
        TrackMouseEvent(&mut tracking)?;
    }
    Ok(())
}

unsafe extern "system" fn button_proc(
    button: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    _reference: usize,
) -> LRESULT {
    let result = match message {
        WM_MOUSEMOVE if !is_hovered(button) => enter(button),
        WM_MOUSELEAVE | WM_CANCELMODE | WM_NCDESTROY => leave(button),
        WM_SHOWWINDOW | WM_ENABLE if word.0 == 0 => leave(button),
        _ => Ok(()),
    };
    if let Err(error) = result {
        eprintln!("Could not update icon hover: {error}");
    }
    if message == WM_NCDESTROY
        && !RemoveWindowSubclass(button, Some(button_proc), HOVER_SUBCLASS).as_bool()
    {
        eprintln!("Could not remove icon hover subclass");
    }
    DefSubclassProc(button, message, word, data)
}
