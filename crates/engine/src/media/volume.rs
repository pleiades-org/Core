//! A player's volume as the now-playing bar's slider shows it, whether it comes from Windows'
//! mixer (a 0.0–1.0 level) or from the player's own account (a percentage).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeLevel {
    /// 0 to `MAX_PERCENT`.
    pub percent: u8,
    /// Silenced in Windows' mixer without changing its level.
    pub muted: bool,
}

impl VolumeLevel {
    pub const MAX_PERCENT: u8 = 100;

    /// Percentages above the maximum, as a careless caller or service might send, are full.
    pub fn new(percent: u8, muted: bool) -> Self {
        Self {
            percent: percent.min(Self::MAX_PERCENT),
            muted,
        }
    }

    /// From a mixer level; anything outside 0.0–1.0, or not a number, is brought inside it.
    pub fn from_scalar(level: f32, muted: bool) -> Self {
        let level = if level.is_nan() {
            0.
        } else {
            level.clamp(0., 1.)
        };
        Self::new((level * f32::from(Self::MAX_PERCENT)).round() as u8, muted)
    }

    /// The mixer level for a percentage.
    pub fn scalar(percent: u8) -> f32 {
        f32::from(percent.min(Self::MAX_PERCENT)) / f32::from(Self::MAX_PERCENT)
    }

    /// Nothing is heard: muted, or turned all the way down.
    pub fn is_silent(self) -> bool {
        self.muted || self.percent == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_levels_round_to_whole_percentages_and_back() {
        for (level, percent) in [(0., 0), (0.004, 0), (0.005, 1), (0.5, 50), (0.996, 100)] {
            assert_eq!(VolumeLevel::from_scalar(level, false).percent, percent);
        }
        for percent in 0..=VolumeLevel::MAX_PERCENT {
            let level = VolumeLevel::scalar(percent);
            assert_eq!(VolumeLevel::from_scalar(level, false).percent, percent);
        }
    }

    #[test]
    fn levels_outside_the_range_are_brought_inside_it() {
        assert_eq!(VolumeLevel::from_scalar(-0.2, false).percent, 0);
        assert_eq!(VolumeLevel::from_scalar(1.7, false).percent, 100);
        assert_eq!(VolumeLevel::from_scalar(f32::NAN, false).percent, 0);
        assert_eq!(VolumeLevel::new(250, false).percent, 100);
        assert_eq!(VolumeLevel::scalar(250), 1.);
    }

    #[test]
    fn muted_or_zero_is_silent() {
        assert!(VolumeLevel::new(60, true).is_silent());
        assert!(VolumeLevel::new(0, false).is_silent());
        assert!(!VolumeLevel::new(1, false).is_silent());
    }
}
