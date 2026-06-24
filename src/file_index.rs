use crate::{
    command::CommandResult,
    search_text::{normalize_file_search_text, take_top_scored},
};
use std::{collections::HashSet, env, fs, path::{Component, Path, PathBuf}};
use walkdir::WalkDir;

const MAX_INDEX_DEPTH: usize = 16;
const MAX_INDEXED_FILES: usize = 150_000;
const MAX_CONTENT_SEARCH_BYTES: u64 = 1_000_000;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "webm", "wmv", "m4v", "flv", "mpeg", "mpg",
];
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "heic", "avif", "svg", "ico",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub normalized_name: String,
    pub normalized_stem: String,
    pub path: PathBuf,
    pub extension: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    files: Vec<FileEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSearchScope {
    AllFiles,
    Content,
    Videos,
    Images,
    Extension(String),
}

impl FileIndex {
    pub fn load_from_user_directories() -> Self {
        let mut files = Vec::new();
        let mut seen_paths = HashSet::new();

        for search_directory in user_search_directories() {
            if !search_directory.exists() {
                continue;
            }

            for directory_entry in WalkDir::new(search_directory)
                .max_depth(MAX_INDEX_DEPTH)
                .into_iter()
                .filter_entry(|entry| should_enter_path(entry.path()))
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
            {
                if files.len() >= MAX_INDEXED_FILES {
                    break;
                }

                let path = directory_entry.path().to_path_buf();
                if !seen_paths.insert(path.clone()) {
                    continue;
                }

                let Some(name) = path
                    .file_name()
                    .and_then(|file_name| file_name.to_str())
                    .map(ToOwned::to_owned)
                else {
                    continue;
                };

                let extension = normalized_extension(&path);
                let normalized_name = normalize_file_search_text(&name);
                let normalized_stem = path
                    .file_stem()
                    .and_then(|file_stem| file_stem.to_str())
                    .map(normalize_file_search_text)
                    .unwrap_or_else(|| normalized_name.clone());
                files.push(FileEntry {
                    name,
                    normalized_name,
                    normalized_stem,
                    path,
                    extension,
                });
            }
        }

        files.sort_by_key(|file| file.name.to_lowercase());
        Self { files }
    }

    pub fn search(
        &self,
        query: &str,
        scope: FileSearchScope,
        max_results: usize,
    ) -> Vec<CommandResult> {
        let normalized_query = normalize_file_search_text(query);
        if normalized_query.is_empty() {
            return Vec::new();
        }

        let query_words = normalized_query
            .split_whitespace()
            .collect::<Vec<&str>>();
        let first_word = query_words.first().copied();

        take_top_scored(
            self.files.iter().filter(|file| {
                file_matches_scope(file, &scope)
                    && first_word.is_none_or(|word| file.normalized_name.contains(word))
            }),
            |file| score_file(file, &normalized_query, &scope, &query_words),
            max_results,
            |file| file.normalized_name.clone(),
        )
        .into_iter()
        .map(|(score, file)| {
            CommandResult::file(
                file.name.clone(),
                file.path.display().to_string(),
                file.path.clone(),
                score,
            )
        })
        .collect()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    #[doc(hidden)]
    pub fn from_entries(files: Vec<FileEntry>) -> Self {
        Self { files }
    }

    pub fn recent_files(&self, max_results: usize) -> Vec<CommandResult> {
        self.recent_files_for_scope(FileSearchScope::AllFiles, max_results)
    }

    pub fn recent_files_for_scope(
        &self,
        scope: FileSearchScope,
        max_results: usize,
    ) -> Vec<CommandResult> {
        let mut files_with_modified_times = self
            .files
            .iter()
            .filter(|file| file_matches_scope(file, &scope))
            .filter_map(|file| {
                let modified_at = fs::metadata(&file.path).ok()?.modified().ok()?;
                Some((modified_at, file))
            })
            .collect::<Vec<_>>();

        files_with_modified_times.sort_by_key(|(modified_at, file)| {
            (std::cmp::Reverse(*modified_at), file.name.to_lowercase())
        });

        files_with_modified_times
            .into_iter()
            .take(max_results)
            .map(|(_, file)| {
                CommandResult::file(
                    file.name.clone(),
                    file.path.display().to_string(),
                    file.path.clone(),
                    82,
                )
            })
            .collect()
    }
}

pub fn scope_from_tag(tag: &str) -> Option<FileSearchScope> {
    let normalized_tag = tag
        .trim()
        .trim_start_matches('@')
        .trim_start_matches('.')
        .to_lowercase();

    if normalized_tag.contains('/') {
        return normalized_tag.split('/').find_map(scope_from_tag);
    }

    match normalized_tag.as_str() {
        "file" | "files" => Some(FileSearchScope::AllFiles),
        "file:content" | "files:content" | "content" => Some(FileSearchScope::Content),
        "video" | "videos" | "vid" | "vids" => Some(FileSearchScope::Videos),
        "image" | "images" | "picture" | "pictures" | "pic" | "pics" => {
            Some(FileSearchScope::Images)
        }
        extension if is_probable_file_extension(extension) => {
            Some(FileSearchScope::Extension(extension.to_string()))
        }
        _ => None,
    }
}

fn user_search_directories() -> Vec<PathBuf> {
    let user_profile = env::var_os("USERPROFILE").map(PathBuf::from);
    let system_drive = env::var_os("SystemDrive").map(PathBuf::from);
    let mut directories = Vec::new();
    let mut seen_directories = HashSet::new();

    if let Some(user_profile) = user_profile.as_ref() {
        push_unique_directory(
            &mut directories,
            &mut seen_directories,
            user_profile.clone(),
        );
        for directory_name in [
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Videos",
            "Music",
        ] {
            push_unique_directory(
                &mut directories,
                &mut seen_directories,
                user_profile.join(directory_name),
            );
        }
    }

    for drive_root in existing_drive_roots() {
        if system_drive
            .as_ref()
            .is_some_and(|system_drive| same_drive_root(system_drive, &drive_root))
        {
            continue;
        }

        push_unique_directory(&mut directories, &mut seen_directories, drive_root);
    }

    directories
}

fn existing_drive_roots() -> Vec<PathBuf> {
    ('A'..='Z')
        .map(|drive_letter| PathBuf::from(format!("{drive_letter}:\\")))
        .filter(|drive_root| drive_root.is_dir())
        .collect()
}

fn push_unique_directory(
    directories: &mut Vec<PathBuf>,
    seen_directories: &mut HashSet<String>,
    directory: PathBuf,
) {
    let directory_key = directory.to_string_lossy().to_lowercase();
    if seen_directories.insert(directory_key) {
        directories.push(directory);
    }
}

fn same_drive_root(first_path: &Path, second_path: &Path) -> bool {
    drive_prefix(first_path)
        .zip(drive_prefix(second_path))
        .is_some_and(|(first_drive, second_drive)| first_drive.eq_ignore_ascii_case(&second_drive))
}

fn drive_prefix(path: &Path) -> Option<String> {
    path.components().find_map(|component| match component {
        Component::Prefix(prefix_component) => {
            Some(prefix_component.as_os_str().to_string_lossy().to_string())
        }
        _ => None,
    })
}

fn should_enter_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
        return true;
    };

    !matches!(
        file_name.to_lowercase().as_str(),
        "node_modules"
            | ".git"
            | "target"
            | "appdata"
            | "windows"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "temp"
            | "tmp"
            | "$recycle.bin"
            | "system volume information"
    )
}

