//! Song search is independent of the local search worker and existing media controls.
use super::LauncherState;
use crate::windows::{
    settings::SpotifySettings,
    spotify::{Event, SpotifyService},
};
use core_engine::search::{song_query, Song, SongSearch, SongStatus};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct SongFlow {
    service: Option<SpotifyService>,
    settings: SpotifySettings,
    requested: Option<String>,
    pub snapshot: Option<Arc<SongSearch>>,
}

impl LauncherState {
    fn spotify_settings(&self) -> SpotifySettings {
        self.auto_save.latest(&self.settings.saved).music.spotify
    }

    pub(super) fn spotify_settings_changed(&mut self) {
        let settings = self.spotify_settings();
        if self.songs.settings == settings {
            return;
        }
        self.songs.settings = settings.clone();
        self.songs.requested = None;
        self.songs.snapshot = None;
        if let Some(service) = &self.songs.service {
            service.configure(settings);
        }
        if let Some(view) = &self.view {
            view.spotify_status(&crate::windows::spotify::connection_status(
                self.settings.folder(),
                &self.spotify_settings(),
            ));
        }
    }

    fn spotify_service(&mut self) -> Result<&SpotifyService, String> {
        if !self.options.network || self.options.dry_run || self.options.probe {
            return Err("Spotify connections are disabled in dry runs and probes.".into());
        }
        self.spotify_settings_changed();
        if self.songs.service.is_none() {
            let folder = self.settings.folder().ok_or(
                "Core's settings folder is unavailable. Spotify cannot save its connection.",
            )?;
            let service = SpotifyService::new(self.window, folder)?;
            service.configure(self.spotify_settings());
            self.songs.service = Some(service);
        }
        Ok(self.songs.service.as_ref().expect("Spotify service"))
    }

    pub(super) fn songs_for_query(&mut self, query: &str) {
        self.spotify_settings_changed();
        let Some(payload) = song_query(query) else {
            self.songs.requested = None;
            self.songs.snapshot = None;
            if let Some(service) = &self.songs.service {
                service.cancel_search();
            }
            return;
        };
        if self.songs.requested.as_deref() == Some(payload) {
            return;
        }
        let settings = self.spotify_settings();
        let status = match (
            settings.enabled,
            settings.client_id.is_empty(),
            payload.is_empty(),
        ) {
            (false, _, _) => SongStatus::Disabled,
            (_, true, _) => SongStatus::SetupRequired,
            (_, _, true) => SongStatus::Ready,
            _ => SongStatus::Loading,
        };
        self.songs.requested = Some(payload.to_owned());
        self.songs.snapshot = Some(Arc::new(SongSearch {
            query: payload.into(),
            status,
            songs: Arc::from([]),
        }));
        if status != SongStatus::Loading {
            return;
        }
        match self.spotify_service() {
            Ok(service) => service.search(payload.to_owned()),
            Err(_) => {
                self.songs.snapshot = Some(Arc::new(SongSearch {
                    query: payload.into(),
                    status: SongStatus::NetworkError,
                    songs: Arc::from([]),
                }))
            }
        }
    }

    pub fn receive_spotify(&mut self) {
        let events = self
            .songs
            .service
            .as_ref()
            .map(SpotifyService::take_events)
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Search(reading) => {
                    if self.songs.requested.as_deref() != Some(&reading.query) {
                        continue;
                    }
                    self.songs.snapshot = Some(Arc::new(reading));
                    if self.visible {
                        self.queue_search();
                    }
                }
                Event::Account(result) => {
                    self.songs.requested = None;
                    self.songs.snapshot = None;
                    let text = result.map_or_else(
                        |error| error,
                        |()| {
                            crate::windows::spotify::connection_status(
                                self.settings.folder(),
                                &self.spotify_settings(),
                            )
                        },
                    );
                    if let Some(view) = &self.view {
                        view.spotify_status(&text);
                    }
                    if self.visible {
                        self.queue_search();
                    }
                }
                Event::Playback(result) => {
                    if let Some(view) = &self.view {
                        view.set_footer(&result.map_or_else(
                            |error| error,
                            |()| "Playing the selected song in Spotify".into(),
                        ));
                    }
                    self.request_media_reading();
                }
            }
        }
    }

    pub(super) fn connect_spotify(&mut self) {
        let result = (|| {
            let settings = self.spotify_settings();
            settings.validate()?;
            if !settings.enabled || settings.client_id.is_empty() {
                return Err("Enable song search and add your Spotify app Client ID first.".into());
            }
            self.spotify_service()?.connect()
        })();
        let text = result.map_or_else(
            |error| error,
            |()| {
                "Finish signing in to Spotify in your browser. This expires after three minutes."
                    .into()
            },
        );
        if let Some(view) = &self.view {
            view.spotify_status(&text);
        }
    }

    pub(super) fn disconnect_spotify(&mut self) {
        let result = self.spotify_service().and_then(SpotifyService::disconnect);
        let text = result.map_or_else(|error| error, |()| "Disconnecting Spotify…".into());
        self.songs.requested = None;
        self.songs.snapshot = None;
        if let Some(view) = &self.view {
            view.spotify_status(&text);
        }
    }

    pub(super) fn play_song(&mut self, song: Song) {
        let result = self
            .spotify_service()
            .and_then(|service| service.play(&song.uri));
        if let Some(view) = &self.view {
            view.set_footer(&result.map_or_else(
                |error| error,
                |()| format!("Starting {} in Spotify…", song.title),
            ));
        }
    }
}
