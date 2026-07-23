use crate::{
    command::CommandResult,
    search_text::{normalize_file_search_text, take_top_scored},
};
use std::{
    collections::{BinaryHeap, HashMap, HashSet},
    cmp::Ordering,
    env, fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};
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
    pub normalized_path: String,
    pub path: PathBuf,
    pub extension: Option<String>,
    pub modified_at: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    files: Vec<FileEntry>,
    /// Extension → indices into `files` for fast `@mp4` / `@pdf` style scopes.
    by_extension: HashMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSearchScope {
    AllFiles,
    Content,
    Videos,
    Images,
    Extension(String),
}

impl FileSearchScope {
    pub fn label(&self) -> String {
        match self {
            FileSearchScope::AllFiles => "All files".to_string(),
            FileSearchScope::Content => "File contents".to_string(),
            FileSearchScope::Videos => "Videos".to_string(),
            FileSearchScope::Images => "Images".to_string(),
            FileSearchScope::Extension(extension) => format!(".{extension}"),
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            FileSearchScope::AllFiles => "Files".to_string(),
            FileSearchScope::Content => "Content".to_string(),
            FileSearchScope::Videos => "Videos".to_string(),
            FileSearchScope::Images => "Images".to_string(),
            FileSearchScope::Extension(extension) => format!(".{extension}"),
        }
    }
}

impl FileIndex {
    pub fn load_from_user_directories() -> Self {
        let mut files = Vec::new();
        let mut seen_paths = HashSet::new();

        'outer: for search_directory in user_search_directories() {
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
                    break 'outer;
                }

                let path = directory_entry.path().to_path_buf();
                if !seen_paths.insert(path.clone()) {
                    continue;
                }

                if let Some(entry) = file_entry_from_path(path) {
                    files.push(entry);
                }
            }
        }

        files.sort_by_key(|file| file.name.to_lowercase());
        Self::from_entries(files)
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
            self.candidates_for_scope(&scope).filter(|file| {
                first_word.is_none_or(|word| {
                    file.normalized_name.contains(word) || file.normalized_path.contains(word)
                })
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

    pub fn scoped_file_count(&self, scope: &FileSearchScope) -> usize {
        match scope {
            FileSearchScope::AllFiles => self.files.len(),
            FileSearchScope::Extension(extension) => self
                .by_extension
                .get(extension.as_str())
                .map(|indices| indices.len())
                .unwrap_or(0),
            FileSearchScope::Videos => VIDEO_EXTENSIONS
                .iter()
                .map(|extension| {
                    self.by_extension
                        .get(*extension)
                        .map(|indices| indices.len())
                        .unwrap_or(0)
                })
                .sum(),
            FileSearchScope::Images => IMAGE_EXTENSIONS
                .iter()
                .map(|extension| {
                    self.by_extension
                        .get(*extension)
                        .map(|indices| indices.len())
                        .unwrap_or(0)
                })
                .sum(),
            FileSearchScope::Content => self
                .files
                .iter()
                .filter(|file| file_matches_scope(file, scope))
                .count(),
        }
    }

    #[doc(hidden)]
    pub fn from_entries(files: Vec<FileEntry>) -> Self {
        let mut by_extension: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, file) in files.iter().enumerate() {
            if let Some(extension) = file.extension.as_ref() {
                by_extension
                    .entry(extension.clone())
                    .or_default()
                    .push(index);
            }
        }
        Self {
            files,
            by_extension,
        }
    }

    pub fn recent_files(&self, max_results: usize) -> Vec<CommandResult> {
        self.recent_files_for_scope(FileSearchScope::AllFiles, max_results)
    }

    pub fn recent_files_for_scope(
        &self,
        scope: FileSearchScope,
        max_results: usize,
    ) -> Vec<CommandResult> {
        if max_results == 0 {
            return Vec::new();
        }

        // Min-heap (via Reverse) of the current top-N most recent files.
        // Top of the heap is the oldest member of the top-N and is the eviction candidate.
        let mut heap: BinaryHeap<std::cmp::Reverse<RecentHeapItem<'_>>> = BinaryHeap::new();
        for file in self.candidates_for_scope(&scope) {
            let item = RecentHeapItem { file };
            if heap.len() < max_results {
                heap.push(std::cmp::Reverse(item));
                continue;
            }
            if let Some(std::cmp::Reverse(oldest)) = heap.peek() {
                if item.cmp(oldest) == Ordering::Greater {
                    heap.pop();
                    heap.push(std::cmp::Reverse(item));
                }
            }
        }

        let mut files = heap
            .into_iter()
            .map(|std::cmp::Reverse(item)| item)
            .collect::<Vec<_>>();
        files.sort_by(|left, right| right.cmp(left));
        files
            .into_iter()
            .map(|item| {
                CommandResult::file(
                    item.file.name.clone(),
                    item.file.path.display().to_string(),
                    item.file.path.clone(),
                    82,
                )
            })
            .collect()
    }

    fn candidates_for_scope<'a>(
        &'a self,
        scope: &FileSearchScope,
    ) -> Box<dyn Iterator<Item = &'a FileEntry> + 'a> {
        match scope {
            FileSearchScope::AllFiles => Box::new(self.files.iter()),
            FileSearchScope::Extension(extension) => {
                let indices = self
                    .by_extension
                    .get(extension.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                Box::new(indices.iter().filter_map(|&index| self.files.get(index)))
            }
            FileSearchScope::Videos => Box::new(VIDEO_EXTENSIONS.iter().flat_map(|extension| {
                self.by_extension
                    .get(*extension)
                    .into_iter()
                    .flat_map(|indices| indices.iter().filter_map(|&index| self.files.get(index)))
            })),
            FileSearchScope::Images => Box::new(IMAGE_EXTENSIONS.iter().flat_map(|extension| {
                self.by_extension
                    .get(*extension)
                    .into_iter()
                    .flat_map(|indices| indices.iter().filter_map(|&index| self.files.get(index)))
            })),
            FileSearchScope::Content => Box::new(
                self.files
                    .iter()
                    .filter(|file| file_matches_scope(file, &FileSearchScope::Content)),
            ),
        }
    }
}

