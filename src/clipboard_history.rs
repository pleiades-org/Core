use crate::{
    command::{CommandCategory, CommandResult, FeatureAction},
    search_text::normalize_search_text,
};
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

const CLIPBOARD_HISTORY_FILE_NAME: &str = "clipboard_history.toml";
const FREE_RETENTION_DAYS: i64 = 92;
const MAX_FREE_ITEMS: usize = 500;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClipboardItemKind {
    Text,
    Link,
    Color,
    Image,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClipboardHistoryItem {
    pub id: String,
    pub text: String,
    pub kind: ClipboardItemKind,
    #[serde(default)]
    pub image_path: Option<PathBuf>,
    pub is_pinned: bool,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ClipboardHistoryStore {
    items: Vec<ClipboardHistoryItem>,
}

pub fn search_clipboard_history(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();
    let mut store = load_clipboard_history_store();
    prune_clipboard_history(&mut store);
    let _ = save_clipboard_history_store(&store);

    if trimmed_search_text.eq_ignore_ascii_case("clear") {
        return vec![CommandResult::feature(
            "Clear clipboard history",
            "Pinned items are also removed",
            CommandCategory::Clipboard,
            FeatureAction::ClearClipboardHistory,
            95,
        )];
    }

    let operation_search = parse_clipboard_operation(trimmed_search_text);
    let search_filter = operation_search
        .as_ref()
        .map(|operation| operation.search_text.as_str())
        .unwrap_or(trimmed_search_text);

    let items = ranked_clipboard_items(&store.items, search_filter);
    if let Some(operation) = operation_search {
        return items
            .into_iter()
            .take(10)
            .map(|item| match operation.kind {
                ClipboardOperationKind::Pin => CommandResult::feature(
                    format!("Pin clipboard item {}", preview_text(&item.text)),
                    kind_label(&item.kind),
                    CommandCategory::Clipboard,
                    FeatureAction::PinClipboardItem { item_id: item.id },
                    92,
                ),
                ClipboardOperationKind::Delete => CommandResult::feature(
                    format!("Delete clipboard item {}", preview_text(&item.text)),
                    kind_label(&item.kind),
                    CommandCategory::Clipboard,
                    FeatureAction::DeleteClipboardItem { item_id: item.id },
                    92,
                ),
            })
            .collect();
    }

    if trimmed_search_text.is_empty() {
        return clipboard_home_results(store.items);
    }

    items.into_iter().take(12).map(clipboard_result).collect()
}

pub fn record_clipboard_text(clipboard_text: &str) -> io::Result<bool> {
    let normalized_clipboard_text = clipboard_text.trim();
    if normalized_clipboard_text.is_empty()
        || should_ignore_clipboard_text(normalized_clipboard_text)
    {
        return Ok(false);
    }

    let item_id = clipboard_item_id(normalized_clipboard_text);
    let now = Local::now().timestamp();
    let mut store = load_clipboard_history_store();

    if let Some(existing_item) = store.items.iter_mut().find(|item| item.id == item_id) {
        existing_item.last_used_at = now;
        save_clipboard_history_store(&store)?;
        return Ok(false);
    }

    store.items.push(ClipboardHistoryItem {
        id: item_id,
        text: normalized_clipboard_text.to_string(),
        kind: classify_clipboard_text(normalized_clipboard_text),
        image_path: None,
        is_pinned: false,
        created_at: now,
        last_used_at: now,
    });
    prune_clipboard_history(&mut store);
    save_clipboard_history_store(&store)?;
    Ok(true)
}

pub fn record_clipboard_image(image_bytes: &[u8], extension: &str) -> io::Result<bool> {
    if image_bytes.is_empty() {
        return Ok(false);
    }

    let item_id = clipboard_bytes_id(image_bytes);
    let now = Local::now().timestamp();
    let mut store = load_clipboard_history_store();
    if let Some(existing_item) = store.items.iter_mut().find(|item| item.id == item_id) {
        existing_item.last_used_at = now;
        save_clipboard_history_store(&store)?;
        return Ok(false);
    }

    let image_directory = crate::paths::clipboard_images_dir();
    fs::create_dir_all(&image_directory)?;
    let normalized_extension = normalize_image_extension(extension);
    let image_path = image_directory.join(format!("{item_id}.{normalized_extension}"));
    fs::write(&image_path, image_bytes)?;

    store.items.push(ClipboardHistoryItem {
        id: item_id,
        text: format!("Image clipboard item ({normalized_extension})"),
        kind: ClipboardItemKind::Image,
        image_path: Some(image_path),
        is_pinned: false,
        created_at: now,
        last_used_at: now,
    });
    prune_clipboard_history(&mut store);
    save_clipboard_history_store(&store)?;
    Ok(true)
}

pub fn pin_clipboard_item(item_id: &str) -> io::Result<()> {
    let mut store = load_clipboard_history_store();
    if let Some(item) = store.items.iter_mut().find(|item| item.id == item_id) {
        item.is_pinned = true;
    }
    save_clipboard_history_store(&store)
}

pub fn delete_clipboard_item(item_id: &str) -> io::Result<()> {
    let mut store = load_clipboard_history_store();
    if let Some(position) = store.items.iter().position(|item| item.id == item_id) {
        let removed_item = store.items.remove(position);
        if let Some(image_path) = removed_item.image_path {
            let _ = fs::remove_file(image_path);
        }
    }
    save_clipboard_history_store(&store)
}

pub fn clear_clipboard_history() -> io::Result<()> {
    let store = load_clipboard_history_store();
    for item in &store.items {
        if let Some(image_path) = &item.image_path {
            let _ = fs::remove_file(image_path);
        }
    }
    save_clipboard_history_store(&ClipboardHistoryStore::default())
}

pub fn list_clipboard_items(search_text: &str) -> Vec<ClipboardHistoryItem> {
    let store = load_clipboard_history_store();
    ranked_clipboard_items(&store.items, search_text.trim())
}

pub fn invalidate_clipboard_store_cache() {
    if let Ok(mut cache) = clipboard_store_cache().lock() {
        *cache = None;
    }
}

pub fn clipboard_item_kind_label(kind: &ClipboardItemKind) -> &'static str {
    kind_label(kind)
}

pub fn clipboard_item_preview(text: &str) -> String {
    preview_text(text)
}

pub fn clipboard_item_list_preview(text: &str) -> String {
    preview_text_with_limit(text, 52)
}

pub fn clipboard_item_relative_time(timestamp: i64) -> String {
    let now = Local::now().timestamp();
    let elapsed_seconds = (now - timestamp).max(0);
    if elapsed_seconds < 60 {
        "Just now".to_string()
    } else if elapsed_seconds < 3600 {
        format!("{} min ago", elapsed_seconds / 60)
    } else if elapsed_seconds < 86_400 {
        format!("{} hr ago", elapsed_seconds / 3600)
    } else {
        format!("{} d ago", elapsed_seconds / 86_400)
    }
}

pub fn clipboard_item_accent_color(kind: &ClipboardItemKind) -> u32 {
    match kind {
        ClipboardItemKind::Text => 0x71717a,
        ClipboardItemKind::Link => 0x38bdf8,
        ClipboardItemKind::Color => 0xc084fc,
        ClipboardItemKind::Image => 0x14b8a6,
    }
}

pub fn parse_clipboard_color(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.starts_with('#') && trimmed.len() >= 4 {
        let hex = trimmed.trim_start_matches('#');
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 | 8 => hex.to_string(),
            _ => return None,
        };
        let value = u32::from_str_radix(&expanded[..6], 16).ok()?;
        return Some(value);
    }
    None
}

