//! The volume mixer in search: `@volume` or `@mix` lists every program with sound in Windows'
//! mixer, and the whole PC's volume first. Each row carries its level; the launcher draws a
//! slider for it, steps it with Left and Right, and mutes it on Enter.
use super::{parse_query, Action, CommandKind, ParsedQuery, ResultKind, SearchBatch, SearchResult};
use crate::{media::MixerApp, VISIBLE_RESULT_LIMIT};

pub const ID_PREFIX: &str = "mixer:";
const LIST_MESSAGE: &str = "← → to change the volume · Enter to mute";
const LOADING_MESSAGE: &str = "Reading the volume mixer…";
const EMPTY_MESSAGE: &str = "Nothing has sound right now";
const NO_MATCH_MESSAGE: &str = "No program with sound matches";

/// Whether search results for `query` are the volume mixer, so the launcher reads it.
pub fn wants_mixer(query: &str) -> bool {
    matches!(
        parse_query(query),
        ParsedQuery::Command {
            kind: CommandKind::Mixer,
            ..
        }
    )
}

/// `mixer` is None until the launcher has read it. `payload` narrows the list to names that
/// contain it, as in `@mix spot`.
pub(super) fn mixer_results(payload: &str, mixer: Option<&[MixerApp]>) -> SearchBatch {
    let Some(mixer) = mixer else {
        return SearchBatch {
            results: Vec::new(),
            message: LOADING_MESSAGE,
        };
    };
    let wanted = payload.trim().to_lowercase();
    let results: Vec<SearchResult> = mixer
        .iter()
        .filter(|app| wanted.is_empty() || app.name.to_lowercase().contains(&wanted))
        .take(VISIBLE_RESULT_LIMIT)
        .map(mixer_row)
        .collect();
    let message = match (results.is_empty(), mixer.is_empty()) {
        (false, _) => LIST_MESSAGE,
        (true, true) => EMPTY_MESSAGE,
        (true, false) => NO_MATCH_MESSAGE,
    };
    SearchBatch { results, message }
}

fn mixer_row(app: &MixerApp) -> SearchResult {
    let state = if app.level.muted {
        "Muted"
    } else if app.is_system() {
        "Everything this PC plays"
    } else if app.active {
        "Playing"
    } else {
        "Quiet now"
    };
    SearchResult {
        kind: ResultKind::Volume,
        id: format!("{ID_PREFIX}{}", app.id).into(),
        title: app.name.clone(),
        // The level is not written here: the row's slider shows it, also while it is dragged.
        description: state.into(),
        action: Action::Mixer {
            app: app.id.clone(),
            level: app.level,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{VolumeLevel, SYSTEM_VOLUME_ID};

    fn app(id: &str, name: &str, percent: u8, muted: bool, active: bool) -> MixerApp {
        MixerApp {
            id: id.into(),
            name: name.into(),
            level: VolumeLevel::new(percent, muted),
            active,
        }
    }

    fn mixer() -> Vec<MixerApp> {
        vec![
            app(SYSTEM_VOLUME_ID, "System volume", 80, false, false),
            app(r"c:\apps\spotify.exe", "Spotify", 62, false, true),
            app(r"c:\apps\chrome.exe", "Helium", 14, true, true),
            app(r"c:\apps\game.exe", "Game", 100, false, false),
        ]
    }

    #[test]
    fn both_commands_ask_for_the_mixer_and_nothing_else_does() {
        for query in ["@volume", "@mix", "@MIXER spot", " @mix  "] {
            assert!(wants_mixer(query), "{query}");
        }
        for query in ["volume", "@media", "@vol", "@mixx", "mix"] {
            assert!(!wants_mixer(query), "{query}");
        }
    }

    #[test]
    fn every_program_is_a_row_that_carries_its_level() {
        let batch = mixer_results("", Some(&mixer()));
        assert_eq!(batch.message, LIST_MESSAGE);
        let titles: Vec<&str> = batch.results.iter().map(|row| &*row.title).collect();
        assert_eq!(titles, ["System volume", "Spotify", "Helium", "Game"]);
        assert!(batch
            .results
            .iter()
            .all(|row| row.kind == ResultKind::Volume && row.id.starts_with(ID_PREFIX)));
        assert_eq!(
            batch.results[1].action,
            Action::Mixer {
                app: r"c:\apps\spotify.exe".into(),
                level: VolumeLevel::new(62, false)
            }
        );
        let details: Vec<&str> = batch.results.iter().map(|row| &*row.description).collect();
        assert_eq!(
            details,
            ["Everything this PC plays", "Playing", "Muted", "Quiet now"]
        );
    }

    #[test]
    fn a_name_narrows_the_list_and_each_empty_case_is_explained() {
        let narrowed = mixer_results(" SPOT ", Some(&mixer()));
        assert_eq!(narrowed.results.len(), 1);
        assert_eq!(&*narrowed.results[0].title, "Spotify");
        let missing = mixer_results("nothing", Some(&mixer()));
        assert!(missing.results.is_empty());
        assert_eq!(missing.message, NO_MATCH_MESSAGE);
        assert_eq!(mixer_results("", Some(&[])).message, EMPTY_MESSAGE);
        assert_eq!(mixer_results("", None).message, LOADING_MESSAGE);
    }

    #[test]
    fn a_long_mixer_shows_as_many_rows_as_fit() {
        let many: Vec<MixerApp> = (0..VISIBLE_RESULT_LIMIT + 4)
            .map(|index| {
                app(
                    &format!("app{index}"),
                    &format!("App {index}"),
                    50,
                    false,
                    true,
                )
            })
            .collect();
        assert_eq!(
            mixer_results("", Some(&many)).results.len(),
            VISIBLE_RESULT_LIMIT
        );
    }
}
