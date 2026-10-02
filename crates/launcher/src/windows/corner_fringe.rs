//! Anti-aliased edges for Core's rounded corners.
//!
//! A window region keeps or drops whole pixels, so on its own the curve is a staircase. The
//! region keeps only the pixels fully inside the curve; a small window over each rounded corner
//! draws the partly covered rest in the background color, blended over whatever is behind Core.
//! They are owned by Core's window, so they stay above it and are destroyed with it, and they
//! ignore the pointer.

use super::window_placement::{ClipShape, Corner, CornerMask};
use std::cell::Cell;
use windows::{
    core::{w, PCWSTR},
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
};

const CLASS_NAME: PCWSTR = w!("CoreCornerFringe");

/// Where Core is and what its corners look like; the corners are redrawn when this changes.
#[derive(Clone, Copy, PartialEq)]
struct Placement {
    /// Core's window, in screen pixels.
    bounds: RECT,
    shape: ClipShape,
    background: COLORREF,
}

pub struct CornerFringe {
    /// One per corner, in `Corner::ALL` order.
    windows: [HWND; 4],
    placement: Cell<Option<Placement>>,
    /// Core's own opacity, which the fringe follows while it fades.
    opacity: Cell<u8>,
}

impl CornerFringe {
    /// Starts hidden, as Core does, until `set_opacity` shows it.
    pub fn create(owner: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        register_class(instance)?;
        let mut windows = [HWND::default(); 4];
        for window in &mut windows {
            *window = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    CLASS_NAME,
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    Some(owner),
                    None,
                    Some(instance),
                    None,
                )
            }?;
        }
        Ok(Self {
            windows,
            placement: Cell::new(None),
            opacity: Cell::new(0),
        })
    }

    /// Follows Core to `bounds` with its current corner shape and background color.
    pub fn place(
        &self,
        bounds: RECT,
        shape: ClipShape,
        background: COLORREF,
    ) -> windows::core::Result<()> {
        let placement = Placement {
            bounds,
            shape,
            background,
        };
        if self.placement.get() == Some(placement) {
            return Ok(());
        }
        // Forget the old placement first: a failure leaves the corners unknown, so they are
        // drawn again next time.
        self.placement.set(None);
        let mask = CornerMask::new(shape.radius());
        let mut rounded = [false; 4];
        for (corner, origin) in shape.rounded_corners() {
            let index = corner as usize;
            rounded[index] = true;
            let position = POINT {
                x: bounds.left + origin.x,
                y: bounds.top + origin.y,
            };
            draw(
                self.windows[index],
                &mask,
                corner,
                position,
                background,
                self.opacity.get(),
            )?;
        }
        self.placement.set(Some(placement));
        self.show(rounded);
        Ok(())
    }

    /// Matches Core's opacity; at zero, Core is hidden and so are its corners.
    pub fn set_opacity(&self, opacity: u8) {
        if self.opacity.replace(opacity) == opacity {
            return;
        }
        let Some(placement) = self.placement.get() else {
            return;
        };
        let mut rounded = [false; 4];
        for (corner, _) in placement.shape.rounded_corners() {
            rounded[corner as usize] = true;
        }
        if opacity > 0 {
            for (window, _) in self.windows.iter().zip(rounded).filter(|(_, round)| *round) {
                // The drawn pixels are kept; only how strongly they are blended changes.
                if let Err(error) = unsafe {
                    UpdateLayeredWindow(
                        *window,
                        None,
                        None,
                        None,
                        None,
                        None,
                        COLORREF(0),
                        Some(&blend(opacity)),
                        ULW_ALPHA,
                    )
                } {
                    eprintln!("Could not fade Core's corners: {error}");
                }
            }
        }
        self.show(rounded);
    }

    /// Shows the rounded corners while Core is visible; square corners need no fringe.
    fn show(&self, rounded: [bool; 4]) {
        let visible = self.opacity.get() > 0;
        for (window, round) in self.windows.iter().zip(rounded) {
            unsafe {
                let _ = ShowWindow(*window, if visible && round { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }
}

fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    let class = WNDCLASSW {
        lpfnWndProc: Some(fringe_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let error = windows::core::Error::from_win32();
        // Every view registers it; the first registration serves the rest.
        if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    Ok(())
}

unsafe extern "system" fn fringe_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
) -> LRESULT {
    DefWindowProcW(window, message, word, long)
}

fn blend(opacity: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: opacity,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

/// The premultiplied BGRA pixel for `coverage` of the background. Fully covered pixels are
/// Core's own, inside its region, so the fringe leaves them clear.
fn fringe_pixel(background: COLORREF, coverage: u8) -> u32 {
    if coverage == u8::MAX {
        return 0;
    }
    let channel = |shift: u32| {
        let value = (background.0 >> shift) & 0xff;
        (value * u32::from(coverage) + 127) / 255
    };
    // COLORREF is 0x00BBGGRR; the bitmap wants 0xAARRGGBB.
    (u32::from(coverage) << 24) | (channel(0) << 16) | (channel(8) << 8) | channel(16)
}

/// Draws `corner`'s fringe into `window`, a `mask.radius()` square at `position` on screen.
fn draw(
    window: HWND,
    mask: &CornerMask,
    corner: Corner,
    position: POINT,
    background: COLORREF,
    opacity: u8,
) -> windows::core::Result<()> {
    let size = mask.radius();
    let screen = ScreenContext(unsafe { GetDC(None) });
    if screen.0.is_invalid() {
        return Err(windows::core::Error::from_win32());
    }
    let memory = MemoryContext(unsafe { CreateCompatibleDC(Some(screen.0)) });
    if memory.0.is_invalid() {
        return Err(windows::core::Error::from_win32());
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            // Negative: rows run top to bottom, as the mask's do.
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = Bitmap(unsafe {
        CreateDIBSection(Some(memory.0), &info, DIB_RGB_COLORS, &mut bits, None, 0)
    }?);
    let pixels =
        unsafe { std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize) };
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let (x, y) = (index as i32 % size, index as i32 / size);
        *pixel = fringe_pixel(background, mask.coverage(corner, x, y));
    }
    let previous = unsafe { SelectObject(memory.0, bitmap.0.into()) };
    let result = unsafe {
        UpdateLayeredWindow(
            window,
            Some(screen.0),
            Some(&position),
            Some(&SIZE { cx: size, cy: size }),
            Some(memory.0),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&blend(opacity)),
            ULW_ALPHA,
        )
    };
    unsafe {
        SelectObject(memory.0, previous);
    }
    result
}

struct ScreenContext(HDC);
impl Drop for ScreenContext {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(None, self.0);
        }
    }
}

struct MemoryContext(HDC);
impl Drop for MemoryContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fringe_pixels_are_premultiplied_background_and_leave_covered_pixels_to_core() {
        let background = COLORREF(0x00_30_20_10);
        assert_eq!(fringe_pixel(background, 0), 0);
        assert_eq!(fringe_pixel(background, u8::MAX), 0);
        assert_eq!(fringe_pixel(background, 254), 0xfe_10_20_30);
        assert_eq!(fringe_pixel(COLORREF(0x00_ff_ff_ff), 128), 0x80_80_80_80);
    }
}