fn clipboard_home_results(items: Vec<ClipboardHistoryItem>) -> Vec<CommandResult> {
    if items.is_empty() {
        return vec![CommandResult::informational(
            "Clipboard history",
            "Copied text, links, and colors will appear here automatically",
        )];
    }

    let mut ranked_items = items;
    ranked_items.sort_by_key(|item| (std::cmp::Reverse(item.is_pinned), -item.last_used_at));
    ranked_items
        .into_iter()
        .take(12)
        .map(clipboard_result)
        .collect()
}

fn clipboard_result(item: ClipboardHistoryItem) -> CommandResult {
    let pin_label = if item.is_pinned { "Pinned · " } else { "" };
    let relative_time = clipboard_item_relative_time(item.last_used_at);
    let confidence = if item.is_pinned { 92 } else { 84 };

    if item.kind == ClipboardItemKind::Image {
        if let Some(image_path) = item.image_path {
            return CommandResult {
                title: preview_text(&item.text),
                subtitle: format!("{pin_label}Image · {relative_time}"),
                copy_text: item.text,
                explanation: None,
                icon_path: Some(image_path.clone()),
                calculation_display: None,
                category: CommandCategory::Clipboard,
                action: crate::command::CommandAction::Feature(
                    FeatureAction::CopyClipboardImage { image_path },
                ),
                confidence,
            };
        }
    }

    CommandResult::copyable_feature(
        preview_text(&item.text),
        format!(
            "{pin_label}{} · {relative_time}",
            kind_label(&item.kind)
        ),
        item.text,
        CommandCategory::Clipboard,
        confidence,
    )
}

