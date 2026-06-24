use crate::{
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction, NoteExportFormat},
    search_text::normalize_search_text,
    settings::config_directory,
};
use chrono::Local;
use std::{
    fs, io,
    path::{Path, PathBuf},
};


const DELETED_NOTE_RETENTION_DAYS: i64 = 60;
const MIN_QUICK_NOTE_WIDTH: f32 = 260.;
const MIN_QUICK_NOTE_HEIGHT: f32 = 180.;
const MAX_QUICK_NOTE_WIDTH: f32 = 720.;
const MAX_QUICK_NOTE_HEIGHT: f32 = 640.;

#[derive(Clone, Debug)]
struct NoteSearchRecord {
    title: String,
    body: String,
    path: PathBuf,
    updated_at: i64,
}

pub fn search_notes(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();
    let _ = delete_expired_deleted_notes();

    if trimmed_search_text.is_empty() {
        return note_home_results();
    }

    if is_deleted_notes_query(trimmed_search_text) {
        return deleted_note_results();
    }

    if let Some(note_query) = trimmed_search_text
        .strip_prefix("delete ")
        .or_else(|| trimmed_search_text.strip_prefix("remove "))
    {
        return note_operation_results(note_query, NoteOperation::Delete);
    }

    if let Some(export_query) = trimmed_search_text.strip_prefix("export ") {
        if let Some((export_format, note_query)) = parse_note_export_query(export_query) {
            return note_operation_results(note_query, NoteOperation::Export(export_format));
        }
    }

    if let Some(note_text) = trimmed_search_text
        .strip_prefix("new ")
        .or_else(|| trimmed_search_text.strip_prefix("create "))
    {
        let (title, body) = parse_note_text(note_text);
        return vec![create_note_result(title, body)];
    }

    let mut results = ranked_notes(trimmed_search_text)
        .into_iter()
        .take(10)
        .map(note_result)
        .collect::<Vec<_>>();

    let (title, body) = parse_note_text(trimmed_search_text);
    if !title.is_empty() {
        results.push(create_note_result(title, body));
    }

    results
}

pub fn load_note_for_editing(note_path: &Path) -> io::Result<(String, String)> {
    let note_text = fs::read_to_string(note_path)?;
    let title = note_title(note_path, &note_text);
    Ok((title, note_body_for_editing(&note_text)))
}

pub fn save_note_content(note_path: &Path, title: &str, body: &str) -> io::Result<()> {
    let updated_at = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let existing_text = fs::read_to_string(note_path).unwrap_or_default();
    let created_line = existing_text
        .lines()
        .find(|line| line.trim().starts_with("Created:"))
        .map(|line| format!("{line}\n"))
        .unwrap_or_else(|| format!("Created: {updated_at}\n"));
    let body_text = if body.trim().is_empty() {
        String::new()
    } else {
        format!("\n{}\n", body.trim_end())
    };
    let note_text = format!(
        "# {}\n\n{created_line}Updated: {updated_at}{body_text}",
        title.trim()
    );
    fs::write(note_path, note_text)
}

pub fn quick_note_path() -> PathBuf {
    crate::paths::quick_note_file()
}

pub fn load_quick_note() -> String {
    fs::read_to_string(quick_note_path()).unwrap_or_default()
}

pub fn save_quick_note(content: &str) -> io::Result<()> {
    if let Some(quick_note_directory) = quick_note_path().parent() {
        fs::create_dir_all(quick_note_directory)?;
    }
    fs::write(quick_note_path(), content)
}

pub fn quick_note_window_size(settings: &crate::settings::LauncherSettings) -> (f32, f32) {
    let width = settings
        .quick_note_width
        .clamp(MIN_QUICK_NOTE_WIDTH, MAX_QUICK_NOTE_WIDTH);
    let height = settings
        .quick_note_height
        .clamp(MIN_QUICK_NOTE_HEIGHT, MAX_QUICK_NOTE_HEIGHT);
    (width, height)
}

