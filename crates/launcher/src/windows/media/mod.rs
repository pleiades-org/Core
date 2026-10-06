//! Windows' media sessions: what is playing, its controls and album art. One worker thread owns
//! every WinRT object. It starts the first time media is used; while Core is hidden it sleeps
//! with no subscriptions or timers, and change events are watched only while Core is visible.

/// Waits at most `$limit` for a WinRT operation, so a player that never answers cannot stall the
/// media worker. The status is checked every 2 ms, and only while an operation is in flight.
macro_rules! finish {
    ($operation:expr, $limit:expr) => {{
        let operation = $operation;
        let deadline = std::time::Instant::now() + $limit;
        loop {
            // 0 is AsyncStatus::Started; any other status has a result or an error.
            if operation.Status()?.0 != 0 {
                break operation.GetResults();
            }
            if std::time::Instant::now() >= deadline {
                let _ = operation.Cancel();
                break Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    "The media app did not answer in time.",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }};
}

mod album_art;
mod app_volume;
mod hotkeys;
mod mixer;
mod read_sessions;
mod send_command;
mod session_worker;
mod title_watch;
mod window_players;

pub use hotkeys::MediaHotkeys;
pub use title_watch::{TitleWatch, MEDIA_TITLE_CHANGED};

pub fn decode_artwork(
    bytes: &[u8],
    edge: u32,
) -> windows::core::Result<super::application_icon::ApplicationIcon> {
    album_art::from_bytes(bytes, edge)
}

use super::application_icon::ApplicationIcon;
use core_engine::media::{
    MediaCommand, MediaPolicy, MediaSession, MixerApp, PlaybackState, VolumeLevel,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const MEDIA_READY: u32 = WM_APP + 16;
/// Presses queued while the worker is busy; more than this are dropped rather than replayed late.
const QUEUED_REQUEST_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaAction {
    Control(MediaCommand),
    /// Move to this position, in 100 ns units from the start of the track.
    Seek(i64),
}

#[derive(Clone, Debug)]
pub struct MediaRequest {
    pub action: MediaAction,
    /// The player to use. None lets the worker choose with `policy` from what it reads then.
    pub target: Option<Arc<str>>,
    pub policy: MediaPolicy,
    pub sticky: Option<Arc<str>>,
    /// Without Windows' media sessions, send the keyboard media key instead.
    pub key_fallback: bool,
}

#[derive(Debug)]
pub struct MediaOutcome {
    pub action: MediaAction,
    pub app_id: Option<Arc<str>>,
    pub app_name: Option<Arc<str>>,
    /// The player's state when the command was sent.
    pub before: Option<PlaybackState>,
    pub result: Result<(), String>,
}

pub struct MediaReading {
    pub sessions: Vec<MediaSession>,
    /// Album art for the sessions that have it, when art was asked for.
    pub art: Vec<(Arc<str>, Arc<ApplicationIcon>)>,
    /// Windows' media sessions could not be read; commands use the media keys instead.
    pub unavailable: bool,
    /// Only track positions changed; titles, states and art are as in the last full reading.
    pub timeline_only: bool,
    /// Processes of players read from their window, whose title changes Core watches while
    /// visible, since such players announce nothing to Windows.
    pub window_processes: Vec<u32>,
}

/// What the bar's volume slider asks of a player's volume in Windows' mixer: a change, a
/// reading, or a change and then a reading.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VolumeWork {
    app_id: Arc<str>,
    set: Option<u8>,
    read: bool,
}

pub struct VolumeReading {
    pub app_id: Arc<str>,
    /// The level read, or why the player has no volume in Windows' mixer right now.
    pub level: Result<VolumeLevel, String>,
}

/// A change the person made to one row of the volume mixer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerChange {
    Volume(u8),
    Muted(bool),
}

/// What `@volume` asks of Windows' mixer: changes to its rows, and then the mixer read again
/// when that is wanted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MixerWork {
    changes: Vec<(Arc<str>, MixerChange)>,
    read: bool,
}

#[derive(Default)]
struct Work {
    /// Core asked to read the sessions again.
    requested: bool,
    /// A player announced a change; read again once its burst of events settles.
    changed: bool,
    /// Only a track position changed.
    timelines: bool,
    /// Watch for changes: while Core is visible.
    watch: bool,
    /// Core was hidden; drop the subscriptions without reading again.
    watch_changed: bool,
    /// Album art size in pixels; 0 when no art is shown.
    art_edge: u32,
    requests: VecDeque<MediaRequest>,
    /// The slider's newest wish; it acts at once, without waiting for players to settle.
    volume: Option<VolumeWork>,
    /// The mixer's waiting changes and reading; they too act at once.
    mixer: Option<MixerWork>,
    shutdown: bool,
}

