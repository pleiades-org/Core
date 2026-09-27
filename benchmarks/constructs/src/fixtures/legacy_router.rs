// Exact legacy functions/types, extracted by prepare.ps1; wrapper below is benchmark-only.

#![allow(dead_code)]

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSearchScope {
    AllFiles,
    Content,
    Videos,
    Images,
    Extension(String),
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

fn is_probable_file_extension(tag: &str) -> bool {
    (1..=16).contains(&tag.len())
        && tag
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScopedQuery {
    scope: QueryScope,
    scope_tag: String,
    search_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum QueryScope {
    Applications,
    Calculator,
    Web,
    Files(FileSearchScope),
    Notes,
    Focus,
    Clipboard,
    WindowManagement,
    Snippets,
    Quicklinks,
    Calendar,
    System,
    Emoji,
    CustomCommands,
    Aliases,
    Hotkeys,
    Context,
    Terminal,
    DevTools,
    Git,
    Package,
    Process,
    Color,
    Screenshot,
    Lookup,
    GitHub,
    Media,
    Network,
}

fn parse_scoped_query(query: &str) -> Option<ScopedQuery> {
    let mut scope = None;
    let mut scope_tag = None;
    let mut search_words = Vec::new();

    for word in query.split_whitespace() {
        if scope.is_none() {
            let cleaned_word = word
                .trim_matches(|character: char| matches!(character, ',' | ';' | ':' | '(' | ')'));
            let possible_scope_tag = cleaned_word.strip_prefix('@');

            if let Some(tag) = possible_scope_tag {
                if let Some(parsed_scope) = query_scope_from_tag(tag) {
                    scope = Some(parsed_scope);
                    scope_tag = Some(tag.to_lowercase());
                    continue;
                }
            }
        }

        search_words.push(word);
    }

    Some(ScopedQuery {
        scope: scope?,
        scope_tag: scope_tag?,
        search_text: search_words.join(" "),
    })
}

fn query_scope_from_tag(scope_tag: &str) -> Option<QueryScope> {
    let normalized_scope_tag = scope_tag
        .trim()
        .trim_start_matches('@')
        .trim_start_matches('.')
        .to_lowercase();

    if normalized_scope_tag.is_empty() {
        return None;
    }

    match normalized_scope_tag.as_str() {
        "app" | "apps" | "application" | "applications" | "a" => {
            Some(QueryScope::Applications)
        }
        "calc" | "calculator" | "math" | "calculate" => Some(QueryScope::Calculator),
        "web" | "google" | "search" | "w" => Some(QueryScope::Web),
        "note" | "notes" | "note-taking" => Some(QueryScope::Notes),
        "focus" | "block" | "blocks" | "f" => Some(QueryScope::Focus),
        "clipboard" | "clip" | "clips" => Some(QueryScope::Clipboard),
        "window" | "windows" | "win" | "wm" => Some(QueryScope::WindowManagement),
        "snippet" | "snippets" | "snip" | "snips" | "expand" | "text" => {
            Some(QueryScope::Snippets)
        }
        "quicklink" | "quicklinks" | "ql" | "link" | "links" | "url" | "urls" => {
            Some(QueryScope::Quicklinks)
        }
        "calendar" | "cal" | "schedule" | "meeting" | "meetings" | "event" | "events" => {
            Some(QueryScope::Calendar)
        }
        "system" | "sys" | "control" | "controls" => Some(QueryScope::System),
        "context" | "ctx" => Some(QueryScope::Context),
        "now" | "media" | "spotify" => Some(QueryScope::Media),
        "screenshot" | "ocr" | "capture" => Some(QueryScope::Screenshot),
        "define" | "dict" | "translate" | "weather" => Some(QueryScope::Lookup),
        "github" | "gh" => Some(QueryScope::GitHub),
        "network" | "net" | "ip" => Some(QueryScope::Network),
        "emoji" | "emojis" | "e" => Some(QueryScope::Emoji),
        "custom" | "customs" | "customcommand" | "customcommands" | "command" | "commands" => {
            Some(QueryScope::CustomCommands)
        }
        "alias" | "aliases" => Some(QueryScope::Aliases),
        "hotkey" | "hotkeys" | "shortcut" | "shortcuts" => Some(QueryScope::Hotkeys),
        "cmd" | "terminal" | "term" | "shell" => Some(QueryScope::Terminal),
        "dev" | "util" | "devtools" | "dev-tools" => Some(QueryScope::DevTools),
        "git" | "repo" | "repository" => Some(QueryScope::Git),
        "winget" | "pkg" | "package" | "packages" | "install" => Some(QueryScope::Package),
        "scoop" | "choco" | "chocolatey" => Some(QueryScope::Package),
        "kill" | "quit" | "process" | "processes" => Some(QueryScope::Process),
        "color" | "colors" | "colour" | "colours" | "colorclip" => Some(QueryScope::Color),
        extension => file_scope_from_extension_tag(extension).map(QueryScope::Files),
    }
}

fn file_scope_from_extension_tag(tag: &str) -> Option<FileSearchScope> {
    let file_scope = scope_from_tag(tag)?;
    match &file_scope {
        FileSearchScope::Extension(extension) => {
            if extension.len() == 1 {
                // Only well-known single-letter extensions become scopes.
                // Letters that are command prefixes (@d → @dev, @a → @app) stay free for typing.
                if !SINGLE_LETTER_FILE_EXTENSIONS.contains(&extension.as_str()) {
                    return None;
                }
            } else if is_command_scope_prefix(tag) {
                // While typing @cal…, do not treat the partial tag as an extension.
                return None;
            }
        }
        _ => {}
    }
    Some(file_scope)
}

/// Single-letter extensions that are useful as `@x` scopes and do not collide with
/// one-letter command shortcuts or common partial command prefixes.
const SINGLE_LETTER_FILE_EXTENSIONS: &[&str] = &["c", "h", "r", "s", "o", "m"];

fn is_command_scope_prefix(tag: &str) -> bool {
    if tag.is_empty() {
        return true;
    }

    COMMAND_SCOPE_TAGS.iter().any(|known| {
        known.starts_with(tag) || tag.starts_with(known)
    })
}

const COMMAND_SCOPE_TAGS: &[&str] = &[
    "app", "apps", "application", "applications", "a", "calc", "calculator", "math", "calculate",
    "web", "google", "search", "w", "note", "notes", "note-taking", "focus", "block", "blocks",
    "f", "clipboard", "clip", "clips", "window", "windows", "win", "wm", "snippet", "snippets",
    "snip", "snips", "expand", "text", "quicklink", "quicklinks", "ql", "link", "links", "url",
    "urls", "calendar", "cal", "schedule", "meeting", "meetings", "event", "events", "system",
    "sys", "control", "controls", "context", "ctx", "now", "media", "spotify", "screenshot",
    "ocr", "capture", "define", "dict", "translate", "weather", "github", "gh", "network", "net",
    "ip", "scoop", "choco", "chocolatey", "colorclip", "emoji", "emojis", "e",
    "custom", "customs", "customcommand",
    "customcommands", "command", "commands", "alias", "aliases", "hotkey", "hotkeys", "shortcut",
    "shortcuts", "cmd", "terminal", "term", "shell", "dev", "util", "devtools", "dev-tools",
    "git", "repo", "repository", "winget", "pkg", "package", "packages", "install", "kill",
    "quit", "process", "processes", "color", "colors", "colour", "colours", "file", "files",
    "file:content", "files:content", "content", "video", "videos", "vid", "vids", "image",
    "images", "picture", "pictures", "pic", "pics",
];

pub fn describe(query: &str) -> Option<(u8, String)> {
    let parsed = parse_scoped_query(query)?;
    std::hint::black_box(&parsed);
    let command = match parsed.scope {
        QueryScope::Applications => 1,
        QueryScope::Calculator => 2,
        QueryScope::Web => 3,
        QueryScope::Quicklinks => 4,
        _ => 255,
    };
    Some((command, parsed.search_text))
}