pub fn quick_note_window_origin(
    settings: &crate::settings::LauncherSettings,
) -> (i32, i32, f32, f32) {
    let (width, height) = quick_note_window_size(settings);
    let work_area = primary_work_area();
    let width_px = width.round() as i32;
    let height_px = height.round() as i32;
    let offset_x = settings.quick_note_offset_x.max(0);
    let offset_y = settings.quick_note_offset_y.max(0);

    let (left, top) = match normalize_quick_note_anchor(&settings.quick_note_anchor) {
        QuickNoteAnchor::TopLeft => (work_area.left + offset_x, work_area.top + offset_y),
        QuickNoteAnchor::TopRight => (
            work_area.right - width_px - offset_x,
            work_area.top + offset_y,
        ),
        QuickNoteAnchor::BottomLeft => (
            work_area.left + offset_x,
            work_area.bottom - height_px - offset_y,
        ),
        QuickNoteAnchor::BottomRight => (
            work_area.right - width_px - offset_x,
            work_area.bottom - height_px - offset_y,
        ),
        QuickNoteAnchor::Center => (
            work_area.left + (work_area.width - width_px) / 2 + offset_x,
            work_area.top + (work_area.height - height_px) / 2 + offset_y,
        ),
    };

    (left, top, width, height)
}

pub fn quick_note_anchor_options() -> &'static [(&'static str, &'static str)] {
    &[
        ("top-left", "Top left"),
        ("top-right", "Top right"),
        ("bottom-left", "Bottom left"),
        ("bottom-right", "Bottom right"),
        ("center", "Center"),
    ]
}

pub fn create_markdown_note(title: &str, body: &str) -> io::Result<PathBuf> {
    let notes_directory = notes_directory();
    fs::create_dir_all(&notes_directory)?;

    let note_path = unique_note_path(title);
    let created_at = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let body_text = if body.trim().is_empty() {
        String::new()
    } else {
        format!("\n{}\n", body.trim())
    };
    let note_text = format!("# {}\n\nCreated: {created_at}\n{body_text}", title.trim());
    fs::write(&note_path, note_text)?;
    Ok(note_path)
}

pub fn delete_note_to_recovery(note_path: &Path) -> io::Result<PathBuf> {
    let deleted_notes_directory = deleted_notes_directory();
    fs::create_dir_all(&deleted_notes_directory)?;

    let file_name = note_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or("note.md");
    let deleted_note_path = deleted_notes_directory.join(format!(
        "{}__{}",
        Local::now().timestamp(),
        sanitize_file_name(file_name)
    ));
    fs::rename(note_path, &deleted_note_path)?;
    Ok(deleted_note_path)
}

pub fn restore_deleted_note(deleted_note_path: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(notes_directory())?;

    let file_name = deleted_note_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .and_then(|file_name| file_name.split_once("__").map(|(_, original)| original))
        .unwrap_or("restored-note.md");
    let restored_note_path = unique_path(notes_directory().join(file_name));
    fs::rename(deleted_note_path, &restored_note_path)?;
    Ok(restored_note_path)
}

pub fn export_note(note_path: &Path, export_format: NoteExportFormat) -> io::Result<PathBuf> {
    let note_text = fs::read_to_string(note_path)?;
    let export_path = export_path_for_note(note_path, &export_format);
    if let Some(export_directory) = export_path.parent() {
        fs::create_dir_all(export_directory)?;
    }

    let export_text = match export_format {
        NoteExportFormat::PlainText => markdown_to_plain_text(&note_text),
        NoteExportFormat::Markdown => note_text,
        NoteExportFormat::Html => markdown_to_html(&note_text),
    };
    fs::write(&export_path, export_text)?;
    Ok(export_path)
}

fn note_home_results() -> Vec<CommandResult> {
    let mut results = vec![CommandResult::informational(
        "Notes",
        "Use @note title | markdown body to create, search, export, delete, or recover notes",
    )];
    results.extend(recent_notes().into_iter().take(8).map(note_result));
    results.extend(deleted_note_results().into_iter().take(3));
    results
}

fn note_operation_results(note_query: &str, operation: NoteOperation) -> Vec<CommandResult> {
    ranked_notes(note_query)
        .into_iter()
        .take(8)
        .map(|note| match &operation {
            NoteOperation::Delete => CommandResult::feature(
                format!("Delete note {}", note.title),
                "Recoverable for 60 days",
                CommandCategory::Note,
                FeatureAction::DeleteNote {
                    note_path: note.path,
                },
                94,
            ),
            NoteOperation::Export(export_format) => {
                let format_label = export_format_label(export_format);
                CommandResult::feature(
                    format!("Export {format_label} {}", note.title),
                    note.path.display().to_string(),
                    CommandCategory::Note,
                    FeatureAction::ExportNote {
                        note_path: note.path,
                        export_format: export_format.clone(),
                    },
                    92,
                )
            }
        })
        .collect()
}

