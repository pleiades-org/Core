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
    playback: Option<PlaybackNotice>,
    next_playback_request: u64,
    pub snapshot: Option<Arc<SongSearch>>,
}

struct PlaybackNotice {
    request_id: u64,
    query: String,
    uri: Arc<str>,
    message: String,
}

impl SongFlow {
    fn new_playback_request(&mut self) -> u64 {
        self.next_playback_request = self.next_playback_request.wrapping_add(1);
        self.next_playback_request
    }

    fn playback_started(
        &mut self,
        request_id: u64,
        query: &str,
        song: &Song,
        result: Result<(), String>,
    ) {
        self.playback = Some(PlaybackNotice {
            request_id,
            query: song_query(query).unwrap_or_default().to_owned(),
            uri: song.uri.clone(),
            message: result.map_or_else(
                |error| error,
                |()| format!("Starting {} in Spotify…", song.title),
            ),
        });
    }

    fn playback_finished(&mut self, request_id: u64, result: Result<(), String>) {
        let Some(notice) = self
            .playback
            .as_mut()
            .filter(|notice| notice.request_id == request_id)
        else {
            return;
        };
        notice.message = result.map_or_else(
            |error| error,
            |()| "Playback requested on your active Spotify device".into(),
        );
    }

    pub(super) fn playback_notice(&self, query: &str, uri: &str) -> Option<&str> {
        let notice = self.playback.as_ref()?;
        (song_query(query) == Some(&notice.query) && &*notice.uri == uri)
            .then_some(notice.message.as_str())
    }
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
        self.songs.playback = None;
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
            self.songs.playback = None;
            if let Some(service) = &self.songs.service {
                service.cancel_search();
            }
            return;
        };
        if self.songs.requested.as_deref() == Some(payload) {
            return;
        }
        self.songs.playback = None;
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
                    self.songs.playback = None;
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
                Event::Playback { request_id, result } => {
                    self.songs.playback_finished(request_id, result);
                    self.refresh_footer();
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
        self.songs.playback = None;
        if let Some(view) = &self.view {
            view.spotify_status(&text);
        }
    }

    pub(super) fn play_song(&mut self, song: Song) {
        let request_id = self.songs.new_playback_request();
        let result = self
            .spotify_service()
            .and_then(|service| service.play(request_id, &song.uri));
        let query = self
            .view
            .as_ref()
            .map(|view| view.query())
            .unwrap_or_default();
        self.songs
            .playback_started(request_id, &query, &song, result);
        self.refresh_footer();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(uri: &str) -> Song {
        Song {
            uri: uri.into(),
            title: "Selected song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            artwork: None,
        }
    }

    #[test]
    fn footer_refreshes_retain_the_playback_error_for_the_selected_result() {
        let selected = song("spotify:track:0123456789abcdefghijkl");
        let mut flow = SongFlow::default();
        flow.playback_started(1, "@song selected", &selected, Ok(()));
        flow.playback_finished(1, Err("No active Spotify device".into()));

        for _ in 0..3 {
            assert_eq!(
                flow.playback_notice("@song selected", &selected.uri),
                Some("No active Spotify device")
            );
        }
        assert!(flow
            .playback_notice("@song changed", &selected.uri)
            .is_none());
        assert!(flow.playback_notice("selected", &selected.uri).is_none());
        assert!(flow
            .playback_notice("@song selected", "different-uri")
            .is_none());
    }

    #[test]
    fn an_old_playback_completion_cannot_replace_a_new_song_notice() {
        let previous = song("spotify:track:0123456789abcdefghijkl");
        let selected = song("spotify:track:abcdefghijkl0123456789");
        let mut flow = SongFlow::default();
        flow.playback_started(1, "@song selected", &previous, Ok(()));
        flow.playback_started(
            2,
            "@song selected",
            &selected,
            Err("Spotify is busy".into()),
        );
        flow.playback_finished(1, Ok(()));

        assert_eq!(
            flow.playback_notice("@song selected", &selected.uri),
            Some("Spotify is busy")
        );
    }

    #[test]
    fn an_older_completion_for_the_same_uri_cannot_hide_a_retry_failure() {
        let selected = song("spotify:track:0123456789abcdefghijkl");
        let mut flow = SongFlow::default();
        let first_request = flow.new_playback_request();
        flow.playback_started(first_request, "@song selected", &selected, Ok(()));
        let retry = flow.new_playback_request();
        flow.playback_started(
            retry,
            "@song selected",
            &selected,
            Err("Spotify is busy".into()),
        );

        flow.playback_finished(first_request, Ok(()));

        assert_eq!(
            flow.playback_notice("@song selected", &selected.uri),
            Some("Spotify is busy")
        );
    }
}
