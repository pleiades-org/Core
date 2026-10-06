//! Media sessions and which one Core controls. The launcher reads Windows' sessions and sends
//! the commands; this module only decides, so every rule is testable without a player.
mod choose_session;
mod known_apps;
mod mixer;
mod timeline;
mod volume;

pub use choose_session::{bar_session, choose_target, MediaPolicy, PriorityMode};
pub use known_apps::{
    app_key, classify_app, is_app_process, is_spotify, known_app, known_app_name, known_program,
    AppClass, KnownApp,
};
pub use mixer::{sort_mixer, step_volume, MixerApp, SYSTEM_VOLUME_ID, VOLUME_STEP};
pub use timeline::{format_clock, playback_position, PlaybackProgress, Timeline};
pub use volume::VolumeLevel;

use std::sync::Arc;

/// Ordered by how strongly a session claims the controls: playing first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlaybackState {
    /// Playing, or changing tracks, which players report briefly between two playing states.
    Playing,
    Paused,
    /// Opened or stopped: loaded but not started.
    Idle,
}

impl PlaybackState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Idle => "Stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    TogglePlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Turns shuffle on, or off again.
    Shuffle,
    /// Moves repeat on to its next mode.
    Repeat,
}

impl MediaCommand {
    pub fn label(self) -> &'static str {
        match self {
            Self::TogglePlayPause => "Play / pause",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
            Self::Shuffle => "Shuffle",
            Self::Repeat => "Repeat",
        }
    }
}

/// What a player does when it reaches the end of what it plays, in the order one press after
/// another steps through them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    /// The album, playlist or queue starts again.
    All,
    /// The same track starts again.
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat all",
            Self::One => "Repeat one",
        }
    }
}

/// What a player allows right now; a control it does not allow is not offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaControls {
    pub play: bool,
    pub pause: bool,
    pub next: bool,
    pub previous: bool,
    pub seek: bool,
    /// Whether the player shuffles now; None when it does not let Core change that.
    pub shuffle: Option<bool>,
    /// How the player repeats now; None when it does not let Core change that.
    pub repeat: Option<RepeatMode>,
}

impl MediaControls {
    /// The controls once `command` took effect: shuffle the other way, repeat one mode on.
    pub fn after(self, command: MediaCommand) -> Self {
        match command {
            MediaCommand::Shuffle => Self {
                shuffle: self.shuffle.map(|on| !on),
                ..self
            },
            MediaCommand::Repeat => Self {
                repeat: self.repeat.map(RepeatMode::next),
                ..self
            },
            _ => self,
        }
    }

    /// "Shuffle on" or "Repeat all": how the setting `command` changes stands now. None for
    /// the other commands, and for a setting the player does not offer.
    pub fn mode_label(self, command: MediaCommand) -> Option<&'static str> {
        match command {
            MediaCommand::Shuffle => {
                self.shuffle
                    .map(|on| if on { "Shuffle on" } else { "Shuffle off" })
            }
            MediaCommand::Repeat => self.repeat.map(RepeatMode::label),
            _ => None,
        }
    }

    pub fn allows(self, command: MediaCommand, state: PlaybackState) -> bool {
        match command {
            MediaCommand::TogglePlayPause => {
                if state == PlaybackState::Playing {
                    self.pause
                } else {
                    self.play
                }
            }
            MediaCommand::Play => self.play,
            MediaCommand::Pause => self.pause,
            MediaCommand::Next => self.next,
            MediaCommand::Previous => self.previous,
            MediaCommand::Shuffle => self.shuffle.is_some(),
            MediaCommand::Repeat => self.repeat.is_some(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaSession {
    /// Windows' identifier for the player (its AppUserModelID).
    pub app_id: Arc<str>,
    pub app_name: Arc<str>,
    pub title: Arc<str>,
    pub artist: Arc<str>,
    pub album: Arc<str>,
    pub state: PlaybackState,
    pub class: AppClass,
    /// Windows' own choice, the session that most recently took the media keys.
    pub is_system_current: bool,
    pub controls: MediaControls,
    pub timeline: Option<Timeline>,
}

impl MediaSession {
    /// The key preferences and ignore lists use, shared by every install of a known app.
    pub fn key(&self) -> Arc<str> {
        app_key(&self.app_id)
    }

    /// "Title — Artist", or whichever of them is known.
    pub fn heading(&self) -> String {
        match (self.title.is_empty(), self.artist.is_empty()) {
            (false, false) => format!("{} — {}", self.title, self.artist),
            (false, true) => self.title.to_string(),
            (true, false) => self.artist.to_string(),
            (true, true) => format!("{} media", self.app_name),
        }
    }
}

/// Everything search needs about media: the sessions, the person's priority settings, and
/// what Core itself did recently.
#[derive(Clone, Debug, Default)]
pub struct MediaState {
    pub sessions: Arc<[MediaSession]>,
    pub policy: MediaPolicy,
    /// The app Core sent a command to moments ago; it stays the target while players settle.
    pub sticky: Option<Arc<str>>,
    /// The app Core paused, so the now-playing bar keeps offering to resume it.
    pub paused_by_core: Option<Arc<str>>,
    /// Windows' media sessions could not be read; commands fall back to the media keys.
    pub unavailable: bool,
}

impl MediaState {
    pub fn target(&self, command: MediaCommand) -> Option<&MediaSession> {
        choose_target(
            &self.sessions,
            &self.policy,
            command,
            self.sticky.as_deref(),
        )
        .map(|index| &self.sessions[index])
    }

    pub fn bar(&self) -> Option<&MediaSession> {
        bar_session(
            &self.sessions,
            &self.policy,
            self.sticky.as_deref(),
            self.paused_by_core.as_deref(),
        )
        .map(|index| &self.sessions[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_and_repeat_step_from_what_the_player_reports_and_only_where_it_offers_them() {
        let offered = MediaControls {
            shuffle: Some(false),
            repeat: Some(RepeatMode::Off),
            ..MediaControls::default()
        };
        assert!(offered.allows(MediaCommand::Shuffle, PlaybackState::Paused));
        assert!(offered.allows(MediaCommand::Repeat, PlaybackState::Idle));
        assert_eq!(
            offered.mode_label(MediaCommand::Shuffle),
            Some("Shuffle off")
        );
        let shuffled = offered.after(MediaCommand::Shuffle);
        assert_eq!(shuffled.shuffle, Some(true));
        assert_eq!(shuffled.repeat, Some(RepeatMode::Off));
        assert_eq!(
            shuffled.mode_label(MediaCommand::Shuffle),
            Some("Shuffle on")
        );
        assert_eq!(shuffled.after(MediaCommand::Shuffle), offered);
        // Repeat goes round: everything, one track, off.
        let mut repeated = offered;
        let mut labels = Vec::new();
        for _ in 0..3 {
            repeated = repeated.after(MediaCommand::Repeat);
            labels.extend(repeated.mode_label(MediaCommand::Repeat));
        }
        assert_eq!(labels, ["Repeat all", "Repeat one", "Repeat off"]);
        assert_eq!(repeated, offered);
        // The other commands change neither.
        assert_eq!(offered.after(MediaCommand::Next), offered);
        assert_eq!(offered.mode_label(MediaCommand::Next), None);

        let plain = MediaControls::default();
        for command in [MediaCommand::Shuffle, MediaCommand::Repeat] {
            assert!(!plain.allows(command, PlaybackState::Playing));
            assert_eq!(plain.after(command), plain);
            assert_eq!(plain.mode_label(command), None);
        }
    }
}
