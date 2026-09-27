use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const NORMALIZE_CACHE_MAX_ENTRIES: usize = 256;

struct NormalizeCache {
    entries: HashMap<String, String>,
    order: Vec<String>,
}

impl NormalizeCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn get(&self, key: &str) -> Option<String> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, value: String) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), value);
            return;
        }

        while self.order.len() >= NORMALIZE_CACHE_MAX_ENTRIES {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
        }

        self.order.push(key.clone());
        self.entries.insert(key, value);
    }
}

static NORMALIZE_CACHE: LazyLock<Mutex<NormalizeCache>> =
    LazyLock::new(|| Mutex::new(NormalizeCache::new()));
static FILE_NORMALIZE_CACHE: LazyLock<Mutex<NormalizeCache>> =
    LazyLock::new(|| Mutex::new(NormalizeCache::new()));

fn normalize_search_text_uncached(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalize free-text search queries: trim, lowercase, collapse whitespace.
pub fn normalize_search_text(text: &str) -> String {
    if let Ok(cache) = NORMALIZE_CACHE.lock() {
        if let Some(cached) = cache.get(text) {
            return cached;
        }
    }

    let normalized = normalize_search_text_uncached(text);

    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
        cache.insert(text.to_string(), normalized.clone());
    }

    normalized
}

fn normalize_file_search_text_uncached(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .replace(['_', '-', '.'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Like [`normalize_search_text`], but also treats `_`, `-`, and `.` as word separators (for file names).
pub fn normalize_file_search_text(text: &str) -> String {
    if let Ok(cache) = FILE_NORMALIZE_CACHE.lock() {
        if let Some(cached) = cache.get(text) {
            return cached;
        }
    }

    let normalized = normalize_file_search_text_uncached(text);

    if let Ok(mut cache) = FILE_NORMALIZE_CACHE.lock() {
        cache.insert(text.to_string(), normalized.clone());
    }

    normalized
}

/// Normalize a user-defined keyword after stripping an optional trigger prefix (`;`, `>`, etc.).
pub fn normalize_keyword(keyword: &str, strip_prefix: Option<char>) -> String {
    let trimmed = keyword.trim();
    let without_prefix = match strip_prefix {
        Some(prefix) => trimmed.trim_start_matches(prefix),
        None => trimmed,
    };
    without_prefix
        .to_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect()
}

fn sort_scored_by_rank<T>(scored: &mut [(u8, T)], tie_breaker: impl Fn(&T) -> String) {
    scored.sort_by(|left, right| {
        let left_key = (Reverse(left.0), tie_breaker(&left.1));
        let right_key = (Reverse(right.0), tie_breaker(&right.1));
        left_key.cmp(&right_key)
    });
}

/// Score items, sort by descending score then tie-breaker, and take the top N matches.
pub fn take_top_scored<T>(
    items: impl IntoIterator<Item = T>,
    score: impl Fn(&T) -> Option<u8>,
    max_results: usize,
    tie_breaker: impl Fn(&T) -> String,
) -> Vec<(u8, T)> {
    let mut scored = items
        .into_iter()
        .filter_map(|item| score(&item).map(|score| (score, item)))
        .collect::<Vec<_>>();

    if scored.is_empty() || max_results == 0 {
        return Vec::new();
    }

    if scored.len() <= max_results {
        sort_scored_by_rank(&mut scored, tie_breaker);
        return scored;
    }

    let select_index = max_results - 1;
    scored.select_nth_unstable_by(select_index, |left, right| {
        let left_key = (Reverse(left.0), tie_breaker(&left.1));
        let right_key = (Reverse(right.0), tie_breaker(&right.1));
        left_key.cmp(&right_key)
    });
    scored.truncate(max_results);
    sort_scored_by_rank(&mut scored, tie_breaker);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace_and_case() {
        assert_eq!(normalize_search_text("  Hello   World  "), "hello world");
    }

    #[test]
    fn normalizes_keywords_with_prefix() {
        assert_eq!(normalize_keyword(";reply", Some(';')), "reply");
        assert_eq!(normalize_keyword(">repo", Some('>')), "repo");
    }

    #[test]
    fn normalizes_file_name_separators() {
        assert_eq!(
            normalize_file_search_text("my-cool_file.txt"),
            "my cool file txt"
        );
    }
}