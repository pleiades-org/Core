//! Players that publish no media session to Windows, read from their main window instead.
//! Spotify does this when its "Show desktop overlay when using media keys" setting is off: its
//! window title is "Artist - Title" while it plays and its own name while paused, and it answers
//! the standard media commands (WM_APPCOMMAND) sent to that window.
use core_engine::media::{
    app_key, classify_app, known_app_name, MediaCommand, MediaControls, MediaSession, PlaybackState,
};
use windows::core::BOOL;
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindow, GetWindowTextW, GetWindowThreadProcessId,
            PostMessageW, GW_OWNER, WM_APPCOMMAND,
        },
    },
};

struct KnownPlayer {
    /// The executable's file name, compared without case.
    executable: &'static str,
    /// The identifier Windows uses for the same app when it does publish a session, so a
    /// preference or the "paused by Core" mark applies whichever way it is read.
    app_id: &'static str,
    class_prefix: &'static str,
    /// Titles shown while nothing plays.
    idle_titles: &'static [&'static str],
}

const PLAYERS: &[KnownPlayer] = &[KnownPlayer {
    executable: "spotify.exe",
    app_id: "Spotify.exe",
    class_prefix: "Chrome_WidgetWin",
    idle_titles: &["Spotify", "Spotify Free", "Spotify Premium"],
}];

// WM_APPCOMMAND commands, in the high word of the long parameter.
const APPCOMMAND_MEDIA_NEXTTRACK: isize = 11;
const APPCOMMAND_MEDIA_PREVIOUSTRACK: isize = 12;
const APPCOMMAND_MEDIA_PLAY_PAUSE: isize = 14;

pub(super) struct WindowPlayer {
    pub info: MediaSession,
    pub window: isize,
    pub process: u32,
}

/// Window players for apps that Windows reports no session for; a player that publishes a
/// session is read from it instead, with art and progress.
pub(super) fn beside(sessions: &[MediaSession]) -> Vec<WindowPlayer> {
    find_window_players()
        .into_iter()
        .filter(|player| {
            let key = app_key(&player.info.app_id);
            !sessions
                .iter()
                .any(|session| app_key(&session.app_id) == key)
        })
        .collect()
}

/// The known players running now, one per app.
pub(super) fn find_window_players() -> Vec<WindowPlayer> {
    let mut windows: Vec<(isize, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut windows as *mut Vec<(isize, u32)> as isize),
        );
    }
    let mut executables: Vec<(u32, Option<String>)> = Vec::new();
    let mut players: Vec<WindowPlayer> = Vec::new();
    for (window, process) in windows {
        let executable = match executables.iter().find(|(known, _)| *known == process) {
            Some((_, name)) => name.clone(),
            None => {
                let name = executable_name(process);
                executables.push((process, name.clone()));
                name
            }
        };
        let Some(executable) = executable else {
            continue;
        };
        let handle = HWND(window as *mut _);
        let Some(player) = PLAYERS.iter().find(|player| {
            executable.eq_ignore_ascii_case(player.executable)
                && class_name(handle).starts_with(player.class_prefix)
        }) else {
            continue;
        };
        // Spotify can keep its main window hidden in the tray; its title still follows the track.
        let title = window_text(handle);
        if title.is_empty()
            || players
                .iter()
                .any(|found| &*found.info.app_id == player.app_id)
        {
            continue;
        }
        players.push(WindowPlayer {
            info: session(player, &title),
            window,
            process,
        });
    }
    players
}

/// Sends a control to a player's window. Play and pause both toggle: the target was chosen
/// because its state differs from the one asked for.
pub(super) fn send(window: isize, command: MediaCommand) -> Result<(), String> {
    let code = match command {
        MediaCommand::TogglePlayPause | MediaCommand::Play | MediaCommand::Pause => {
            APPCOMMAND_MEDIA_PLAY_PAUSE
        }
        MediaCommand::Next => APPCOMMAND_MEDIA_NEXTTRACK,
        MediaCommand::Previous => APPCOMMAND_MEDIA_PREVIOUSTRACK,
        // A window takes no such command, and these players are never offered it.
        MediaCommand::Shuffle | MediaCommand::Repeat => {
            return Err("This player's shuffle and repeat cannot be reached from Core".into())
        }
    };
    let handle = HWND(window as *mut _);
    unsafe {
        PostMessageW(
            Some(handle),
            WM_APPCOMMAND,
            WPARAM(window as usize),
            LPARAM(code << 16),
        )
    }
    .map_err(|error| format!("Could not reach the player's window: {error}"))
}

