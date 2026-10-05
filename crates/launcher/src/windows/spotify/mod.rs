//! Optional Spotify worker. Nothing starts until an explicit song search or settings action.
mod api;
mod authorization;
mod encoding;
mod playback;
mod token_store;
mod volume;
use crate::windows::settings::SpotifySettings;
use api::Client;
pub use authorization::REDIRECT_URI;
use core_engine::search::{Song, SongSearch, SongStatus};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const SPOTIFY_READY: u32 = WM_APP + 19;

pub fn load_artwork(url: &str) -> Option<crate::windows::application_icon::ApplicationIcon> {
    let path = api::artwork_path(url)?;
    let response = crate::windows::http::get(
        crate::windows::http::Request {
            secure: true,
            host: "i.scdn.co",
            port: 443,
            path,
            max_bytes: 2 * 1024 * 1024,
        },
        std::time::Instant::now() + Duration::from_secs(6),
    )
    .ok()?;
    if response.status != 200 {
        return None;
    }
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
    let icon = crate::windows::media::decode_artwork(&response.body, 48).ok();
    if initialized {
        unsafe {
            RoUninitialize();
        }
    }
    icon
}
const SEARCH_SETTLE: Duration = Duration::from_millis(350);
const DISABLED: &str = "Enable Spotify song search in Music settings first.";
const BUSY: &str = "Spotify is busy. Finish the current action and try again.";

enum Command {
    Connect,
    Disconnect,
    Play { request_id: u64, song: Song },
}
/// What the bar's volume slider asks of Spotify's volume: a change, a reading, or a change
/// and then a reading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VolumeWork {
    set: Option<u8>,
    read: bool,
}
pub enum Event {
    Search(SongSearch),
    Account(Result<(), String>),
    Playback {
        request_id: u64,
        result: Result<String, String>,
    },
    /// The active device's volume, or why Spotify could not read or change it.
    Volume(Result<u8, String>),
}

#[derive(Default)]
struct Pending {
    settings: SpotifySettings,
    revision: u64,
    search: Option<String>,
    current_query: Option<String>,
    command: Option<Command>,
    /// The slider's newest wish. Changes made faster than Spotify answers collapse into it,
    /// so a drag sends one request at a time.
    volume: Option<VolumeWork>,
    busy: bool,
    /// The command in flight starts a song, which a newer choice may take the place of.
    starting_song: bool,
    events: Vec<Event>,
    shutdown: bool,
}
#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

struct Work {
    revision: u64,
    settings: SpotifySettings,
    command: Option<Command>,
    volume: Option<VolumeWork>,
    query: Option<String>,
}

pub struct SpotifyService {
    shared: Arc<Shared>,
}

impl SpotifyService {
    pub fn new(window: HWND, folder: PathBuf) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let address = window.0 as usize;
        thread::Builder::new()
            .name("core-spotify".into())
            .spawn(move || run(address, folder.join("spotify-token.bin"), worker_shared))
            .map_err(|_| "Could not start the Spotify worker.")?;
        Ok(Self { shared })
    }

    pub fn configure(&self, settings: SpotifySettings) {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        if pending.settings == settings {
            return;
        }
        pending.settings = settings;
        invalidate(&mut pending);
        self.shared.wake.notify_one();
    }

    pub fn search(&self, query: String) {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        pending.current_query = Some(query.clone());
        pending.search = Some(query);
        self.shared.wake.notify_one();
    }

    pub fn cancel_search(&self) {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        pending.current_query = None;
        pending.search = None;
        pending
            .events
            .retain(|event| !matches!(event, Event::Search(_)));
    }

    pub fn connect(&self) -> Result<(), String> {
        self.command(Command::Connect)
    }
    /// Starts `song`. A newer choice takes the place of a song that is still waiting or being
    /// started, so choosing again never finds Spotify busy: the earlier song's checks stop at
    /// their next step and its result is dropped. Only a sign-in is not interrupted.
    pub fn play(&self, request_id: u64, song: &Song) -> Result<(), String> {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        if !pending.settings.enabled {
            return Err(DISABLED.into());
        }
        let song_waiting = matches!(pending.command, Some(Command::Play { .. }));
        let other_waiting = pending.command.is_some() && !song_waiting;
        let other_in_flight = pending.busy && !pending.starting_song;
        if other_waiting || other_in_flight {
            return Err(BUSY.into());
        }
        if pending.starting_song {
            // Work compares this number to learn that it was given up on.
            pending.revision += 1;
        }
        pending.command = Some(Command::Play {
            request_id,
            song: song.clone(),
        });
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        invalidate(&mut pending);
        pending.command = Some(Command::Disconnect);
        self.shared.wake.notify_one();
        Ok(())
    }

    fn command(&self, command: Command) -> Result<(), String> {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        if !pending.settings.enabled {
            return Err(DISABLED.into());
        }
        if pending.busy || pending.command.is_some() {
            return Err(BUSY.into());
        }
        pending.command = Some(command);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Reads the active device's volume; the answer arrives as `Event::Volume`.
    pub fn read_volume(&self) -> Result<(), String> {
        self.ask_volume(|work| work.read = true)
    }

    /// Sets it. Only a failure is answered: the slider already shows what the person chose.
    pub fn set_volume(&self, percent: u8) -> Result<(), String> {
        self.ask_volume(|work| work.set = Some(percent))
    }

    /// Unlike a command, a wish for the volume never finds Spotify busy: it waits its turn,
    /// and a newer one replaces it.
    fn ask_volume(&self, ask: impl FnOnce(&mut VolumeWork)) -> Result<(), String> {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        if !pending.settings.enabled {
            return Err(DISABLED.into());
        }
        ask(pending.volume.get_or_insert_with(VolumeWork::default));
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(
            &mut self
                .shared
                .pending
                .lock()
                .expect("Spotify work lock")
                .events,
        )
    }
}