fn ranked_clipboard_items(
    items: &[ClipboardHistoryItem],
    search_text: &str,
) -> Vec<ClipboardHistoryItem> {
    let normalized_search_text = normalize_search_text(search_text);
    let mut scored_items = items
        .iter()
        .filter_map(|item| {
            score_clipboard_item(item, &normalized_search_text).map(|score| (score, item.clone()))
        })
        .collect::<Vec<_>>();

    scored_items.sort_by_key(|(score, item)| {
        (
            std::cmp::Reverse(item.is_pinned),
            std::cmp::Reverse(*score),
            -item.last_used_at,
        )
    });

    scored_items.into_iter().map(|(_, item)| item).collect()
}

fn score_clipboard_item(item: &ClipboardHistoryItem, normalized_search_text: &str) -> Option<u8> {
    if normalized_search_text.is_empty() {
        return Some(70);
    }

    let normalized_text = normalize_search_text(&item.text);
    let kind_text = kind_label(&item.kind).to_lowercase();

    if normalized_text == normalized_search_text {
        return Some(96);
    }

    if normalized_text.starts_with(normalized_search_text) {
        return Some(88);
    }

    if normalized_text.contains(normalized_search_text) {
        return Some(78);
    }

    kind_text.contains(normalized_search_text).then_some(68)
}

struct ClipboardOperation {
    kind: ClipboardOperationKind,
    search_text: String,
}

enum ClipboardOperationKind {
    Pin,
    Delete,
}

fn parse_clipboard_operation(search_text: &str) -> Option<ClipboardOperation> {
    let trimmed_search_text = search_text.trim();
    let (operation_text, remaining_text) = trimmed_search_text
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed_search_text, ""));

    let kind = match operation_text.to_lowercase().as_str() {
        "pin" => ClipboardOperationKind::Pin,
        "delete" | "remove" => ClipboardOperationKind::Delete,
        _ => return None,
    };

    Some(ClipboardOperation {
        kind,
        search_text: remaining_text.trim().to_string(),
    })
}

fn prune_clipboard_history(store: &mut ClipboardHistoryStore) {
    let retention_cutoff = Local::now().timestamp() - FREE_RETENTION_DAYS * 24 * 60 * 60;

    let mut retained_items = Vec::new();
    let mut removed_items = Vec::new();

    for item in store.items.drain(..) {
        if item.is_pinned || item.created_at >= retention_cutoff {
            retained_items.push(item);
        } else {
            removed_items.push(item);
        }
    }

    retained_items.sort_by_key(|item| (std::cmp::Reverse(item.is_pinned), -item.last_used_at));

    let mut unpinned_item_count = 0;
    for item in retained_items {
        if item.is_pinned {
            store.items.push(item);
        } else if unpinned_item_count < MAX_FREE_ITEMS {
            unpinned_item_count += 1;
            store.items.push(item);
        } else {
            removed_items.push(item);
        }
    }

    for item in removed_items {
        if let Some(image_path) = item.image_path {
            let _ = fs::remove_file(image_path);
        }
    }
}

