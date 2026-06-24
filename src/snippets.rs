use crate::{
    command::{CommandCategory, CommandResult, FeatureAction},
    search_text::{normalize_keyword, normalize_search_text},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs, io,
    path::PathBuf,
};

const SNIPPETS_FILE_NAME: &str = "snippets.toml";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnippetRecord {
    pub keyword: String,
    pub title: String,
    pub body: String,
    pub is_team_shared: bool,
    pub updated_at: i64,
}

#[derive(Default, Deserialize, Serialize)]
struct SnippetStore {
    snippets: Vec<SnippetRecord>,
    #[serde(default)]
    hidden_keywords: Vec<String>,
}

pub fn search_snippets(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();
    if trimmed_search_text.is_empty() {
        return snippet_home_results();
    }

    if let Some((keyword, title, body)) = parse_snippet_assignment(trimmed_search_text) {
        return vec![CommandResult::feature(
            format!("Save snippet ;{keyword}"),
            "Create or update a reusable text expansion",
            CommandCategory::Snippet,
            FeatureAction::SaveSnippet {
                keyword,
                title,
                body,
            },
            96,
        )];
    }

    if trimmed_search_text.eq_ignore_ascii_case("export") {
        return vec![CommandResult {
            title: "Open snippets export".to_string(),
            subtitle: snippets_file_path().display().to_string(),
            copy_text: snippets_file_path().display().to_string(),
            explanation: Some("Snippets are stored as TOML for import/export.".to_string()),
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Snippet,
            action: crate::command::CommandAction::OpenPath(snippets_file_path()),
            confidence: 82,
        }];
    }

    ranked_snippets(trimmed_search_text)
        .into_iter()
        .map(snippet_result)
        .collect()
}

pub fn search_snippet_keywords(query: &str) -> Vec<CommandResult> {
    let trimmed_query = query.trim();
    let snippet_query = trimmed_query.strip_prefix(';').unwrap_or(trimmed_query);
    let normalized_query = snippet_keyword(snippet_query);

    if normalized_query.is_empty() {
        return Vec::new();
    }

    load_all_snippets()
        .into_iter()
        .filter(|snippet| snippet_keyword(&snippet.keyword) == normalized_query)
        .map(snippet_result)
        .collect()
}

pub fn save_snippet(keyword: String, title: String, body: String) -> io::Result<()> {
    let normalized_keyword = snippet_keyword(&keyword);
    let mut store = load_snippet_store();

    store
        .snippets
        .retain(|snippet| snippet_keyword(&snippet.keyword) != normalized_keyword);
    store.snippets.push(SnippetRecord {
        keyword: normalized_keyword,
        title: title.trim().to_string(),
        body,
        is_team_shared: false,
        updated_at: Local::now().timestamp(),
    });
    store
        .snippets
        .sort_by_key(|snippet| snippet.keyword.to_lowercase());

    save_snippet_store(&store)
}

pub fn configured_snippets() -> Vec<SnippetRecord> {
    load_all_snippets()
}

pub fn delete_snippet(keyword: &str) -> io::Result<()> {
    let normalized_keyword = snippet_keyword(keyword);
    let mut store = load_snippet_store();
    let store_len_before = store.snippets.len();
    store
        .snippets
        .retain(|snippet| snippet_keyword(&snippet.keyword) != normalized_keyword);

    if store.snippets.len() == store_len_before
        && !store
            .hidden_keywords
            .iter()
            .any(|hidden| snippet_keyword(hidden) == normalized_keyword)
    {
        store.hidden_keywords.push(normalized_keyword);
    }

    save_snippet_store(&store)
}

