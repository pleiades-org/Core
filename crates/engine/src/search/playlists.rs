//! The person's own Spotify playlists, for `@playlist`. The launcher reads their library
//! through their account; this module only narrows it to what was typed.
use super::{
    parse_query, Collection, CommandKind, ParsedQuery, SearchBatch, SearchResult, SongStatus,
};
use std::sync::Arc;

const LOADING_MESSAGE: &str = "Reading your Spotify playlists…";
const UNREACHABLE_MESSAGE: &str = "Could not reach Spotify · type @playlist again to retry";
const EMPTY_MESSAGE: &str = "Your Spotify library has no playlists yet";
const NO_MATCH_MESSAGE: &str = "None of your playlists matches · @playlist lists them all";

/// The library as last read. Anything but `Ready` says why there is nothing to list.
#[derive(Clone, Debug, Default)]
pub struct PlaylistLibrary {
    pub status: SongStatus,
    pub playlists: Arc<[Collection]>,
}

/// What was typed after `@playlist`, which narrows the list; None for any other query.
pub fn playlist_query(query: &str) -> Option<&str> {
    match parse_query(query) {
        ParsedQuery::Command {
            kind: CommandKind::Playlists,
            payload,
        } => Some(payload),
        _ => None,
    }
}

/// How well a name answers what was typed, lower being better: it starts with it, one of
/// its words does, or it only holds it somewhere. None when it does not hold it at all.
fn rank(name: &str, wanted: &str) -> Option<u8> {
    let name = name.to_lowercase();
    if name.starts_with(wanted) {
        Some(0)
    } else if name
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| word.starts_with(wanted))
    {
        Some(1)
    } else {
        name.contains(wanted).then_some(2)
    }
}

