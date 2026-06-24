use crate::{
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction},
    search_text::{normalize_keyword, normalize_search_text},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs, io,
    path::PathBuf,
};

const QUICKLINKS_FILE_NAME: &str = "quicklinks.toml";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuicklinkRecord {
    pub keyword: String,
    pub title: String,
    pub target: String,
    pub hotkey: Option<String>,
    pub is_team_shared: bool,
    pub updated_at: i64,
}

#[derive(Default, Deserialize, Serialize)]
struct QuicklinkStore {
    quicklinks: Vec<QuicklinkRecord>,
    #[serde(default)]
    hidden_keywords: Vec<String>,
}

pub fn search_quicklinks(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();
    if trimmed_search_text.is_empty() {
        return quicklink_home_results();
    }

    if let Some((keyword, title, target)) = parse_quicklink_assignment(trimmed_search_text) {
        return vec![CommandResult::feature(
            format!("Save quicklink {keyword}"),
            target.clone(),
            CommandCategory::Quicklink,
            FeatureAction::SaveQuicklink {
                keyword,
                title,
                target,
            },
            96,
        )];
    }

    ranked_quicklinks(trimmed_search_text)
        .into_iter()
        .map(quicklink_result)
        .collect()
}

pub fn search_quicklink_keywords(query: &str) -> Vec<CommandResult> {
    let trimmed_query = query.trim();
    let keyword_query = trimmed_query.strip_prefix('>').unwrap_or(trimmed_query);
    let normalized_query = quicklink_keyword(keyword_query);

    if normalized_query.is_empty() {
        return Vec::new();
    }

    load_all_quicklinks()
        .into_iter()
        .filter(|quicklink| quicklink_keyword(&quicklink.keyword) == normalized_query)
        .map(quicklink_result)
        .collect()
}

pub fn save_quicklink(keyword: String, title: String, target: String) -> io::Result<()> {
    let normalized_keyword = quicklink_keyword(&keyword);
    let mut store = load_quicklink_store();

    store
        .quicklinks
        .retain(|quicklink| quicklink_keyword(&quicklink.keyword) != normalized_keyword);
    store.quicklinks.push(QuicklinkRecord {
        keyword: normalized_keyword,
        title: title.trim().to_string(),
        target: target.trim().to_string(),
        hotkey: None,
        is_team_shared: false,
        updated_at: Local::now().timestamp(),
    });
    store
        .quicklinks
        .sort_by_key(|quicklink| quicklink.keyword.to_lowercase());

    save_quicklink_store(&store)
}

pub fn configured_quicklinks() -> Vec<QuicklinkRecord> {
    load_all_quicklinks()
}

pub fn delete_quicklink(keyword: &str) -> io::Result<()> {
    let normalized_keyword = quicklink_keyword(keyword);
    let mut store = load_quicklink_store();
    let store_len_before = store.quicklinks.len();
    store
        .quicklinks
        .retain(|quicklink| quicklink_keyword(&quicklink.keyword) != normalized_keyword);

    if store.quicklinks.len() == store_len_before
        && !store
            .hidden_keywords
            .iter()
            .any(|hidden| quicklink_keyword(hidden) == normalized_keyword)
    {
        store.hidden_keywords.push(normalized_keyword);
    }

    save_quicklink_store(&store)
}

