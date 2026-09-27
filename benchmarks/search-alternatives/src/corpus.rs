use crate::arena::IndexArena;
use std::path::PathBuf;

pub const QUERIES: &[(&str, &str)] = &[
    ("short", "a"),
    ("short", "re"),
    ("short", "rs"),
    ("broad", "document"),
    ("broad", "2026"),
    ("broad", "app"),
    ("medium", "audio"),
    ("medium", "report"),
    ("medium", "config"),
    ("rare", "needle_7fd9"),
    ("rare", "migration_final"),
    ("miss", "zzqxyw_no_such_file"),
    ("miss", "report_zzq"),
    ("unicode", "café"),
    ("unicode", "東京"),
    ("phrase", "visual studio"),
    ("repeated", "aaaa"),
];

const STEMS: &[&str] = &[
    "application_config",
    "annual_report_2026",
    "audio_mixer",
    "document_draft",
    "visual studio code",
    "project_settings",
    "image_editor",
    "budget_2026",
    "terminal",
    "release_notes",
    "source_search",
    "meeting_notes",
    "readme",
    "browser_profile",
    "calendar",
    "photo_archive",
    "data_export",
    "test_runner",
    "presentation_document",
    "video_player",
    "café_menu",
    "東京_map",
    "aaaa_archive",
    "system_monitor",
    "account_report",
    "notification_service",
    "backup_document",
    "migration_script",
    "archive_2025",
    "daily_document",
    "application_shortcut",
];

/// Deterministic synthetic names; these paths are never opened or scanned.
pub fn paths(count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|file_id| {
            let mixed = (file_id as u64)
                .wrapping_mul(6364136223846793005)
                .rotate_left(17);
            let stem = match file_id {
                7 => "needle_7fd9",
                13 => "migration_final",
                _ => STEMS[(mixed as usize) % STEMS.len()],
            };
            let extension =
                ["rs", "txt", "exe", "md", "pdf", "png", "lnk"][(mixed >> 32) as usize % 7];
            PathBuf::from(format!(
                "C:/core-benchmark/group_{:03}/sub_{}/{stem}_{file_id:06x}.{extension}",
                file_id % 97,
                file_id % 11
            ))
        })
        .collect()
}

pub fn arena(count: usize) -> IndexArena {
    IndexArena::from_paths(PathBuf::from("C:/core-benchmark"), paths(count))
}

pub fn lexical_ranks(arena: &IndexArena) -> Vec<u32> {
    let mut identifiers: Vec<_> = (0..arena.files.len()).collect();
    identifiers.sort_unstable_by(|&left, &right| {
        arena.files[left]
            .name_lower
            .cmp(&arena.files[right].name_lower)
            .then(left.cmp(&right))
    });
    let mut ranks = vec![0; identifiers.len()];
    for (rank, identifier) in identifiers.into_iter().enumerate() {
        ranks[identifier] = rank as u32;
    }
    ranks
}
