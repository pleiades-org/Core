//! Launcher data paths under the user config directory.

use crate::{settings::config_directory, test_support::test_data_dir};
use std::path::PathBuf;

fn data_root() -> PathBuf {
    test_data_dir().unwrap_or_else(config_directory)
}

pub fn config_file() -> PathBuf {
    data_root().join("config.toml")
}

pub fn data_file(file_name: &str) -> PathBuf {
    data_root().join(file_name)
}

pub fn data_subdirectory(name: &str) -> PathBuf {
    data_root().join(name)
}

pub fn notes_dir() -> PathBuf {
    data_subdirectory("notes")
}

pub fn deleted_notes_dir() -> PathBuf {
    data_subdirectory("deleted_notes")
}

pub fn icons_dir() -> PathBuf {
    data_subdirectory("icons")
}

pub fn lucide_cache_dir() -> PathBuf {
    data_subdirectory("lucide_cache")
}

pub fn currency_rates_file() -> PathBuf {
    data_file("currency_rates.toml")
}

pub fn clipboard_images_dir() -> PathBuf {
    data_subdirectory("clipboard_images")
}

pub fn deleted_files_dir() -> PathBuf {
    data_subdirectory("deleted_files")
}

pub fn quick_note_file() -> PathBuf {
    data_file("quick_note.md")
}

pub fn cache_dir() -> PathBuf {
    data_subdirectory("cache")
}