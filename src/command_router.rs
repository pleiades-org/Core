use std::cell::RefCell;
use std::time::{Duration, Instant};

use crate::{
    app_index::ApplicationIndex,
    calculator::{self, CalculationContext},
    calendar, clipboard_history, clipboard_tools, color_tools, context_actions,
    command::{
        BuiltInAction, CommandAction, CommandCategory, CommandResult, FeatureAction,
        FileOperationKind,
    },
    custom_commands, destiny, dev_tools, emoji_picker,
    file_index::{scope_from_tag, FileIndex, FileSearchScope},
    focus, git_commands, github_tools, lookup_tools, media_tools, network_tools, notes,
    package_manager, process_manager, quicklinks, screenshot_tools,
    settings::LauncherSettings,
    snippets, system_controls, terminal, window_management,
};

const MAX_APPLICATION_RESULTS: usize = 8;
const MAX_FILE_RESULTS: usize = 80;
const SEARCH_CACHE_TTL: Duration = Duration::from_millis(150);
const CALCULATION_CONTEXT_TTL: Duration = Duration::from_secs(1);

struct SearchCacheEntry {
    query: String,
    results: Vec<CommandResult>,
    cached_at: Instant,
}

pub struct CommandRouter {
    application_index: ApplicationIndex,
    file_index: FileIndex,
    settings: LauncherSettings,
    search_cache: RefCell<Option<SearchCacheEntry>>,
    calculation_context: RefCell<CalculationContext>,
    calculation_context_built_at: RefCell<Instant>,
}

impl CommandRouter {
    pub fn new(
        settings: LauncherSettings,
        application_index: ApplicationIndex,
        file_index: FileIndex,
    ) -> Self {
        let calculation_context = CalculationContext::from_settings(settings.clone());
        Self {
            application_index,
            file_index,
            settings,
            search_cache: RefCell::new(None),
            calculation_context: RefCell::new(calculation_context),
            calculation_context_built_at: RefCell::new(Instant::now()),
        }
    }