impl Work {
    fn pending(&self) -> bool {
        self.requested
            || self.changed
            || self.timelines
            || self.watch_changed
            || !self.requests.is_empty()
            || self.volume.is_some()
            || self.mixer.is_some()
    }
}

#[derive(Default)]
struct Shared {
    work: Mutex<Work>,
    wake: Condvar,
    reading: Mutex<Option<MediaReading>>,
    outcomes: Mutex<Vec<MediaOutcome>>,
    volume: Mutex<Option<VolumeReading>>,
    mixer: Mutex<Option<Vec<MixerApp>>>,
}

impl Shared {
    /// Called on Windows' thread pool by session events; the worker does the reading.
    fn announce(&self, timeline_only: bool) {
        let mut work = self.work.lock().expect("media work lock");
        if timeline_only {
            work.timelines = true;
        } else {
            work.changed = true;
        }
        self.wake.notify_one();
    }
}

pub struct MediaService {
    shared: Arc<Shared>,
}

impl MediaService {
    pub fn start(window: HWND) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let address = window.0 as usize;
        let worker = shared.clone();
        // Detached: a player's slow answer must not delay exit. Shutdown stops it posting.
        thread::Builder::new()
            .name("core-media".into())
            .spawn(move || session_worker::run(address, worker))?;
        Ok(Self { shared })
    }

    /// Reads the sessions again. `watch` keeps them current as players change, for while Core
    /// is visible; `art_edge` is the album art size in pixels, or 0 for none.
    pub fn refresh(&self, watch: bool, art_edge: u32) {
        let mut work = self.shared.work.lock().expect("media work lock");
        work.requested = true;
        work.watch = watch;
        work.art_edge = art_edge;
        self.shared.wake.notify_one();
    }

    /// Core was hidden: drop the change subscriptions.
    pub fn stop_watching(&self) {
        let mut work = self.shared.work.lock().expect("media work lock");
        if work.watch {
            work.watch = false;
            work.watch_changed = true;
            self.shared.wake.notify_one();
        }
    }

    /// False when too many presses are already waiting.
    pub fn send(&self, request: MediaRequest) -> bool {
        let mut work = self.shared.work.lock().expect("media work lock");
        if work.requests.len() >= QUEUED_REQUEST_LIMIT {
            return false;
        }
        work.requests.push_back(request);
        self.shared.wake.notify_one();
        true
    }

    /// A player read from its window changed its title: read again once the change settles.
    pub fn player_changed(&self) {
        self.shared.announce(false);
    }

    pub fn take_reading(&self) -> Option<MediaReading> {
        self.shared
            .reading
            .lock()
            .expect("media reading lock")
            .take()
    }

    pub fn take_outcomes(&self) -> Vec<MediaOutcome> {
        std::mem::take(&mut *self.shared.outcomes.lock().expect("media outcome lock"))
    }

    /// Reads a player's volume in Windows' mixer; the reading arrives as the sessions do.
    pub fn read_volume(&self, app_id: Arc<str>) {
        self.ask_volume(app_id, |work| work.read = true);
    }

    /// Sets it. Changes made faster than the worker applies them collapse into the newest.
    pub fn set_volume(&self, app_id: Arc<str>, percent: u8) {
        self.ask_volume(app_id, |work| work.set = Some(percent));
    }

    /// What still waits for the same player is kept, so a reading asked for just before a
    /// drag is not lost; anything waiting for another player is dropped.
    fn ask_volume(&self, app_id: Arc<str>, ask: impl FnOnce(&mut VolumeWork)) {
        let mut work = self.shared.work.lock().expect("media work lock");
        let mut volume = work
            .volume
            .take()
            .filter(|waiting| waiting.app_id == app_id)
            .unwrap_or(VolumeWork {
                app_id,
                set: None,
                read: false,
            });
        ask(&mut volume);
        work.volume = Some(volume);
        self.shared.wake.notify_one();
    }

    pub fn take_volume(&self) -> Option<VolumeReading> {
        self.shared.volume.lock().expect("media volume lock").take()
    }

    /// Reads Windows' volume mixer for `@volume`; the list arrives as the sessions do.
    pub fn read_mixer(&self) {
        let mut work = self.shared.work.lock().expect("media work lock");
        work.mixer.get_or_insert_with(MixerWork::default).read = true;
        self.shared.wake.notify_one();
    }

    /// Changes one row of the mixer, and reads the mixer again afterwards when `then_read`.
    /// A newer volume for a row replaces one still waiting, so a drag never falls behind.
    pub fn change_mixer(&self, id: Arc<str>, change: MixerChange, then_read: bool) {
        let mut work = self.shared.work.lock().expect("media work lock");
        let mixer = work.mixer.get_or_insert_with(MixerWork::default);
        if matches!(change, MixerChange::Volume(_)) {
            mixer.changes.retain(|(waiting, earlier)| {
                *waiting != id || !matches!(earlier, MixerChange::Volume(_))
            });
        }
        mixer.changes.push((id, change));
        mixer.read |= then_read;
        self.shared.wake.notify_one();
    }

    pub fn take_mixer(&self) -> Option<Vec<MixerApp>> {
        self.shared.mixer.lock().expect("media mixer lock").take()
    }
}

