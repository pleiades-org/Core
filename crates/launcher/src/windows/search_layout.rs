use super::theme::{self, scale};
use windows::Win32::Foundation::RECT;

const CLOCK_WIDTH: i32 = 80;
const CLOCK_GAP: i32 = 16;

pub struct SearchLayout {
    pub input: RECT,
    pub settings: RECT,
    pub results: RECT,
    pub footer: RECT,
    pub clock: RECT,
    pub power: RECT,
    /// Command output, in place of the results; the window is sized to fit it.
    pub output: RECT,
    /// The now-playing bar across the top, and its previous, play / pause and next buttons.
    /// Empty when the bar is hidden.
    pub media_info: RECT,
    pub media_buttons: [RECT; 3],
    /// Shuffle and repeat, left of previous, which the bar shows under the pointer.
    pub media_modes: [RECT; 2],
}

impl SearchLayout {
    /// Use the clamped client size so display scaling cannot push controls off screen.
    /// `top` is the now-playing bar's height in pixels: everything from the search box down to
    /// the results moves below it, while the footer stays at the bottom.
    pub fn new(width: i32, height: i32, dpi: u32, top: i32) -> Self {
        let right_edge = |margin| width - scale(margin, dpi);
        let bottom_edge = |margin| height - scale(margin, dpi);
        let below_bar = |margin| top + scale(margin, dpi);
        let results_top = below_bar(theme::RESULTS_TOP);
        let clock_left = (width - scale(CLOCK_WIDTH, dpi)) / 2;
        Self {
            input: RECT {
                left: scale(60, dpi),
                top: below_bar(25),
                right: right_edge(94),
                bottom: below_bar(59),
            },
            settings: RECT {
                left: right_edge(66),
                top: below_bar(24),
                right: right_edge(26),
                bottom: below_bar(60),
            },
            results: RECT {
                left: scale(12, dpi),
                top: results_top,
                right: right_edge(12),
                bottom: bottom_edge(theme::FOOTER_HEIGHT),
            },
            // Aligned with the section label and footer text; the scroll bar sits at the edge.
            output: RECT {
                left: scale(24, dpi),
                top: results_top,
                right: right_edge(12),
                bottom: bottom_edge(theme::FOOTER_HEIGHT + theme::OUTPUT_GAP).max(results_top),
            },
            footer: RECT {
                left: scale(24, dpi),
                top: bottom_edge(30),
                right: clock_left - scale(CLOCK_GAP, dpi),
                bottom: bottom_edge(12),
            },
            clock: RECT {
                left: clock_left,
                top: bottom_edge(30),
                right: clock_left + scale(CLOCK_WIDTH, dpi),
                bottom: bottom_edge(12),
            },
            power: RECT {
                left: right_edge(68),
                top: bottom_edge(40),
                right: right_edge(28),
                bottom: bottom_edge(8),
            },
            media_info: RECT {
                left: 0,
                top: 0,
                right: if top > 0 { width } else { 0 },
                bottom: top,
            },
            media_buttons: if top > 0 {
                super::view::media_button_areas(width, 0, dpi)
            } else {
                [RECT::default(); 3]
            },
            media_modes: if top > 0 {
                super::view::media_mode_areas(width, 0, dpi)
            } else {
                [RECT::default(); 2]
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_results_reserve_footer_space_on_short_and_narrow_scaled_displays() {
        for (width, height, dpi) in [
            (640, 566, 96),
            (1280, 1040, 192),
            (1080, 1040, 192),
            (2560, 1440, 384),
        ] {
            let layout = SearchLayout::new(width, height, dpi, 0);
            for control in [
                layout.input,
                layout.settings,
                layout.results,
                layout.footer,
                layout.clock,
                layout.power,
            ] {
                assert!(control.left >= 0 && control.right <= width);
                assert!(control.top >= 0 && control.bottom <= height);
                assert!(control.right > control.left && control.bottom > control.top);
            }
            assert!(layout.results.bottom <= layout.power.top);
            assert!(layout.footer.right < layout.clock.left);
            assert!(layout.clock.right < layout.power.left);
            assert!((layout.clock.left + layout.clock.right - width).abs() <= 1);
            assert_eq!(layout.media_info, RECT::default());
        }
    }

    #[test]
    fn output_takes_the_place_of_the_results_above_the_footer() {
        let layout = SearchLayout::new(640, 500, 96, 0);
        assert_eq!(layout.output.top, layout.results.top);
        assert!(layout.output.bottom <= layout.footer.top);
        assert_eq!(layout.output.left, layout.footer.left);
        // Too short for any output: it collapses instead of covering the input.
        let cramped = SearchLayout::new(640, 120, 96, 0);
        assert!(cramped.output.top >= cramped.input.bottom);
        assert!(cramped.output.bottom >= cramped.output.top);
    }

    #[test]
    fn the_now_playing_bar_moves_the_search_rows_down_and_leaves_the_footer() {
        let bar = scale(theme::MEDIA_BAR_HEIGHT, 144);
        let plain = SearchLayout::new(960, 900, 144, 0);
        let with_bar = SearchLayout::new(960, 900 + bar, 144, bar);
        for (before, after) in [
            (plain.input, with_bar.input),
            (plain.settings, with_bar.settings),
            (plain.results, with_bar.results),
        ] {
            assert_eq!(after.top, before.top + bar);
        }
        assert_eq!(with_bar.footer.top, plain.footer.top + bar);
        assert_eq!(
            with_bar.footer.bottom - with_bar.footer.top,
            plain.footer.bottom - plain.footer.top
        );
        assert_eq!(with_bar.media_info.bottom, bar);
        assert!(with_bar
            .media_buttons
            .iter()
            .chain(&with_bar.media_modes)
            .all(|button| button.top >= 0 && button.bottom <= bar && button.right <= 960));
        assert!(with_bar.media_buttons[2].left > with_bar.media_buttons[0].left);
        assert!(with_bar.media_modes[1].right <= with_bar.media_buttons[0].left);
        assert_eq!(plain.media_modes, [RECT::default(); 2]);
    }
}
