use crate::windows::{painting, theme::Palette};
use windows::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SCROLLBAR_STYLE: usize = 2;

/// Keep native scrollbar input/accessibility, but draw it using Core's palette.
pub fn install(window: HWND) -> windows::core::Result<()> {
    unsafe { SetWindowSubclass(window, Some(scrollbar_proc), SCROLLBAR_STYLE, 0).ok() }
}

pub fn update(window: HWND, palette: Palette) -> windows::core::Result<()> {
    unsafe {
        if GetPropW(window, w!("Core.ScrollBackground")).0 as usize
            == palette.background.0 as usize + 1
            && GetPropW(window, w!("Core.ScrollThumb")).0 as usize
                == palette.secondary.0 as usize + 1
        {
            return Ok(());
        }
        SetPropW(
            window,
            w!("Core.ScrollBackground"),
            Some(HANDLE((palette.background.0 as usize + 1) as *mut _)),
        )?;
        SetPropW(
            window,
            w!("Core.ScrollThumb"),
            Some(HANDLE((palette.secondary.0 as usize + 1) as *mut _)),
        )?;
        let _ = InvalidateRect(Some(window), None, false);
    }
    Ok(())
}

unsafe fn draw(window: HWND, context: HDC) {
    let mut area = RECT::default();
    if GetClientRect(window, &mut area).is_err() {
        return;
    }
    let background = COLORREF(
        (GetPropW(window, w!("Core.ScrollBackground")).0 as usize).saturating_sub(1) as u32,
    );
    let foreground =
        COLORREF((GetPropW(window, w!("Core.ScrollThumb")).0 as usize).saturating_sub(1) as u32);
    painting::fill(context, &area, background);
    let mut info = SCROLLBARINFO {
        cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    if GetScrollBarInfo(window, OBJID_CLIENT, &mut info).is_ok() {
        let inset = (area.right / 3).max(1);
        let thumb = RECT {
            left: inset,
            right: area.right - inset,
            top: info.xyThumbTop,
            bottom: info.xyThumbBottom,
        };
        painting::rounded(context, &thumb, inset, foreground);
        // Small end markers correspond to the native line-up/down hit areas.
        for top in [info.dxyLineButton / 2, area.bottom - info.dxyLineButton / 2] {
            let marker = RECT {
                left: inset,
                right: area.right - inset,
                top,
                bottom: top + 1,
            };
            painting::fill(context, &marker, foreground);
        }
    }
}

unsafe extern "system" fn scrollbar_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _identifier: usize,
    _reference: usize,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let context = BeginPaint(window, &mut paint);
            draw(window, context);
            let _ = EndPaint(window, &paint);
            return LRESULT(0);
        }
        WM_PRINTCLIENT => {
            draw(window, HDC(word.0 as *mut _));
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_NCDESTROY => {
            let _ = RemovePropW(window, w!("Core.ScrollBackground"));
            let _ = RemovePropW(window, w!("Core.ScrollThumb"));
            if !RemoveWindowSubclass(window, Some(scrollbar_proc), SCROLLBAR_STYLE).as_bool() {
                eprintln!("Could not remove quicklink scrollbar styling");
            }
        }
        _ => {}
    }
    DefSubclassProc(window, message, word, data)
}
