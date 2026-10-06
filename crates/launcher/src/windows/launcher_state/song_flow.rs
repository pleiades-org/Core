//! Searching Spotify for songs, albums and artists, and the person's playlists, are
//! independent of the local search worker and of the existing media controls.
use super::LauncherState;
use crate::windows::{
    settings::SpotifySettings,
    spotify::{Event, SpotifyService},
};
use core_engine::search::{
    catalog_query, playlist_query, CatalogKind, Collection, PlayMode, PlaylistLibrary, Song,
    SongSearch, SongStatus,
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct SongFlow {
    service: Option<SpotifyService>,
    settings: SpotifySettings,
    /// The catalog search last asked for: what it looks for, and the text.
    requested: Option<(CatalogKind, String)>,
    playback: Option<PlaybackNotice>,
    next_playback_request: u64,
    pub snapshot: Option<Arc<SongSearch>>,
    /// The person's playlists as last read. They are kept between uses of `@playlist`, so
    /// the list shows at once the next time while it is read again.
    pub playlists: Option<Arc<PlaylistLibrary>>,
    /// The results are the playlists', and the library was asked for when they began to be.
    playlists_shown: bool,
}

struct PlaybackNotice {
    request_id: u64,
    query: String,
    uri: Arc<str>,
    message: String,
}

/// What was typed after `@song`, `@album`, `@artist` or `@playlist`: a notice about
/// something that was started belongs to the results that text gave.
fn spotify_payload(query: &str) -> Option<&str> {
    catalog_query(query)
        .map(|(_, payload)| payload)
        .or_else(|| playlist_query(query))
}

impl SongFlow {
    fn new_playback_request(&mut self) -> u64 {
        self.next_playback_request = self.next_playback_request.wrapping_add(1);
        self.next_playback_request
    }

    /// `uri` and `title`: the song, playlist, album or artist that was chosen.
    fn playback_started(
        &mut self,
        request_id: u64,
        query: &str,
        uri: &Arc<str>,
        title: &str,
        result: Result<(), String>,
    ) {
        self.playback = Some(PlaybackNotice {
            request_id,
            query: spotify_payload(query).unwrap_or_default().to_owned(),
            uri: uri.clone(),
            message: result
                .map_or_else(|error| error, |()| format!("Starting {title} in Spotify…")),
        });
    }

    fn playback_finished(&mut self, request_id: u64, result: Result<String, String>) {
        let Some(notice) = self
            .playback
            .as_mut()
            .filter(|notice| notice.request_id == request_id)
        else {
            return;
        };
        notice.message = result.unwrap_or_else(|error| error);
    }

    pub(super) fn playback_notice(&self, query: &str, uri: &str) -> Option<&str> {
        let notice = self.playback.as_ref()?;
        (spotify_payload(query) == Some(&notice.query) && &*notice.uri == uri)
            .then_some(notice.message.as_str())
    }
}

impl LauncherState {
    pub(super) fn spotify_settings(&self) -> SpotifySettings {
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
        self.forget_playlists();
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

    pub(super) fn spotify_service(&mut self) -> Result<&SpotifyService, String> {
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
        let Some((kind, payload)) = catalog_query(query) else {
            self.songs.requested = None;
            self.songs.snapshot = None;
            // A playlist that was started keeps its notice while its results show.
            if playlist_query(query).is_none() {
                self.songs.playback = None;
            }
            if let Some(service) = &self.songs.service {
                service.cancel_search();
            }
            return;
        };
        let asked = |(asked_kind, asked_text): &(CatalogKind, String)| {
            *asked_kind == kind && asked_text == payload
        };
        if self.songs.requested.as_ref().is_some_and(asked) {
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
        let snapshot = |status| {
            Some(Arc::new(SongSearch {
                kind,
                query: payload.into(),
                status,
                ..SongSearch::default()
            }))
        };
        self.songs.requested = Some((kind, payload.to_owned()));
        self.songs.snapshot = snapshot(status);
        if status != SongStatus::Loading {
            return;
        }
        match self.spotify_service() {
            Ok(service) => service.search(kind, payload.to_owned()),
            Err(_) => self.songs.snapshot = snapshot(SongStatus::NetworkError),
        }
    }

    /// Runs with every search. Typing `@playlist` asks Spotify for the person's library once,
    /// however the text after it changes; what was read last time shows meanwhile.
    pub(super) fn playlists_for_query(&mut self, query: &str) {
        // First, so a changed setting forgets the old library before this one is asked for.
        self.spotify_settings_changed();
        if playlist_query(query).is_none() {
            self.songs.playlists_shown = false;
            return;
        }
        if std::mem::replace(&mut self.songs.playlists_shown, true) {
            return;
        }
        let settings = self.spotify_settings();
        let unavailable = match (settings.enabled, settings.client_id.is_empty()) {
            (false, _) => Some(SongStatus::Disabled),
            (_, true) => Some(SongStatus::SetupRequired),
            _ => None,
        };
        let asked = match unavailable {
            Some(status) => Err(status),
            None => self
                .spotify_service()
                .and_then(SpotifyService::load_playlists)
                .map_err(|_| SongStatus::NetworkError),
        };
        let listed = self
            .songs
            .playlists
            .as_ref()
            .is_some_and(|library| library.status == SongStatus::Ready);
        match asked {
            // The list read last time stays until the new reading arrives.
            Ok(()) if listed => {}
            Ok(()) => self.show_playlists(SongStatus::Loading),
            Err(status) => self.show_playlists(status),
        }
    }

    /// Shows why there are no playlists to list.
    fn show_playlists(&mut self, status: SongStatus) {
        self.songs.playlists = Some(Arc::new(PlaylistLibrary {
            status,
            playlists: Arc::from([]),
        }));
    }

    /// The library belongs to a connection or settings that changed: it is asked for again
    /// the next time a search wants it.
    fn forget_playlists(&mut self) {
        self.songs.playlists = None;
        self.songs.playlists_shown = false;
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
                    let asked = self.songs.requested.as_ref().is_some_and(|(kind, text)| {
                        *kind == reading.kind && **text == *reading.query
                    });
                    if !asked {
                        continue;
                    }
                    self.songs.snapshot = Some(Arc::new(reading));
                    if self.visible {
                        self.queue_search();
                    }
                }
                Event::Playlists(library) => {
                    self.songs.playlists = Some(Arc::new(library));
                    if self.visible && self.songs.playlists_shown {
                        self.queue_search();
                    }
                }
                Event::Account(result) => {
                    self.songs.requested = None;
                    self.songs.snapshot = None;
                    self.songs.playback = None;
                    // Connecting again may have allowed the playlists; disconnecting ends them.
                    self.forget_playlists();
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
                Event::Volume(answer) => self.receive_spotify_volume(answer),
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
        self.forget_playlists();
        if let Some(view) = &self.view {
            view.spotify_status(&text);
        }
    }

    pub(super) fn play_song(&mut self, song: Song) {
        let request_id = self.songs.new_playback_request();
        let result = self
            .spotify_service()
            .and_then(|service| service.play(request_id, &song));
        let query = self.acted_query();
        self.songs
            .playback_started(request_id, &query, &song.uri, &song.title, result);
        self.refresh_footer();
    }

    /// A playlist, album or artist. `mode`: as it is from the row itself, or the button
    /// beside it the person chose.
    pub(super) fn play_collection(&mut self, collection: Collection, mode: PlayMode) {
        let request_id = self.songs.new_playback_request();
        let result = self
            .spotify_service()
            .and_then(|service| service.play_collection(request_id, &collection, mode));
        let query = self.acted_query();
        // "Starting Chill Mix shuffled in Spotify…"
        let title = format!("{}{}", collection.name, mode.manner());
        self.songs
            .playback_started(request_id, &query, &collection.uri, &title, result);
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
            album_uri: None,
            artwork: None,
        }
    }

    /// A song chosen from the results of `@song selected`.
    fn started(flow: &mut SongFlow, request_id: u64, song: &Song, result: Result<(), String>) {
        flow.playback_started(request_id, "@song selected", &song.uri, &song.title, result);
    }

    #[test]
    fn a_started_playlist_keeps_its_notice_only_for_the_text_that_listed_it() {
        let uri: Arc<str> = "spotify:playlist:0123456789abcdefghijkl".into();
        let mut flow = SongFlow::default();
        flow.playback_started(4, "@playlist chill", &uri, "Chill Mix", Ok(()));
        assert_eq!(
            flow.playback_notice("@playlist chill", &uri),
            Some("Starting Chill Mix in Spotify…")
        );
        flow.playback_finished(4, Ok("Playing in Spotify · This PC".into()));
        assert_eq!(
            flow.playback_notice("@PLAYLISTS  chill", &uri),
            Some("Playing in Spotify · This PC")
        );
        // An album or an artist that was started keeps its notice the same way.
        let album: Arc<str> = "spotify:album:0123456789abcdefghijkl".into();
        flow.playback_started(
            5,
            "@album absolution",
            &album,
            "Absolution shuffled",
            Ok(()),
        );
        assert_eq!(
            flow.playback_notice("@albums absolution", &album),
            Some("Starting Absolution shuffled in Spotify…")
        );
        assert!(flow.playback_notice("@artist absolution", &uri).is_none());
        flow.playback_started(4, "@playlist chill", &uri, "Chill Mix", Ok(()));
        // Other text, another row, or the same words after another command: no notice.
        assert!(flow.playback_notice("@playlist chi", &uri).is_none());
        assert!(flow.playback_notice("@playlist chill", "other").is_none());
        assert!(flow.playback_notice("chill", &uri).is_none());
    }

    #[test]
    fn footer_refreshes_retain_the_playback_error_for_the_selected_result() {
        let selected = song("spotify:track:0123456789abcdefghijkl");
        let mut flow = SongFlow::default();
        started(&mut flow, 1, &selected, Ok(()));
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
        started(&mut flow, 1, &previous, Ok(()));
        started(&mut flow, 2, &selected, Err("Spotify is busy".into()));
        flow.playback_finished(1, Ok("Playing in Spotify".into()));

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
        started(&mut flow, first_request, &selected, Ok(()));
        let retry = flow.new_playback_request();
        started(&mut flow, retry, &selected, Err("Spotify is busy".into()));

        flow.playback_finished(first_request, Ok("Playing in Spotify".into()));

        assert_eq!(
            flow.playback_notice("@song selected", &selected.uri),
            Some("Spotify is busy")
        );
    }
}