fn deleted_note_results() -> Vec<CommandResult> {
    let mut deleted_notes = deleted_notes();
    deleted_notes.sort_by_key(|note| -note.updated_at);

    if deleted_notes.is_empty() {
        return vec![CommandResult::informational(
            "Deleted notes",
            "No recoverable notes are currently stored",
        )];
    }

    deleted_notes
        .into_iter()
        .take(12)
        .map(|note| {
            CommandResult::feature(
                format!("Recover note {}", note.title),
                "Deleted notes are retained for 60 days",
                CommandCategory::Note,
                FeatureAction::RestoreNote {
                    deleted_note_path: note.path,
                },
                88,
            )
        })
        .collect()
}

fn note_result(note: NoteSearchRecord) -> CommandResult {
    CommandResult {
        title: note.title,
        subtitle: preview_note_body(&note.body),
        copy_text: note.path.display().to_string(),
        explanation: Some("Open Markdown note".to_string()),
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Note,
        action: CommandAction::OpenPath(note.path),
        confidence: 88,
    }
}

fn create_note_result(title: String, body: String) -> CommandResult {
    CommandResult::feature(
        format!("Create note {title}"),
        "Markdown note",
        CommandCategory::Note,
        FeatureAction::CreateNote { title, body },
        84,
    )
}

fn ranked_notes(search_text: &str) -> Vec<NoteSearchRecord> {
    let normalized_search_text = normalize_search_text(search_text);
    let mut scored_notes = active_notes()
        .into_iter()
        .filter_map(|note| score_note(&note, &normalized_search_text).map(|score| (score, note)))
        .collect::<Vec<_>>();

    scored_notes.sort_by_key(|(score, note)| {
        (
            std::cmp::Reverse(*score),
            -note.updated_at,
            note.title.to_lowercase(),
        )
    });

    scored_notes.into_iter().map(|(_, note)| note).collect()
}

fn recent_notes() -> Vec<NoteSearchRecord> {
    let mut notes = active_notes();
    notes.sort_by_key(|note| -note.updated_at);
    notes
}

fn active_notes() -> Vec<NoteSearchRecord> {
    notes_from_directory(&notes_directory())
}

fn deleted_notes() -> Vec<NoteSearchRecord> {
    notes_from_directory(&deleted_notes_directory())
}

fn notes_from_directory(directory: &Path) -> Vec<NoteSearchRecord> {
    let Ok(directory_entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    directory_entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("md"))
        .filter_map(|path| note_from_path(&path))
        .collect()
}

fn note_from_path(path: &Path) -> Option<NoteSearchRecord> {
    let body = fs::read_to_string(path).ok()?;
    let title = note_title(path, &body);
    let updated_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified_time| modified_time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    Some(NoteSearchRecord {
        title,
        body,
        path: path.to_path_buf(),
        updated_at,
    })
}

fn score_note(note: &NoteSearchRecord, normalized_search_text: &str) -> Option<u8> {
    if normalized_search_text.is_empty() {
        return Some(50);
    }

    let title = normalize_search_text(&note.title);
    let body = normalize_search_text(&note.body);

    if title == normalized_search_text {
        return Some(96);
    }

    if title.starts_with(normalized_search_text) {
        return Some(88);
    }

    if title.contains(normalized_search_text) {
        return Some(80);
    }

    body.contains(normalized_search_text).then_some(66)
}

fn note_title(path: &Path, body: &str) -> String {
    body.lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_stem()
                .and_then(|file_stem| file_stem.to_str())
                .map(|file_stem| file_stem.replace(['-', '_'], " "))
        })
        .unwrap_or_else(|| "Untitled note".to_string())
}

fn parse_note_text(note_text: &str) -> (String, String) {
    let trimmed_note_text = note_text.trim();
    let (title, body) = trimmed_note_text
        .split_once('|')
        .or_else(|| trimmed_note_text.split_once(" -- "))
        .unwrap_or((trimmed_note_text, ""));

    (title.trim().to_string(), body.trim().to_string())
}