    fn calculation_context(&self) -> std::cell::Ref<'_, CalculationContext> {
        let stale = self
            .calculation_context_built_at
            .borrow()
            .elapsed()
            >= CALCULATION_CONTEXT_TTL;
        if stale {
            *self.calculation_context.borrow_mut() =
                CalculationContext::from_settings(self.settings.clone());
            *self.calculation_context_built_at.borrow_mut() = Instant::now();
        }
        self.calculation_context.borrow()
    }

    pub fn search(&self, query: &str) -> Vec<CommandResult> {
        self.search_cached(query)
    }

    fn search_cached(&self, query: &str) -> Vec<CommandResult> {
        if let Some(cache) = self.search_cache.borrow().as_ref() {
            if cache.query == query && cache.cached_at.elapsed() < SEARCH_CACHE_TTL {
                return cache.results.clone();
            }
        }

        let results = self.search_with_alias_depth(query, 0);
        *self.search_cache.borrow_mut() = Some(SearchCacheEntry {
            query: query.to_string(),
            results: results.clone(),
            cached_at: Instant::now(),
        });
        results
    }

    fn search_with_alias_depth(&self, query: &str, alias_depth: usize) -> Vec<CommandResult> {
        let trimmed_query = query.trim();

        if trimmed_query.is_empty() {
            return self.default_results();
        }

        if let Some((bang, search_term)) = match_bang_query(trimmed_query) {
            let url = if search_term.is_empty() {
                bang.home_url.to_string()
            } else {
                let encoded = search_term.replace(' ', "+");
                bang.search_url.replace("{}", &encoded)
            };

            let title = if search_term.is_empty() {
                format!("Search {}", bang.name)
            } else {
                format!("Search {} for \"{}\"", bang.name, search_term)
            };

            return vec![CommandResult {
                title,
                subtitle: url.clone(),
                copy_text: url.clone(),
                explanation: None,
                icon_path: None,
                calculation_display: None,
                category: CommandCategory::Web,
                action: CommandAction::OpenUrl(url),
                confidence: 100,
            }];
        }

        if alias_depth < 5 {
            if let Some(expanded_query) =
                custom_commands::expand_alias_query(trimmed_query, &self.settings)
            {
                return self.search_with_alias_depth(&expanded_query, alias_depth + 1);
            }
        }

        if let Some(scoped_query) = parse_scoped_query(trimmed_query) {
            return self.scoped_results(scoped_query);
        }

        if trimmed_query.starts_with('@') {
            return scope_hint_results(trimmed_query);
        }

        let mut results = self.collect_unscoped_results(trimmed_query);
        results.sort_by(result_sort_order);
        results
    }

    fn collect_unscoped_results(&self, trimmed_query: &str) -> Vec<CommandResult> {
        let calculation_context = self.calculation_context();
        let mut results = calculator::evaluate_calculation(trimmed_query, &calculation_context);
        results.extend(self.built_in_results(trimmed_query));
        results.extend(snippets::search_snippet_keywords(trimmed_query));
        results.extend(quicklinks::search_quicklink_keywords(trimmed_query));
        results.extend(emoji_picker::search_colon_trigger(trimmed_query));
        results.extend(custom_commands::search_custom_commands(
            trimmed_query,
            &self.settings,
        ));
        results.extend(context_actions::search_context_actions(trimmed_query, &self.settings));
        results.extend(system_controls::search_system_controls(trimmed_query));
        results.extend(window_management::search_window_commands(trimmed_query));
        results.extend(dev_tools::search_inline(trimmed_query));
        results.extend(process_manager::search_inline(trimmed_query));
        results.extend(color_tools::search_inline(trimmed_query));
        results.extend(screenshot_tools::search_inline(trimmed_query));
        results.extend(lookup_tools::search_inline(trimmed_query));
        results.extend(media_tools::search_inline(trimmed_query));
        results.extend(network_tools::search_inline(trimmed_query));
        results.extend(clipboard_tools::search_inline(trimmed_query));

        if let Some(focus_query) = parse_named_feature_query(trimmed_query, "focus") {
            results.extend(focus::search_focus_commands(focus_query));
        }

        if let Some(calendar_query) = parse_named_feature_query(trimmed_query, "calendar") {
            results.extend(calendar::search_calendar(calendar_query));
        }

        if let Some(clipboard_query) = clipboard_query_from_text(trimmed_query) {
            results.extend(clipboard_history::search_clipboard_history(clipboard_query));
        }

        let typed_url = typed_url_from_query(trimmed_query);
        if let Some(url) = typed_url.clone() {
            results.push(CommandResult::open_website(
                url,
                typed_url_display_label(trimmed_query),
            ));
        }

        results.extend(
            self.application_index
                .search(trimmed_query, MAX_APPLICATION_RESULTS),
        );

        if self.settings.index_user_files && should_include_universal_file_results(trimmed_query) {
            results.extend(
                self.file_index
                    .search(trimmed_query, FileSearchScope::AllFiles, 6),
            );
        }

        if self.settings.show_web_search_result
            && typed_url.is_none()
            && looks_like_web_search(trimmed_query)
        {
            results.push(CommandResult::web_search(trimmed_query));
        }

        results
    }

    pub fn reload_application_index(&mut self) {
        self.application_index = ApplicationIndex::load_from_windows_start_menu();
    }

    pub fn replace_application_index(&mut self, application_index: ApplicationIndex) {
        self.application_index = application_index;
    }

    pub fn replace_file_index(&mut self, file_index: FileIndex) {
        self.file_index = file_index;
    }

    pub fn settings(&self) -> &LauncherSettings {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: LauncherSettings) {
        self.settings = settings;
        *self.calculation_context.borrow_mut() =
            CalculationContext::from_settings(self.settings.clone());
        *self.calculation_context_built_at.borrow_mut() = Instant::now();
        *self.search_cache.borrow_mut() = None;
    }

    pub fn indexed_application_count(&self) -> usize {
        self.application_index.application_count()
    }

    pub fn indexed_file_count(&self) -> usize {
        self.file_index.file_count()
    }

    fn scoped_results(&self, scoped_query: ScopedQuery) -> Vec<CommandResult> {
        let search_text = remove_trailing_polite_words(&scoped_query.search_text);
        let scope_tag = scoped_query.scope_tag.as_str();

        match scoped_query.scope {
            QueryScope::Applications => self
                .application_index
                .search(search_text, MAX_APPLICATION_RESULTS),
            QueryScope::Calculator => {
                let calculation_context = self.calculation_context();
                calculator::evaluate_calculation(search_text, &calculation_context)
            }
            QueryScope::Web => vec![CommandResult::web_search(search_text)],
            QueryScope::Files(file_scope) => self.file_results(search_text, file_scope),
            QueryScope::Notes => notes::search_notes(search_text),
            QueryScope::Focus => focus::search_focus_commands(search_text),
            QueryScope::Clipboard => clipboard_history::search_clipboard_history(search_text),
            QueryScope::WindowManagement => window_management::search_window_commands(search_text),
            QueryScope::Snippets => snippets::search_snippets(search_text),
            QueryScope::Quicklinks => quicklinks::search_quicklinks(search_text),
            QueryScope::Calendar => calendar::search_calendar(search_text),
            QueryScope::System => system_controls::search_system_controls(search_text),
            QueryScope::Context => context_actions::search_context_actions(search_text, &self.settings),
            QueryScope::Emoji => emoji_picker::search_emojis(search_text),
            QueryScope::Destiny => destiny::search_d2(search_text),
            QueryScope::CustomCommands => {
                custom_commands::search_custom_commands(search_text, &self.settings)
            }
            QueryScope::Aliases => custom_commands::search_aliases(search_text, &self.settings),
            QueryScope::Hotkeys => custom_commands::search_hotkeys(search_text, &self.settings),
            QueryScope::Terminal => terminal::search_terminal_scope(search_text),
            QueryScope::DevTools => dev_tools::search_dev_tools(search_text),
            QueryScope::Git => git_commands::search_git_commands(search_text),
            QueryScope::Package => package_manager::search_packages(search_text, scope_tag),
            QueryScope::Process => process_manager::search_processes_scoped(search_text),
            QueryScope::Color => {
                if scope_tag == "colorclip" {
                    clipboard_tools::search_clipboard_color(search_text)
                } else {
                    color_tools::search_color_tools(search_text)
                }
            }
            QueryScope::Screenshot => screenshot_tools::search_screenshot_tools(search_text),
            QueryScope::Lookup => lookup_tools::search_lookup(search_text, scope_tag),
            QueryScope::GitHub => github_tools::search_github(search_text),
            QueryScope::Media => media_tools::search_media(search_text),
            QueryScope::Network => network_tools::search_network(search_text),
        }
    }

    fn default_results(&self) -> Vec<CommandResult> {
        let mut results = vec![
            CommandResult::informational("Recent usages", "Items you run will appear here"),
            CommandResult::copyable_feature(
                "@note",
                "Markdown notes in the launcher",
                "@note ",
                CommandCategory::Note,
                72,
            ),
            CommandResult::built_in(
                "Open settings",
                "Configure Core Launcher",
                BuiltInAction::OpenSettings,
                50,
            ),
        ];

        if let Some(next_event_result) = calendar::next_event_prompt_result() {
            results.push(next_event_result);
        }

        results
    }

    fn built_in_results(&self, query: &str) -> Vec<CommandResult> {
        let normalized_query = query.trim().to_lowercase();
        let mut results = Vec::new();

        if "settings".contains(&normalized_query) || normalized_query.contains("settings") {
            results.push(CommandResult::built_in(
                "Open settings",
                "Configure Core Launcher",
                BuiltInAction::OpenSettings,
                82,
            ));
        }

        if "reload apps".contains(&normalized_query) || normalized_query.contains("reload") {
            results.push(CommandResult::built_in(
                "Reload applications",
                "Refresh the Windows Start Menu index",
                BuiltInAction::ReloadApplications,
                80,
            ));
        }

        if "quit".starts_with(&normalized_query) || normalized_query == "exit" {
            results.push(CommandResult::built_in(
                "Quit Core Launcher",
                "Close the background launcher process",
                BuiltInAction::Quit,
                75,
            ));
        }

        if let Some(url) = typed_url_from_query(query) {
            results.push(CommandResult::open_website(
                url,
                typed_url_display_label(query),
            ));
        }

        results
    }

    fn file_results(&self, search_text: &str, file_scope: FileSearchScope) -> Vec<CommandResult> {
        if search_text.is_empty() {
            return self
                .file_index
                .recent_files_for_scope(file_scope, MAX_FILE_RESULTS);
        }

        if search_text.eq_ignore_ascii_case("recent") {
            return self.file_index.recent_files(MAX_FILE_RESULTS);
        }

        let (file_operation, file_query) = parse_file_operation_query(search_text);
        let results = self
            .file_index
            .search(file_query, file_scope, MAX_FILE_RESULTS);

        if let Some(file_operation) = file_operation {
            return results
                .into_iter()
                .filter_map(|result| file_operation_result(result, file_operation.clone()))
                .collect();
        }

        results
    }
}

