//! Media controls in search: `@media` / `@music`, and the words play, pause, next, previous,
//! prev, shuffle, repeat, loop and now playing typed on their own, which offer a control above
//! application matches.
use super::{parse_query, Action, CommandKind, ParsedQuery, ResultKind, SearchBatch, SearchResult};
use crate::{
    media::{
        choose_target, MediaCommand, MediaPolicy, MediaSession, MediaState, PlaybackState,
        RepeatMode,
    },
    VISIBLE_RESULT_LIMIT,
};
use std::sync::Arc;

pub(super) const KEYWORD_MESSAGE: &str = "Enter to control media · ↓ for apps";
const LIST_MESSAGE: &str = "Enter to play or pause · ↓ for next, previous and other players";
const LOADING_MESSAGE: &str = "Reading media sessions…";
const NO_PLAYER_MESSAGE: &str = "No player is visible to Windows · Enter sends the media key";
const MEDIA_KEY_DETAIL: &str = "Sends the keyboard media key";
const UNAVAILABLE_MESSAGE: &str =
    "Windows media sessions are unavailable · Enter sends the media key";

/// Whole-query words; anything longer (`playlist`, `next friday`) searches as usual.
const KEYWORDS: [(&str, MediaCommand); 9] = [
    ("play", MediaCommand::Play),
    ("pause", MediaCommand::Pause),
    ("next", MediaCommand::Next),
    ("previous", MediaCommand::Previous),
    ("prev", MediaCommand::Previous),
    ("shuffle", MediaCommand::Shuffle),
    ("repeat", MediaCommand::Repeat),
    ("loop", MediaCommand::Repeat),
    ("now playing", MediaCommand::TogglePlayPause),
];

/// Words that end an `@media` query, as in `@media spotify next`.
const PAYLOAD_COMMANDS: [(&str, MediaCommand); 11] = [
    ("play", MediaCommand::Play),
    ("resume", MediaCommand::Play),
    ("pause", MediaCommand::Pause),
    ("next", MediaCommand::Next),
    ("skip", MediaCommand::Next),
    ("previous", MediaCommand::Previous),
    ("prev", MediaCommand::Previous),
    ("shuffle", MediaCommand::Shuffle),
    ("repeat", MediaCommand::Repeat),
    ("loop", MediaCommand::Repeat),
    ("toggle", MediaCommand::TogglePlayPause),
];

pub(super) fn keyword(text: &str) -> Option<MediaCommand> {
    KEYWORDS
        .iter()
        .find(|(phrase, _)| same_words(text, phrase))
        .map(|(_, command)| *command)
}

/// Whether search results for `query` use media sessions, so the launcher can read them first.
pub fn wants_media(query: &str) -> bool {
    match parse_query(query) {
        ParsedQuery::Command {
            kind: CommandKind::Media,
            ..
        } => true,
        ParsedQuery::Search(payload) => keyword(payload).is_some(),
        _ => false,
    }
}

fn same_words(text: &str, phrase: &str) -> bool {
    let mut typed = text.split_whitespace();
    phrase.split(' ').all(|word| {
        typed
            .next()
            .is_some_and(|typed| typed.eq_ignore_ascii_case(word))
    }) && typed.next().is_none()
}

/// The single control a keyword offers.
pub(super) fn keyword_result(command: MediaCommand, state: Option<&MediaState>) -> SearchResult {
    match state {
        None => command_row(command, None, "Uses your music app first"),
        Some(state) if blind(state) => command_row(command, None, blind_detail(command)),
        Some(state) => command_row(command, state.target(command), missing_target(command)),
    }
}

/// No player is visible, to Windows or by its window: controls send the keyboard's media keys,
/// which some players listen for themselves.
fn blind(state: &MediaState) -> bool {
    state.unavailable || state.sessions.is_empty()
}

/// What a control does while no player is visible. The keyboard has no key for shuffle or
/// repeat, so those have nothing to reach.
fn blind_detail(command: MediaCommand) -> &'static str {
    match command {
        MediaCommand::Shuffle | MediaCommand::Repeat => "No media app is open",
        _ => MEDIA_KEY_DETAIL,
    }
}

