//! Sends a control to the chosen player. The session is chosen again from a fresh reading, so a
//! press never acts on a player that closed or changed state since the results were shown.
use super::{
    read_sessions::{read_sessions, OPERATION_TIMEOUT},
    MediaAction, MediaOutcome, MediaRequest,
};
use core_engine::media::{choose_target, MediaCommand, MediaSession, PlaybackState};
use windows::{
    core::Result,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    },
    Win32::UI::Input::KeyboardAndMouse::*,
};

pub(super) fn execute(manager: Option<&SessionManager>, request: &MediaRequest) -> MediaOutcome {
    let Some(manager) = manager else {
        return MediaOutcome {
            action: request.action,
            app_id: None,
            app_name: None,
            before: None,
            // A media-key shortcut sending its own key would only trigger itself again.
            result: if request.key_fallback {
                media_key(request.action)
            } else {
                Err("Windows media sessions are unavailable".into())
            },
        };
    };
    let sessions = match read_sessions(manager, false) {
        Ok(read) => read.kept,
        Err(error) => return failure(request, format!("Could not read media sessions: {error}")),
    };
    let infos: Vec<MediaSession> = sessions.iter().map(|entry| entry.info.clone()).collect();
    let command = match request.action {
        MediaAction::Control(command) => command,
        // Seeking moves within whatever track the bar shows; any player qualifies.
        MediaAction::Seek(_) => MediaCommand::TogglePlayPause,
    };
    let index = match &request.target {
        Some(target) => match infos.iter().position(|info| info.app_id == *target) {
            Some(index) => index,
            None => {
                return failure(
                    request,
                    format!(
                        "{} is no longer open",
                        core_engine::media::known_app_name(target)
                    ),
                )
            }
        },
        None => match choose_target(&infos, &request.policy, command, request.sticky.as_deref()) {
            Some(index) => index,
            None => {
                return failure(
                    request,
                    match (command, infos.is_empty()) {
                        (_, true) => "No media app is open".into(),
                        (MediaCommand::Pause, _) => "Nothing is playing".into(),
                        (MediaCommand::Play, _) => "Already playing".into(),
                        _ => "No media app can do that".into(),
                    },
                )
            }
        },
    };
    let info = &infos[index];
    let result = match send(&sessions[index].session, info, request.action) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "{} did not accept {}",
            info.app_name,
            describe(request.action)
        )),
        Err(error) => Err(format!("Could not reach {}: {error}", info.app_name)),
    };
    MediaOutcome {
        action: request.action,
        app_id: Some(info.app_id.clone()),
        app_name: Some(info.app_name.clone()),
        before: Some(info.state),
        result,
    }
}

/// Play/pause follows the state Core read, because not every player supports toggling.
fn send(session: &Session, info: &MediaSession, action: MediaAction) -> Result<bool> {
    let operation = match action {
        MediaAction::Control(MediaCommand::TogglePlayPause)
            if info.state == PlaybackState::Playing =>
        {
            session.TryPauseAsync()?
        }
        MediaAction::Control(MediaCommand::TogglePlayPause | MediaCommand::Play) => {
            session.TryPlayAsync()?
        }
        MediaAction::Control(MediaCommand::Pause) => session.TryPauseAsync()?,
        MediaAction::Control(MediaCommand::Next) => session.TrySkipNextAsync()?,
        MediaAction::Control(MediaCommand::Previous) => session.TrySkipPreviousAsync()?,
        MediaAction::Seek(position) => session.TryChangePlaybackPositionAsync(position)?,
    };
    finish!(operation, OPERATION_TIMEOUT)
}

fn describe(action: MediaAction) -> &'static str {
    match action {
        MediaAction::Control(MediaCommand::TogglePlayPause) => "play / pause",
        MediaAction::Control(MediaCommand::Play) => "play",
        MediaAction::Control(MediaCommand::Pause) => "pause",
        MediaAction::Control(MediaCommand::Next) => "next track",
        MediaAction::Control(MediaCommand::Previous) => "previous track",
        MediaAction::Seek(_) => "a new position",
    }
}

fn failure(request: &MediaRequest, message: String) -> MediaOutcome {
    MediaOutcome {
        action: request.action,
        app_id: None,
        app_name: None,
        before: None,
        result: Err(message),
    }
}

/// Without Windows' media sessions, the keyboard's media keys are the only way to reach a
/// player; Windows chooses which one.
fn media_key(action: MediaAction) -> std::result::Result<(), String> {
    let key = match action {
        MediaAction::Control(
            MediaCommand::TogglePlayPause | MediaCommand::Play | MediaCommand::Pause,
        ) => VK_MEDIA_PLAY_PAUSE,
        MediaAction::Control(MediaCommand::Next) => VK_MEDIA_NEXT_TRACK,
        MediaAction::Control(MediaCommand::Previous) => VK_MEDIA_PREV_TRACK,
        MediaAction::Seek(_) => return Err("Seeking needs Windows media sessions".into()),
    };
    let input = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [
        input(KEYEVENTF_EXTENDEDKEY),
        input(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!(
            "Windows did not accept the media key: {}",
            windows::core::Error::from_win32()
        ))
    }
}