pub fn file_search_scope_from_query(query: &str) -> Option<FileSearchScope> {
    parse_scoped_query(query).and_then(|scoped_query| match scoped_query.scope {
        QueryScope::Files(file_scope) => Some(file_scope),
        _ => None,
    })
}

pub fn clipboard_search_scope_from_query(query: &str) -> Option<String> {
    parse_scoped_query(query).and_then(|scoped_query| match scoped_query.scope {
        QueryScope::Clipboard => Some(scoped_query.search_text),
        _ => None,
    })
}

pub fn clipboard_browse_filter_from_query(query: &str) -> String {
    if let Some(filter) = clipboard_search_scope_from_query(query) {
        return filter;
    }

    let trimmed_query = query.trim();
    if trimmed_query.eq_ignore_ascii_case("clip") || trimmed_query.eq_ignore_ascii_case("clipboard")
    {
        return String::new();
    }

    if let Some((first, rest)) = trimmed_query.split_once(char::is_whitespace) {
        if first.eq_ignore_ascii_case("clip") {
            return rest.trim().to_string();
        }
    }

    trimmed_query.to_string()
}

pub fn is_clipboard_browse_query(query: &str) -> bool {
    clipboard_search_scope_from_query(query).is_some()
        || clipboard_query_from_text(query).is_some()
}

pub fn is_implicit_file_browse_query(query: &str) -> bool {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() || trimmed_query.starts_with('@') {
        return false;
    }

    if typed_url_from_query(trimmed_query).is_some() {
        return false;
    }

    trimmed_query.contains('\\')
        || trimmed_query.contains('/')
        || trimmed_query
            .rsplit_once('.')
            .is_some_and(|(_, extension)| {
                !extension.is_empty()
                    && extension.len() <= 8
                    && extension.chars().all(|character| character.is_ascii_alphanumeric())
            })
}