fn classify_clipboard_text(clipboard_text: &str) -> ClipboardItemKind {
    if looks_like_color(clipboard_text) {
        ClipboardItemKind::Color
    } else if looks_like_link(clipboard_text) {
        ClipboardItemKind::Link
    } else {
        ClipboardItemKind::Text
    }
}

fn should_ignore_clipboard_text(clipboard_text: &str) -> bool {
    if clipboard_text.len() > 8_000 {
        return true;
    }

    let sensitive_label_regex =
        Regex::new(r"(?i)(password|passwd|secret|token|api[_-]?key|private[_-]?key)\s*[:=]").ok();
    if sensitive_label_regex
        .as_ref()
        .is_some_and(|regex| regex.is_match(clipboard_text))
    {
        return true;
    }

    let compact_text = clipboard_text.trim();
    compact_text.len() >= 48
        && compact_text.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '-' | '_')
        })
}

fn looks_like_link(text: &str) -> bool {
    let normalized_text = text.trim().to_lowercase();
    normalized_text.starts_with("http://")
        || normalized_text.starts_with("https://")
        || normalized_text.starts_with("mailto:")
        || normalized_text.starts_with("file://")
}

fn looks_like_color(text: &str) -> bool {
    Regex::new(r"(?i)^\s*(#[0-9a-f]{3,8}|rgb\([^)]+\)|rgba\([^)]+\)|hsl\([^)]+\))\s*$")
        .ok()
        .is_some_and(|regex| regex.is_match(text))
}

fn kind_label(kind: &ClipboardItemKind) -> &'static str {
    match kind {
        ClipboardItemKind::Text => "Text",
        ClipboardItemKind::Link => "Link",
        ClipboardItemKind::Color => "Color",
        ClipboardItemKind::Image => "Image",
    }
}

fn preview_text(text: &str) -> String {
    preview_text_with_limit(text, 72)
}

fn preview_text_with_limit(text: &str, max_characters: usize) -> String {
    let normalized_text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized_text.chars().count() <= max_characters {
        return normalized_text;
    }

    let preview = normalized_text
        .chars()
        .take(max_characters.saturating_sub(1))
        .collect::<String>();
    format!("{preview}…")
}

fn clipboard_item_id(text: &str) -> String {
    clipboard_bytes_id(text.as_bytes())
}

fn clipboard_bytes_id(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn normalize_image_extension(extension: &str) -> String {
    match extension
        .trim()
        .trim_start_matches('.')
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "jpg".to_string(),
        "webp" => "webp".to_string(),
        "gif" => "gif".to_string(),
        "bmp" => "bmp".to_string(),
        "tif" | "tiff" => "tiff".to_string(),
        "ico" => "ico".to_string(),
        _ => "png".to_string(),
    }
}

#[derive(Clone, Debug)]
struct CachedStore {
    path: PathBuf,
    mtime: Option<std::time::SystemTime>,
    store: ClipboardHistoryStore,
}

fn clipboard_store_cache() -> &'static Mutex<Option<CachedStore>> {
    static CLIPBOARD_STORE_CACHE: OnceLock<Mutex<Option<CachedStore>>> = OnceLock::new();
    CLIPBOARD_STORE_CACHE.get_or_init(|| Mutex::new(None))
}

