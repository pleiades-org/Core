//! A volume drawn like the sliders in Settings: a thin track, filled up to a round thumb, and
//! the glyph that says how loud it is. The now-playing bar's slider and each row of the volume
//! mixer are one of these; each places its own track and follows the pointer itself.
use super::*;
use core_engine::media::VolumeLevel;

// Geometry in 96-DPI pixels.
const TRACK_THICKNESS: i32 = 2;
const FILLED_THICKNESS: i32 = 4;
pub(super) const THUMB_DIAMETER: i32 = 12;
/// The ring around the thumb while it is dragged.
const DRAG_RING: i32 = 2;

pub(super) const VOLUME_GLYPH: &str = "\u{e767}";
pub(super) const MUTED_GLYPH: &str = "\u{e74f}";
/// Quiet, medium and loud.
pub(super) const LEVEL_GLYPHS: [&str; 3] = ["\u{e993}", "\u{e994}", "\u{e995}"];

/// The thumb on a track, and the colour of it and of the filled part before it.
#[derive(Clone, Copy)]
pub(super) struct Thumb {
    pub percent: u8,
    pub dragging: bool,
    pub color: COLORREF,
}

/// The line itself, centred on `middle`: its left end is silence and its right end full volume.
pub(super) fn track(left: i32, right: i32, middle: i32, dpi: u32) -> RECT {
    let half = (scale(TRACK_THICKNESS, dpi) / 2).max(1);
    painting::rectangle(left, middle - half, right, middle + half)
}

fn span(track: &RECT) -> i32 {
    (track.right - track.left).max(1)
}

/// The level under a pointer at `x`; beyond either end it is silence or full volume.
pub(super) fn percent_at(track: &RECT, x: i32) -> u8 {
    let span = span(track);
    let offset = (x - track.left).clamp(0, span);
    ((offset * i32::from(VolumeLevel::MAX_PERCENT) + span / 2) / span) as u8
}

pub(super) fn thumb_center(track: &RECT, percent: u8) -> i32 {
    let full = i32::from(VolumeLevel::MAX_PERCENT);
    track.left + (span(track) * i32::from(percent).min(full) + full / 2) / full
}

/// Draws the track, and on it the thumb when the level is known.
pub(super) fn draw(context: HDC, track: &RECT, thumb: Option<Thumb>, dpi: u32, palette: Palette) {
    painting::rounded(
        context,
        track,
        (track.bottom - track.top) / 2,
        palette.secondary,
    );
    let Some(thumb) = thumb else {
        return;
    };
    let center = thumb_center(track, thumb.percent);
    let middle = (track.top + track.bottom) / 2;
    let filled = (scale(FILLED_THICKNESS, dpi) / 2).max(1);
    if center > track.left {
        painting::rounded(
            context,
            &painting::rectangle(track.left, middle - filled, center, middle + filled),
            filled,
            thumb.color,
        );
    }
    let circle = |diameter: i32| {
        let half = diameter / 2;
        painting::rectangle(
            center - half,
            middle - half,
            center - half + diameter,
            middle - half + diameter,
        )
    };
    let diameter = scale(THUMB_DIAMETER, dpi);
    if thumb.dragging {
        let ring = diameter + scale(DRAG_RING, dpi) * 2;
        painting::rounded(context, &circle(ring), ring / 2, palette.accent);
    }
    painting::rounded(context, &circle(diameter), diameter / 2, thumb.color);
}

/// The speaker for a level: crossed out when nothing can be heard, else with more waves the
/// louder it is. A level not known yet is a plain speaker.
pub(super) fn level_glyph(level: Option<VolumeLevel>) -> &'static str {
    let Some(level) = level else {
        return VOLUME_GLYPH;
    };
    if level.is_silent() {
        return MUTED_GLYPH;
    }
    let step = usize::from(VolumeLevel::MAX_PERCENT).div_ceil(LEVEL_GLYPHS.len());
    LEVEL_GLYPHS[(usize::from(level.percent - 1) / step).min(LEVEL_GLYPHS.len() - 1)]
}

pub(super) fn contains(area: &RECT, point: POINT) -> bool {
    unsafe { PtInRect(area, point) }.as_bool()
}

/// The pointer in a mouse message. Signed: a dragging pointer moves left of and above the
/// control.
pub(super) fn pointer(data: LPARAM) -> POINT {
    POINT {
        x: i32::from(data.0 as u16 as i16),
        y: i32::from((data.0 >> 16) as u16 as i16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_and_levels_convert_both_ways_and_stop_at_the_ends() {
        for dpi in [96, 144, 192] {
            let track = track(scale(40, dpi), scale(230, dpi), scale(26, dpi), dpi);
            assert!(track.bottom > track.top, "{dpi}");
            assert_eq!((track.top + track.bottom) / 2, scale(26, dpi), "{dpi}");
            assert_eq!(percent_at(&track, track.left), 0);
            assert_eq!(percent_at(&track, track.right), 100);
            assert_eq!(percent_at(&track, track.left - 500), 0);
            assert_eq!(percent_at(&track, track.right + 500), 100);
            for percent in 0..=VolumeLevel::MAX_PERCENT {
                assert_eq!(
                    percent_at(&track, thumb_center(&track, percent)),
                    percent,
                    "{dpi}"
                );
            }
            // A level beyond full is drawn as full.
            assert_eq!(thumb_center(&track, 250), track.right, "{dpi}");
        }
        // A track with no room keeps a span, so nothing divides by zero.
        let empty = track(50, 50, 10, 96);
        assert_eq!(percent_at(&empty, 0), 0);
        assert_eq!(percent_at(&empty, 400), 100);
    }

    #[test]
    fn the_glyph_follows_the_level_and_shows_silence() {
        assert_eq!(level_glyph(None), VOLUME_GLYPH);
        assert_eq!(level_glyph(Some(VolumeLevel::new(0, false))), MUTED_GLYPH);
        assert_eq!(level_glyph(Some(VolumeLevel::new(80, true))), MUTED_GLYPH);
        for (percent, expected) in [
            (1, LEVEL_GLYPHS[0]),
            (34, LEVEL_GLYPHS[0]),
            (35, LEVEL_GLYPHS[1]),
            (68, LEVEL_GLYPHS[1]),
            (69, LEVEL_GLYPHS[2]),
            (100, LEVEL_GLYPHS[2]),
        ] {
            assert_eq!(
                level_glyph(Some(VolumeLevel::new(percent, false))),
                expected,
                "{percent}"
            );
        }
    }

    #[test]
    fn mouse_coordinates_are_signed() {
        let packed = |x: i16, y: i16| LPARAM(((y as u16 as isize) << 16) | (x as u16 as isize));
        assert_eq!(pointer(packed(12, 30)), POINT { x: 12, y: 30 });
        assert_eq!(pointer(packed(-8, -3)), POINT { x: -8, y: -3 });
    }
}
