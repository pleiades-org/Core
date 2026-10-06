use super::settings::ScreenPosition;
use windows::Win32::{Foundation::*, Graphics::Gdi::*};

/// A display's full bounds and its taskbar-free work area, in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

/// A corner of the window, in the order `ClipShape` stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    pub fn is_left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }

    pub fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight)
    }
}

/// Sub-rows sampled per pixel row; across each one the curve's coverage is exact.
const MASK_SUBROWS: u16 = 16;

/// How much of each pixel in a rounded corner's square lies inside the curve, from 0 (outside)
/// to 255 (fully inside). A window region keeps or drops whole pixels, so it holds only the
/// fully inside ones; the partly covered rest is the anti-aliased fringe drawn over the desktop.
pub struct CornerMask {
    radius: i32,
    /// The top-left square, row by row; the other corners mirror it.
    coverage: Vec<u8>,
}

impl CornerMask {
    /// `radius` is in physical pixels.
    pub fn new(radius: i32) -> Self {
        let radius = radius.max(0);
        let size = radius as usize;
        let curve = radius as f32;
        let mut coverage = Vec::with_capacity(size * size);
        for row in 0..radius {
            // Where the curve crosses each sub-row: everything right of it is inside.
            let crossings: Vec<f32> = (0..MASK_SUBROWS)
                .map(|sample| {
                    let height = row as f32 + (f32::from(sample) + 0.5) / f32::from(MASK_SUBROWS);
                    let above_center = curve - height;
                    curve - (curve * curve - above_center * above_center).max(0.).sqrt()
                })
                .collect();
            coverage.extend((0..radius).map(|column| {
                let inside: f32 = crossings
                    .iter()
                    .map(|crossing| (column as f32 + 1. - crossing).clamp(0., 1.))
                    .sum();
                (inside / f32::from(MASK_SUBROWS) * 255.).round() as u8
            }));
        }
        Self { radius, coverage }
    }

    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// Coverage at `x`, `y` within `corner`'s square, both from its top-left pixel.
    pub fn coverage(&self, corner: Corner, x: i32, y: i32) -> u8 {
        let last = self.radius - 1;
        let column = if corner.is_left() { x } else { last - x };
        let row = if corner.is_top() { y } else { last - y };
        self.coverage[(row * self.radius + column) as usize]
    }

    /// Pixels of a top-left row, counted from the window's edge, that are not fully inside.
    /// Coverage only grows toward the window's inside, so they are all at the start.
    fn outside(&self, row: i32) -> i32 {
        let start = (row * self.radius) as usize;
        self.coverage[start..start + self.radius as usize]
            .iter()
            .take_while(|&&coverage| coverage < u8::MAX)
            .count() as i32
    }