fn invalidate(pending: &mut Pending) {
    pending.revision += 1;
    pending.search = None;
    pending.current_query = None;
    pending.command = None;
    pending.volume = None;
    pending.events.clear();
}

impl Drop for SpotifyService {
    fn drop(&mut self) {
        let mut pending = self.shared.pending.lock().expect("Spotify work lock");
        pending.shutdown = true;
        invalidate(&mut pending);
        self.shared.wake.notify_one();
    }
}

fn run(address: usize, path: PathBuf, shared: Arc<Shared>) {
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
    serve(address, path, &shared);
    if initialized {
        unsafe {
            RoUninitialize();
        }
    }
}

fn serve(address: usize, path: PathBuf, shared: &Arc<Shared>) {
    let mut client: Option<Client> = None;
    while let Some(work) = next_work(shared) {
        if client
            .as_ref()
            .is_none_or(|client| client.settings.client_id != work.settings.client_id)
        {
            client = Some(Client::new(work.settings.clone(), path.clone()));
        }
        let cancelled = || {
            let pending = shared.pending.lock().expect("Spotify cancellation lock");
            pending.shutdown || pending.revision != work.revision
        };
        let client = client.as_mut().expect("configured Spotify client");
        client.settings = work.settings;
        let event = match work.volume {
            Some(volume) => change_volume(client, volume, &cancelled),
            None => perform(client, work.command, work.query, &cancelled),
        };
        publish(address, shared, work.revision, event);
    }
}

fn next_work(shared: &Shared) -> Option<Work> {
    let mut pending = shared.pending.lock().expect("Spotify work lock");
    loop {
        while pending.command.is_none()
            && pending.volume.is_none()
            && pending.search.is_none()
            && !pending.shutdown
        {
            pending = shared.wake.wait(pending).expect("Spotify wake lock");
        }
        if pending.shutdown {
            return None;
        }
        // Only a search waits for a pause in typing; commands and the volume act at once.
        if pending.command.is_none() && pending.volume.is_none() {
            let (settled, timeout) = shared
                .wake
                .wait_timeout(pending, SEARCH_SETTLE)
                .expect("Spotify settle lock");
            pending = settled;
            if !timeout.timed_out() {
                continue;
            }
        }
        if pending.shutdown {
            return None;
        }
        // One thing at a time: a command, else the volume, else the search, which stays
        // queued behind the other two.
        let command = pending.command.take();
        let volume = if command.is_none() {
            pending.volume.take()
        } else {
            None
        };
        let query = if command.is_none() && volume.is_none() {
            pending.search.take()
        } else {
            None
        };
        pending.busy = command.is_some();
        pending.starting_song = matches!(command, Some(Command::Play { .. }));
        return Some(Work {
            revision: pending.revision,
            settings: pending.settings.clone(),
            command,
            volume,
            query,
        });
    }
}

fn publish(address: usize, shared: &Shared, revision: u64, event: Option<Event>) {
    let mut pending = shared.pending.lock().expect("Spotify work lock");
    pending.busy = false;
    pending.starting_song = false;
    if pending.shutdown || pending.revision != revision {
        return;
    }
    let Some(event) = event else {
        return;
    };
    if let Event::Search(reading) = &event {
        if pending.current_query.as_deref() != Some(&reading.query) {
            return;
        }
        pending
            .events
            .retain(|waiting| !matches!(waiting, Event::Search(_)));
    }
    if matches!(event, Event::Volume(_)) {
        // Only the newest answer about the volume matters.
        pending
            .events
            .retain(|waiting| !matches!(waiting, Event::Volume(_)));
    }
    pending.events.push(event);
    // The shutdown lock prevents notifications from outliving Core's window.
    if let Err(error) = unsafe {
        PostMessageW(
            Some(HWND(address as *mut _)),
            SPOTIFY_READY,
            WPARAM(0),
            LPARAM(0),
        )
    } {
        eprintln!("Could not notify Core of Spotify results: {error}");
    }
}