fn clipboard_query_from_text(query: &str) -> Option<&str> {
    parse_named_feature_query(query, "clipboard").or_else(|| {
        if query.trim().eq_ignore_ascii_case("clip") {
            Some("")
        } else {
            parse_named_feature_query(query, "clip")
        }
    })
}

fn should_include_universal_file_results(query: &str) -> bool {
    let trimmed_query = query.trim();
    trimmed_query.len() >= 3
        && !trimmed_query.starts_with('@')
        && !trimmed_query.starts_with(';')
        && !trimmed_query.starts_with('>')
        && typed_url_from_query(trimmed_query).is_none()
        && !looks_like_calculation_query(trimmed_query)
        && trimmed_query
            .chars()
            .any(|character| character.is_ascii_alphanumeric())
}

fn looks_like_calculation_query(query: &str) -> bool {
    let normalized = query.trim().to_lowercase();
    if normalized.chars().any(|character| matches!(character, '+' | '*' | '/' | '%' | '=')) {
        return true;
    }

    if normalized.contains('-') {
        let looks_numeric = normalized
            .chars()
            .filter(|character| !character.is_whitespace() && *character != '-')
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ':'));
        if looks_numeric {
            return true;
        }
    }

    const CALCULATION_HINTS: &[&str] = &[
        " from now",
        " ago",
        " to ",
        " in ",
        "unix",
        "hex",
        "bin",
        "oct",
        "sqrt",
        "percent",
        "% of",
    ];

    CALCULATION_HINTS
        .iter()
        .any(|hint| normalized.contains(hint))
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
    Destiny,
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
        "d2" | "destiny" | "destiny2" | "weapon" | "weapons" => Some(QueryScope::Destiny),
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
            if extension.len() < 2 || is_command_scope_prefix(tag) {
                return None;
            }
        }
        _ => {}
    }
    Some(file_scope)
}

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
    "d2", "destiny", "destiny2", "weapon", "weapons", "custom", "customs", "customcommand",
    "customcommands", "command", "commands", "alias", "aliases", "hotkey", "hotkeys", "shortcut",
    "shortcuts", "cmd", "terminal", "term", "shell", "dev", "util", "devtools", "dev-tools",
    "git", "repo", "repository", "winget", "pkg", "package", "packages", "install", "kill",
    "quit", "process", "processes", "color", "colors", "colour", "colours", "file", "files",
    "file:content", "files:content", "content", "video", "videos", "vid", "vids", "image",
    "images", "picture", "pictures", "pic", "pics",
];

fn scope_hint_results(query: &str) -> Vec<CommandResult> {
    let trimmed_query = query.trim();
    let partial_tag = trimmed_query
        .strip_prefix('@')
        .unwrap_or(trimmed_query)
        .trim()
        .to_lowercase();

    let mut results = vec![CommandResult::informational(
        "Core command scopes",
        "Pick a scope below. Files use @files or @pdf, not bare @.",
    )];

    results.extend(
        SCOPE_HINTS
            .iter()
            .filter(|(tag, _, _)| partial_tag.is_empty() || tag.starts_with(&partial_tag))
            .map(|(tag, _title, subtitle)| {
                CommandResult::copyable_feature(
                    format!("@{tag}"),
                    (*subtitle).to_string(),
                    format!("@{tag} "),
                    CommandCategory::Help,
                    if partial_tag.is_empty() { 70 } else { 88 },
                )
            }),
    );

    results
}

const SCOPE_HINTS: &[(&str, &str, &str)] = &[
    ("app", "Applications", "Search installed apps"),
    ("calc", "Calculator", "Math, dates, and time zones"),
    ("cmd", "Terminal", "Run a shell command in the launcher"),
    ("dev", "Dev tools", "UUID, hash, encode, JSON, and more"),
    ("git", "Git", "Common git shortcuts"),
    ("winget", "Packages", "Search and install with winget"),
    ("scoop", "Scoop", "Search and install with Scoop"),
    ("choco", "Chocolatey", "Search and install with Chocolatey"),
    ("kill", "Processes", "Find and kill running processes"),
    ("color", "Colors", "Convert hex, rgb, and hsl"),
    ("colorclip", "Clipboard color", "Parse color from clipboard text"),
    ("screenshot", "Screenshot", "Capture screen and OCR"),
    ("define", "Dictionary", "Look up word definitions"),
    ("translate", "Translate", "Translate text between languages"),
    ("weather", "Weather", "Quick weather lookup"),
    ("github", "GitHub", "Search issues, PRs, and repos"),
    ("now", "Now playing", "Current media session"),
    ("network", "Network", "IP, ping, and DNS tools"),
    ("d2", "Destiny 2", "Search weapons and perks"),
    ("files", "Files", "Search indexed user files"),
    ("web", "Web", "Open a web search"),
    ("note", "Notes", "Create and manage Markdown notes"),
    ("snippet", "Snippets", "Text expansions with ;keyword"),
    ("quicklink", "Quicklinks", "Saved links with >keyword"),
    ("clipboard", "Clipboard", "Search clipboard history"),
    ("calendar", "Calendar", "Events and meetings"),
    ("focus", "Focus", "Timed app blocking"),
    ("window", "Windows", "Move and resize windows"),
    ("system", "System", "Lock, sleep, volume, brightness"),
    ("emoji", "Emoji", "Copy emoji by name"),
    ("custom", "Custom commands", "Configured shell commands"),
    ("alias", "Aliases", "First-word query expansions"),
    ("hotkey", "Hotkeys", "Configured query shortcuts"),
    ("pdf", "PDF files", "Search indexed PDF files"),
    ("mp4", "MP4 files", "Search indexed video files"),
];