fn file_matches_scope(file: &FileEntry, scope: &FileSearchScope) -> bool {
    match scope {
        FileSearchScope::AllFiles => true,
        FileSearchScope::Content => is_probable_text_file(file),
        FileSearchScope::Videos => file
            .extension
            .as_deref()
            .is_some_and(|extension| VIDEO_EXTENSIONS.contains(&extension)),
        FileSearchScope::Images => file
            .extension
            .as_deref()
            .is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension)),
        FileSearchScope::Extension(expected_extension) => {
            file.extension.as_deref() == Some(expected_extension.as_str())
        }
    }
}

fn score_file(
    file: &FileEntry,
    normalized_query: &str,
    scope: &FileSearchScope,
    query_words: &[&str],
) -> Option<u8> {
    if matches!(scope, FileSearchScope::Content) {
        return score_file_content(file, normalized_query, query_words);
    }

    let searchable_name = &file.normalized_name;
    let searchable_stem = &file.normalized_stem;

    if searchable_stem == normalized_query || searchable_name == normalized_query {
        return Some(92);
    }

    if searchable_stem.starts_with(normalized_query)
        || searchable_name.starts_with(normalized_query)
    {
        return Some(84);
    }

    if searchable_stem.contains(normalized_query) || searchable_name.contains(normalized_query) {
        return Some(76);
    }

    let all_words_match = query_words
        .iter()
        .all(|query_word| searchable_name.contains(query_word));

    all_words_match.then_some(68)
}

fn score_file_content(file: &FileEntry, normalized_query: &str, query_words: &[&str]) -> Option<u8> {
    let metadata = fs::metadata(&file.path).ok()?;
    if metadata.len() > MAX_CONTENT_SEARCH_BYTES {
        return None;
    }

    let file_content = fs::read_to_string(&file.path).ok()?;
    let searchable_content = normalize_file_search_text(&file_content);
    if searchable_content.contains(normalized_query) {
        return Some(82);
    }

    query_words
        .iter()
        .all(|query_word| searchable_content.contains(query_word))
        .then_some(72)
}

fn is_probable_text_file(file: &FileEntry) -> bool {
    file.extension.as_deref().is_some_and(|extension| {
        matches!(
            extension,
            "txt"
                | "md"
                | "markdown"
                | "rs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "html"
                | "css"
                | "scss"
                | "py"
                | "java"
                | "cs"
                | "cpp"
                | "c"
                | "h"
                | "hpp"
                | "go"
                | "rb"
                | "php"
                | "xml"
                | "csv"
                | "log"
                | "ini"
                | "ps1"
                | "bat"
                | "cmd"
                | "sh"
        )
    })
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim_start_matches('.').to_lowercase())
        .filter(|extension| !extension.is_empty())
}

fn is_probable_file_extension(tag: &str) -> bool {
    (1..=16).contains(&tag.len())
        && tag
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_file_scope_aliases() {
        assert_eq!(scope_from_tag("@video"), Some(FileSearchScope::Videos));
        assert_eq!(scope_from_tag("@vid"), Some(FileSearchScope::Videos));
        assert_eq!(scope_from_tag("@image"), Some(FileSearchScope::Images));
        assert_eq!(
            scope_from_tag("@pdf"),
            Some(FileSearchScope::Extension("pdf".to_string()))
        );
        assert_eq!(
            scope_from_tag("@zip"),
            Some(FileSearchScope::Extension("zip".to_string()))
        );
        assert_eq!(
            scope_from_tag("@c"),
            Some(FileSearchScope::Extension("c".to_string()))
        );
    }
}
