use super::*;
use windows::Win32::UI::{
    Controls::{ODA_DRAWENTIRE, ODT_BUTTON},
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const PRINT_SUBCLASS: usize = 2;

pub(super) fn install(button: HWND) -> windows::core::Result<()> {
    unsafe { SetWindowSubclass(button, Some(button_proc), PRINT_SUBCLASS, 0).ok() }
}

/// The themed native button does not print owner-drawn content itself. Forward
/// its print request through the same drawing path used on screen.
unsafe fn print_button(button: HWND, context: HDC) -> windows::core::Result<()> {
    let parent = GetParent(button)?;
    let mut area = RECT::default();
    GetClientRect(button, &mut area)?;
    let identifier = GetDlgCtrlID(button) as u32;
    let item = DRAWITEMSTRUCT {
        CtlType: ODT_BUTTON,
        CtlID: identifier,
        itemAction: ODA_DRAWENTIRE,
        itemState: if GetFocus() == button {
            ODS_FOCUS
        } else {
            Default::default()
        },
        hwndItem: button,
        hDC: context,
        rcItem: area,
        ..Default::default()
    };
    SendMessageW(
        parent,
        WM_DRAWITEM,
        Some(WPARAM(identifier as usize)),
        Some(LPARAM((&item as *const DRAWITEMSTRUCT) as isize)),
    );
    Ok(())
}

unsafe extern "system" fn button_proc(
    button: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
    _subclass: usize,
    _reference: usize,
) -> LRESULT {
    if message == WM_PRINTCLIENT {
        match print_button(button, HDC(word.0 as *mut _)) {
            Ok(()) => return LRESULT(0),
            Err(error) => eprintln!("Could not capture a setup button: {error}"),
        }
    }
    if message == WM_NCDESTROY
        && !RemoveWindowSubclass(button, Some(button_proc), PRINT_SUBCLASS).as_bool()
    {
        eprintln!("Could not remove setup button capture handler");
    }
    DefSubclassProc(button, message, word, long)
}