fn parse_file_operation_query(search_text: &str) -> (Option<FileOperationKind>, &str) {
    let trimmed_search_text = search_text.trim();
    let normalized_search_text = trimmed_search_text.to_lowercase();
    let operation_patterns = [
        ("copy path ", FileOperationKind::CopyPath),
        ("path ", FileOperationKind::CopyPath),
        ("copy name ", FileOperationKind::CopyName),
        ("name ", FileOperationKind::CopyName),
        ("copy file ", FileOperationKind::CopyFileReference),
        ("file ", FileOperationKind::CopyFileReference),
        ("show ", FileOperationKind::ShowInFolder),
        ("reveal ", FileOperationKind::ShowInFolder),
        ("finder ", FileOperationKind::ShowInFolder),
        ("explorer ", FileOperationKind::ShowInFolder),
        ("delete ", FileOperationKind::DeleteToRecovery),
        ("remove ", FileOperationKind::DeleteToRecovery),
    ];

    for (prefix, operation) in operation_patterns {
        if normalized_search_text.starts_with(prefix) {
            return (Some(operation), trimmed_search_text[prefix.len()..].trim());
        }
    }

    (None, trimmed_search_text)
}

fn file_operation_result(
    result: CommandResult,
    operation: FileOperationKind,
) -> Option<CommandResult> {
    let file_path = match &result.action {
        CommandAction::OpenPath(file_path) if result.category == CommandCategory::File => {
            file_path.clone()
        }
        _ => return None,
    };

    let (title_prefix, subtitle) = match operation {
        FileOperationKind::CopyPath => ("Copy path", file_path.display().to_string()),
        FileOperationKind::CopyName => (
            "Copy name",
            file_path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or("")
                .to_string(),
        ),
        FileOperationKind::CopyFileReference => ("Copy file", file_path.display().to_string()),
        FileOperationKind::ShowInFolder => ("Show in folder", file_path.display().to_string()),
        FileOperationKind::DeleteToRecovery => ("Delete file", "Move to Core recovery".to_string()),
    };

    Some(CommandResult::feature(
        format!("{title_prefix} {}", result.title),
        subtitle,
        CommandCategory::File,
        FeatureAction::FileOperation {
            operation,
            file_path,
        },
        86,
    ))
}

fn remove_trailing_polite_words(query: &str) -> &str {
    let mut trimmed_query = query.trim();
    for polite_word in ["please", "pls", "thanks", "thank you"] {
        if let Some(stripped_query) = trimmed_query
            .strip_suffix(polite_word)
            .map(str::trim)
            .filter(|stripped_query| !stripped_query.is_empty())
        {
            trimmed_query = stripped_query;
        }
    }

    trimmed_query
}

fn looks_like_web_search(query: &str) -> bool {
    let normalized_query = query.trim();
    !normalized_query.is_empty()
        && !normalized_query.contains('\\')
        && !normalized_query.contains(':')
        && normalized_query
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}

pub fn typed_url_from_query(query: &str) -> Option<String> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() || trimmed_query.contains(char::is_whitespace) {
        return None;
    }

    if trimmed_query.starts_with('@') {
        return None;
    }

    let lower_query = trimmed_query.to_lowercase();
    if lower_query.starts_with("http://") || lower_query.starts_with("https://") {
        return Some(trimmed_query.to_string());
    }

    if trimmed_query.contains('\\') {
        return None;
    }

    if trimmed_query.contains(':') {
        return None;
    }

    let path_query = trimmed_query.trim_end_matches('/');
    if path_query.contains('/') {
        let (host_part, _) = path_query.split_once('/')?;
        if is_typed_url_host(host_part) {
            return Some(format!("https://{path_query}"));
        }
        return None;
    }

    if is_typed_url_host(path_query) {
        return Some(format!("https://{path_query}"));
    }

    None
}

pub fn typed_url_display_label(query: &str) -> String {
    query.trim().trim_end_matches('/').to_string()
}

fn is_typed_url_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() || !host.contains('.') {
        return false;
    }

    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }

    if !labels.iter().all(|label| is_hostname_label(label)) {
        return false;
    }

    let tld = labels.last().copied().unwrap_or_default().to_lowercase();
    if !is_likely_tld(&tld) {
        return false;
    }

    if labels.len() == 2 && is_common_file_extension(&tld) {
        return false;
    }

    true
}

fn is_hostname_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