/// `library` is None until the launcher has asked for it. The rows keep the order of the
/// person's library, with better matches of `payload` first.
pub(super) fn playlist_results(payload: &str, library: Option<&PlaylistLibrary>) -> SearchBatch {
    let Some(library) = library else {
        return SearchBatch {
            results: Vec::new(),
            message: SongStatus::Disabled.message(),
        };
    };
    if library.status != SongStatus::Ready {
        return SearchBatch {
            results: Vec::new(),
            message: match library.status {
                SongStatus::Loading => LOADING_MESSAGE,
                SongStatus::NetworkError => UNREACHABLE_MESSAGE,
                other => other.message(),
            },
        };
    }
    let wanted = payload.trim().to_lowercase();
    let mut matches: Vec<(u8, &Collection)> = library
        .playlists
        .iter()
        .filter_map(|playlist| Some((rank(&playlist.name, &wanted)?, playlist)))
        .collect();
    // Stable: playlists that match equally well stay in the library's order.
    matches.sort_by_key(|(rank, _)| *rank);
    let results: Vec<SearchResult> = matches
        .into_iter()
        .take(crate::VISIBLE_RESULT_LIMIT)
        .map(|(_, playlist)| playlist.row())
        .collect();
    let message = match (results.is_empty(), library.playlists.is_empty()) {
        (false, _) => SongStatus::Ready.message(),
        (true, true) => EMPTY_MESSAGE,
        (true, false) => NO_MATCH_MESSAGE,
    };
    SearchBatch { results, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        applications::ApplicationCatalog,
        search::{Action, CollectionKind, ResultKind},
    };

    fn playlist(id: char, name: &str, owner: &str, songs: Option<u32>) -> Collection {
        Collection {
            kind: CollectionKind::Playlist,
            uri: format!("spotify:playlist:{}", id.to_string().repeat(22)).into(),
            name: name.into(),
            by: owner.into(),
            year: None,
            songs,
            artwork: None,
        }
    }

    fn library() -> PlaylistLibrary {
        PlaylistLibrary {
            status: SongStatus::Ready,
            playlists: Arc::from([
                playlist('a', "Late Night Chill", "Robert", Some(42)),
                playlist('b', "Gym", "Robert", Some(1)),
                playlist('c', "Chill Mix", "Spotify", None),
                playlist('d', "Roadtrip (chilled)", "", Some(120)),
                playlist('e', "Köln Nächte", "Robert", Some(7)),
            ]),
        }
    }

    fn names(batch: &SearchBatch) -> Vec<&str> {
        batch.results.iter().map(|row| &*row.title).collect()
    }

    #[test]
    fn the_command_is_explicit_and_keeps_what_was_typed_after_it() {
        assert_eq!(playlist_query("@playlist"), Some(""));
        assert_eq!(
            playlist_query("@PLAYLISTS  Late Night "),
            Some("Late Night")
        );
        for query in ["playlist chill", "@song chill", "@play", "@playlistx"] {
            assert_eq!(playlist_query(query), None, "{query}");
        }
    }

    #[test]
    fn the_whole_library_is_listed_in_its_own_order_and_each_row_plays_its_playlist() {
        let library = library();
        let batch = playlist_results("", Some(&library));
        assert_eq!(
            names(&batch),
            [
                "Late Night Chill",
                "Gym",
                "Chill Mix",
                "Roadtrip (chilled)",
                "Köln Nächte"
            ]
        );
        assert_eq!(batch.message, SongStatus::Ready.message());
        assert!(batch
            .results
            .iter()
            .all(|row| row.kind == ResultKind::Media));
        assert_eq!(
            batch.results[1].action,
            Action::PlayCollection(library.playlists[1].clone())
        );
        assert_eq!(batch.results[1].id, library.playlists[1].uri);
        let details: Vec<&str> = batch.results.iter().map(|row| &*row.description).collect();
        assert_eq!(
            details,
            [
                "Playlist by Robert · 42 songs · Spotify",
                "Playlist by Robert · 1 song · Spotify",
                "Playlist by Spotify · Spotify",
                "Playlist · 120 songs · Spotify",
                "Playlist by Robert · 7 songs · Spotify"
            ]
        );
    }

    #[test]
    fn typing_narrows_the_list_with_the_best_matches_first() {
        let library = library();
        // Starts with it, then a word that starts with it, then anywhere in the name.
        assert_eq!(
            names(&playlist_results("chill", Some(&library))),
            ["Chill Mix", "Late Night Chill", "Roadtrip (chilled)"]
        );
        assert_eq!(
            names(&playlist_results(" NIGHT ", Some(&library))),
            ["Late Night Chill"]
        );
        assert_eq!(
            names(&playlist_results("ight", Some(&library))),
            ["Late Night Chill"]
        );
        // Case is ignored beyond ASCII too.
        assert_eq!(
            names(&playlist_results("NÄCH", Some(&library))),
            ["Köln Nächte"]
        );
        let none = playlist_results("jazz", Some(&library));
        assert!(none.results.is_empty());
        assert_eq!(none.message, NO_MATCH_MESSAGE);
    }

    #[test]
    fn a_long_library_shows_as_many_rows_as_fit() {
        let many: Vec<Collection> = (0..crate::VISIBLE_RESULT_LIMIT + 5)
            .map(|index| Collection {
                uri: format!("spotify:playlist:{index:022}").into(),
                ..playlist('x', &format!("Mix {index}"), "", None)
            })
            .collect();
        let library = PlaylistLibrary {
            status: SongStatus::Ready,
            playlists: many.into(),
        };
        assert_eq!(
            playlist_results("", Some(&library)).results.len(),
            crate::VISIBLE_RESULT_LIMIT
        );
        // One beyond the visible rows is still found by its name.
        assert_eq!(
            names(&playlist_results("mix 12", Some(&library))),
            ["Mix 12"]
        );
    }

    #[test]
    fn every_state_without_a_library_explains_itself_and_offers_nothing_to_play() {
        for (status, message) in [
            (SongStatus::Disabled, SongStatus::Disabled.message()),
            (
                SongStatus::SetupRequired,
                SongStatus::SetupRequired.message(),
            ),
            (
                SongStatus::ConnectRequired,
                SongStatus::ConnectRequired.message(),
            ),
            (
                SongStatus::PermissionRequired,
                SongStatus::PermissionRequired.message(),
            ),
            (SongStatus::Loading, LOADING_MESSAGE),
            (SongStatus::NetworkError, UNREACHABLE_MESSAGE),
            (SongStatus::AccessDenied, SongStatus::AccessDenied.message()),
            (SongStatus::RateLimited, SongStatus::RateLimited.message()),
        ] {
            let library = PlaylistLibrary {
                status,
                // A list read earlier is not offered while it cannot be trusted.
                playlists: library().playlists,
            };
            let batch = playlist_results("", Some(&library));
            assert!(batch.results.is_empty(), "{status:?}");
            assert_eq!(batch.message, message, "{status:?}");
        }
        assert!(playlist_results("", None).results.is_empty());
        let empty = PlaylistLibrary {
            status: SongStatus::Ready,
            playlists: Arc::from([]),
        };
        assert_eq!(playlist_results("", Some(&empty)).message, EMPTY_MESSAGE);
    }

    #[test]
    fn the_engine_lists_playlists_only_for_the_playlist_command() {
        let mut engine = super::super::SearchEngine::default();
        engine.set_playlists(Some(Arc::new(library())));
        let catalog = ApplicationCatalog::default();
        assert_eq!(engine.search("@playlist gym", &catalog).results.len(), 1);
        assert!(engine
            .search("gym", &catalog)
            .results
            .iter()
            .all(|row| !matches!(row.action, Action::PlayCollection(_))));
        // `@p` offers the command itself.
        assert!(engine
            .search("@pl", &catalog)
            .results
            .iter()
            .any(|row| &*row.title == "@playlist"));
    }
}
