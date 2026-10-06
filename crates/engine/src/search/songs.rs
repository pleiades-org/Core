//! Spotify catalog snapshots: the songs `@song` found, or the albums and artists `@album` and
//! `@artist` did. Network access and account credentials stay in the launcher.
use super::{
    parse_query, Action, Collection, CommandKind, ParsedQuery, ResultKind, SearchBatch,
    SearchResult,
};
use std::sync::Arc;

/// What an explicit command searches Spotify's catalog for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CatalogKind {
    #[default]
    Songs,
    Albums,
    Artists,
}

impl CatalogKind {
    fn of(command: CommandKind) -> Option<Self> {
        match command {
            CommandKind::Songs => Some(Self::Songs),
            CommandKind::Albums => Some(Self::Albums),
            CommandKind::Artists => Some(Self::Artists),
            _ => None,
        }
    }

    /// The `type` Spotify's search takes, which is also the name of its list in the answer
    /// once an `s` follows it.
    pub fn api_type(self) -> &'static str {
        match self {
            Self::Songs => "track",
            Self::Albums => "album",
            Self::Artists => "artist",
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            Self::Songs => "Type @song followed by a song or artist",
            Self::Albums => "Type @album followed by an album or artist",
            Self::Artists => "Type @artist followed by an artist's name",
        }
    }

    fn nothing_found(self) -> &'static str {
        match self {
            Self::Songs => "No matching Spotify songs",
            Self::Albums => "No matching Spotify albums",
            Self::Artists => "No matching Spotify artists",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub uri: Arc<str>,
    pub title: Arc<str>,
    pub artist: Arc<str>,
    pub album: Arc<str>,
    /// The album's own address. The song is started inside its album, which every Spotify
    /// device accepts; None plays the song on its own.
    pub album_uri: Option<Arc<str>>,
    pub artwork: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SongStatus {
    #[default]
    Disabled,
    SetupRequired,
    ConnectRequired,
    /// Connected, but before Core asked for what this needs: connecting again grants it.
    PermissionRequired,
    Loading,
    Ready,
    NetworkError,
    AccessDenied,
    RateLimited,
}

impl SongStatus {
    pub fn message(self) -> &'static str {
        match self {
            Self::Disabled => "Enable Spotify song search in Settings → Music",
            Self::SetupRequired => {
                "Add your Spotify app Client ID in Settings → Music → Spotify song search"
            }
            Self::ConnectRequired => "Connect Spotify in Settings → Music → Spotify song search",
            Self::PermissionRequired => {
                "Click Connect Spotify again in Settings → Music to allow your playlists"
            }
            Self::Loading => "Searching Spotify…",
            Self::Ready => "Enter to play in Spotify · ↑ ↓ to select",
            Self::NetworkError => "Could not reach Spotify · edit the search to retry",
            Self::AccessDenied => {
                "Spotify refused access · check Premium and your app's allowed users"
            }
            Self::RateLimited => "Spotify's request limit was reached · try again later",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SongSearch {
    /// What was searched for: `songs` holds what a song search found, `collections` what a
    /// search for albums or artists did.
    pub kind: CatalogKind,
    pub query: Arc<str>,
    pub status: SongStatus,
    pub songs: Arc<[Song]>,
    pub collections: Arc<[Collection]>,
}

/// Only an explicit command sends text to Spotify: what it searches for, and the text.
pub fn catalog_query(query: &str) -> Option<(CatalogKind, &str)> {
    match parse_query(query) {
        ParsedQuery::Command { kind, payload } => Some((CatalogKind::of(kind)?, payload)),
        _ => None,
    }
}

/// `command`: one of the three that search the catalog; any other finds nothing.
pub(super) fn catalog_results(
    command: CommandKind,
    payload: &str,
    snapshot: Option<&SongSearch>,
) -> SearchBatch {
    let kind = CatalogKind::of(command).unwrap_or_default();
    let Some(snapshot) = snapshot else {
        return SearchBatch {
            results: Vec::new(),
            message: SongStatus::Disabled.message(),
        };
    };
    if snapshot.status != SongStatus::Ready {
        return SearchBatch {
            results: Vec::new(),
            message: snapshot.status.message(),
        };
    }
    if payload.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: kind.prompt(),
        };
    }
    // Also what another of the three commands found for the same words.
    if snapshot.kind != kind || snapshot.query.as_ref() != payload {
        return SearchBatch {
            results: Vec::new(),
            message: SongStatus::Loading.message(),
        };
    }
    let results: Vec<SearchResult> = match kind {
        CatalogKind::Songs => snapshot
            .songs
            .iter()
            .take(crate::VISIBLE_RESULT_LIMIT)
            .map(song_row)
            .collect(),
        CatalogKind::Albums | CatalogKind::Artists => snapshot
            .collections
            .iter()
            .take(crate::VISIBLE_RESULT_LIMIT)
            .map(Collection::row)
            .collect(),
    };
    let message = if results.is_empty() {
        kind.nothing_found()
    } else {
        SongStatus::Ready.message()
    };
    SearchBatch { results, message }
}

fn song_row(song: &Song) -> SearchResult {
    SearchResult {
        kind: ResultKind::Media,
        id: song.uri.clone(),
        title: song.title.clone(),
        description: format!("{} · {} · Spotify", song.artist, song.album).into(),
        action: Action::PlaySong(song.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{applications::ApplicationCatalog, search::CollectionKind};

    #[test]
    fn catalog_search_is_explicit_and_keeps_the_original_payload() {
        assert_eq!(
            catalog_query("@SONG Björk - Jóga"),
            Some((CatalogKind::Songs, "Björk - Jóga"))
        );
        assert_eq!(
            catalog_query("@album  Homogenic "),
            Some((CatalogKind::Albums, "Homogenic"))
        );
        assert_eq!(
            catalog_query("@Artists Björk"),
            Some((CatalogKind::Artists, "Björk"))
        );
        assert_eq!(catalog_query("@artist"), Some((CatalogKind::Artists, "")));
        for query in ["song Björk", "album Björk", "@music spotify", "@playlist a"] {
            assert_eq!(catalog_query(query), None, "{query}");
        }
    }

    fn album() -> Collection {
        Collection {
            kind: CollectionKind::Album,
            uri: "spotify:album:0123456789abcdefghijkl".into(),
            name: "Homogenic".into(),
            by: "Björk".into(),
            year: Some(1997),
            songs: Some(10),
            artwork: None,
        }
    }

    #[test]
    fn albums_and_artists_are_listed_only_for_their_own_command_and_words() {
        let album = album();
        let artist = Collection {
            kind: CollectionKind::Artist,
            uri: "spotify:artist:abcdefghijkl0123456789".into(),
            name: "Björk".into(),
            by: "".into(),
            year: None,
            songs: None,
            artwork: None,
        };
        let mut engine = super::super::SearchEngine::default();
        let catalog = ApplicationCatalog::default();
        engine.set_songs(Some(Arc::new(SongSearch {
            kind: CatalogKind::Albums,
            query: "Björk".into(),
            status: SongStatus::Ready,
            collections: Arc::from([album.clone()]),
            ..SongSearch::default()
        })));
        let found = engine.search("@album Björk", &catalog);
        assert_eq!(found.results[0].action, Action::PlayCollection(album));
        assert_eq!(
            &*found.results[0].description,
            "Album by Björk · 1997 · 10 songs · Spotify"
        );
        assert_eq!(found.message, SongStatus::Ready.message());
        // The same words after another command wait for that command's own answer.
        for query in ["@artist Björk", "@song Björk", "@album Björk x"] {
            let waiting = engine.search(query, &catalog);
            assert!(waiting.results.is_empty(), "{query}");
            assert_eq!(waiting.message, SongStatus::Loading.message(), "{query}");
        }

        engine.set_songs(Some(Arc::new(SongSearch {
            kind: CatalogKind::Artists,
            query: "Björk".into(),
            status: SongStatus::Ready,
            collections: Arc::from([artist.clone()]),
            ..SongSearch::default()
        })));
        let found = engine.search("@artists Björk", &catalog);
        assert_eq!(found.results[0].action, Action::PlayCollection(artist));
        assert_eq!(&*found.results[0].description, "Artist · Spotify");
    }

    #[test]
    fn each_command_says_what_to_type_and_when_nothing_matched() {
        let ready = |kind| SongSearch {
            kind,
            query: "zzz".into(),
            status: SongStatus::Ready,
            ..SongSearch::default()
        };
        for (command, kind, searched, prompt, nothing) in [
            (
                CommandKind::Songs,
                CatalogKind::Songs,
                "track",
                "Type @song followed by a song or artist",
                "No matching Spotify songs",
            ),
            (
                CommandKind::Albums,
                CatalogKind::Albums,
                "album",
                "Type @album followed by an album or artist",
                "No matching Spotify albums",
            ),
            (
                CommandKind::Artists,
                CatalogKind::Artists,
                "artist",
                "Type @artist followed by an artist's name",
                "No matching Spotify artists",
            ),
        ] {
            let snapshot = ready(kind);
            assert_eq!(kind.api_type(), searched);
            assert_eq!(
                catalog_results(command, "", Some(&snapshot)).message,
                prompt
            );
            assert_eq!(
                catalog_results(command, "zzz", Some(&snapshot)).message,
                nothing
            );
        }
        // The commands themselves are offered as they are typed.
        let mut engine = super::super::SearchEngine::default();
        let catalog = ApplicationCatalog::default();
        let offered: Vec<String> = engine
            .search("@a", &catalog)
            .results
            .iter()
            .map(|row| row.title.to_string())
            .collect();
        assert!(offered.contains(&"@album".to_owned()), "{offered:?}");
        assert!(offered.contains(&"@artist".to_owned()), "{offered:?}");
    }

    #[test]
    fn results_play_the_selected_uri_and_never_reuse_an_old_query() {
        let song = Song {
            uri: "spotify:track:123".into(),
            title: "Jóga".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            album_uri: None,
            artwork: None,
        };
        let snapshot = SongSearch {
            query: "Björk".into(),
            status: SongStatus::Ready,
            songs: Arc::from([song.clone()]),
            ..SongSearch::default()
        };
        let mut engine = super::super::SearchEngine::default();
        engine.set_songs(Some(Arc::new(snapshot)));
        let catalog = ApplicationCatalog::default();
        let found = engine.search("@song Björk", &catalog);
        assert_eq!(found.results[0].action, Action::PlaySong(song));
        assert!(engine.search("@song another", &catalog).results.is_empty());
    }

    #[test]
    fn disabled_and_failed_searches_offer_no_playback_actions() {
        for status in [
            SongStatus::Disabled,
            SongStatus::ConnectRequired,
            SongStatus::Loading,
            SongStatus::AccessDenied,
            SongStatus::RateLimited,
        ] {
            let snapshot = SongSearch {
                status,
                ..Default::default()
            };
            for command in [
                CommandKind::Songs,
                CommandKind::Albums,
                CommandKind::Artists,
            ] {
                assert!(catalog_results(command, "track", Some(&snapshot))
                    .results
                    .is_empty());
            }
        }
    }
}
