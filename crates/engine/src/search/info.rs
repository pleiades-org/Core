//! `@info`: the installed Core version and the latest release, as the launcher's update check
//! last saw it. Enter copies the version, checks for updates, or opens the release notes.
use super::{parse_query, Action, CommandKind, ParsedQuery, ResultKind, SearchBatch, SearchResult};
use std::sync::Arc;

/// Whether a query shows `@info`, so the launcher can refresh it when an update check ends.
pub fn wants_info(query: &str) -> bool {
    matches!(
        parse_query(query),
        ParsedQuery::Command {
            kind: CommandKind::Info,
            ..
        }
    )
}

pub(super) const MESSAGE: &str = "Enter to copy the version · ↓ for the latest release";

/// What the launcher knows about Core's own version and releases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub version: Arc<str>,
    pub latest: LatestRelease,
    /// The installed version's release notes.
    pub notes_url: Arc<str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LatestRelease {
    /// No check has read a release yet.
    Unknown,
    Checking,
    /// The latest release is this version, which is not newer than Core.
    UpToDate(Arc<str>),
    /// A newer release exists; it is not downloaded (Notify mode, or a read-only folder).
    Available(Arc<str>),
    /// A newer release is downloaded and verified; Core installs it on restart.
    Ready(Arc<str>),
    /// Updates are turned off in Settings.
    Off,
    /// The last check failed, with the reason.
    Failed(Arc<str>),
}

pub(super) fn info_results(payload: &str, info: Option<&AppInfo>) -> SearchBatch {
    if !payload.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: "Use @info without anything after it",
        };
    }
    let Some(info) = info else {
        return SearchBatch {
            results: Vec::new(),
            message: "Core's version is unavailable",
        };
    };
    let (title, description) = match &info.latest {
        LatestRelease::Unknown => (
            "Latest: not checked yet".to_owned(),
            "Enter to check GitHub for updates".to_owned(),
        ),
        LatestRelease::Checking => (
            "Latest: checking GitHub…".to_owned(),
            "The answer appears here".to_owned(),
        ),
        LatestRelease::UpToDate(latest) => (
            format!("Latest: {latest}"),
            "Up to date · Enter to check again".to_owned(),
        ),
        LatestRelease::Available(latest) => (
            format!("Latest: {latest}"),
            "Newer than this version · Enter to open the release page".to_owned(),
        ),
        LatestRelease::Ready(latest) => (
            format!("Latest: {latest}"),
            "Downloaded and verified · Enter to restart and install".to_owned(),
        ),
        LatestRelease::Off => (
            "Latest: not checked".to_owned(),
            "Updates are off · Settings > Behaviour".to_owned(),
        ),
        LatestRelease::Failed(reason) => (
            "Latest: unknown".to_owned(),
            format!("{reason} · Enter to try again"),
        ),
    };
    SearchBatch {
        results: vec![
            SearchResult {
                kind: ResultKind::System,
                id: "core:version".into(),
                title: format!("Core {}", info.version).into(),
                description: "Installed version · Enter to copy".into(),
                action: Action::CopyText(info.version.clone()),
            },
            SearchResult {
                kind: ResultKind::System,
                id: "core:latest".into(),
                title: title.into(),
                description: description.into(),
                action: Action::Update,
            },
            SearchResult {
                kind: ResultKind::System,
                id: "core:notes".into(),
                title: format!("What's new in {}", info.version).into(),
                description: "Release notes on GitHub".into(),
                action: Action::OpenUrl(info.notes_url.clone()),
            },
        ],
        message: MESSAGE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(latest: LatestRelease) -> AppInfo {
        AppInfo {
            version: "2.2.0".into(),
            latest,
            notes_url: "https://example.com/v2.2.0".into(),
        }
    }

    #[test]
    fn info_shows_the_installed_version_then_the_latest_release() {
        let batch = info_results("", Some(&info(LatestRelease::UpToDate("2.2.0".into()))));
        let titles: Vec<_> = batch.results.iter().map(|row| row.title.as_ref()).collect();
        assert_eq!(
            titles,
            ["Core 2.2.0", "Latest: 2.2.0", "What's new in 2.2.0"]
        );
        assert_eq!(batch.results[0].action, Action::CopyText("2.2.0".into()));
        assert_eq!(batch.results[1].action, Action::Update);
        assert_eq!(
            batch.results[2].action,
            Action::OpenUrl("https://example.com/v2.2.0".into())
        );
    }

    #[test]
    fn every_update_state_explains_the_latest_release() {
        for (latest, title, description) in [
            (
                LatestRelease::Unknown,
                "Latest: not checked yet",
                "Enter to check",
            ),
            (
                LatestRelease::Checking,
                "Latest: checking GitHub…",
                "appears here",
            ),
            (
                LatestRelease::Available("2.3.0".into()),
                "Latest: 2.3.0",
                "release page",
            ),
            (
                LatestRelease::Ready("2.3.0".into()),
                "Latest: 2.3.0",
                "restart and install",
            ),
            (LatestRelease::Off, "Latest: not checked", "Updates are off"),
            (
                LatestRelease::Failed("GitHub returned HTTP 503".into()),
                "Latest: unknown",
                "HTTP 503 · Enter to try again",
            ),
        ] {
            let batch = info_results("", Some(&info(latest)));
            assert_eq!(batch.results[1].title.as_ref(), title);
            assert!(
                batch.results[1].description.contains(description),
                "{}",
                batch.results[1].description
            );
        }
    }

    #[test]
    fn info_takes_no_arguments() {
        assert!(info_results("now", Some(&info(LatestRelease::Unknown)))
            .results
            .is_empty());
        assert!(info_results("", None).results.is_empty());
    }
}