/// `@media [app] [command]`: the chosen player's controls, then the other players.
pub(super) fn media_results(payload: &str, state: Option<&MediaState>) -> SearchBatch {
    let (filter, explicit) = split_payload(payload);
    let Some(state) = state.filter(|state| !blind(state)) else {
        let detail = |command| match state {
            Some(_) => blind_detail(command),
            None => "Uses your music app first",
        };
        if !filter.is_empty() && state.is_some() {
            return SearchBatch {
                results: Vec::new(),
                message: "No open media app matches that name",
            };
        }
        let mut commands = vec![
            MediaCommand::TogglePlayPause,
            MediaCommand::Next,
            MediaCommand::Previous,
        ];
        if let Some(explicit) = explicit {
            commands.retain(|command| *command != explicit);
            commands.insert(0, explicit);
        }
        return SearchBatch {
            results: commands
                .into_iter()
                .map(|command| command_row(command, None, detail(command)))
                .collect(),
            message: match state {
                Some(state) if state.unavailable => UNAVAILABLE_MESSAGE,
                Some(_) => NO_PLAYER_MESSAGE,
                None => LOADING_MESSAGE,
            },
        };
    };
    let sessions: Vec<MediaSession> = state
        .sessions
        .iter()
        .filter(|session| matches_app(session, &filter))
        .cloned()
        .collect();
    if sessions.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: if filter.is_empty() {
                "No media is open · start a music app, then try again"
            } else {
                "No open media app matches that name"
            },
        };
    }
    // Naming an app reaches it even when it is ignored for automatic choices.
    let policy = if filter.is_empty() {
        state.policy.clone()
    } else {
        MediaPolicy {
            ignored: Arc::from([]),
            ..state.policy.clone()
        }
    };
    let target = |command| {
        choose_target(&sessions, &policy, command, state.sticky.as_deref())
            .map(|index| &sessions[index])
    };
    // A named app keeps every control on itself, even one it cannot do now: Enter then reports
    // why, instead of the launcher choosing another player.
    let named = (!filter.is_empty()).then(|| sessions.first()).flatten();
    let main = target(MediaCommand::TogglePlayPause).or(named);
    let mut results = Vec::new();
    let explicit = explicit.filter(|command| *command != MediaCommand::TogglePlayPause);
    if let Some(command) = explicit {
        results.push(match (target(command), named) {
            (Some(session), _) => command_row(command, Some(session), ""),
            (None, Some(session)) => named_row(command, session),
            (None, None) => command_row(command, None, missing_target(command)),
        });
    }
    if let Some(session) = main {
        results.push(now_playing_row(session));
        for command in [
            MediaCommand::Next,
            MediaCommand::Previous,
            MediaCommand::Shuffle,
            MediaCommand::Repeat,
        ] {
            if Some(command) != explicit && session.controls.allows(command, session.state) {
                results.push(command_row(command, Some(session), ""));
            }
        }
    }
    let mut others: Vec<&MediaSession> = sessions
        .iter()
        .filter(|session| main.is_none_or(|main| main.app_id != session.app_id))
        .collect();
    others.sort_by_key(|session| session.state);
    results.extend(others.into_iter().map(now_playing_row));
    results.truncate(VISIBLE_RESULT_LIMIT);
    SearchBatch {
        results,
        message: LIST_MESSAGE,
    }
}

fn split_payload(payload: &str) -> (String, Option<MediaCommand>) {
    let words: Vec<&str> = payload.split_whitespace().collect();
    if let Some((last, rest)) = words.split_last() {
        if let Some((_, command)) = PAYLOAD_COMMANDS
            .iter()
            .find(|(word, _)| last.eq_ignore_ascii_case(word))
        {
            return (rest.join(" ").to_lowercase(), Some(*command));
        }
    }
    (words.join(" ").to_lowercase(), None)
}

fn matches_app(session: &MediaSession, filter: &str) -> bool {
    filter.is_empty()
        || session.app_name.to_lowercase().contains(filter)
        || session.key().starts_with(filter)
}

fn missing_target(command: MediaCommand) -> &'static str {
    match command {
        MediaCommand::Pause => "Nothing is playing",
        MediaCommand::Play => "Already playing",
        MediaCommand::Shuffle | MediaCommand::Repeat => "No open player lets Core change this",
        _ => "No media app is open",
    }
}

