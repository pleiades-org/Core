//! Track position. Windows reports a position at a moment; Core advances it while playing
//! instead of asking again, so the progress bar needs no polling.
use std::time::Duration;

/// As Windows reports it, in 100-nanosecond units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeline {
    pub start: i64,
    pub end: i64,
    pub position: i64,
    /// When `position` was measured, as a Windows file time (100 ns since 1601); 0 if unknown.
    pub updated: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackProgress {
    pub elapsed: Duration,
    pub duration: Duration,
}

impl PlaybackProgress {
    /// How far through the track, from 0 to 1.
    pub fn fraction(self) -> f64 {
        if self.duration.is_zero() {
            return 0.;
        }
        (self.elapsed.as_secs_f64() / self.duration.as_secs_f64()).clamp(0., 1.)
    }

    /// Until the elapsed clock next shows a different second.
    pub fn until_next_second(self) -> Duration {
        Duration::from_secs(1) - Duration::from_nanos(u64::from(self.elapsed.subsec_nanos()))
    }
}

/// Where playback is at file time `now`. None when the player reports no track length.
pub fn playback_position(timeline: Timeline, playing: bool, now: i64) -> Option<PlaybackProgress> {
    let duration = timeline.end.saturating_sub(timeline.start);
    if duration <= 0 {
        return None;
    }
    let mut elapsed = timeline.position.saturating_sub(timeline.start);
    if playing && timeline.updated > 0 {
        elapsed = elapsed.saturating_add(now.saturating_sub(timeline.updated).max(0));
    }
    Some(PlaybackProgress {
        elapsed: ticks(elapsed.clamp(0, duration)),
        duration: ticks(duration),
    })
}

fn ticks(value: i64) -> Duration {
    Duration::from_nanos((value.max(0) as u64).saturating_mul(100))
}

/// `3:07`, or `1:02:03` past an hour.
pub fn format_clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 10_000_000;

    fn timeline(position: i64, updated: i64) -> Timeline {
        Timeline {
            start: 0,
            end: 200 * SECOND,
            position,
            updated,
        }
    }

    #[test]
    fn playing_tracks_advance_from_the_reported_moment_and_paused_ones_do_not() {
        let reported = timeline(60 * SECOND, 1_000 * SECOND);
        let later = 1_005 * SECOND + SECOND / 2;
        let playing = playback_position(reported, true, later).unwrap();
        assert_eq!(playing.elapsed, Duration::from_millis(65_500));
        assert_eq!(playing.duration, Duration::from_secs(200));
        assert_eq!(playing.until_next_second(), Duration::from_millis(500));
        let paused = playback_position(reported, false, later).unwrap();
        assert_eq!(paused.elapsed, Duration::from_secs(60));
    }

    #[test]
    fn positions_stay_within_the_track_and_unknown_lengths_have_no_progress() {
        let reported = timeline(190 * SECOND, 1_000 * SECOND);
        let past_end = playback_position(reported, true, 2_000 * SECOND).unwrap();
        assert_eq!(past_end.elapsed, past_end.duration);
        assert_eq!(past_end.fraction(), 1.);
        // A clock behind the report, or no report time at all, never moves backwards.
        let behind = playback_position(reported, true, 0).unwrap();
        assert_eq!(behind.elapsed, Duration::from_secs(190));
        assert_eq!(
            playback_position(timeline(10 * SECOND, 0), true, i64::MAX)
                .unwrap()
                .elapsed,
            Duration::from_secs(10)
        );
        let unknown = Timeline {
            start: 0,
            end: 0,
            position: 0,
            updated: 0,
        };
        assert_eq!(playback_position(unknown, true, 0), None);
    }

    #[test]
    fn clocks_show_minutes_and_seconds_or_hours_when_needed() {
        assert_eq!(format_clock(Duration::from_secs(0)), "0:00");
        assert_eq!(format_clock(Duration::from_secs(187)), "3:07");
        assert_eq!(format_clock(Duration::from_secs(3723)), "1:02:03");
    }
}
