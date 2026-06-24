use crate::command::{CommandAction, CommandResult};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

const RECENT_USAGE_FILE_NAME: &str = "recent_usage.toml";
const MAX_STORED_ITEMS: usize = 50;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RecentUsageItem {
    result: CommandResult,
    use_count: u32,
    last_used_at: i64,
    #[serde(default)]
    pinned: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct RecentUsageStore {
    items: Vec<RecentUsageItem>,
}

pub fn home_results(max_results: usize) -> Vec<CommandResult> {
    home_results_from_store(&load_recent_usage_store(), max_results)
}

fn home_results_from_store(store: &RecentUsageStore, max_results: usize) -> Vec<CommandResult> {
    let ranked = ranked_usage_items(&store.items);

    if ranked.is_empty() {
        return vec![CommandResult::informational(
            "Recent",
            "Items you run will appear here",
        )];
    }

    ranked
        .into_iter()
        .take(max_results)
        .map(recent_result_with_subtitle)
        .collect()
}

pub fn record_usage(selected_result: &CommandResult) -> io::Result<()> {
    if matches!(selected_result.action, CommandAction::None) {
        return Ok(());
    }

    let now = Local::now().timestamp();
    let item_id = usage_item_id(selected_result);
    let mut store = load_recent_usage_store();

    if let Some(existing_item) = store
        .items
        .iter_mut()
        .find(|item| usage_item_id(&item.result) == item_id)
    {
        existing_item.use_count = existing_item.use_count.saturating_add(1);
        existing_item.last_used_at = now;
        existing_item.result = selected_result.clone();
    } else {
        store.items.push(RecentUsageItem {
            result: selected_result.clone(),
            use_count: 1,
            last_used_at: now,
            pinned: false,
        });
    }

    prune_recent_usage(&mut store);
    save_recent_usage_store(&store)
}

pub fn pin_usage_item(item_id: &str) -> io::Result<()> {
    let mut store = load_recent_usage_store();
    if let Some(existing_item) = store
        .items
        .iter_mut()
        .find(|item| usage_item_id(&item.result) == item_id)
    {
        existing_item.pinned = true;
        save_recent_usage_store(&store)
    } else {
        Ok(())
    }
}

pub fn clear_recent_usage() -> io::Result<()> {
    save_recent_usage_store(&RecentUsageStore::default())
}

pub fn format_last_used(timestamp: i64) -> String {
    let now = Local::now().timestamp();
    let delta = now.saturating_sub(timestamp);

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3_600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3_600)
    } else if delta < 604_800 {
        format!("{}d ago", delta / 86_400)
    } else {
        format!("{}w ago", delta / 604_800)
    }
}

pub fn usage_item_id(result: &CommandResult) -> String {
    format!(
        "{}|{}|{}|{:?}",
        result.title, result.subtitle, result.copy_text, result.action
    )
}

fn recent_result_with_subtitle(item: RecentUsageItem) -> CommandResult {
    let mut result = item.result;
    let relative = format_last_used(item.last_used_at);
    let pin_prefix = if item.pinned { "Pinned · " } else { "" };

    if result.subtitle.is_empty() {
        result.subtitle = format!("{pin_prefix}{relative}");
    } else {
        result.subtitle = format!("{pin_prefix}{} · {relative}", result.subtitle);
    }

    result
}

fn ranked_usage_items(items: &[RecentUsageItem]) -> Vec<RecentUsageItem> {
    let mut ranked = items.to_vec();
    ranked.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.use_count.cmp(&left.use_count))
            .then_with(|| right.last_used_at.cmp(&left.last_used_at))
    });
    ranked
}

fn prune_recent_usage(store: &mut RecentUsageStore) {
    if store.items.len() <= MAX_STORED_ITEMS {
        return;
    }

    store.items = ranked_usage_items(&store.items)
        .into_iter()
        .take(MAX_STORED_ITEMS)
        .collect();
}

fn load_recent_usage_store() -> RecentUsageStore {
    fs::read_to_string(recent_usage_file_path())
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_recent_usage_store(store: &RecentUsageStore) -> io::Result<()> {
    let recent_usage_path = recent_usage_file_path();
    if let Some(recent_usage_directory) = recent_usage_path.parent() {
        fs::create_dir_all(recent_usage_directory)?;
    }

    let recent_usage_text = toml::to_string_pretty(store).unwrap_or_default();
    fs::write(recent_usage_path, recent_usage_text)
}

fn recent_usage_file_path() -> PathBuf {
    crate::paths::data_file(RECENT_USAGE_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandAction, CommandCategory, CommandResult};
    use crate::test_support::with_test_data_dir;

    #[test]
    fn ranked_usage_items_prefers_pinned_then_higher_use_count() {
        let items = vec![
            RecentUsageItem {
                result: CommandResult::informational("A", "once"),
                use_count: 10,
                last_used_at: 100,
                pinned: false,
            },
            RecentUsageItem {
                result: CommandResult::informational("B", "pinned"),
                use_count: 1,
                last_used_at: 50,
                pinned: true,
            },
            RecentUsageItem {
                result: CommandResult::informational("C", "often"),
                use_count: 5,
                last_used_at: 200,
                pinned: false,
            },
        ];

        let ranked = ranked_usage_items(&items);
        assert_eq!(ranked[0].result.title, "B");
        assert_eq!(ranked[1].result.title, "A");
    }

    #[test]
    fn format_last_used_uses_hours_for_recent_activity() {
        let now = Local::now().timestamp();
        assert_eq!(format_last_used(now - 7_200), "2h ago");
    }

    #[test]
    fn record_usage_skips_informational_actions() {
        with_test_data_dir(|test_dir| {
            let result = CommandResult::informational("Help", "Nothing actionable");
            assert!(matches!(result.action, CommandAction::None));
            assert!(record_usage(&result).is_ok());
            assert!(!test_dir.join("recent_usage.toml").exists());
        });
    }

    #[test]
    fn record_usage_persists_actionable_results_in_test_data_dir() {
        with_test_data_dir(|test_dir| {
            let result = CommandResult::copyable_feature(
                "Test",
                "Example",
                "test",
                CommandCategory::Help,
                80,
            );

            record_usage(&result).expect("record usage");

            let usage_path = test_dir.join("recent_usage.toml");
            assert!(usage_path.exists());
            let saved = fs::read_to_string(&usage_path).expect("read recent usage");
            assert!(saved.contains("Test"));
        });
    }

    #[test]
    fn home_results_shows_placeholder_when_empty() {
        let results = home_results_from_store(&RecentUsageStore::default(), 8);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Recent");
        assert!(matches!(results[0].category, CommandCategory::Help));
    }
}