    /// Runs of rows in `corner`'s square that leave out the same number of edge pixels, as
    /// (first row, row after the last, pixels left out). Rows that leave none out are skipped.
    fn outside_runs(&self, corner: Corner) -> Vec<(i32, i32, i32)> {
        let mut runs: Vec<(i32, i32, i32)> = Vec::new();
        for y in 0..self.radius {
            let row = if corner.is_top() {
                y
            } else {
                self.radius - 1 - y
            };
            let outside = self.outside(row);
            match runs.last_mut() {
                Some((_, end, width)) if *end == y && *width == outside => *end += 1,
                _ if outside > 0 => runs.push((y, y + 1, outside)),
                _ => {}
            }
        }
        runs
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

    pub fn radius(self) -> i32 {
        self.radius
    }

    /// The corners that float, each with the window position of its `radius`-sized square.
    /// Corners touching a screen edge stay square, as do all of them without rounding.
    pub fn rounded_corners(self) -> impl Iterator<Item = (Corner, POINT)> {
        Corner::ALL
            .into_iter()
            .zip(self.corners)
            .filter(move |_| self.radius > 0)
            .filter(|(_, attached)| !attached)
            .map(move |(corner, _)| {
                let x = if corner.is_left() {
                    0
                } else {
                    self.width - self.radius
                };
                let y = if corner.is_top() {
                    0
                } else {
                    self.height - self.radius
                };
                (corner, POINT { x, y })
            })
    }
}

/// Keeps the pixels fully inside each rounded corner's curve, matching `CornerMask`, so the
/// corner windows draw every partly covered pixel and nothing is drawn twice.
pub fn clip_to_edges(window: HWND, shape: ClipShape) -> windows::core::Result<()> {
    let region = Region::new(unsafe { CreateRectRgn(0, 0, shape.width, shape.height) })?;
    let mask = CornerMask::new(shape.radius);
    for (corner, origin) in shape.rounded_corners() {
        for (first, end, outside) in mask.outside_runs(corner) {
            let left = if corner.is_left() {
                origin.x
            } else {
                origin.x + shape.radius - outside
            };
            let cut = Region::new(unsafe {
                CreateRectRgn(left, origin.y + first, left + outside, origin.y + end)
            })?;
            if unsafe { CombineRgn(Some(region.0), Some(region.0), Some(cut.0), RGN_DIFF) }
                == RGN_ERROR
            {
                return Err(windows::core::Error::from_win32());
            }
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

    #[test]
    fn only_floating_corners_are_rounded_each_at_its_own_square() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let top = bounds(area, 640, 300, ScreenPosition::Top);
        assert_eq!(
            ClipShape::new(top, area, 16)
                .rounded_corners()
                .collect::<Vec<_>>(),
            [
                (Corner::BottomLeft, POINT { x: 0, y: 284 }),
                (Corner::BottomRight, POINT { x: 624, y: 284 }),
            ]
        );
        let centered = bounds(area, 640, 300, ScreenPosition::Center);
        assert_eq!(
            ClipShape::new(centered, area, 16).rounded_corners().count(),
            4
        );
        assert_eq!(
            ClipShape::new(centered, area, 0).rounded_corners().count(),
            0
        );
    }

    #[test]
    fn corner_mask_is_a_smooth_quarter_circle_and_the_region_keeps_only_full_pixels() {
        const RADIUS: i32 = 16;
        let last = RADIUS - 1;
        let mask = CornerMask::new(RADIUS);
        let top_left = |x, y| mask.coverage(Corner::TopLeft, x, y);
        let pixels = || (0..RADIUS).flat_map(|y| (0..RADIUS).map(move |x| (x, y)));
        assert_eq!(top_left(0, 0), 0);
        assert_eq!(top_left(last, last), u8::MAX);
        // Coverage grows toward the window's inside along every row and column.
        for (x, y) in pixels().filter(|&(x, _)| x > 0) {
            assert!(top_left(x, y) >= top_left(x - 1, y), "({x}, {y})");
            assert!(top_left(y, x) >= top_left(y, x - 1), "({y}, {x})");
        }
        // Partly covered pixels smooth the curve instead of stepping it.
        let partial = pixels()
            .filter(|&(x, y)| (1..u8::MAX).contains(&top_left(x, y)))
            .count();
        assert!(partial >= RADIUS as usize, "{partial}");
        let area: f32 = pixels()
            .map(|(x, y)| f32::from(top_left(x, y)) / 255.)
            .sum();
        let quarter_disc = std::f32::consts::PI * (RADIUS * RADIUS) as f32 / 4.;
        assert!((area - quarter_disc).abs() < 1., "{area} vs {quarter_disc}");
        // The other corners mirror the top-left one.
        for (x, y) in pixels() {
            let coverage = top_left(x, y);
            assert_eq!(mask.coverage(Corner::TopRight, last - x, y), coverage);
            assert_eq!(mask.coverage(Corner::BottomLeft, x, last - y), coverage);
            assert_eq!(
                mask.coverage(Corner::BottomRight, last - x, last - y),
                coverage
            );
        }
        // The region leaves out exactly the pixels that are not fully inside.
        for corner in Corner::ALL {
            let mut outside = [0; RADIUS as usize];
            for (first, end, width) in mask.outside_runs(corner) {
                outside[first as usize..end as usize].fill(width);
            }
            for (x, y) in pixels() {
                let from_edge = if corner.is_left() { x } else { last - x };
                assert_eq!(
                    from_edge < outside[y as usize],
                    mask.coverage(corner, x, y) < u8::MAX,
                    "{corner:?} ({x}, {y})"
                );
            }
        }
    }
}
