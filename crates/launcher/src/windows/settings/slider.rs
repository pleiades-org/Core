use crate::windows::{
    painting,
    theme::{scale, Palette},
};
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::InvalidateRect,
    UI::{
        Controls::{
            InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDIS_HOT, CDIS_SELECTED,
            CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_SKIPDEFAULT, ICC_BAR_CLASSES,
            INITCOMMONCONTROLSEX, NMCUSTOMDRAW, TBCD_CHANNEL, TBCD_THUMB, TBM_GETTHUMBRECT,
            TBM_SETLINESIZE, TBM_SETPAGESIZE, TBM_SETPOS, TBM_SETRANGEMAX, TBM_SETRANGEMIN,
            TB_ENDTRACK, TB_THUMBTRACK,
        },
        Input::KeyboardAndMouse::GetFocus,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{GetClientRect, SendMessageW, WM_NCDESTROY, WM_PAINT, WM_USER},
    },
};

pub use windows::Win32::UI::Controls::{TBS_NOTICKS as STYLE, TRACKBAR_CLASSW as CLASS};

/// Not exported by the `windows` crate; documented as `WM_USER` in CommCtrl.h.
const TBM_GETPOS: u32 = WM_USER;
const REPAINT_SUBCLASS: usize = 8;
const TRACK_THICKNESS: i32 = 2;
const FILLED_THICKNESS: i32 = 4;
const THUMB_DIAMETER: i32 = 16;
const FOCUS_RING: i32 = 2;

/// What a slider notification means for settings persistence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideStage {
    /// The thumb is being dragged; the value is not final.
    Dragging,
    /// Keyboard or click steps: preview now, save after a short pause.
    Stepped,
    /// The user released the mouse or key.
    Finished,
}

impl SlideStage {
    pub fn from_code(code: u32) -> Self {
        match code {
            TB_THUMBTRACK => Self::Dragging,
            TB_ENDTRACK => Self::Finished,
            _ => Self::Stepped,
        }
    }
}

/// Registers the trackbar window class. Safe to call more than once.
pub fn register() -> windows::core::Result<()> {
    let classes = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES,
    };
    if unsafe { InitCommonControlsEx(&classes) }.as_bool() {
        Ok(())
    } else {
        Err(windows::core::Error::from_win32())
    }
}

pub fn configure(control: HWND, maximum: u32, page: u32) -> windows::core::Result<()> {
    unsafe {
        SetWindowSubclass(control, Some(repaint_proc), REPAINT_SUBCLASS, 0).ok()?;
        SendMessageW(control, TBM_SETRANGEMIN, Some(WPARAM(0)), Some(LPARAM(0)));
        SendMessageW(
            control,
            TBM_SETRANGEMAX,
            Some(WPARAM(1)),
            Some(LPARAM(maximum as isize)),
        );
        SendMessageW(control, TBM_SETLINESIZE, None, Some(LPARAM(1)));
        SendMessageW(control, TBM_SETPAGESIZE, None, Some(LPARAM(page as isize)));
    }
    Ok(())
}

/// Makes every repaint of a slider a whole one. Windows repaints only the area of its own
/// thumb as that moves or changes state, and the thumb Core draws is wider and, with its
/// ring, taller: what lay outside stayed behind as arcs, and the new thumb showed cut off.
/// So before the slider paints, all of it is marked as needing to.
unsafe extern "system" fn repaint_proc(
    control: HWND,
    message: u32,
    word: WPARAM,
    data: LPARAM,
    _subclass: usize,
    _reference: usize,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let _ = InvalidateRect(Some(control), None, false);
        }
        WM_NCDESTROY
            if !RemoveWindowSubclass(control, Some(repaint_proc), REPAINT_SUBCLASS).as_bool() =>
        {
            eprintln!("Could not remove the slider's repaint subclass");
        }
        _ => {}
    }
    DefSubclassProc(control, message, word, data)
}

pub fn set_position(control: HWND, value: u32) {
    unsafe {
        SendMessageW(
            control,
            TBM_SETPOS,
            Some(WPARAM(1)),
            Some(LPARAM(value as isize)),
        );
    }
}

pub fn position(control: HWND) -> u32 {
    unsafe { SendMessageW(control, TBM_GETPOS, None, None) }
        .0
        .max(0) as u32
}

/// Paints a thin track, a filled portion up to the thumb and a round thumb in the Core palette.
pub fn custom_draw(draw: &NMCUSTOMDRAW, dpi: u32, palette: Palette) -> LRESULT {
    if draw.dwDrawStage == CDDS_PREPAINT {
        // Windows leaves the slider's top rows as they are, and the thumb's ring reaches
        // them: each repaint starts from a slider that is all background.
        let mut client = RECT::default();
        if unsafe { GetClientRect(draw.hdr.hwndFrom, &mut client) }.is_ok() {
            painting::fill(draw.hdc, &client, palette.background);
        }
        return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
    }
    if draw.dwDrawStage != CDDS_ITEMPREPAINT {
        return LRESULT(CDRF_DODEFAULT as isize);
    }
    match draw.dwItemSpec as u32 {
        TBCD_CHANNEL => draw_track(draw, dpi, palette),
        TBCD_THUMB => draw_thumb(draw, dpi, palette),
        _ => {}
    }
    LRESULT(CDRF_SKIPDEFAULT as isize)
}