fn parse_note_export_query(export_query: &str) -> Option<(NoteExportFormat, &str)> {
    let (format_text, note_query) = export_query
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((export_query.trim(), ""));
    let export_format = match format_text.to_lowercase().as_str() {
        "txt" | "text" | "plain" | "plain-text" => NoteExportFormat::PlainText,
        "md" | "markdown" => NoteExportFormat::Markdown,
        "html" | "web" => NoteExportFormat::Html,
        _ => return None,
    };

    Some((export_format, note_query.trim()))
}

fn is_deleted_notes_query(search_text: &str) -> bool {
    matches!(
        search_text.to_lowercase().as_str(),
        "deleted" | "trash" | "recover" | "recovery"
    )
}

fn delete_expired_deleted_notes() -> io::Result<()> {
    let retention_cutoff = Local::now().timestamp() - DELETED_NOTE_RETENTION_DAYS * 24 * 60 * 60;
    for deleted_note in deleted_notes() {
        if deleted_note.updated_at < retention_cutoff {
            let _ = fs::remove_file(deleted_note.path);
        }
    }
    Ok(())
}

fn unique_note_path(title: &str) -> PathBuf {
    unique_path(notes_directory().join(format!("{}.md", sanitize_file_name(title))))
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }

    let parent_directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(notes_directory);
    let file_stem = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .unwrap_or("note");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for copy_index in 2..=999 {
        let file_name = match extension {
            Some(extension) => format!("{file_stem}-{copy_index}.{extension}"),
            None => format!("{file_stem}-{copy_index}"),
        };
        let candidate_path = parent_directory.join(file_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
    }

    path
}

fn export_path_for_note(note_path: &Path, export_format: &NoteExportFormat) -> PathBuf {
    let extension = match export_format {
        NoteExportFormat::PlainText => "txt",
        NoteExportFormat::Markdown => "md",
        NoteExportFormat::Html => "html",
    };
    let file_stem = note_path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .unwrap_or("note");

    unique_path(
        config_directory()
            .join("note_exports")
            .join(format!("{file_stem}.{extension}")),
    )
}

