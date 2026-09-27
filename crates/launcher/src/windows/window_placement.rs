use super::settings::ScreenPosition;
use windows::Win32::{Foundation::*, Graphics::Gdi::*};

/// A display's full bounds and its taskbar-free work area, in physical pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScreenArea {
    pub monitor: RECT,
    pub work: RECT,
}

impl ScreenArea {
    /// Zero spacing keeps Core inside the work area, clear of the taskbar. Any other spacing is
    /// measured from the physical screen edge, so a large enough gap can clear (or overlap) it.
    pub fn placement(self, spacing: i32) -> RECT {
        if spacing <= 0 {
            return self.work;
        }
        let area = self.monitor;
        // Always leave at least one pixel so tiny displays cannot produce an inverted rectangle.
        let horizontal = spacing.min((area.right - area.left - 1) / 2).max(0);
        let vertical = spacing.min((area.bottom - area.top - 1) / 2).max(0);
        RECT {
            left: area.left + horizontal,
            top: area.top + vertical,
            right: area.right - horizontal,
            bottom: area.bottom - vertical,
        }
    }

    /// The edges that square Core's corners. Spaced windows float, so no corner touches an edge.
    pub fn edges(self, spacing: i32) -> RECT {
        if spacing <= 0 {
            self.work
        } else {
            self.monitor
        }
    }
}

pub fn screen_area(window: HWND) -> windows::core::Result<ScreenArea> {
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err(windows::core::Error::from_win32());
    }
    Ok(ScreenArea {
        monitor: info.rcMonitor,
        work: info.rcWork,
    })
}

pub fn bounds(area: RECT, width: i32, height: i32, position: ScreenPosition) -> RECT {
    let width = width.min(area.right - area.left).max(1);
    let height = height.min(area.bottom - area.top).max(1);
    let left = match position {
        ScreenPosition::Left | ScreenPosition::BottomLeft => area.left,
        ScreenPosition::Right | ScreenPosition::BottomRight => area.right - width,
        _ => area.left + (area.right - area.left - width) / 2,
    };
    let top = match position {
        ScreenPosition::Top => area.top,
        ScreenPosition::Bottom | ScreenPosition::BottomLeft | ScreenPosition::BottomRight => {
            area.bottom - height
        }
        _ => area.top + (area.bottom - area.top - height) / 2,
    };
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// Everything the window's clipping region depends on. Moving the window without changing
/// its size or which corners touch a screen edge keeps the same shape, so the region is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipShape {
    width: i32,
    height: i32,
    radius: i32,
    corners: [bool; 4],
}

impl ClipShape {
    /// `radius` is in physical pixels. Zero produces a plain rectangle.
    pub fn new(bounds: RECT, edges: RECT, radius: i32) -> Self {
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let radius = radius.min(width / 2).min(height / 2).max(0);
        Self {
            width,
            height,
            radius,
            corners: if radius == 0 {
                [false; 4]
            } else {
                attached_corners(bounds, edges)
            },
        }
    }
}

pub fn clip_to_edges(window: HWND, shape: ClipShape) -> windows::core::Result<()> {
    let ClipShape {
        width,
        height,
        radius,
        corners,
    } = shape;
    let region = Region::new(unsafe {
        if radius == 0 {
            CreateRectRgn(0, 0, width, height)
        } else {
            CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2)
        }
    })?;
    for (attached, left, top) in [
        (corners[0], 0, 0),
        (corners[1], width - radius, 0),
        (corners[2], 0, height - radius),
        (corners[3], width - radius, height - radius),
    ] {
        if !attached {
            continue;
        }
        let corner =
            Region::new(unsafe { CreateRectRgn(left, top, left + radius + 1, top + radius + 1) })?;
        if unsafe { CombineRgn(Some(region.0), Some(region.0), Some(corner.0), RGN_OR) }
            == RGN_ERROR
        {
            return Err(windows::core::Error::from_win32());
        }
    }
    if unsafe { SetWindowRgn(window, Some(region.0), true) } == 0 {
        return Err(windows::core::Error::from_win32());
    }
    // Windows owns the combined region after a successful SetWindowRgn.
    std::mem::forget(region);
    Ok(())
}