fn is_likely_tld(tld: &str) -> bool {
    const COMMON_TLDS: &[&str] = &[
        "com", "org", "net", "edu", "gov", "io", "co", "uk", "de", "fr", "dev", "app", "ai", "tv",
        "me", "us", "ca", "au", "jp", "xyz", "info", "biz", "cloud", "tech", "online", "site",
    ];

    COMMON_TLDS.contains(&tld)
        || (tld.len() >= 2
            && tld.len() <= 24
            && tld.chars().all(|character| character.is_ascii_alphabetic()))
}

fn is_common_file_extension(extension: &str) -> bool {
    const FILE_EXTENSIONS: &[&str] = &[
        "pdf", "txt", "doc", "docx", "png", "jpg", "jpeg", "gif", "webp", "zip", "rar", "7z", "exe",
        "msi", "csv", "json", "toml", "md", "rs", "mp4", "mp3", "wav", "mov", "avi", "mkv", "html",
        "htm", "css", "js", "ts", "tsx", "jsx", "xml", "yaml", "yml", "ini", "cfg", "log", "bat",
        "ps1", "sh", "c", "cpp", "h", "hpp", "java", "py", "go", "sql", "db", "sqlite",
    ];

    FILE_EXTENSIONS.contains(&extension)
}

fn result_sort_order(left: &CommandResult, right: &CommandResult) -> std::cmp::Ordering {
    result_sort_tier(left)
        .cmp(&result_sort_tier(right))
        .then_with(|| right.confidence.cmp(&left.confidence))
        .then_with(|| left.title.cmp(&right.title))
}

fn result_sort_tier(result: &CommandResult) -> u8 {
    match result.category {
        CommandCategory::Calculation => 0,
        CommandCategory::Web if result.confidence >= 90 => 1,
        CommandCategory::Application => 1,
        CommandCategory::File => 3,
        CommandCategory::Web => 2,
        _ => 1,
    }
}

fn parse_named_feature_query<'a>(query: &'a str, feature_name: &str) -> Option<&'a str> {
    let trimmed_query = query.trim();
    let (first_word, remaining_query) = trimmed_query
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed_query, ""));

    first_word
        .eq_ignore_ascii_case(feature_name)
        .then_some(remaining_query.trim())
}

struct BangSearch {
    prefix_bang: &'static str,
    prefix_expanded: &'static str,
    name: &'static str,
    search_url: &'static str,
    home_url: &'static str,
}

const BANGS: &[BangSearch] = &[
    BangSearch {
        prefix_bang: "!g",
        prefix_expanded: "Google | ",
        name: "Google",
        search_url: "https://www.google.com/search?q={}",
        home_url: "https://www.google.com",
    },
    BangSearch {
        prefix_bang: "!yt",
        prefix_expanded: "YouTube | ",
        name: "YouTube",
        search_url: "https://www.youtube.com/results?search_query={}",
        home_url: "https://www.youtube.com",
    },
    BangSearch {
        prefix_bang: "!w",
        prefix_expanded: "Wikipedia | ",
        name: "Wikipedia",
        search_url: "https://en.wikipedia.org/wiki/Special:Search?search={}",
        home_url: "https://en.wikipedia.org",
    },
    BangSearch {
        prefix_bang: "!wiki",
        prefix_expanded: "Wikipedia | ",
        name: "Wikipedia",
        search_url: "https://en.wikipedia.org/wiki/Special:Search?search={}",
        home_url: "https://en.wikipedia.org",
    },
    BangSearch {
        prefix_bang: "!gh",
        prefix_expanded: "GitHub | ",
        name: "GitHub",
        search_url: "https://github.com/search?q={}",
        home_url: "https://github.com",
    },
    BangSearch {
        prefix_bang: "!d",
        prefix_expanded: "DuckDuckGo | ",
        name: "DuckDuckGo",
        search_url: "https://duckduckgo.com/?q={}",
        home_url: "https://duckduckgo.com",
    },
];

