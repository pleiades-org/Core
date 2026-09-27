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
}

impl SearchLayout {
    /// Use the clamped client size so display scaling cannot push controls off screen.
    pub fn new(width: i32, height: i32, dpi: u32) -> Self {
        let right_edge = |margin| width - scale(margin, dpi);
        let bottom_edge = |margin| height - scale(margin, dpi);
        let results_top = scale(theme::RESULTS_TOP, dpi);
        let clock_left = (width - scale(CLOCK_WIDTH, dpi)) / 2;
        Self {
            input: RECT {
                left: scale(60, dpi),
                top: scale(25, dpi),
                right: right_edge(94),
                bottom: scale(59, dpi),
            },
            settings: RECT {
                left: right_edge(66),
                top: scale(24, dpi),
                right: right_edge(26),
                bottom: scale(60, dpi),
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
            let layout = SearchLayout::new(width, height, dpi);
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
        }
    }

    #[test]
    fn output_takes_the_place_of_the_results_above_the_footer() {
        let layout = SearchLayout::new(640, 500, 96);
        assert_eq!(layout.output.top, layout.results.top);
        assert!(layout.output.bottom <= layout.footer.top);
        assert_eq!(layout.output.left, layout.footer.left);
        // Too short for any output: it collapses instead of covering the input.
        let cramped = SearchLayout::new(640, 120, 96);
        assert!(cramped.output.top >= cramped.input.bottom);
        assert!(cramped.output.bottom >= cramped.output.top);
    }
}
