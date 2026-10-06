//! Reads Windows' media sessions into the engine's plain data. WinRT objects stay here.
use core_engine::media::{
    classify_app, known_app_name, MediaControls, MediaSession, PlaybackState, RepeatMode, Timeline,
};
use std::{sync::Arc, time::Duration};
use windows::{
    core::{Result, HSTRING},
    Media::{
        Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as SessionManager,
            GlobalSystemMediaTransportControlsSessionMediaProperties as MediaProperties,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        },
        MediaPlaybackAutoRepeatMode as AutoRepeat,
    },
};

pub(super) const OPERATION_TIMEOUT: Duration = Duration::from_secs(2);
/// Titles and names are shown on one line; longer text from a player is cut here.
const TEXT_LIMIT: usize = 256;

pub(super) struct ReadSession {
    pub info: MediaSession,
    pub session: Session,
    pub properties: Option<MediaProperties>,
}

pub(super) struct SessionRead {
    /// One session per player: browsers can report one per tab under the same identifier,
    /// and Core addresses players by identifier. The one playing, or Windows' current one.
    pub kept: Vec<ReadSession>,
    /// Every session, including other tabs of the same browser, for change notifications.
    pub all: Vec<Session>,
}

pub(super) fn read_sessions(manager: &SessionManager, properties: bool) -> Result<SessionRead> {
    let current = manager
        .GetCurrentSession()
        .and_then(|session| session.SourceAppUserModelId())
        .map(|identifier| identifier.to_string())
        .ok();
    let mut read: Vec<ReadSession> = Vec::new();
    let mut all = Vec::new();
    for session in manager.GetSessions()? {
        all.push(session.clone());
        let entry = match read_session(session, current.as_deref(), properties) {
            Ok(Some(entry)) => entry,
            Ok(None) => continue,
            Err(error) => {
                eprintln!("Could not read a media session: {error}");
                continue;
            }
        };
        match read
            .iter_mut()
            .find(|existing| existing.info.app_id == entry.info.app_id)
        {
            Some(existing) if outranks(&entry.info, &existing.info) => *existing = entry,
            Some(_) => {}
            None => read.push(entry),
        }
    }
    read.sort_by(|first, second| first.info.app_id.cmp(&second.info.app_id));
    Ok(SessionRead { kept: read, all })
}

fn outranks(candidate: &MediaSession, existing: &MediaSession) -> bool {
    (candidate.state, !candidate.is_system_current) < (existing.state, !existing.is_system_current)
}

fn read_session(
    session: Session,
    current: Option<&str>,
    properties: bool,
) -> Result<Option<ReadSession>> {
    let app_id = session.SourceAppUserModelId()?.to_string();
    let playback = session.GetPlaybackInfo()?;
    let Some(state) = playback_state(playback.PlaybackStatus()?) else {
        return Ok(None);
    };
    let controls = playback.Controls()?;
    let controls = MediaControls {
        play: controls.IsPlayEnabled()?,
        pause: controls.IsPauseEnabled()?,
        next: controls.IsNextEnabled()?,
        previous: controls.IsPreviousEnabled()?,
        seek: controls.IsPlaybackPositionEnabled()?,
        // A player that offers a setting without saying how it stands has it off.
        shuffle: controls.IsShuffleEnabled()?.then(|| {
            playback
                .IsShuffleActive()
                .and_then(|active| active.Value())
                .unwrap_or(false)
        }),
        repeat: controls.IsRepeatEnabled()?.then(|| {
            playback
                .AutoRepeatMode()
                .and_then(|mode| mode.Value())
                .map_or(RepeatMode::Off, repeat_mode)
        }),
    };
    let media_properties = if properties {
        match finish!(session.TryGetMediaPropertiesAsync()?, OPERATION_TIMEOUT) {
            Ok(media_properties) => Some(media_properties),
            Err(error) => {
                eprintln!("Could not read what {app_id} is playing: {error}");
                None
            }
        }
    } else {
        None
    };
    let field = |read: fn(&MediaProperties) -> Result<HSTRING>| -> Arc<str> {
        media_properties
            .as_ref()
            .and_then(|media_properties| read(media_properties).ok())
            .map_or_else(|| Arc::from(""), |text| bounded(&text.to_string()))
    };
    let app_name = known_app_name(&app_id);
    let info = MediaSession {
        class: classify_app(&app_id, &app_name),
        app_name: app_name.into(),
        title: field(MediaProperties::Title),
        artist: field(MediaProperties::Artist),
        album: field(MediaProperties::AlbumTitle),
        state,
        is_system_current: current == Some(app_id.as_str()),
        controls,
        timeline: read_timeline(&session),
        app_id: app_id.into(),
    };
    Ok(Some(ReadSession {
        info,
        session,
        properties: media_properties,
    }))
}