/// What shuffle or repeat is on `session` now and what Enter does about it; None for the
/// other controls, and for a player that does not offer the setting.
fn mode_detail(command: MediaCommand, session: &MediaSession) -> Option<&'static str> {
    match command {
        MediaCommand::Shuffle => session.controls.shuffle.map(|on| {
            if on {
                "On · Enter to turn off"
            } else {
                "Off · Enter to turn on"
            }
        }),
        MediaCommand::Repeat => session.controls.repeat.map(|mode| match mode {
            RepeatMode::Off => "Off · Enter to repeat everything",
            RepeatMode::All => "Repeating everything · Enter to repeat one track",
            RepeatMode::One => "Repeating one track · Enter to turn off",
        }),
        _ => None,
    }
}

fn command_id(command: MediaCommand) -> &'static str {
    match command {
        MediaCommand::TogglePlayPause => "toggle",
        MediaCommand::Play => "play",
        MediaCommand::Pause => "pause",
        MediaCommand::Next => "next",
        MediaCommand::Previous => "previous",
        MediaCommand::Shuffle => "shuffle",
        MediaCommand::Repeat => "repeat",
    }
}

fn command_row(
    command: MediaCommand,
    target: Option<&MediaSession>,
    missing: &str,
) -> SearchResult {
    if let (MediaCommand::TogglePlayPause, Some(session)) = (command, target) {
        return now_playing_row(session);
    }
    SearchResult {
        kind: ResultKind::Media,
        id: format!(
            "media:{}:{}",
            command_id(command),
            target.map_or("", |session| &session.app_id)
        )
        .into(),
        title: command.label().into(),
        description: match target {
            Some(session) => match mode_detail(command, session) {
                Some(mode) => format!("{} · {mode}", session.app_name).into(),
                None => format!("{} · {}", session.app_name, session.heading()).into(),
            },
            None => missing.into(),
        },
        action: Action::Media {
            command,
            target: target.map(|session| session.app_id.clone()),
        },
    }
}

/// A control the named app cannot do right now, still aimed at that app.
fn named_row(command: MediaCommand, session: &MediaSession) -> SearchResult {
    SearchResult {
        kind: ResultKind::Media,
        id: format!("media:{}:{}", command_id(command), session.app_id).into(),
        title: command.label().into(),
        description: format!("{} · {}", session.app_name, missing_target(command)).into(),
        action: Action::Media {
            command,
            target: Some(session.app_id.clone()),
        },
    }
}