fn markdown_to_plain_text(markdown_text: &str) -> String {
    markdown_text
        .lines()
        .map(|line| {
            line.trim_start_matches('#')
                .trim_start_matches(['-', '*', ' '])
                .replace("**", "")
                .replace(['*', '`'], "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn markdown_to_html(markdown_text: &str) -> String {
    let body = markdown_text
        .lines()
        .map(markdown_line_to_html)
        .collect::<Vec<_>>()
        .join("\n");

    format!("<!doctype html>\n<meta charset=\"utf-8\">\n<body>\n{body}\n</body>\n")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownLineKind {
    Empty,
    Heading1,
    Heading2,
    Heading3,
    Bullet,
    Checkbox { checked: bool },
    Paragraph,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownInlineStyle {
    Plain,
    Bold,
    Italic,
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownInlineSegment {
    pub text: String,
    pub style: MarkdownInlineStyle,
}

pub fn parse_markdown_line(line: &str) -> (MarkdownLineKind, String) {
    let trimmed_line = line.trim();
    if let Some(heading) = trimmed_line.strip_prefix("### ") {
        return (MarkdownLineKind::Heading3, heading.to_string());
    }
    if let Some(heading) = trimmed_line.strip_prefix("## ") {
        return (MarkdownLineKind::Heading2, heading.to_string());
    }
    if let Some(heading) = trimmed_line.strip_prefix("# ") {
        return (MarkdownLineKind::Heading1, heading.to_string());
    }
    if let Some(item) = trimmed_line.strip_prefix("- [ ] ") {
        return (MarkdownLineKind::Checkbox { checked: false }, item.to_string());
    }
    if let Some(item) = trimmed_line.strip_prefix("- [x] ") {
        return (MarkdownLineKind::Checkbox { checked: true }, item.to_string());
    }
    if let Some(item) = trimmed_line.strip_prefix("- ") {
        return (MarkdownLineKind::Bullet, item.to_string());
    }
    if trimmed_line.is_empty() {
        return (MarkdownLineKind::Empty, String::new());
    }

    (MarkdownLineKind::Paragraph, trimmed_line.to_string())
}

pub fn parse_markdown_inline(text: &str) -> Vec<MarkdownInlineSegment> {
    let mut segments = Vec::new();
    let mut index = 0;
    let chars: Vec<char> = text.chars().collect();

    while index < chars.len() {
        if chars[index] == '`' {
            if let Some(end) = chars[index + 1..]
                .iter()
                .position(|character| *character == '`')
            {
                let segment = chars[index + 1..index + 1 + end]
                    .iter()
                    .collect::<String>();
                if !segment.is_empty() {
                    segments.push(MarkdownInlineSegment {
                        text: segment,
                        style: MarkdownInlineStyle::Code,
                    });
                }
                index += end + 2;
                continue;
            }
        }

        if chars[index] == '*' && chars.get(index + 1) == Some(&'*') {
            if let Some(end) = chars[index + 2..]
                .windows(2)
                .position(|window| window == ['*', '*'])
            {
                let segment = chars[index + 2..index + 2 + end]
                    .iter()
                    .collect::<String>();
                if !segment.is_empty() {
                    segments.push(MarkdownInlineSegment {
                        text: segment,
                        style: MarkdownInlineStyle::Bold,
                    });
                }
                index += end + 4;
                continue;
            }
        }

        if chars[index] == '*' {
            if let Some(end) = chars[index + 1..]
                .iter()
                .position(|character| *character == '*')
            {
                let segment = chars[index + 1..index + 1 + end]
                    .iter()
                    .collect::<String>();
                if !segment.is_empty() {
                    segments.push(MarkdownInlineSegment {
                        text: segment,
                        style: MarkdownInlineStyle::Italic,
                    });
                }
                index += end + 2;
                continue;
            }
        }

        let start = index;
        while index < chars.len() {
            if chars[index] == '`' {
                break;
            }
            if chars[index] == '*' {
                break;
            }
            index += 1;
        }

        let segment = chars[start..index].iter().collect::<String>();
        if !segment.is_empty() {
            segments.push(MarkdownInlineSegment {
                text: segment,
                style: MarkdownInlineStyle::Plain,
            });
        }
    }

    if segments.is_empty() {
        segments.push(MarkdownInlineSegment {
            text: String::new(),
            style: MarkdownInlineStyle::Plain,
        });
    }

    segments
}

fn markdown_line_to_html(line: &str) -> String {
    let (kind, text) = parse_markdown_line(line);
    match kind {
        MarkdownLineKind::Empty => "<br>".to_string(),
        MarkdownLineKind::Heading1 => format!("<h1>{}</h1>", escape_html(&text)),
        MarkdownLineKind::Heading2 => format!("<h2>{}</h2>", escape_html(&text)),
        MarkdownLineKind::Heading3 => format!("<h3>{}</h3>", escape_html(&text)),
        MarkdownLineKind::Bullet => format!("<li>{}</li>", escape_html(&text)),
        MarkdownLineKind::Checkbox { checked } => format!(
            "<label><input type=\"checkbox\"{}> {}</label>",
            if checked { " checked" } else { "" },
            escape_html(&text)
        ),
        MarkdownLineKind::Paragraph => format!("<p>{}</p>", escape_html(&text)),
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn preview_note_body(body: &str) -> String {
    let preview = body
        .lines()
        .filter(|line| !line.trim().starts_with("# "))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");

    if preview.chars().count() <= 96 {
        preview
    } else {
        format!("{}...", preview.chars().take(95).collect::<String>())
    }
}

fn export_format_label(export_format: &NoteExportFormat) -> &'static str {
    match export_format {
        NoteExportFormat::PlainText => "plain text",
        NoteExportFormat::Markdown => "Markdown",
        NoteExportFormat::Html => "HTML",
    }
}

fn sanitize_file_name(file_name: &str) -> String {
    let sanitized = file_name
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if sanitized.is_empty() {
        "untitled-note".to_string()
    } else {
        sanitized
    }
}

fn notes_directory() -> PathBuf {
    crate::paths::notes_dir()
}

fn deleted_notes_directory() -> PathBuf {
    crate::paths::deleted_notes_dir()
}

fn note_body_for_editing(note_text: &str) -> String {
    let mut lines = note_text.lines();
    let _ = lines.next();

    let mut body_lines = Vec::new();
    let mut skipping_metadata = true;
    for line in lines {
        let trimmed_line = line.trim();
        if skipping_metadata {
            if trimmed_line.is_empty() {
                continue;
            }
            if trimmed_line.starts_with("Created:") || trimmed_line.starts_with("Updated:") {
                continue;
            }
            skipping_metadata = false;
        }
        body_lines.push(line);
    }

    let mut body = body_lines.join("\n");
    while body.ends_with('\n') {
        body.pop();
    }
    body
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickNoteAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

fn normalize_quick_note_anchor(anchor: &str) -> QuickNoteAnchor {
    match anchor.trim().to_lowercase().as_str() {
        "top-left" | "topleft" | "left" => QuickNoteAnchor::TopLeft,
        "bottom-left" | "bottomleft" => QuickNoteAnchor::BottomLeft,
        "bottom-right" | "bottomright" => QuickNoteAnchor::BottomRight,
        "center" | "middle" => QuickNoteAnchor::Center,
        _ => QuickNoteAnchor::TopRight,
    }
}

#[derive(Clone, Copy, Debug)]
struct WorkArea {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    width: i32,
    height: i32,
}

fn primary_work_area() -> WorkArea {
    platform::primary_work_area().unwrap_or(WorkArea {
        left: 0,
        top: 0,
        right: 1280,
        bottom: 800,
        width: 1280,
        height: 800,
    })
}

#[cfg(target_os = "windows")]
mod platform {
    use super::WorkArea;
    use windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY},
        UI::WindowsAndMessaging::GetSystemMetrics,
    };

    pub fn primary_work_area() -> Option<WorkArea> {
        let monitor = unsafe {
            MonitorFromPoint(
                windows::Win32::Foundation::POINT { x: 0, y: 0 },
                MONITOR_DEFAULTTOPRIMARY,
            )
        };
        if monitor.0.is_null() {
            return fallback_work_area();
        }

        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let succeeded = unsafe { GetMonitorInfoW(monitor, &mut monitor_info).as_bool() };
        if !succeeded {
            return fallback_work_area();
        }

        Some(work_area_from_rect(monitor_info.rcWork))
    }

    fn fallback_work_area() -> Option<WorkArea> {
        let width = unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) };
        let height = unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) };
        if width <= 0 || height <= 0 {
            return None;
        }

        Some(WorkArea {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
            width,
            height,
        })
    }

    fn work_area_from_rect(rect: RECT) -> WorkArea {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        WorkArea {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            width,
            height,
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::WorkArea;

    pub fn primary_work_area() -> Option<WorkArea> {
        None
    }
}

enum NoteOperation {
    Delete,
    Export(NoteExportFormat),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_text_with_markdown_body() {
        let (title, body) = parse_note_text("Meeting | ## Agenda");

        assert_eq!(title, "Meeting");
        assert_eq!(body, "## Agenda");
    }

    #[test]
    fn parses_note_export_query() {
        let (format, query) = parse_note_export_query("html Meeting").unwrap();

        assert_eq!(format, NoteExportFormat::Html);
        assert_eq!(query, "Meeting");
    }

    #[test]
    fn parses_markdown_line_kinds() {
        assert_eq!(
            parse_markdown_line("# Title"),
            (MarkdownLineKind::Heading1, "Title".to_string())
        );
        assert_eq!(
            parse_markdown_line("- [x] Done"),
            (
                MarkdownLineKind::Checkbox { checked: true },
                "Done".to_string()
            )
        );
    }

    #[test]
    fn parses_markdown_inline_segments() {
        let segments = parse_markdown_inline("Hello **world** and `code`");
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].text, "Hello ");
        assert_eq!(segments[1].style, MarkdownInlineStyle::Bold);
        assert_eq!(segments[2].text, " and ");
        assert_eq!(segments[3].style, MarkdownInlineStyle::Code);
    }

    #[test]
    fn extracts_note_body_for_editing() {
        let body = note_body_for_editing(
            "# Meeting\n\nCreated: 2026-01-01\nUpdated: 2026-01-02\n\n## Agenda\n- Item",
        );

        assert_eq!(body, "## Agenda\n- Item");
    }
}