fn perform(
    client: &mut Client,
    command: Option<Command>,
    query: Option<String>,
    cancelled: &impl Fn() -> bool,
) -> Option<Event> {
    if cancelled() {
        return None;
    }
    match command {
        Some(Command::Disconnect) => Some(Event::Account(client.disconnect())),
        Some(Command::Connect) => {
            let result = authorization::authorize(&client.settings.client_id, cancelled).and_then(
                |authorization| {
                    if cancelled() {
                        return Err("Spotify connection cancelled.".into());
                    }
                    client
                        .exchange_code(authorization)
                        .map_err(|error| error.message.to_owned())
                },
            );
            Some(Event::Account(result))
        }
        Some(Command::Play { request_id, song }) => {
            let result = client
                .play(&song.uri, song.album_uri.as_deref(), cancelled)
                .map(|device| format!("Playing in Spotify · {}", device.name))
                .map_err(|error| error.message.to_owned());
            Some(Event::Playback { request_id, result })
        }
        None => {
            let query = query?;
            if !client.settings.enabled || cancelled() {
                return None;
            }
            let (status, songs) = match client.search(&query) {
                Ok(songs) => (SongStatus::Ready, songs.into()),
                Err(error) => (error.status, Arc::from([])),
            };
            Some(Event::Search(SongSearch {
                query: query.into(),
                status,
                songs,
            }))
        }
    }
}

/// A change, then a reading. A change that worked says nothing, because the slider already
/// shows what the person chose.
fn change_volume(
    client: &mut Client,
    work: VolumeWork,
    cancelled: &impl Fn() -> bool,
) -> Option<Event> {
    if !client.settings.enabled || cancelled() {
        return None;
    }
    let level = (|| {
        if let Some(percent) = work.set {
            volume::set_volume(client, percent, cancelled)?;
        }
        if work.read {
            volume::read_volume(client).map(Some)
        } else {
            Ok(None)
        }
    })();
    match level {
        Ok(None) => None,
        Ok(Some(percent)) => Some(Event::Volume(Ok(percent))),
        Err(error) => Some(Event::Volume(Err(error.message.to_owned()))),
    }
}