fn now_playing_row(session: &MediaSession) -> SearchResult {
    let verb = if session.state == PlaybackState::Playing {
        "pause"
    } else {
        "play"
    };
    SearchResult {
        kind: ResultKind::Media,
        id: format!("media:toggle:{}", session.app_id).into(),
        title: session.heading().into(),
        description: format!(
            "{} · {} · Enter to {verb}",
            session.app_name,
            session.state.label()
        )
        .into(),
        action: Action::Media {
            command: MediaCommand::TogglePlayPause,
            target: Some(session.app_id.clone()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{classify_app, MediaControls, PriorityMode};

    fn session(app_id: &str, title: &str, state: PlaybackState) -> MediaSession {
        MediaSession {
            app_id: app_id.into(),
            app_name: crate::media::known_app_name(app_id).into(),
            title: title.into(),
            artist: "Artist".into(),
            album: "".into(),
            state,
            class: classify_app(app_id, ""),
            is_system_current: false,
            controls: MediaControls {
                play: true,
                pause: true,
                next: true,
                previous: true,
                seek: false,
                ..MediaControls::default()
            },
            timeline: None,
        }
    }

    fn state() -> MediaState {
        MediaState {
            sessions: Arc::from([
                session("Chrome", "Video", PlaybackState::Playing),
                session("Spotify.exe", "Song", PlaybackState::Paused),
            ]),
            ..MediaState::default()
        }
    }

    fn batch_targets(batch: &SearchBatch) -> Vec<(MediaCommand, Option<&str>)> {
        batch.results.iter().map(target).collect()
    }

    fn target(result: &SearchResult) -> (MediaCommand, Option<&str>) {
        match &result.action {
            Action::Media { command, target } => (*command, target.as_deref()),
            other => panic!("not a media action: {other:?}"),
        }
    }

    #[test]
    fn only_whole_media_words_are_keywords() {
        for (text, command) in [
            ("play", MediaCommand::Play),
            (" PAUSE ", MediaCommand::Pause),
            ("Next", MediaCommand::Next),
            ("prev", MediaCommand::Previous),
            ("Shuffle", MediaCommand::Shuffle),
            ("repeat", MediaCommand::Repeat),
            ("loop", MediaCommand::Repeat),
            ("now   playing", MediaCommand::TogglePlayPause),
        ] {
            assert_eq!(keyword(text), Some(command), "{text}");
        }
        for text in [
            "playn",
            "next friday",
            "now",
            "playing",
            "",
            "play pause",
            "loopback",
            "repeat all",
        ] {
            assert_eq!(keyword(text), None, "{text}");
        }
        assert!(wants_media("@music"));
        assert!(wants_media("@MEDIA spotify next"));
        assert!(wants_media("pause"));
        assert!(!wants_media("spotify"));
    }

    #[test]
    fn keywords_follow_music_first_and_the_explicit_word() {
        let state = state();
        assert_eq!(
            target(&keyword_result(MediaCommand::Pause, Some(&state))),
            (MediaCommand::Pause, Some("Chrome"))
        );
        assert_eq!(
            target(&keyword_result(MediaCommand::Next, Some(&state))),
            (MediaCommand::Next, Some("Spotify.exe"))
        );
        let now_playing = keyword_result(MediaCommand::TogglePlayPause, Some(&state));
        assert_eq!(now_playing.title.as_ref(), "Song — Artist");
        assert_eq!(
            now_playing.description.as_ref(),
            "Spotify · Paused · Enter to play"
        );
        // Before sessions are read, the launcher chooses when Enter is pressed.
        assert_eq!(
            target(&keyword_result(MediaCommand::Play, None)),
            (MediaCommand::Play, None)
        );
    }

    #[test]
    fn the_media_list_shows_the_chosen_player_then_the_others() {
        let state = state();
        let batch = media_results("", Some(&state));
        let rows: Vec<_> = batch.results.iter().map(target).collect();
        assert_eq!(
            rows,
            [
                (MediaCommand::TogglePlayPause, Some("Spotify.exe")),
                (MediaCommand::Next, Some("Spotify.exe")),
                (MediaCommand::Previous, Some("Spotify.exe")),
                (MediaCommand::TogglePlayPause, Some("Chrome")),
            ]
        );
        let playing_first = MediaState {
            policy: MediaPolicy {
                mode: PriorityMode::PlayingFirst,
                ..MediaPolicy::default()
            },
            ..state.clone()
        };
        assert_eq!(
            target(&media_results("", Some(&playing_first)).results[0]),
            (MediaCommand::TogglePlayPause, Some("Chrome"))
        );
    }

    #[test]
    fn naming_an_app_and_a_command_targets_that_app_even_when_ignored() {
        let state = MediaState {
            policy: MediaPolicy {
                ignored: Arc::from([Arc::from("chrome")]),
                ..MediaPolicy::default()
            },
            ..state()
        };
        let batch = media_results("chrome next", Some(&state));
        assert_eq!(
            target(&batch.results[0]),
            (MediaCommand::Next, Some("Chrome"))
        );
        assert!(batch
            .results
            .iter()
            .all(|result| target(result).1 == Some("Chrome")));
        assert!(media_results("winamp", Some(&state)).results.is_empty());
        // With no player visible at all, the controls send the keyboard's media keys.
        let blind = media_results("", Some(&MediaState::default()));
        assert_eq!(blind.message, NO_PLAYER_MESSAGE);
        assert_eq!(
            batch_targets(&blind),
            [
                (MediaCommand::TogglePlayPause, None),
                (MediaCommand::Next, None),
                (MediaCommand::Previous, None),
            ]
        );
        assert_eq!(
            keyword_result(MediaCommand::Pause, Some(&MediaState::default()))
                .description
                .as_ref(),
            MEDIA_KEY_DETAIL
        );
    }

    #[test]
    fn shuffle_and_repeat_are_offered_by_the_player_that_has_them_and_say_what_enter_does() {
        let mut state = state();
        // Nothing open offers either: the word still answers, and says why Enter does nothing.
        let nobody = keyword_result(MediaCommand::Shuffle, Some(&state));
        assert_eq!(target(&nobody), (MediaCommand::Shuffle, None));
        assert_eq!(
            nobody.description.as_ref(),
            "No open player lets Core change this"
        );
        assert!(batch_targets(&media_results("", Some(&state)))
            .iter()
            .all(|(command, _)| !matches!(command, MediaCommand::Shuffle | MediaCommand::Repeat)));

        let mut sessions = state.sessions.to_vec();
        sessions[1].controls.shuffle = Some(false);
        sessions[1].controls.repeat = Some(RepeatMode::All);
        state.sessions = sessions.into();
        let shuffle = keyword_result(MediaCommand::Shuffle, Some(&state));
        assert_eq!(
            target(&shuffle),
            (MediaCommand::Shuffle, Some("Spotify.exe"))
        );
        assert_eq!(shuffle.title.as_ref(), "Shuffle");
        assert_eq!(
            shuffle.description.as_ref(),
            "Spotify · Off · Enter to turn on"
        );
        // The player's own rows gain both, after next and previous.
        let batch = media_results("", Some(&state));
        assert_eq!(
            batch_targets(&batch),
            [
                (MediaCommand::TogglePlayPause, Some("Spotify.exe")),
                (MediaCommand::Next, Some("Spotify.exe")),
                (MediaCommand::Previous, Some("Spotify.exe")),
                (MediaCommand::Shuffle, Some("Spotify.exe")),
                (MediaCommand::Repeat, Some("Spotify.exe")),
                (MediaCommand::TogglePlayPause, Some("Chrome")),
            ]
        );
        assert_eq!(
            batch.results[4].description.as_ref(),
            "Spotify · Repeating everything · Enter to repeat one track"
        );
        // `@media loop` puts repeat first and does not list it twice.
        let asked = media_results("loop", Some(&state));
        assert_eq!(
            target(&asked.results[0]),
            (MediaCommand::Repeat, Some("Spotify.exe"))
        );
        assert_eq!(
            batch_targets(&asked)
                .iter()
                .filter(|(command, _)| *command == MediaCommand::Repeat)
                .count(),
            1
        );
        // A named player that lacks the setting keeps the row and says so.
        let chrome = media_results("chrome shuffle", Some(&state));
        assert_eq!(
            target(&chrome.results[0]),
            (MediaCommand::Shuffle, Some("Chrome"))
        );
        assert_eq!(
            chrome.results[0].description.as_ref(),
            "Google Chrome · No open player lets Core change this"
        );
        // No player at all: the keyboard has no key for these.
        assert_eq!(
            keyword_result(MediaCommand::Repeat, Some(&MediaState::default()))
                .description
                .as_ref(),
            "No media app is open"
        );
    }

    #[test]
    fn a_named_app_keeps_controls_it_cannot_do_now_instead_of_switching_players() {
        let state = state();
        // Chrome is playing, so play has nothing to do there; it must not start Spotify.
        let batch = media_results("chrome play", Some(&state));
        assert_eq!(
            target(&batch.results[0]),
            (MediaCommand::Play, Some("Chrome"))
        );
        assert_eq!(
            batch.results[0].description.as_ref(),
            "Google Chrome · Already playing"
        );
        // Spotify is paused: pause stays on Spotify.
        let batch = media_results("spotify pause", Some(&state));
        assert_eq!(
            target(&batch.results[0]),
            (MediaCommand::Pause, Some("Spotify.exe"))
        );
    }

    #[test]
    fn unread_or_unavailable_sessions_still_offer_the_controls() {
        let loading = media_results("next", None);
        assert_eq!(loading.message, LOADING_MESSAGE);
        assert_eq!(target(&loading.results[0]), (MediaCommand::Next, None));
        assert_eq!(loading.results.len(), 3);
        let unavailable = MediaState {
            unavailable: true,
            ..MediaState::default()
        };
        assert_eq!(
            media_results("", Some(&unavailable)).message,
            UNAVAILABLE_MESSAGE
        );
    }
}
