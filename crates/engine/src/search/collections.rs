//! What Spotify plays as a whole, from its start: one of the person's playlists, an album, or
//! an artist's songs. `@playlist` lists the first from their library; `@album` and `@artist`
//! find the others in Spotify's catalog. Every such row plays the same way, and ends in the
//! same two buttons.
use super::{Action, ResultKind, SearchResult};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionKind {
    Playlist,
    Album,
    Artist,
}

impl CollectionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Playlist => "Playlist",
            Self::Album => "Album",
            Self::Artist => "Artist",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub kind: CollectionKind,
    pub uri: Arc<str>,
    pub name: Arc<str>,
    /// Who made it: a playlist's owner, an album's artists. Empty for an artist, and when
    /// Spotify does not say.
    pub by: Arc<str>,
    /// The year an album came out, when Spotify says.
    pub year: Option<u16>,
    /// How many songs it holds, when Spotify says.
    pub songs: Option<u32>,
    pub artwork: Option<Arc<str>>,
}

impl Collection {
    /// "Album by Muse · 2003 · 14 songs · Spotify", with whatever of it is known.
    fn description(&self) -> String {
        let mut text = String::from(self.kind.label());
        if !self.by.is_empty() {
            text.push_str(" by ");
            text.push_str(&self.by);
        }
        if let Some(year) = self.year {
            text.push_str(&format!(" · {year}"));
        }
        match self.songs {
            Some(1) => text.push_str(" · 1 song"),
            Some(count) => text.push_str(&format!(" · {count} songs")),
            None => {}
        }
        text.push_str(" · Spotify");
        text
    }

    /// The row that plays it.
    pub(super) fn row(&self) -> SearchResult {
        SearchResult {
            kind: ResultKind::Media,
            id: self.uri.clone(),
            title: self.name.clone(),
            description: self.description().into(),
            action: Action::PlayCollection(self.clone()),
        }
    }
}

/// How a chosen playlist, album or artist is played. Enter on its row plays it as it is; the
/// buttons beside the row offer the other two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayMode {
    /// From its start, with the shuffle and repeat settings Spotify already has.
    #[default]
    AsItIs,
    /// In a random order.
    Shuffled,
    /// Over again when it ends.
    Looped,
}

impl PlayMode {
    /// The choices beside a row, from left to right.
    pub const OPTIONS: [Self; 2] = [Self::Shuffled, Self::Looped];

    /// The choice one step to the right of this one, or to the left: the row itself comes
    /// first. None at either end.
    pub fn step(self, right: bool) -> Option<Self> {
        match (self, right) {
            (Self::AsItIs, true) | (Self::Looped, false) => Some(Self::Shuffled),
            (Self::Shuffled, true) => Some(Self::Looped),
            (Self::Shuffled, false) => Some(Self::AsItIs),
            (Self::Looped, true) | (Self::AsItIs, false) => None,
        }
    }

    /// Completes "Playing…" and "Starting <name>…": nothing, " shuffled" or " on repeat".
    pub fn manner(self) -> &'static str {
        match self {
            Self::AsItIs => "",
            Self::Shuffled => " shuffled",
            Self::Looped => " on repeat",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(kind: CollectionKind, by: &str, year: Option<u16>, songs: Option<u32>) -> String {
        Collection {
            kind,
            uri: "spotify:album:0123456789abcdefghijkl".into(),
            name: "Name".into(),
            by: by.into(),
            year,
            songs,
            artwork: None,
        }
        .row()
        .description
        .to_string()
    }

    #[test]
    fn a_row_says_what_it_is_and_only_what_spotify_told() {
        assert_eq!(
            collection(CollectionKind::Album, "Muse", Some(2003), Some(14)),
            "Album by Muse · 2003 · 14 songs · Spotify"
        );
        assert_eq!(
            collection(CollectionKind::Album, "", None, Some(1)),
            "Album · 1 song · Spotify"
        );
        assert_eq!(
            collection(CollectionKind::Playlist, "Robert", None, Some(42)),
            "Playlist by Robert · 42 songs · Spotify"
        );
        assert_eq!(
            collection(CollectionKind::Artist, "", None, None),
            "Artist · Spotify"
        );
    }

    #[test]
    fn a_row_plays_what_it_shows() {
        let album = Collection {
            kind: CollectionKind::Album,
            uri: "spotify:album:0123456789abcdefghijkl".into(),
            name: "Absolution".into(),
            by: "Muse".into(),
            year: Some(2003),
            songs: Some(14),
            artwork: None,
        };
        let row = album.row();
        assert_eq!(row.kind, ResultKind::Media);
        assert_eq!(row.id, album.uri);
        assert_eq!(&*row.title, "Absolution");
        assert_eq!(row.action, Action::PlayCollection(album));
    }

    #[test]
    fn right_and_left_walk_from_the_row_over_its_two_buttons_and_stop_at_the_ends() {
        let mut mode = PlayMode::default();
        let mut walked = vec![mode];
        while let Some(next) = mode.step(true) {
            mode = next;
            walked.push(mode);
        }
        assert_eq!(
            walked,
            [PlayMode::AsItIs, PlayMode::Shuffled, PlayMode::Looped]
        );
        assert_eq!(walked[1..], PlayMode::OPTIONS);
        while let Some(previous) = mode.step(false) {
            mode = previous;
        }
        assert_eq!(mode, PlayMode::AsItIs);
        assert_eq!(
            [
                PlayMode::AsItIs.manner(),
                PlayMode::Shuffled.manner(),
                PlayMode::Looped.manner()
            ],
            ["", " shuffled", " on repeat"]
        );
    }
}
