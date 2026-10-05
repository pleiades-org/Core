//! Spotify catalog snapshots. Network access and account credentials stay in the launcher.
use super::{parse_query, Action, CommandKind, ParsedQuery, ResultKind, SearchBatch, SearchResult};
use std::sync::Arc;

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
    pub query: Arc<str>,
    pub status: SongStatus,
    pub songs: Arc<[Song]>,
}

/// Only an explicit song command sends text to Spotify.
pub fn song_query(query: &str) -> Option<&str> {
    match parse_query(query) {
        ParsedQuery::Command {
            kind: CommandKind::Songs,
            payload,
        } => Some(payload),
        _ => None,
    }
}

pub(super) fn song_results(payload: &str, snapshot: Option<&SongSearch>) -> SearchBatch {
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
            message: "Type @song followed by a song or artist",
        };
    }
    if snapshot.query.as_ref() != payload {
        return SearchBatch {
            results: Vec::new(),
            message: SongStatus::Loading.message(),
        };
    }
    let results: Vec<_> = snapshot
        .songs
        .iter()
        .take(crate::VISIBLE_RESULT_LIMIT)
        .map(|song| SearchResult {
            kind: ResultKind::Media,
            id: song.uri.clone(),
            title: song.title.clone(),
            description: format!("{} · {} · Spotify", song.artist, song.album).into(),
            action: Action::PlaySong(song.clone()),
        })
        .collect();
    let message = if results.is_empty() {
        "No matching Spotify songs"
    } else {
        SongStatus::Ready.message()
    };
    SearchBatch { results, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::ApplicationCatalog;

    #[test]
    fn song_search_is_explicit_and_keeps_the_original_payload() {
        assert_eq!(song_query("@SONG Björk - Jóga"), Some("Björk - Jóga"));
        assert_eq!(song_query("song Björk"), None);
        assert_eq!(song_query("@music spotify"), None);
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
            assert!(song_results("track", Some(&snapshot)).results.is_empty());
        }
    }
}