/// Closed sessions are gone; a track change counts as playing so the target does not flicker.
fn playback_state(status: Status) -> Option<PlaybackState> {
    if status == Status::Playing || status == Status::Changing {
        Some(PlaybackState::Playing)
    } else if status == Status::Paused {
        Some(PlaybackState::Paused)
    } else if status == Status::Closed {
        None
    } else {
        Some(PlaybackState::Idle)
    }
}

fn repeat_mode(mode: AutoRepeat) -> RepeatMode {
    if mode == AutoRepeat::List {
        RepeatMode::All
    } else if mode == AutoRepeat::Track {
        RepeatMode::One
    } else {
        RepeatMode::Off
    }
}

/// Windows' name for a repeat mode, for asking a player to use it.
pub(super) fn auto_repeat(mode: RepeatMode) -> AutoRepeat {
    match mode {
        RepeatMode::Off => AutoRepeat::None,
        RepeatMode::All => AutoRepeat::List,
        RepeatMode::One => AutoRepeat::Track,
    }
}

pub(super) fn read_timeline(session: &Session) -> Option<Timeline> {
    let timeline = session.GetTimelineProperties().ok()?;
    Some(Timeline {
        start: timeline.StartTime().ok()?.Duration,
        end: timeline.EndTime().ok()?.Duration,
        position: timeline.Position().ok()?.Duration,
        updated: timeline.LastUpdatedTime().ok()?.UniversalTime,
    })
}

fn bounded(text: &str) -> Arc<str> {
    let text = text.trim();
    match text.char_indices().nth(TEXT_LIMIT) {
        Some((end, _)) => format!("{}…", &text[..end]).into(),
        None => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_player_text_is_cut_at_a_character_boundary() {
        let long = "é".repeat(TEXT_LIMIT + 10);
        let cut = bounded(&long);
        assert_eq!(cut.chars().count(), TEXT_LIMIT + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(&*bounded("  Song  "), "Song");
    }

    #[test]
    fn repeat_modes_keep_their_meaning_to_and_from_windows() {
        for mode in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            assert_eq!(repeat_mode(auto_repeat(mode)), mode);
        }
        assert_eq!(auto_repeat(RepeatMode::All), AutoRepeat::List);
        assert_eq!(auto_repeat(RepeatMode::One), AutoRepeat::Track);
    }

    #[test]
    fn playing_and_windows_current_sessions_win_duplicates() {
        let session = |state, current| MediaSession {
            app_id: "Chrome".into(),
            app_name: "Google Chrome".into(),
            title: "".into(),
            artist: "".into(),
            album: "".into(),
            state,
            class: core_engine::media::AppClass::Browser,
            is_system_current: current,
            controls: MediaControls::default(),
            timeline: None,
        };
        assert!(outranks(
            &session(PlaybackState::Playing, false),
            &session(PlaybackState::Paused, true)
        ));
        assert!(outranks(
            &session(PlaybackState::Paused, true),
            &session(PlaybackState::Paused, false)
        ));
        assert!(!outranks(
            &session(PlaybackState::Paused, false),
            &session(PlaybackState::Paused, false)
        ));
    }
}
