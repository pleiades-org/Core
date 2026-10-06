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
}

impl MediaCommand {
    pub fn label(self) -> &'static str {
        match self {
            Self::TogglePlayPause => "Play / pause",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
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
}

impl MediaControls {
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