fn attached_corners(bounds: RECT, area: RECT) -> [bool; 4] {
    let left = bounds.left == area.left;
    let right = bounds.right == area.right;
    let top = bounds.top == area.top;
    let bottom = bounds.bottom == area.bottom;
    [left || top, right || top, left || bottom, right || bottom]
}

struct Region(HRGN);
impl Region {
    fn new(region: HRGN) -> windows::core::Result<Self> {
        if region.0.is_null() {
            Err(windows::core::Error::from_win32())
        } else {
            Ok(Self(region))
        }
    }
}
impl Drop for Region {
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
    fn positions_and_attached_corners_follow_work_area_on_negative_coordinate_monitor() {
        let area = RECT {
            left: -1920,
            top: -100,
            right: 0,
            bottom: 940,
        };
        for (position, left, top, corners) in [
            (ScreenPosition::Center, -1280, 270, [false; 4]),
            (ScreenPosition::Top, -1280, -100, [true, true, false, false]),
            (
                ScreenPosition::Bottom,
                -1280,
                640,
                [false, false, true, true],
            ),
            (ScreenPosition::Left, -1920, 270, [true, false, true, false]),
            (ScreenPosition::Right, -640, 270, [false, true, false, true]),
            (
                ScreenPosition::BottomLeft,
                -1920,
                640,
                [true, false, true, true],
            ),
            (
                ScreenPosition::BottomRight,
                -640,
                640,
                [false, true, true, true],
            ),
        ] {
            let placed = bounds(area, 640, 300, position);
            assert_eq!((placed.left, placed.top), (left, top));
            assert_eq!(attached_corners(placed, area), corners);
        }
        let taller = bounds(area, 640, 550, ScreenPosition::Bottom);
        assert_eq!(taller.bottom, area.bottom);
        assert_eq!(taller.top, 390);
        assert_eq!(bounds(area, 4000, 4000, ScreenPosition::Center), area);
    }

    #[test]
    fn edge_spacing_measures_from_the_screen_edge_and_floats_every_corner() {
        let screen = ScreenArea {
            monitor: RECT {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1080,
            },
            work: RECT {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1032,
            },
        };
        assert_eq!(screen.placement(0), screen.work);
        let spaced = screen.placement(60);
        assert_eq!(
            (spaced.left, spaced.top, spaced.right, spaced.bottom),
            (-1860, 60, -60, 1020)
        );
        let placed = bounds(spaced, 640, 300, ScreenPosition::BottomRight);
        assert_eq!((placed.right, placed.bottom), (-60, 1020));
        assert_eq!(attached_corners(placed, screen.edges(60)), [false; 4]);
        let tiny = ScreenArea {
            monitor: RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 50,
            },
            work: RECT::default(),
        }
        .placement(200);
        assert!(tiny.right > tiny.left && tiny.bottom > tiny.top);
    }

    #[test]
    fn clip_shape_changes_only_with_size_radius_or_attached_corners() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let centered = bounds(area, 640, 300, ScreenPosition::Center);
        let shape = ClipShape::new(centered, area, 16);
        // Moving without touching an edge keeps the region.
        let moved = RECT {
            left: centered.left + 40,
            right: centered.right + 40,
            ..centered
        };
        assert_eq!(ClipShape::new(moved, area, 16), shape);
        // Taller (another result), rounder, or now touching the top edge: a new region.
        let taller = bounds(area, 640, 352, ScreenPosition::Center);
        assert_ne!(ClipShape::new(taller, area, 16), shape);
        assert_ne!(ClipShape::new(centered, area, 8), shape);
        let top = bounds(area, 640, 300, ScreenPosition::Top);
        assert_ne!(ClipShape::new(top, area, 16), shape);
        // Square corners ignore edges, and radii beyond half the size clamp to the same shape.
        assert_eq!(
            ClipShape::new(top, area, 0),
            ClipShape::new(centered, area, 0)
        );
        assert_eq!(
            ClipShape::new(centered, area, 500),
            ClipShape::new(centered, area, 150)
        );
    }
}
