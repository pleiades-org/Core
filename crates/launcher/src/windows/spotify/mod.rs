//! Optional Spotify worker. Nothing starts until an explicit song search or settings action.
mod api;
mod authorization;
mod encoding;
mod token_store;
use crate::windows::settings::SpotifySettings;
use api::Client;
pub use authorization::REDIRECT_URI;
use core_engine::search::{SongSearch, SongStatus};
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

enum Command {
    Connect,
    Disconnect,
    Play { request_id: u64, uri: String },
}
pub enum Event {
    Search(SongSearch),
    Account(Result<(), String>),
    Playback {
        request_id: u64,
        result: Result<(), String>,
    },
}

#[derive(Default)]
struct Pending {
    settings: SpotifySettings,
    revision: u64,
    search: Option<String>,
    current_query: Option<String>,
    command: Option<Command>,
    busy: bool,
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
    pub fn play(&self, request_id: u64, uri: &str) -> Result<(), String> {
        self.command(Command::Play {
            request_id,
            uri: uri.to_owned(),
        })
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
            return Err("Enable Spotify song search in Music settings first.".into());
        }
        if pending.busy || pending.command.is_some() {
            return Err("Spotify is busy. Finish the current action and try again.".into());
        }
        pending.command = Some(command);
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
        let event = perform(client, work.command, work.query, &cancelled);
        publish(address, shared, work.revision, event);
    }
}

fn next_work(shared: &Shared) -> Option<Work> {
    let mut pending = shared.pending.lock().expect("Spotify work lock");
    loop {
        while pending.command.is_none() && pending.search.is_none() && !pending.shutdown {
            pending = shared.wake.wait(pending).expect("Spotify wake lock");
        }
        if pending.shutdown {
            return None;
        }
        if pending.command.is_none() {
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
        let command = pending.command.take();
        let query = if command.is_none() {
            pending.search.take()
        } else {
            None
        };
        pending.busy = command.is_some();
        return Some(Work {
            revision: pending.revision,
            settings: pending.settings.clone(),
            command,
            query,
        });
    }
}

fn publish(address: usize, shared: &Shared, revision: u64, event: Option<Event>) {
    let mut pending = shared.pending.lock().expect("Spotify work lock");
    pending.busy = false;
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
        Some(Command::Play { request_id, uri }) => {
            let result = client.play(&uri).map_err(|error| error.message.to_owned());
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

pub fn connection_status(folder: Option<PathBuf>, settings: &SpotifySettings) -> String {
    let Some(folder) = folder else {
        return "Core's settings folder is unavailable.".into();
    };
    match token_store::load(&folder.join("spotify-token.bin"), &settings.client_id) {
        Ok(Some(_)) => "Connected. Enter on a song plays it on your active Spotify device.".into(),
        Ok(None) => "Connect Spotify once, then search with @song.".into(),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_changes_discard_stale_work_and_results() {
        let service = SpotifyService {
            shared: Arc::new(Shared::default()),
        };
        service.configure(SpotifySettings {
            enabled: true,
            client_id: "a".repeat(32),
        });
        service.search("old song".into());
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
                && pending.events.is_empty()
        );
        drop(pending);
        assert!(service
            .play(1, "spotify:track:0123456789abcdefghijkl")
            .is_err());
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
                uri: "spotify:track:0123456789abcdefghijkl".into()
            }),
            None,
            &|| true
        )
        .is_none());
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
                result: Ok(()),
            }),
        );
        assert!(shared.pending.lock().unwrap().events.is_empty());
    }
}