fn session(player: &KnownPlayer, title: &str) -> MediaSession {
    let (state, title, artist) = describe(title, player.idle_titles);
    let app_name = known_app_name(player.app_id);
    MediaSession {
        app_id: player.app_id.into(),
        class: classify_app(player.app_id, &app_name),
        app_name: app_name.into(),
        title: title.into(),
        artist: artist.into(),
        album: "".into(),
        state,
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

/// "Artist - Title" while playing; an idle title while paused or stopped.
fn describe<'title>(
    title: &'title str,
    idle_titles: &[&str],
) -> (PlaybackState, &'title str, &'title str) {
    let title = title.trim();
    if idle_titles
        .iter()
        .any(|idle| title.eq_ignore_ascii_case(idle))
    {
        return (PlaybackState::Paused, "", "");
    }
    match title.split_once(" - ") {
        Some((artist, track)) => (PlaybackState::Playing, track.trim(), artist.trim()),
        None => (PlaybackState::Playing, title, ""),
    }
}

/// Top-level, unowned windows of a class a known player uses, with their process. The class
/// is checked first, so only a few windows need their process looked up.
unsafe extern "system" fn collect_window(window: HWND, list: LPARAM) -> BOOL {
    let windows = &mut *(list.0 as *mut Vec<(isize, u32)>);
    let owned = GetWindow(window, GW_OWNER).is_ok_and(|owner| !owner.0.is_null());
    if !owned {
        let class = class_name(window);
        if PLAYERS
            .iter()
            .any(|player| class.starts_with(player.class_prefix))
        {
            let mut process = 0;
            GetWindowThreadProcessId(window, Some(&mut process));
            if process != 0 {
                windows.push((window.0 as isize, process));
            }
        }
    }
    BOOL(1)
}

fn window_text(window: HWND) -> String {
    let mut text = [0_u16; 512];
    let length = unsafe { GetWindowTextW(window, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..length.min(text.len())])
}

fn class_name(window: HWND) -> String {
    let mut text = [0_u16; 128];
    let length = unsafe { GetClassNameW(window, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..length.min(text.len())])
}

fn executable_name(process: u32) -> Option<String> {
    let path = executable_path(process)?;
    path.rsplit(['\\', '/']).next().map(str::to_owned)
}

/// The program a process runs, with its folders.
pub(super) fn executable_path(process: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut path = [0_u16; 1024];
    let mut length = path.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    read.ok()?;
    Some(String::from_utf16_lossy(&path[..length as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &[&str] = &["Spotify", "Spotify Free", "Spotify Premium"];

    #[test]
    fn titles_give_the_track_while_playing_and_idle_names_mean_paused() {
        assert_eq!(
            describe("SHIFT UP - Make up my mind (SGF version)", IDLE),
            (
                PlaybackState::Playing,
                "Make up my mind (SGF version)",
                "SHIFT UP"
            )
        );
        // Only the first separator splits artist from title.
        assert_eq!(
            describe("Artist - Song - Live", IDLE),
            (PlaybackState::Playing, "Song - Live", "Artist")
        );
        assert_eq!(
            describe("Spotify Premium", IDLE),
            (PlaybackState::Paused, "", "")
        );
        assert_eq!(describe(" spotify ", IDLE), (PlaybackState::Paused, "", ""));
        // An advertisement or podcast without a separator still plays.
        assert_eq!(
            describe("Advertisement", IDLE),
            (PlaybackState::Playing, "Advertisement", "")
        );
    }

    #[test]
    fn a_spotify_window_becomes_a_music_session_under_spotifys_own_identifier() {
        let session = session(&PLAYERS[0], "Prides - Out Of The Blue");
        assert_eq!(&*session.app_id, "Spotify.exe");
        assert_eq!(&*session.app_name, "Spotify");
        assert_eq!(session.class, core_engine::media::AppClass::Music);
        assert_eq!(&*session.title, "Out Of The Blue");
        assert!(session.controls.next && !session.controls.seek);
    }

    /// Prints the players found from their windows. Reads titles only; sends nothing.
    /// Run by hand: `cargo test -p core-launcher-v2 window_player_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the windows of the person's own players"]
    fn window_player_probe() {
        for player in find_window_players() {
            println!(
                "{} pid {} window {:#x}: {:?} {:?} by {:?}",
                player.info.app_id,
                player.process,
                player.window,
                player.info.state,
                player.info.title,
                player.info.artist
            );
        }
    }
}