fn load_clipboard_history_store() -> ClipboardHistoryStore {
    let path = clipboard_history_file_path();
    let current_mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();

    if let Ok(cache) = clipboard_store_cache().lock() {
        if let Some(cached) = cache.as_ref() {
            if cached.path == path && cached.mtime == current_mtime {
                return cached.store.clone();
            }
        }
    }

    let store: ClipboardHistoryStore = fs::read_to_string(&path)
        .ok()
        .and_then(|store_text| toml::from_str(&store_text).ok())
        .unwrap_or_default();

    if let Ok(mut cache) = clipboard_store_cache().lock() {
        *cache = Some(CachedStore {
            path,
            mtime: current_mtime,
            store: store.clone(),
        });
    }

    store
}

fn save_clipboard_history_store(store: &ClipboardHistoryStore) -> io::Result<()> {
    let clipboard_history_path = clipboard_history_file_path();
    if let Some(clipboard_history_directory) = clipboard_history_path.parent() {
        fs::create_dir_all(clipboard_history_directory)?;
    }

    let clipboard_history_text = toml::to_string_pretty(store).unwrap_or_default();
    fs::write(&clipboard_history_path, clipboard_history_text)?;
    let new_mtime = fs::metadata(&clipboard_history_path).and_then(|m| m.modified()).ok();

    if let Ok(mut cache) = clipboard_store_cache().lock() {
        *cache = Some(CachedStore {
            path: clipboard_history_path,
            mtime: new_mtime,
            store: store.clone(),
        });
    }

    Ok(())
}

pub fn clipboard_storage_file_path() -> PathBuf {
    clipboard_history_file_path()
}

pub fn clipboard_storage_images_dir() -> PathBuf {
    crate::paths::clipboard_images_dir()
}

fn clipboard_history_file_path() -> PathBuf {
    crate::paths::data_file(CLIPBOARD_HISTORY_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_test_data_dir;

    #[test]
    fn ignores_secret_like_clipboard_text() {
        assert!(should_ignore_clipboard_text("api_key = abc123"));
    }

    #[test]
    fn classifies_color_clipboard_text() {
        assert_eq!(classify_clipboard_text("#ff00aa"), ClipboardItemKind::Color);
    }

    #[test]
    fn records_and_persists_text_to_user_storage() {
        with_test_data_dir(|test_dir| {
            invalidate_clipboard_store_cache();
            let recorded = record_clipboard_text("Hello User Storage Persistence")
                .expect("record clipboard text");
            assert!(recorded);

            let storage_file = test_dir.join(CLIPBOARD_HISTORY_FILE_NAME);
            assert!(storage_file.exists());

            let content = fs::read_to_string(&storage_file).expect("read clipboard storage file");
            assert!(content.contains("Hello User Storage Persistence"));

            let items = list_clipboard_items("");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].text, "Hello User Storage Persistence");
        });
    }

    #[test]
    fn records_image_and_deletes_image_file_on_delete() {
        with_test_data_dir(|_test_dir| {
            invalidate_clipboard_store_cache();
            let fake_image_bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
            let recorded = record_clipboard_image(&fake_image_bytes, "png")
                .expect("record clipboard image");
            assert!(recorded);

            let items = list_clipboard_items("");
            assert_eq!(items.len(), 1);
            let image_path = items[0].image_path.clone().expect("image path");
            assert!(image_path.exists());

            delete_clipboard_item(&items[0].id).expect("delete item");
            assert!(!image_path.exists());
            assert!(list_clipboard_items("").is_empty());
        });
    }

    #[test]
    fn clears_clipboard_history_and_deletes_stored_images() {
        with_test_data_dir(|_test_dir| {
            invalidate_clipboard_store_cache();
            let fake_image_bytes = vec![0xff, 0xd8, 0xff, 0xe0];
            record_clipboard_image(&fake_image_bytes, "jpg").expect("record image");
            record_clipboard_text("sample text").expect("record text");

            let items = list_clipboard_items("");
            assert_eq!(items.len(), 2);
            let image_path = items.iter().find_map(|item| item.image_path.clone()).expect("image path");
            assert!(image_path.exists());

            clear_clipboard_history().expect("clear history");
            assert!(!image_path.exists());
            assert!(list_clipboard_items("").is_empty());
        });
    }
}