/// Orders files by recency: newer > older. Used with `Reverse` for a min-heap of top-N.
#[derive(Clone, Copy)]
struct RecentHeapItem<'a> {
    file: &'a FileEntry,
}

impl PartialEq for RecentHeapItem<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.file.modified_at == other.file.modified_at && self.file.name == other.file.name
    }
}

impl Eq for RecentHeapItem<'_> {}

impl PartialOrd for RecentHeapItem<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RecentHeapItem<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.file.modified_at, other.file.modified_at) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| self.file.name.cmp(&other.file.name))
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

    // Support @file:content and tags that keep an internal colon.
    if let Some((prefix, suffix)) = normalized_tag.split_once(':') {
        if matches!(prefix, "file" | "files") && matches!(suffix, "content" | "contents") {
            return Some(FileSearchScope::Content);
        }
    }

    match normalized_tag.as_str() {
        "file" | "files" => Some(FileSearchScope::AllFiles),
        "file:content" | "files:content" | "content" | "contents" => Some(FileSearchScope::Content),
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

fn file_entry_from_path(path: PathBuf) -> Option<FileEntry> {
    let name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(ToOwned::to_owned)?;

    let extension = normalized_extension(&path);
    let normalized_name = normalize_file_search_text(&name);
    let normalized_stem = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .map(normalize_file_search_text)
        .unwrap_or_else(|| normalized_name.clone());
    let normalized_path = normalize_file_search_text(&path.display().to_string());
    let modified_at = fs::metadata(&path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());

    Some(FileEntry {
        name,
        normalized_name,
        normalized_stem,
        normalized_path,
        path,
        extension,
        modified_at,
    })
}

fn user_search_directories() -> Vec<PathBuf> {
    let user_profile = env::var_os("USERPROFILE").map(PathBuf::from);
    let system_drive = env::var_os("SystemDrive").map(PathBuf::from);
    let mut directories = Vec::new();
    let mut seen_directories = HashSet::new();

    if let Some(user_profile) = user_profile.as_ref() {
        // Prefer known user folders first so common files index early and rank more usefully.
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
        push_unique_directory(
            &mut directories,
            &mut seen_directories,
            user_profile.clone(),
        );
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
    let searchable_path = &file.normalized_path;

    if searchable_stem == normalized_query || searchable_name == normalized_query {
        return Some(94);
    }

    if searchable_stem.starts_with(normalized_query)
        || searchable_name.starts_with(normalized_query)
    {
        return Some(88);
    }

    if searchable_stem.contains(normalized_query) || searchable_name.contains(normalized_query) {
        return Some(80);
    }

    if searchable_path.contains(normalized_query) {
        return Some(74);
    }

    let all_words_match = query_words.iter().all(|query_word| {
        searchable_name.contains(query_word) || searchable_path.contains(query_word)
    });

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
    use std::time::Duration;

    fn sample_entry(name: &str, extension: Option<&str>, modified_secs: u64) -> FileEntry {
        let path = PathBuf::from(format!("C:\\docs\\{name}"));
        FileEntry {
            name: name.to_string(),
            normalized_name: normalize_file_search_text(name),
            normalized_stem: normalize_file_search_text(
                Path::new(name)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(name),
            ),
            normalized_path: normalize_file_search_text(&path.display().to_string()),
            path,
            extension: extension.map(str::to_string),
            modified_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(modified_secs)),
        }
    }

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
        assert_eq!(
            scope_from_tag("file:content"),
            Some(FileSearchScope::Content)
        );
        assert_eq!(scope_from_tag("files"), Some(FileSearchScope::AllFiles));
    }

    #[test]
    fn extension_scope_filters_results() {
        let index = FileIndex::from_entries(vec![
            sample_entry("notes.pdf", Some("pdf"), 30),
            sample_entry("photo.png", Some("png"), 20),
            sample_entry("report.pdf", Some("pdf"), 10),
        ]);

        let results = index.search("note", FileSearchScope::Extension("pdf".to_string()), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "notes.pdf");
    }

    #[test]
    fn empty_extension_scope_returns_recent_for_that_extension() {
        let index = FileIndex::from_entries(vec![
            sample_entry("old.pdf", Some("pdf"), 10),
            sample_entry("new.pdf", Some("pdf"), 40),
            sample_entry("photo.png", Some("png"), 50),
        ]);

        let results =
            index.recent_files_for_scope(FileSearchScope::Extension("pdf".to_string()), 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "new.pdf");
        assert_eq!(results[1].title, "old.pdf");
    }

    #[test]
    fn search_matches_path_fragments() {
        let index = FileIndex::from_entries(vec![sample_entry("budget.xlsx", Some("xlsx"), 10)]);
        let results = index.search("docs budget", FileSearchScope::AllFiles, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "budget.xlsx");
    }

    #[test]
    fn scoped_file_count_respects_extension() {
        let index = FileIndex::from_entries(vec![
            sample_entry("a.pdf", Some("pdf"), 1),
            sample_entry("b.pdf", Some("pdf"), 2),
            sample_entry("c.txt", Some("txt"), 3),
        ]);
        assert_eq!(
            index.scoped_file_count(&FileSearchScope::Extension("pdf".to_string())),
            2
        );
    }
}