fn snippet_home_results() -> Vec<CommandResult> {
    let mut results = vec![
        CommandResult::informational(
            "Snippets",
            "Use @snippet keyword = text to create one, or type ;keyword to insert it",
        ),
        CommandResult {
            title: "Open snippets export".to_string(),
            subtitle: snippets_file_path().display().to_string(),
            copy_text: snippets_file_path().display().to_string(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Snippet,
            action: crate::command::CommandAction::OpenPath(snippets_file_path()),
            confidence: 72,
        },
    ];
    results.extend(load_all_snippets().into_iter().map(snippet_result));
    results
}

fn ranked_snippets(search_text: &str) -> Vec<SnippetRecord> {
    let normalized_search_text = normalize_search_text(search_text);
    let mut scored_snippets = load_all_snippets()
        .into_iter()
        .filter_map(|snippet| {
            score_snippet(&snippet, &normalized_search_text).map(|score| (score, snippet))
        })
        .collect::<Vec<_>>();

    scored_snippets.sort_by_key(|(score, snippet)| {
        (
            std::cmp::Reverse(*score),
            snippet.keyword.to_lowercase(),
            snippet.title.to_lowercase(),
        )
    });

    scored_snippets
        .into_iter()
        .map(|(_, snippet)| snippet)
        .collect()
}

fn snippet_result(snippet: SnippetRecord) -> CommandResult {
    let rendered_body = render_dynamic_placeholders(&snippet.body);
    let shared_label = if snippet.is_team_shared {
        "Shared Teams snippet"
    } else {
        "Local snippet"
    };

    CommandResult::copyable_feature(
        format!(";{}  {}", snippet.keyword, snippet.title),
        shared_label,
        rendered_body,
        CommandCategory::Snippet,
        90,
    )
}

fn score_snippet(snippet: &SnippetRecord, normalized_search_text: &str) -> Option<u8> {
    let keyword = normalize_search_text(&snippet.keyword);
    let title = normalize_search_text(&snippet.title);
    let body = normalize_search_text(&snippet.body);

    if keyword == normalized_search_text {
        return Some(96);
    }

    if keyword.starts_with(normalized_search_text) {
        return Some(88);
    }

    if title.contains(normalized_search_text) {
        return Some(78);
    }

    body.contains(normalized_search_text).then_some(64)
}

fn parse_snippet_assignment(search_text: &str) -> Option<(String, String, String)> {
    let (keyword_text, body_text) = search_text.split_once('=')?;
    let keyword = snippet_keyword(keyword_text.trim().trim_start_matches(';'));
    let body = body_text.trim().to_string();

    if keyword.is_empty() || body.is_empty() {
        return None;
    }

    let title = keyword.replace(['-', '_'], " ");
    Some((keyword, title, body))
}

fn load_all_snippets() -> Vec<SnippetRecord> {
    let store = load_snippet_store();
    let hidden_keywords: HashSet<String> = store
        .hidden_keywords
        .iter()
        .map(|keyword| snippet_keyword(keyword))
        .collect();

    let mut snippets = default_snippets()
        .into_iter()
        .filter(|snippet| !hidden_keywords.contains(&snippet_keyword(&snippet.keyword)))
        .collect::<Vec<_>>();

    for custom_snippet in store.snippets {
        snippets.retain(|snippet| {
            snippet_keyword(&snippet.keyword) != snippet_keyword(&custom_snippet.keyword)
        });
        snippets.push(custom_snippet);
    }

    snippets.sort_by_key(|snippet| snippet.keyword.to_lowercase());
    snippets
}

fn default_snippets() -> Vec<SnippetRecord> {
    [
        ("sig", "Email signature", "Best,\n{user}", true),
        (
            "thanks",
            "Thank-you reply",
            "Thanks for sending this over. I will take a look and get back to you.",
            true,
        ),
        (
            "codeblock",
            "Markdown code block",
            "```{language}\n{cursor}\n```",
            false,
        ),
        ("today", "Current date", "{date}", false),
    ]
    .into_iter()
    .map(|(keyword, title, body, is_team_shared)| SnippetRecord {
        keyword: keyword.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        is_team_shared,
        updated_at: 0,
    })
    .collect()
}

fn render_dynamic_placeholders(body: &str) -> String {
    let now = Local::now();
    let user_name = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Core user".to_string());

    body.replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{time}", &now.format("%H:%M").to_string())
        .replace("{datetime}", &now.format("%Y-%m-%d %H:%M").to_string())
        .replace("{user}", &user_name)
        .replace("{cursor}", "")
}

fn load_snippet_store() -> SnippetStore {
    fs::read_to_string(snippets_file_path())
        .ok()
        .and_then(|store_text| toml::from_str(&store_text).ok())
        .unwrap_or_default()
}

fn save_snippet_store(store: &SnippetStore) -> io::Result<()> {
    let snippets_path = snippets_file_path();
    if let Some(snippets_directory) = snippets_path.parent() {
        fs::create_dir_all(snippets_directory)?;
    }

    let snippets_text = toml::to_string_pretty(store).unwrap_or_default();
    fs::write(snippets_path, snippets_text)
}

pub fn snippets_file_path() -> PathBuf {
    crate::paths::data_file(SNIPPETS_FILE_NAME)
}

fn snippet_keyword(keyword: &str) -> String {
    normalize_keyword(keyword, Some(';'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snippet_assignment() {
        let (keyword, title, body) = parse_snippet_assignment("reply = Hello {user}").unwrap();

        assert_eq!(keyword, "reply");
        assert_eq!(title, "reply");
        assert_eq!(body, "Hello {user}");
    }
}