pub fn connection_status(folder: Option<PathBuf>, settings: &SpotifySettings) -> String {
    let Some(folder) = folder else {
        return "Core's settings folder is unavailable.".into();
    };
    match token_store::load(&folder.join("spotify-token.bin"), &settings.client_id) {
        Ok(Some(_)) => "Connected. Enter plays on your active Spotify device.".into(),
        Ok(None) => "Connect Spotify once, then search with @song.".into(),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song {
            uri: "spotify:track:0123456789abcdefghijkl".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            album_uri: Some("spotify:album:abcdefghijkl0123456789".into()),
            artwork: None,
        }
    }

    #[test]
    fn configuration_changes_discard_stale_work_and_results() {
        let service = SpotifyService {
            shared: Arc::new(Shared::default()),
        };
        service.configure(SpotifySettings {
            enabled: true,
            client_id: "a".repeat(32),
            volume: true,
        });
        service.search("old song".into());
        service.set_volume(40).unwrap();
        service
            .shared
            .pending
            .lock()
            .unwrap()
            .events
            .push(Event::Search(SongSearch::default()));
        service.configure(SpotifySettings::default());
        let pending = service.shared.pending.lock().unwrap();
        assert!(
            pending.search.is_none()
                && pending.current_query.is_none()
                && pending.volume.is_none()
                && pending.events.is_empty()
        );
        drop(pending);
        assert!(service.play(1, &song()).is_err());
        // Switched off, Spotify is not asked about the volume either.
        assert!(service.read_volume().is_err());
        assert!(service.set_volume(40).is_err());
        assert!(service.shared.pending.lock().unwrap().volume.is_none());
    }

    #[test]
    fn the_volume_acts_at_once_and_leaves_a_search_waiting_for_its_typing_pause() {
        let service = SpotifyService {
            shared: Arc::new(Shared::default()),
        };
        service.configure(SpotifySettings {
            enabled: true,
            client_id: "a".repeat(32),
            volume: true,
        });
        service.search("song".into());
        service.read_volume().unwrap();
        service.set_volume(30).unwrap();
        service.set_volume(45).unwrap();
        let started = std::time::Instant::now();
        let work = next_work(&service.shared).unwrap();
        assert!(started.elapsed() < SEARCH_SETTLE);
        // A drag's changes collapsed into the newest, and the reading asked first survived.
        assert_eq!(
            work.volume,
            Some(VolumeWork {
                set: Some(45),
                read: true
            })
        );
        assert!(work.command.is_none() && work.query.is_none());
        let work = next_work(&service.shared).unwrap();
        assert_eq!(work.query.as_deref(), Some("song"));
        assert!(work.volume.is_none());
    }

    #[test]
    fn a_newer_song_choice_replaces_one_still_starting_but_never_a_sign_in() {
        let service = SpotifyService {
            shared: Arc::new(Shared::default()),
        };
        service.configure(SpotifySettings {
            enabled: true,
            client_id: "a".repeat(32),
            volume: false,
        });
        let request = |work: &Work| match &work.command {
            Some(Command::Play { request_id, .. }) => Some(*request_id),
            _ => None,
        };
        // Still waiting: the second choice takes the first one's place.
        service.play(1, &song()).unwrap();
        service.play(2, &song()).unwrap();
        let started = next_work(&service.shared).unwrap();
        assert_eq!(request(&started), Some(2));
        // Being started and confirmed: choosing again is not refused as busy. The song in
        // flight learns it was given up on, and what it reports afterwards is dropped.
        let given_up = || service.shared.pending.lock().unwrap().revision != started.revision;
        assert!(!given_up());
        service.play(3, &song()).unwrap();
        assert!(given_up());
        service.play(4, &song()).unwrap();
        publish(
            0,
            &service.shared,
            started.revision,
            Some(Event::Playback {
                request_id: 2,
                result: Ok("Playing in Spotify".into()),
            }),
        );
        assert!(service.take_events().is_empty());
        let newest = next_work(&service.shared).unwrap();
        assert_eq!(request(&newest), Some(4));
        publish(
            0,
            &service.shared,
            newest.revision,
            Some(Event::Playback {
                request_id: 4,
                result: Ok("Playing in Spotify".into()),
            }),
        );
        assert!(matches!(
            service.take_events().as_slice(),
            [Event::Playback { request_id: 4, .. }]
        ));

        // A sign-in waits for the person in their browser and is not interrupted by a song.
        service.connect().unwrap();
        assert!(service.play(5, &song()).is_err());
        let signing_in = next_work(&service.shared).unwrap();
        assert!(matches!(signing_in.command, Some(Command::Connect)));
        assert_eq!(service.play(6, &song()), Err(BUSY.into()));
        publish(0, &service.shared, signing_in.revision, None);
        assert!(service.play(7, &song()).is_ok());
    }

    #[test]
    fn only_the_newest_answer_about_the_volume_is_kept() {
        let shared = Shared::default();
        publish(0, &shared, 0, Some(Event::Volume(Ok(30))));
        publish(
            0,
            &shared,
            0,
            Some(Event::Volume(Err("No Spotify device is active.".into()))),
        );
        let pending = shared.pending.lock().unwrap();
        assert!(matches!(pending.events.as_slice(), [Event::Volume(Err(_))]));
    }

    #[test]
    fn cancelled_work_never_reaches_authorization_or_playback() {
        let mut client = Client::new(
            SpotifySettings::default(),
            std::env::temp_dir().join("unused-core-spotify-test-token"),
        );
        assert!(perform(&mut client, Some(Command::Connect), None, &|| true).is_none());
        assert!(perform(
            &mut client,
            Some(Command::Play {
                request_id: 1,
                song: song()
            }),
            None,
            &|| true
        )
        .is_none());
        let volume = VolumeWork {
            set: Some(40),
            read: true,
        };
        assert!(change_volume(&mut client, volume, &|| true).is_none());
        // Not cancelled, but song search is off: Spotify is still not asked.
        assert!(change_volume(&mut client, volume, &|| false).is_none());
    }

    #[test]
    fn old_query_completions_and_old_configurations_are_discarded() {
        let shared = Shared::default();
        shared.pending.lock().unwrap().current_query = Some("new song".into());
        publish(
            0,
            &shared,
            0,
            Some(Event::Search(SongSearch {
                query: "old song".into(),
                ..Default::default()
            })),
        );
        publish(
            0,
            &shared,
            1,
            Some(Event::Playback {
                request_id: 1,
                result: Ok("Playing in Spotify".into()),
            }),
        );
        assert!(shared.pending.lock().unwrap().events.is_empty());
    }
}