fn match_bang_query(query: &str) -> Option<(&BangSearch, &str)> {
    let trimmed = query.trim_start();
    for bang in BANGS {
        if trimmed.starts_with(bang.prefix_bang) {
            let rest = &trimmed[bang.prefix_bang.len()..];
            if rest.is_empty() {
                return Some((bang, ""));
            } else if rest.starts_with(' ') {
                return Some((bang, rest.trim()));
            }
        }
        if trimmed.starts_with(bang.prefix_expanded) {
            let rest = &trimmed[bang.prefix_expanded.len()..];
            return Some((bang, rest.trim()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LauncherSettings;

    #[test]
    fn calculation_results_rank_above_features_and_applications() {
        let router = CommandRouter::new(
            LauncherSettings::default(),
            ApplicationIndex::default(),
            FileIndex::default(),
        );

        for query in ["2 days from now", "2 + 2", "2pm pt to uk"] {
            let results = router.search(query);
            assert!(
                results
                    .first()
                    .is_some_and(|result| result.category == CommandCategory::Calculation),
                "expected calculation first for `{query}`, got {:?}",
                results.first().map(|result| &result.category)
            );

            let mut last_tier = 0;
            for result in &results {
                let tier = result_sort_tier(result);
                assert!(
                    tier >= last_tier,
                    "result tiers must stay grouped: calculation, then features, then apps/web"
                );
                last_tier = tier;
            }
        }
    }

    #[test]
    fn parses_application_scope() {
        let scoped_query = parse_scoped_query("@app steam").unwrap();

        assert_eq!(scoped_query.scope, QueryScope::Applications);
        assert_eq!(scoped_query.search_text, "steam");
    }

    #[test]
    fn parses_core_command_starter_aliases() {
        assert_eq!(
            parse_scoped_query("@e rocket").unwrap().scope,
            QueryScope::Emoji
        );
        assert_eq!(file_search_scope_from_query("@e rocket"), None);
        assert_eq!(
            parse_scoped_query("@notes").unwrap().scope,
            QueryScope::Notes
        );
        assert_eq!(
            parse_scoped_query("@clip colors").unwrap().scope,
            QueryScope::Clipboard
        );
        assert_eq!(
            parse_scoped_query("@snippets reply").unwrap().scope,
            QueryScope::Snippets
        );
        assert_eq!(
            parse_scoped_query("@quicklinks docs").unwrap().scope,
            QueryScope::Quicklinks
        );
        assert_eq!(
            parse_scoped_query("@calendar join").unwrap().scope,
            QueryScope::Calendar
        );
    }

    #[test]
    fn parses_extension_scope() {
        let scoped_query = parse_scoped_query("@mp4 wistoria please").unwrap();

        assert_eq!(
            scoped_query.scope,
            QueryScope::Files(FileSearchScope::Extension("mp4".to_string()))
        );
        assert_eq!(
            remove_trailing_polite_words(&scoped_query.search_text),
            "wistoria"
        );

        assert_eq!(
            parse_scoped_query("@pdf invoice").unwrap().scope,
            QueryScope::Files(FileSearchScope::Extension("pdf".to_string()))
        );
        assert_eq!(
            parse_scoped_query("@files invoice").unwrap().scope,
            QueryScope::Files(FileSearchScope::AllFiles)
        );
        assert_eq!(
            parse_scoped_query("@video lecture").unwrap().scope,
            QueryScope::Files(FileSearchScope::Videos)
        );
        assert_eq!(
            file_search_scope_from_query("@pdf invoice"),
            Some(FileSearchScope::Extension("pdf".to_string()))
        );
    }

    #[test]
    fn parses_scope_after_search_text() {
        let scoped_query = parse_scoped_query("wistoria please @mp4").unwrap();

        assert_eq!(
            scoped_query.scope,
            QueryScope::Files(FileSearchScope::Extension("mp4".to_string()))
        );
        assert_eq!(
            remove_trailing_polite_words(&scoped_query.search_text),
            "wistoria"
        );
    }

    #[test]
    fn parses_images_slash_pictures_scope() {
        let scoped_query = parse_scoped_query("@images/pictures skyline").unwrap();

        assert_eq!(
            scoped_query.scope,
            QueryScope::Files(FileSearchScope::Images)
        );
        assert_eq!(scoped_query.search_text, "skyline");
    }

    #[test]
    fn parses_explicit_file_content_scope() {
        let scoped_query = parse_scoped_query("@file:content database url").unwrap();

        assert_eq!(
            scoped_query.scope,
            QueryScope::Files(FileSearchScope::Content)
        );
        assert_eq!(scoped_query.search_text, "database url");
    }

    #[test]
    fn bare_at_and_partial_tags_do_not_trigger_file_scope() {
        assert_eq!(file_search_scope_from_query("@"), None);
        assert_eq!(file_search_scope_from_query("@d"), None);
        assert_eq!(file_search_scope_from_query("@ca"), None);
    }

    #[test]
    fn explicit_file_scopes_still_work() {
        assert_eq!(
            file_search_scope_from_query("@files budget"),
            Some(FileSearchScope::AllFiles)
        );
        assert_eq!(
            file_search_scope_from_query("@pdf invoice"),
            Some(FileSearchScope::Extension("pdf".to_string()))
        );
    }

    #[test]
    fn destiny_scope_does_not_fall_through_to_files() {
        let scoped_query = parse_scoped_query("@d2 hammer").unwrap();
        assert_eq!(scoped_query.scope, QueryScope::Destiny);
        assert_eq!(scoped_query.search_text, "hammer");
    }

    #[test]
    fn terminal_scope_is_recognized() {
        let scoped_query = parse_scoped_query("@cmd ipconfig").unwrap();
        assert_eq!(scoped_query.scope, QueryScope::Terminal);
        assert_eq!(scoped_query.search_text, "ipconfig");
    }

    #[test]
    fn dev_tools_scope_is_recognized() {
        let scoped_query = parse_scoped_query("@dev sha256 hello").unwrap();
        assert_eq!(scoped_query.scope, QueryScope::DevTools);
        assert_eq!(scoped_query.search_text, "sha256 hello");
    }

    #[test]
    fn git_and_package_scopes_are_recognized() {
        assert_eq!(
            parse_scoped_query("@git status").unwrap().scope,
            QueryScope::Git
        );
        assert_eq!(
            parse_scoped_query("@winget search vscode").unwrap().scope,
            QueryScope::Package
        );
    }

    #[test]
    fn process_and_color_scopes_are_recognized() {
        assert_eq!(
            parse_scoped_query("@kill chrome").unwrap().scope,
            QueryScope::Process
        );
        assert_eq!(
            parse_scoped_query("@color hex").unwrap().scope,
            QueryScope::Color
        );
    }

    #[test]
    fn new_raycast_parity_scopes_are_recognized() {
        assert_eq!(
            parse_scoped_query("@screenshot").unwrap().scope,
            QueryScope::Screenshot
        );
        assert_eq!(
            parse_scoped_query("@define hello").unwrap().scope,
            QueryScope::Lookup
        );
        assert_eq!(
            parse_scoped_query("@github issues rust").unwrap().scope,
            QueryScope::GitHub
        );
        assert_eq!(
            parse_scoped_query("@now").unwrap().scope,
            QueryScope::Media
        );
        assert_eq!(
            parse_scoped_query("@network ping").unwrap().scope,
            QueryScope::Network
        );
        assert_eq!(
            parse_scoped_query("@scoop search git").unwrap().scope,
            QueryScope::Package
        );
        assert_eq!(
            parse_scoped_query("@colorclip").unwrap().scope,
            QueryScope::Color
        );
    }

    #[test]
    fn typed_url_detects_bare_domains() {
        assert_eq!(
            typed_url_from_query("x.com"),
            Some("https://x.com".to_string())
        );
        assert_eq!(
            typed_url_from_query("docs.waveterm.dev"),
            Some("https://docs.waveterm.dev".to_string())
        );
        assert_eq!(
            typed_url_from_query("https://x.com/home"),
            Some("https://x.com/home".to_string())
        );
        assert_eq!(
            typed_url_from_query("x.com/status/123"),
            Some("https://x.com/status/123".to_string())
        );
    }

    #[test]
    fn typed_url_ignores_file_like_queries() {
        assert_eq!(typed_url_from_query("report.pdf"), None);
        assert_eq!(typed_url_from_query("notes.md"), None);
        assert_eq!(typed_url_from_query("2 + 2"), None);
        assert_eq!(is_implicit_file_browse_query("report.pdf"), true);
        assert_eq!(is_implicit_file_browse_query("x.com"), false);
    }

    #[test]
    fn skips_universal_file_search_for_calculation_queries() {
        assert!(!should_include_universal_file_results("2 + 2"));
        assert!(!should_include_universal_file_results("2 days from now"));
        assert!(!should_include_universal_file_results("10% of 50"));
        assert!(should_include_universal_file_results("notepad"));
        assert!(!should_include_universal_file_results("ab"));
    }

    #[test]
    fn typed_url_search_returns_open_website_result() {
        let router = CommandRouter::new(
            LauncherSettings::default(),
            ApplicationIndex::default(),
            FileIndex::default(),
        );

        let results = router.search("x.com");
        let web_result = results
            .iter()
            .find(|result| matches!(result.action, CommandAction::OpenUrl(_)))
            .expect("expected open url result");

        assert_eq!(web_result.category, CommandCategory::Web);
        assert_eq!(web_result.title, "Open x.com");
        assert_eq!(web_result.subtitle, "https://x.com");
        assert!(
            !results
                .iter()
                .any(|result| result.title.starts_with("Search the web for")),
            "typed URLs should open directly instead of defaulting to Google search"
        );
    }

    #[test]
    fn applications_have_priority_over_files() {
        let app_result = CommandResult {
            title: "Steam".to_string(),
            subtitle: "".to_string(),
            copy_text: "".to_string(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Application,
            action: CommandAction::None,
            confidence: 100,
        };

        let file_result = CommandResult {
            title: "steam_shortcut.lnk".to_string(),
            subtitle: "".to_string(),
            copy_text: "".to_string(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::File,
            action: CommandAction::None,
            confidence: 100,
        };

        let mut results = vec![file_result.clone(), app_result.clone()];
        results.sort_by(result_sort_order);

        assert_eq!(results[0].category, CommandCategory::Application);
        assert_eq!(results[1].category, CommandCategory::File);
    }

    #[test]
    fn bang_search_works() {
        let router = CommandRouter::new(
            LauncherSettings::default(),
            ApplicationIndex::default(),
            FileIndex::default(),
        );

        let results = router.search("!g fortnite");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, CommandCategory::Web);
        assert_eq!(results[0].title, "Search Google for \"fortnite\"");
        assert_eq!(results[0].subtitle, "https://www.google.com/search?q=fortnite");

        let results_expanded = router.search("Google | fortnite");
        assert_eq!(results_expanded.len(), 1);
        assert_eq!(results_expanded[0].title, "Search Google for \"fortnite\"");
    }
}
