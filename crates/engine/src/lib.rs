//! Pure launcher query interpretation, matching and calculation.
//! Platform adapters own all filesystem, windowing and launch side effects.

pub mod aliases;
pub mod applications;
pub mod calculator;
pub mod conversions;
pub mod media;
pub mod quicklinks;
pub mod search;
pub mod time_conversion;

/// The launcher intentionally presents a small, stable set of choices.
pub const VISIBLE_RESULT_LIMIT: usize = 8;
/// Recently used apps shown as a grid when nothing is typed: six across, three rows.
pub const RECENT_APPLICATION_LIMIT: usize = 18;
