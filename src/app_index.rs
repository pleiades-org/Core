use crate::{
    command::CommandResult,
    icon_cache,
    search_text::{normalize_search_text, take_top_scored},
};
use std::{collections::HashSet, env, path::{Path, PathBuf}};
use walkdir::WalkDir;

const MAX_INDEXED_START_MENU_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationEntry {
    pub name: String,
    pub normalized_name: String,
    pub path: PathBuf,
    pub icon_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationIndex {
    applications: Vec<ApplicationEntry>,
}

impl ApplicationIndex {
    pub fn load_from_windows_start_menu() -> Self {
        let mut applications = Vec::new();
        let mut seen_paths = HashSet::new();

        for start_menu_directory in windows_start_menu_directories() {
            if !start_menu_directory.exists() {
                continue;
            }

            for directory_entry in WalkDir::new(start_menu_directory)
                .max_depth(MAX_INDEXED_START_MENU_DEPTH)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
            {
                let path = directory_entry.path().to_path_buf();
                if !is_launchable_start_menu_file(&path) || !seen_paths.insert(path.clone()) {
                    continue;
                }

                if let Some(application_name) = application_name_from_path(&path) {
                    let icon_path = icon_cache::cached_icon_path_for(&path);
                    let normalized_name = normalize_search_text(&application_name);
                    applications.push(ApplicationEntry {
                        name: application_name,
                        normalized_name,
                        path,
                        icon_path,
                    });
                }
            }
        }

        applications.extend(common_windows_tools());
        applications.sort_by_key(|application| application.name.to_lowercase());
        applications.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));

        Self { applications }
    }

    pub fn search(&self, query: &str, max_results: usize) -> Vec<CommandResult> {
        let normalized_query = normalize_search_text(query);
        if normalized_query.is_empty() {
            return Vec::new();
        }

        take_top_scored(
            self.applications.iter(),
            |application| score_application(application, &normalized_query),
            max_results,
            |application| application.normalized_name.clone(),
        )
        .into_iter()
        .map(|(score, application)| {
            CommandResult::application(
                application.name.clone(),
                application.path.display().to_string(),
                application.path.clone(),
                application.icon_path.clone(),
                score,
            )
        })
        .collect()
    }

    pub fn application_count(&self) -> usize {
        self.applications.len()
    }

    #[doc(hidden)]
    pub fn from_entries(applications: Vec<ApplicationEntry>) -> Self {
        Self { applications }
    }
}

fn windows_start_menu_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if let Some(program_data) = env::var_os("ProgramData") {
        directories.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    if let Some(app_data) = env::var_os("APPDATA") {
        directories.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    directories
}

fn is_launchable_start_menu_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_lowercase().as_str(),
                "lnk" | "url" | "appref-ms" | "exe"
            )
        })
        .unwrap_or(false)
}

fn application_name_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .map(|file_stem| file_stem.replace(['_', '-'], " "))
}

fn common_windows_tools() -> Vec<ApplicationEntry> {
    [
        ("Notepad", system32_executable_path("notepad.exe")),
        ("Calculator", system32_executable_path("calc.exe")),
        ("Command Prompt", system32_executable_path("cmd.exe")),
        (
            "PowerShell",
            system32_executable_path("WindowsPowerShell\\v1.0\\powershell.exe"),
        ),
        ("File Explorer", windows_executable_path("explorer.exe")),
        ("Task Manager", system32_executable_path("taskmgr.exe")),
    ]
    .into_iter()
    .map(|(name, path)| {
        let icon_path = icon_cache::cached_icon_path_for(&path);
        let name = name.to_string();
        let normalized_name = normalize_search_text(&name);
        ApplicationEntry {
            name,
            normalized_name,
            path,
            icon_path,
        }
    })
    .collect()
}

fn system32_executable_path(executable_name: &str) -> PathBuf {
    env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("System32")
        .join(executable_name)
}

fn windows_executable_path(executable_name: &str) -> PathBuf {
    env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join(executable_name)
}

fn score_application(application: &ApplicationEntry, normalized_query: &str) -> Option<u8> {
    let normalized_name = &application.normalized_name;

    if normalized_name == normalized_query {
        return Some(95);
    }

    if normalized_name.starts_with(normalized_query) {
        return Some(88);
    }

    if normalized_name.contains(normalized_query) {
        return Some(76);
    }

    let all_query_words_match = normalized_query
        .split_whitespace()
        .all(|query_word| normalized_name.contains(query_word));

    all_query_words_match.then_some(64)
}


