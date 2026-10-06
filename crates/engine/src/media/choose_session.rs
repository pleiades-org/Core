//! Which session a command or the now-playing bar uses. Windows' own choice (the session that
//! last took the media keys) often lands on a paused browser tab; these rules prefer music.
use super::{app_key, MediaCommand, MediaSession, PlaybackState};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PriorityMode {
    /// Music apps win even while something else plays, so play resumes the music.
    #[default]
    MusicFirst,
    /// Whatever is playing wins; music apps only break ties.
    PlayingFirst,
}

impl PriorityMode {
    pub const ALL: [Self; 2] = [Self::MusicFirst, Self::PlayingFirst];

    pub fn label(self) -> &'static str {
        match self {
            Self::MusicFirst => "Music apps first",
            Self::PlayingFirst => "Playing media first",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::MusicFirst => "music-first",
            Self::PlayingFirst => "playing-first",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| text.eq_ignore_ascii_case(mode.id()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaPolicy {
    pub mode: PriorityMode,
    /// App keys in the person's order; an earlier app wins over a later or unlisted one.
    pub preferred: Arc<[Arc<str>]>,
    /// App keys Core never chooses by itself; `@media <app>` still reaches them.
    pub ignored: Arc<[Arc<str>]>,
}

impl MediaPolicy {
    fn preference(&self, key: &str) -> usize {
        self.preferred
            .iter()
            .position(|preferred| &**preferred == key)
            .unwrap_or(self.preferred.len())
    }

    pub fn ignores(&self, key: &str) -> bool {
        self.ignored.iter().any(|ignored| &**ignored == key)
    }
}

/// The session `command` goes to. An explicit pause only considers what is playing, so with
/// Music-first `pause` still stops what you hear. An explicit play resumes a paused player only
/// if it ranks above everything already playing: with Spotify playing, `play` does not also
/// start a paused browser tab. A player that does not allow the command right now (a stopped
/// app with play disabled, a tab without a next track) is passed over. `sticky` (the app Core
/// just commanded) keeps the target while players change tracks.
pub fn choose_target(
    sessions: &[MediaSession],
    policy: &MediaPolicy,
    command: MediaCommand,
    sticky: Option<&str>,
) -> Option<usize> {
    let allowed = |session: &MediaSession| session.controls.allows(command, session.state);
    if command == MediaCommand::Play {
        let paused = best(sessions, policy, sticky, |session| {
            session.state != PlaybackState::Playing && allowed(session)
        })?;
        let outranked = sessions.iter().any(|session| {
            let key = app_key(&session.app_id);
            session.state == PlaybackState::Playing
                && !policy.ignores(&key)
                && app_rank(session, &key, policy)
                    <= app_rank(
                        &sessions[paused],
                        &app_key(&sessions[paused].app_id),
                        policy,
                    )
        });
        return (!outranked).then_some(paused);
    }
    best(sessions, policy, sticky, |session| {
        allowed(session)
            && (command != MediaCommand::Pause || session.state == PlaybackState::Playing)
    })
}

/// The session the now-playing bar shows: one that is playing, or one Core paused, so it can be
/// resumed from the bar. A player paused in its own window does not keep the bar open.
pub fn bar_session(
    sessions: &[MediaSession],
    policy: &MediaPolicy,
    sticky: Option<&str>,
    paused_by_core: Option<&str>,
) -> Option<usize> {
    best(sessions, policy, sticky, |session| {
        session.state == PlaybackState::Playing || paused_by_core == Some(&*session.app_id)
    })
}

fn best(
    sessions: &[MediaSession],
    policy: &MediaPolicy,
    sticky: Option<&str>,
    eligible: impl Fn(&MediaSession) -> bool,
) -> Option<usize> {
    let candidates: Vec<(usize, &MediaSession, Arc<str>)> = sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| eligible(session))
        .map(|(index, session)| (index, session, app_key(&session.app_id)))
        .filter(|(_, _, key)| !policy.ignores(key))
        .collect();
    if let Some(sticky) = sticky {
        if let Some((index, _, _)) = candidates
            .iter()
            .find(|(_, session, _)| &*session.app_id == sticky)
        {
            return Some(*index);
        }
    }
    candidates
        .iter()
        .min_by_key(|(_, session, key)| rank(session, key, policy))
        .map(|(index, _, _)| *index)
}

/// How strongly the person and the app's kind favour an app, whatever it is doing.
fn app_rank(session: &MediaSession, key: &str, policy: &MediaPolicy) -> [usize; 2] {
    [policy.preference(key), session.class as usize]
}

/// Lower ranks first. Windows' current session only breaks otherwise equal ties.
fn rank(session: &MediaSession, key: &str, policy: &MediaPolicy) -> [usize; 4] {
    let state = session.state as usize;
    let preference = policy.preference(key);
    let class = session.class as usize;
    let recency = usize::from(!session.is_system_current);
    match policy.mode {
        PriorityMode::MusicFirst => [preference, class, state, recency],
        PriorityMode::PlayingFirst => [state, preference, class, recency],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{classify_app, MediaControls};

    fn session(app_id: &str, state: PlaybackState) -> MediaSession {
        MediaSession {
            app_id: app_id.into(),
            app_name: app_id.into(),
            title: "Title".into(),
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

    fn policy(mode: PriorityMode) -> MediaPolicy {
        MediaPolicy {
            mode,
            ..MediaPolicy::default()
        }
    }

    fn chosen<'a>(
        sessions: &'a [MediaSession],
        policy: &MediaPolicy,
        command: MediaCommand,
    ) -> Option<&'a str> {
        choose_target(sessions, policy, command, None).map(|index| &*sessions[index].app_id)
    }

    #[test]
    fn with_nothing_playing_music_resumes_instead_of_a_paused_tab() {
        let mut tab = session("Chrome", PlaybackState::Paused);
        // Windows' own pick is the tab; neither mode follows it over music.
        tab.is_system_current = true;
        let sessions = [session("Spotify.exe", PlaybackState::Paused), tab];
        for mode in PriorityMode::ALL {
            for command in [
                MediaCommand::TogglePlayPause,
                MediaCommand::Play,
                MediaCommand::Next,
            ] {
                assert_eq!(
                    chosen(&sessions, &policy(mode), command),
                    Some("Spotify.exe"),
                    "{mode:?} {command:?}"
                );
            }
            assert_eq!(chosen(&sessions, &policy(mode), MediaCommand::Pause), None);
        }
    }

    #[test]
    fn the_modes_differ_only_when_something_else_is_playing() {
        let sessions = [
            session("Chrome", PlaybackState::Playing),
            session("Spotify.exe", PlaybackState::Paused),
        ];
        let music = policy(PriorityMode::MusicFirst);
        let playing = policy(PriorityMode::PlayingFirst);
        assert_eq!(
            chosen(&sessions, &music, MediaCommand::TogglePlayPause),
            Some("Spotify.exe")
        );
        assert_eq!(
            chosen(&sessions, &playing, MediaCommand::TogglePlayPause),
            Some("Chrome")
        );
        // An explicit pause stops what you hear, and play resumes the music, in both modes.
        for policy in [&music, &playing] {
            assert_eq!(
                chosen(&sessions, policy, MediaCommand::Pause),
                Some("Chrome")
            );
            assert_eq!(
                chosen(&sessions, policy, MediaCommand::Play),
                Some("Spotify.exe")
            );
        }
    }

    #[test]
    fn preferences_order_apps_and_ignored_apps_are_never_chosen() {
        let sessions = [
            session("Spotify.exe", PlaybackState::Paused),
            session(
                "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App",
                PlaybackState::Paused,
            ),
            session("Chrome", PlaybackState::Playing),
        ];
        let preferred = MediaPolicy {
            preferred: Arc::from([Arc::from("applemusic")]),
            ..MediaPolicy::default()
        };
        assert_eq!(
            chosen(&sessions, &preferred, MediaCommand::TogglePlayPause),
            Some("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App")
        );
        let ignoring = MediaPolicy {
            ignored: Arc::from([Arc::from("spotify"), Arc::from("applemusic")]),
            ..MediaPolicy::default()
        };
        assert_eq!(
            chosen(&sessions, &ignoring, MediaCommand::TogglePlayPause),
            Some("Chrome")
        );
        assert_eq!(chosen(&[], &ignoring, MediaCommand::Next), None);
    }

    #[test]
    fn the_app_core_just_commanded_stays_the_target() {
        let sessions = [
            session("Chrome", PlaybackState::Playing),
            session("Spotify.exe", PlaybackState::Paused),
        ];
        let target = choose_target(
            &sessions,
            &policy(PriorityMode::MusicFirst),
            MediaCommand::Next,
            Some("Chrome"),
        );
        assert_eq!(target, Some(0));
        // A sticky app the command cannot use is passed over.
        let target = choose_target(
            &sessions,
            &policy(PriorityMode::MusicFirst),
            MediaCommand::Play,
            Some("Chrome"),
        );
        assert_eq!(target, Some(1));
    }

    #[test]
    fn play_leaves_a_paused_tab_alone_while_music_plays() {
        let sessions = [
            session("Spotify.exe", PlaybackState::Playing),
            session("Helium.QOOR667UA6MLNAR7ZY2SGJF3MA", PlaybackState::Paused),
        ];
        for mode in PriorityMode::ALL {
            assert_eq!(chosen(&sessions, &policy(mode), MediaCommand::Play), None);
        }
        // Two tabs of equal standing: the playing one is enough.
        let tabs = [
            session("Chrome", PlaybackState::Playing),
            session("MSEdge", PlaybackState::Paused),
        ];
        assert_eq!(
            chosen(&tabs, &policy(PriorityMode::MusicFirst), MediaCommand::Play),
            None
        );
        // Preferring the paused app lets play resume it alongside.
        let preferring = MediaPolicy {
            preferred: Arc::from([Arc::from("helium")]),
            ..MediaPolicy::default()
        };
        assert_eq!(
            chosen(&sessions, &preferring, MediaCommand::Play),
            Some("Helium.QOOR667UA6MLNAR7ZY2SGJF3MA")
        );
    }

    #[test]
    fn players_that_cannot_do_the_command_are_passed_over() {
        let mut stopped = session("Spotify.exe", PlaybackState::Idle);
        stopped.controls.play = false;
        stopped.controls.next = false;
        let sessions = [stopped, session("Chrome", PlaybackState::Playing)];
        let music = policy(PriorityMode::MusicFirst);
        // Spotify ranks first, but cannot play or skip now: the playing tab is paused instead.
        assert_eq!(
            chosen(&sessions, &music, MediaCommand::TogglePlayPause),
            Some("Chrome")
        );
        assert_eq!(
            chosen(&sessions, &music, MediaCommand::Next),
            Some("Chrome")
        );
        assert_eq!(chosen(&sessions, &music, MediaCommand::Play), None);
    }

    #[test]
    fn windows_current_session_breaks_ties_between_equals() {
        let mut second = session("MSEdge", PlaybackState::Paused);
        second.is_system_current = true;
        let sessions = [session("Chrome", PlaybackState::Paused), second];
        assert_eq!(
            chosen(
                &sessions,
                &policy(PriorityMode::MusicFirst),
                MediaCommand::TogglePlayPause
            ),
            Some("MSEdge")
        );
    }

    #[test]
    fn the_bar_shows_playing_media_and_what_core_paused() {
        let paused = [session("Spotify.exe", PlaybackState::Paused)];
        let music = policy(PriorityMode::MusicFirst);
        assert_eq!(bar_session(&paused, &music, None, None), None);
        assert_eq!(
            bar_session(&paused, &music, None, Some("Spotify.exe")),
            Some(0)
        );
        let mixed = [
            session("Chrome", PlaybackState::Playing),
            session("Spotify.exe", PlaybackState::Paused),
        ];
        assert_eq!(
            bar_session(&mixed, &music, None, Some("Spotify.exe")),
            Some(1)
        );
        assert_eq!(
            bar_session(
                &mixed,
                &policy(PriorityMode::PlayingFirst),
                None,
                Some("Spotify.exe")
            ),
            Some(0)
        );
        // Paused outside Core: only what is playing qualifies.
        assert_eq!(bar_session(&mixed, &music, None, None), Some(0));
    }

    #[test]
    fn priority_modes_round_trip_through_their_identifiers() {
        for mode in PriorityMode::ALL {
            assert_eq!(PriorityMode::parse(mode.id()), Some(mode));
        }
        assert_eq!(PriorityMode::parse("loudest-first"), None);
    }
}