fn quicklink_home_results() -> Vec<CommandResult> {
    let mut results = vec![
        CommandResult::informational(
            "Quicklinks",
            "Use @quicklink keyword = url-or-path, or type >keyword to open one",
        ),
        CommandResult {
            title: "Open quicklinks export".to_string(),
            subtitle: quicklinks_file_path().display().to_string(),
            copy_text: quicklinks_file_path().display().to_string(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Quicklink,
            action: CommandAction::OpenPath(quicklinks_file_path()),
            confidence: 72,
        },
    ];
    results.extend(load_all_quicklinks().into_iter().map(quicklink_result));
    results
}

fn ranked_quicklinks(search_text: &str) -> Vec<QuicklinkRecord> {
    let normalized_search_text = normalize_search_text(search_text);
    let mut scored_quicklinks = load_all_quicklinks()
        .into_iter()
        .filter_map(|quicklink| {
            score_quicklink(&quicklink, &normalized_search_text).map(|score| (score, quicklink))
        })
        .collect::<Vec<_>>();

    scored_quicklinks.sort_by_key(|(score, quicklink)| {
        (
            std::cmp::Reverse(*score),
            quicklink.keyword.to_lowercase(),
            quicklink.title.to_lowercase(),
        )
    });

    scored_quicklinks
        .into_iter()
        .map(|(_, quicklink)| quicklink)
        .collect()
}

fn quicklink_result(quicklink: QuicklinkRecord) -> CommandResult {
    let action = quicklink_action(&quicklink.target);
    let shared_label = if quicklink.is_team_shared {
        "Shared Teams quicklink"
    } else {
        "Quicklink"
    };

    CommandResult {
        title: format!(">{}  {}", quicklink.keyword, quicklink.title),
        subtitle: format!("{shared_label} - {}", quicklink.target),
        copy_text: quicklink.target,
        explanation: quicklink.hotkey.map(|hotkey| format!("Hotkey: {hotkey}")),
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Quicklink,
        action,
        confidence: 90,
    }
}

fn quicklink_action(target: &str) -> CommandAction {
    if looks_like_url(target) {
        CommandAction::OpenUrl(normalize_url(target))
    } else {
        CommandAction::OpenPath(PathBuf::from(target))
    }
}

fn score_quicklink(quicklink: &QuicklinkRecord, normalized_search_text: &str) -> Option<u8> {
    let keyword = normalize_search_text(&quicklink.keyword);
    let title = normalize_search_text(&quicklink.title);
    let target = normalize_search_text(&quicklink.target);

    if keyword == normalized_search_text {
        return Some(96);
    }

    if keyword.starts_with(normalized_search_text) {
        return Some(88);
    }

    if title.contains(normalized_search_text) {
        return Some(78);
    }

    target.contains(normalized_search_text).then_some(66)
}

fn parse_quicklink_assignment(search_text: &str) -> Option<(String, String, String)> {
    let (keyword_text, target_text) = search_text.split_once('=')?;
    let keyword = quicklink_keyword(keyword_text.trim().trim_start_matches('>'));
    let target = target_text.trim().to_string();

    if keyword.is_empty() || target.is_empty() {
        return None;
    }

    let title = keyword.replace(['-', '_'], " ");
    Some((keyword, title, target))
}

fn load_all_quicklinks() -> Vec<QuicklinkRecord> {
    let store = load_quicklink_store();
    let hidden_keywords: HashSet<String> = store
        .hidden_keywords
        .iter()
        .map(|keyword| quicklink_keyword(keyword))
        .collect();

    let mut quicklinks = default_quicklinks()
        .into_iter()
        .filter(|quicklink| !hidden_keywords.contains(&quicklink_keyword(&quicklink.keyword)))
        .collect::<Vec<_>>();

    for custom_quicklink in store.quicklinks {
        quicklinks.retain(|quicklink| {
            quicklink_keyword(&quicklink.keyword) != quicklink_keyword(&custom_quicklink.keyword)
        });
        quicklinks.push(custom_quicklink);
    }

    quicklinks.sort_by_key(|quicklink| quicklink.keyword.to_lowercase());
    quicklinks
}

fn default_quicklinks() -> Vec<QuicklinkRecord> {
    vec![
        QuicklinkRecord {
            keyword: "docs".to_string(),
            title: "Core docs".to_string(),
            target: "https://www.gpui.rs/".to_string(),
            hotkey: None,
            is_team_shared: true,
            updated_at: 0,
        },
        QuicklinkRecord {
            keyword: "settings".to_string(),
            title: "Core Launcher config".to_string(),
            target: crate::settings::settings_file_path().display().to_string(),
            hotkey: None,
            is_team_shared: false,
            updated_at: 0,
        },
    ]
}

fn load_quicklink_store() -> QuicklinkStore {
    fs::read_to_string(quicklinks_file_path())
        .ok()
        .and_then(|store_text| toml::from_str(&store_text).ok())
        .unwrap_or_default()
}

fn save_quicklink_store(store: &QuicklinkStore) -> io::Result<()> {
    let quicklinks_path = quicklinks_file_path();
    if let Some(quicklinks_directory) = quicklinks_path.parent() {
        fs::create_dir_all(quicklinks_directory)?;
    }

    let quicklinks_text = toml::to_string_pretty(store).unwrap_or_default();
    fs::write(quicklinks_path, quicklinks_text)
}

pub fn quicklinks_file_path() -> PathBuf {
    crate::paths::data_file(QUICKLINKS_FILE_NAME)
}

fn looks_like_url(target: &str) -> bool {
    target.contains("://") || target.starts_with("www.")
}

fn normalize_url(target: &str) -> String {
    if target.contains("://") {
        target.to_string()
    } else {
        format!("https://{target}")
    }
}

fn quicklink_keyword(keyword: &str) -> String {
    normalize_keyword(keyword, Some('>'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quicklink_assignment() {
        let (keyword, title, target) =
            parse_quicklink_assignment("repo = https://github.com/example/repo").unwrap();

        assert_eq!(keyword, "repo");
        assert_eq!(title, "repo");
        assert_eq!(target, "https://github.com/example/repo");
    }
}