fn draw_track(draw: &NMCUSTOMDRAW, dpi: u32, palette: Palette) {
    let mut thumb = RECT::default();
    unsafe {
        SendMessageW(
            draw.hdr.hwndFrom,
            TBM_GETTHUMBRECT,
            None,
            Some(LPARAM(&mut thumb as *mut RECT as isize)),
        );
    }
    let middle = (draw.rc.top + draw.rc.bottom) / 2;
    let band = |right: i32, thickness: i32| {
        let half = scale(thickness, dpi).max(1) / 2;
        painting::rectangle(draw.rc.left, middle - half, right, middle + half.max(1))
    };
    let track_radius = scale(TRACK_THICKNESS, dpi) / 2;
    painting::rounded(
        draw.hdc,
        &band(draw.rc.right, TRACK_THICKNESS),
        track_radius,
        palette.secondary,
    );
    painting::rounded(
        draw.hdc,
        &band((thumb.left + thumb.right) / 2, FILLED_THICKNESS),
        scale(FILLED_THICKNESS, dpi) / 2,
        palette.text,
    );
}

fn draw_thumb(draw: &NMCUSTOMDRAW, dpi: u32, palette: Palette) {
    let center_x = (draw.rc.left + draw.rc.right) / 2;
    let center_y = (draw.rc.top + draw.rc.bottom) / 2;
    let circle = |diameter: i32| {
        let half = diameter / 2;
        painting::rectangle(
            center_x - half,
            center_y - half,
            center_x - half + diameter,
            center_y - half + diameter,
        )
    };
    let diameter = scale(THUMB_DIAMETER, dpi);
    let active = unsafe { GetFocus() } == draw.hdr.hwndFrom
        || draw.uItemState.0 & (CDIS_HOT.0 | CDIS_SELECTED.0) != 0;
    if active {
        let ring = diameter + scale(FOCUS_RING, dpi) * 2;
        painting::rounded(draw.hdc, &circle(ring), ring / 2, palette.accent);
    }
    painting::rounded(draw.hdc, &circle(diameter), diameter / 2, palette.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slider is never shown. Its drawing goes to a surface larger than it, as Windows
    /// would hand it when it starts to repaint.
    #[test]
    fn each_repaint_starts_from_a_slider_that_is_all_background() {
        use crate::windows::settings::BackgroundColor;
        use windows::{
            core::w,
            Win32::{
                Graphics::Gdi::*,
                System::LibraryLoader::GetModuleHandleW,
                UI::{Controls::NMHDR, WindowsAndMessaging::*},
            },
        };
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        register().unwrap();
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let window = |class, style, parent| {
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    w!(""),
                    style,
                    0,
                    0,
                    120,
                    30,
                    parent,
                    None,
                    Some(instance),
                    None,
                )
            }
            .unwrap()
        };
        let parent = window(w!("STATIC"), WS_POPUP, None);
        let control = window(CLASS, WS_CHILD | WINDOW_STYLE(STYLE), Some(parent));
        configure(control, 32, 4).unwrap();
        let palette = Palette::for_background(BackgroundColor::parse("#181818").unwrap());
        let white = COLORREF(0x00FF_FFFF);
        unsafe {
            let screen = GetDC(None);
            let context = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 140, 40);
            ReleaseDC(None, screen);
            let previous = SelectObject(context, bitmap.into());
            // What an earlier thumb left behind, anywhere on the slider.
            painting::fill(context, &painting::rectangle(0, 0, 140, 40), white);
            let draw = NMCUSTOMDRAW {
                hdr: NMHDR {
                    hwndFrom: control,
                    ..Default::default()
                },
                dwDrawStage: CDDS_PREPAINT,
                hdc: context,
                ..Default::default()
            };
            assert_eq!(
                custom_draw(&draw, 96, palette).0,
                CDRF_NOTIFYITEMDRAW as isize
            );
            // Its top rows included, which Windows itself does not repaint.
            for (x, y) in [(0, 0), (119, 0), (60, 1), (0, 29), (119, 29)] {
                assert_eq!(GetPixel(context, x, y), palette.background, "({x}, {y})");
            }
            // Nothing beyond the slider is touched.
            for (x, y) in [(120, 0), (0, 30), (139, 39)] {
                assert_eq!(GetPixel(context, x, y), white, "({x}, {y})");
            }
            // The other stages draw the track and the thumb, and erase nothing.
            painting::fill(context, &painting::rectangle(0, 0, 140, 40), white);
            let later = NMCUSTOMDRAW {
                dwDrawStage: CDDS_ITEMPREPAINT,
                dwItemSpec: usize::MAX,
                ..draw
            };
            assert_eq!(
                custom_draw(&later, 96, palette).0,
                CDRF_SKIPDEFAULT as isize
            );
            assert_eq!(GetPixel(context, 60, 1), white);
            SelectObject(context, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(context);
            DestroyWindow(parent).unwrap();
        }
    }

    #[test]
    fn drag_steps_and_release_are_distinguished() {
        assert_eq!(SlideStage::from_code(TB_THUMBTRACK), SlideStage::Dragging);
        assert_eq!(SlideStage::from_code(TB_ENDTRACK), SlideStage::Finished);
        assert_eq!(SlideStage::from_code(0), SlideStage::Stepped);
    }
}
