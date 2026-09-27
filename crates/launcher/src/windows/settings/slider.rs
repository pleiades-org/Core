use crate::windows::{
    painting,
    theme::{scale, Palette},
};
use windows::Win32::{
    Foundation::*,
    UI::{
        Controls::{
            InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDIS_HOT, CDIS_SELECTED,
            CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_SKIPDEFAULT, ICC_BAR_CLASSES,
            INITCOMMONCONTROLSEX, NMCUSTOMDRAW, TBCD_CHANNEL, TBCD_THUMB, TBM_GETTHUMBRECT,
            TBM_SETLINESIZE, TBM_SETPAGESIZE, TBM_SETPOS, TBM_SETRANGEMAX, TBM_SETRANGEMIN,
            TB_ENDTRACK, TB_THUMBTRACK,
        },
        Input::KeyboardAndMouse::GetFocus,
        WindowsAndMessaging::{SendMessageW, WM_USER},
    },
};

pub use windows::Win32::UI::Controls::{TBS_NOTICKS as STYLE, TRACKBAR_CLASSW as CLASS};

/// Not exported by the `windows` crate; documented as `WM_USER` in CommCtrl.h.
const TBM_GETPOS: u32 = WM_USER;
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

pub fn configure(control: HWND, maximum: u32, page: u32) {
    unsafe {
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

    #[test]
    fn drag_steps_and_release_are_distinguished() {
        assert_eq!(SlideStage::from_code(TB_THUMBTRACK), SlideStage::Dragging);
        assert_eq!(SlideStage::from_code(TB_ENDTRACK), SlideStage::Finished);
        assert_eq!(SlideStage::from_code(0), SlideStage::Stepped);
    }
}
