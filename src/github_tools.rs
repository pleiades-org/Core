use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    secret_store::load_github_token,
    search_text::normalize_search_text,
};
use serde::Deserialize;

const GITHUB_SEARCH_API: &str = "https://api.github.com/search";

pub fn search_github(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);

    if normalized_search_text.is_empty() {
        return github_catalog();
    }

    let (kind, query) = build_github_query(&normalized_search_text);
    search_github_api(kind, &query)
}

pub fn build_github_query(normalized_search_text: &str) -> (&'static str, String) {
    if let Some(rest) = normalized_search_text.strip_prefix("issues ") {
        return ("issues", rest.to_string());
    }
    if let Some(rest) = normalized_search_text.strip_prefix("issue ") {
        return ("issues", rest.to_string());
    }
    if let Some(rest) = normalized_search_text.strip_prefix("prs ") {
        return ("prs", rest.to_string());
    }
    if let Some(rest) = normalized_search_text.strip_prefix("pr ") {
        return ("prs", rest.to_string());
    }
    if let Some(rest) = normalized_search_text.strip_prefix("repo ") {
        return ("repos", rest.to_string());
    }
    if let Some(rest) = normalized_search_text.strip_prefix("repos ") {
        return ("repos", rest.to_string());
    }

    if normalized_search_text.contains('/') && !normalized_search_text.contains(' ') {
        return ("repos", normalized_search_text.to_string());
    }

    ("issues", normalized_search_text.to_string())
}

fn github_catalog() -> Vec<CommandResult> {
    vec![
        hint_result(
            "GitHub issues",
            "issues rust lang:rust",
            "issues ",
            86,
        ),
        hint_result("GitHub pull requests", "prs owner/repo", "prs ", 85),
        hint_result("GitHub repositories", "repo torvalds/linux", "repo ", 84),
    ]
}

fn hint_result(title: &str, subtitle: &str, copy_text: &str, confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(title, subtitle, copy_text, CommandCategory::DevTools, confidence)
}

fn search_github_api(kind: &str, query: &str) -> Vec<CommandResult> {
    let github_query = match kind {
        "issues" => format!("is:issue {query}"),
        "prs" => format!("is:pr repo:{query}"),
        "repos" => format!("repo:{query}"),
        _ => query.to_string(),
    };

    let url = format!(
        "{GITHUB_SEARCH_API}/{}?q={}&per_page=8",
        github_endpoint(kind),
        url_encode(&github_query)
    );

    let mut request = ureq::get(&url);
    request = request.set("Accept", "application/vnd.github+json");
    request = request.set("User-Agent", "CoreLauncher");
    if let Some(token) = load_github_token() {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }

    match request.call() {
        Ok(response) => {
            if !(200..300).contains(&response.status()) {
                return vec![CommandResult::informational(
                    "GitHub search",
                    &format!("GitHub API returned status {}", response.status()),
                )];
            }

            let body = response.into_string().unwrap_or_default();
            parse_github_results(&body, kind)
        }
        Err(error) => vec![CommandResult::informational(
            "GitHub search",
            &format!("Network error: {error}"),
        )],
    }
}

fn github_endpoint(kind: &str) -> &'static str {
    match kind {
        "repos" => "repositories",
        _ => "issues",
    }
}

fn parse_github_results(body: &str, kind: &str) -> Vec<CommandResult> {
    let parsed: GitHubSearchResponse = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => {
            return vec![CommandResult::informational(
                "GitHub search",
                "Could not parse GitHub response",
            )];
        }
    };

    if parsed.items.is_empty() {
        return vec![CommandResult::informational(
            "GitHub search",
            "No results found",
        )];
    }

    parsed
        .items
        .into_iter()
        .take(8)
        .map(|item| github_item_result(item, kind))
        .collect()
}

fn github_item_result(item: GitHubItem, kind: &str) -> CommandResult {
    let title = item.title.unwrap_or_else(|| item.full_name.clone().unwrap_or_default());
    let subtitle = item
        .repository_url
        .as_deref()
        .or(item.html_url.as_deref())
        .unwrap_or("")
        .to_string();
    let url = item.html_url.unwrap_or_default();

    let confidence = match kind {
        "repos" => 88,
        "prs" => 87,
        _ => 86,
    };

    CommandResult {
        title,
        subtitle,
        copy_text: url.clone(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::DevTools,
        action: CommandAction::OpenUrl(url),
        confidence,
    }
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct GitHubSearchResponse {
    items: Vec<GitHubItem>,
}

#[derive(Debug, Deserialize)]
struct GitHubItem {
    title: Option<String>,
    #[serde(rename = "full_name")]
    full_name: Option<String>,
    #[serde(rename = "html_url")]
    html_url: Option<String>,
    #[serde(rename = "repository_url")]
    repository_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_issues_query() {
        let (kind, query) = build_github_query("issues rust lang:rust");
        assert_eq!(kind, "issues");
        assert_eq!(query, "rust lang:rust");
    }

    #[test]
    fn builds_prs_query() {
        let (kind, query) = build_github_query("prs torvalds/linux");
        assert_eq!(kind, "prs");
        assert_eq!(query, "torvalds/linux");
    }

    #[test]
    fn builds_repo_query_from_shorthand() {
        let (kind, query) = build_github_query("torvalds/linux");
        assert_eq!(kind, "repos");
        assert_eq!(query, "torvalds/linux");
    }

    #[test]
    fn empty_scope_returns_catalog() {
        let results = search_github("");
        assert_eq!(results.len(), 3);
    }
}