impl Drop for MediaService {
    fn drop(&mut self) {
        self.shared.work.lock().expect("media work lock").shutdown = true;
        self.shared.wake.notify_one();
    }
}

fn post_ready(address: usize) {
    if let Err(error) = unsafe {
        PostMessageW(
            Some(HWND(address as *mut _)),
            MEDIA_READY,
            WPARAM(0),
            LPARAM(0),
        )
    } {
        eprintln!("Could not deliver media sessions: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    pub(super) fn wait_for<T>(mut poll: impl FnMut() -> Option<T>, what: &str) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = poll() {
                return found;
            }
            assert!(Instant::now() < deadline, "{what} did not arrive");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn mixer_changes_keep_their_order_and_only_the_newest_volume_of_a_row() {
        let service = MediaService {
            shared: Arc::new(Shared::default()),
        };
        let (spotify, game): (Arc<str>, Arc<str>) = ("spotify".into(), "game".into());
        service.read_mixer();
        service.change_mixer(spotify.clone(), MixerChange::Volume(40), false);
        service.change_mixer(game.clone(), MixerChange::Volume(10), false);
        service.change_mixer(spotify.clone(), MixerChange::Muted(true), false);
        service.change_mixer(spotify.clone(), MixerChange::Volume(55), false);
        let work = service.shared.work.lock().unwrap();
        assert!(work.pending());
        assert_eq!(
            work.mixer,
            Some(MixerWork {
                changes: vec![
                    (game, MixerChange::Volume(10)),
                    (spotify.clone(), MixerChange::Muted(true)),
                    (spotify, MixerChange::Volume(55)),
                ],
                // The reading asked for first is still owed after the changes.
                read: true,
            })
        );
        drop(work);
        assert!(service.take_mixer().is_none());
    }

    #[test]
    fn slider_wishes_for_one_player_merge_and_another_player_replaces_them() {
        let service = MediaService {
            shared: Arc::new(Shared::default()),
        };
        let waiting = || service.shared.work.lock().unwrap().volume.clone();
        service.read_volume("Spotify.exe".into());
        service.set_volume("Spotify.exe".into(), 40);
        service.set_volume("Spotify.exe".into(), 55);
        assert_eq!(
            waiting(),
            Some(VolumeWork {
                app_id: "Spotify.exe".into(),
                set: Some(55),
                read: true,
            })
        );
        assert!(service.shared.work.lock().unwrap().pending());
        service.read_volume("Chrome".into());
        assert_eq!(
            waiting(),
            Some(VolumeWork {
                app_id: "Chrome".into(),
                set: None,
                read: true,
            })
        );
        assert!(service.take_volume().is_none());
    }

    /// The whole path a press takes, against the person's own player. It only asks a player
    /// that is already playing to play, which changes nothing; with nothing playing it skips.
    /// Run by hand: `cargo test -p core-launcher-v2 media_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore = "sends a command to the person's own player"]
    fn media_round_trip() {
        // A null window makes the ready notifications harmless thread messages.
        let service = MediaService::start(HWND::default()).unwrap();
        service.refresh(false, 0);
        let reading = wait_for(|| service.take_reading(), "A reading");
        let Some(playing) = reading
            .sessions
            .iter()
            .find(|session| session.state == PlaybackState::Playing)
        else {
            println!("Nothing is playing; skipped.");
            return;
        };
        assert!(service.send(MediaRequest {
            action: MediaAction::Control(MediaCommand::Play),
            target: Some(playing.app_id.clone()),
            policy: MediaPolicy::default(),
            sticky: None,
            key_fallback: false,
        }));
        let outcomes = wait_for(
            || Some(service.take_outcomes()).filter(|outcomes| !outcomes.is_empty()),
            "The outcome",
        );
        println!("{outcomes:?}");
        assert_eq!(outcomes[0].app_id.as_deref(), Some(&*playing.app_id));
        assert_eq!(outcomes[0].before, Some(PlaybackState::Playing));
        assert_eq!(outcomes[0].result, Ok(()));
    }
}
