use crate::command::CommandCategory;
use crate::paths::lucide_cache_dir;
use crate::settings::config_directory;
use gpui::{div, img, prelude::*, px, rgba, Rgba};
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
};

static COLORED_ICON_CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LucideIcon {
    Settings,
    ArrowLeft,
    Zap,
    Calculator,
    AppWindow,
    File,
    Globe,
    CircleQuestionMark,
    StickyNote,
    Target,
    Clipboard,
    Columns2,
    TextQuote,
    Link,
    Calendar,
    Monitor,
    Smile,
    Clock,
    Terminal,
    Heart,
    HeartOff,
    Trash2,
    FolderOpen,
    RefreshCw,
    FileCog,
    Keyboard,
    Command,
    BookOpen,
    Search,
    X,
    Play,
    Pause,
    SkipForward,
    SkipBack,
    Music,
    Volume2,
    Power,
    ChevronDown,
    ChevronUp,
}

impl LucideIcon {
    pub fn file_name(self) -> &'static str {
        match self {
            LucideIcon::Settings => "settings",
            LucideIcon::ArrowLeft => "arrow-left",
            LucideIcon::Zap => "zap",
            LucideIcon::Calculator => "calculator",
            LucideIcon::AppWindow => "app-window",
            LucideIcon::File => "file",
            LucideIcon::Globe => "globe",
            LucideIcon::CircleQuestionMark => "circle-question-mark",
            LucideIcon::StickyNote => "sticky-note",
            LucideIcon::Target => "target",
            LucideIcon::Clipboard => "clipboard",
            LucideIcon::Columns2 => "columns-2",
            LucideIcon::TextQuote => "text-quote",
            LucideIcon::Link => "link",
            LucideIcon::Calendar => "calendar",
            LucideIcon::Monitor => "monitor",
            LucideIcon::Smile => "smile",
            LucideIcon::Clock => "clock",
            LucideIcon::Terminal => "terminal",
            LucideIcon::Heart => "heart",
            LucideIcon::HeartOff => "heart-off",
            LucideIcon::Trash2 => "trash-2",
            LucideIcon::FolderOpen => "folder-open",
            LucideIcon::RefreshCw => "refresh-cw",
            LucideIcon::FileCog => "file-cog",
            LucideIcon::Keyboard => "keyboard",
            LucideIcon::Command => "command",
            LucideIcon::BookOpen => "book-open",
            LucideIcon::Search => "search",
            LucideIcon::X => "x",
            LucideIcon::Play => "play",
            LucideIcon::Pause => "pause",
            LucideIcon::SkipForward => "skip-forward",
            LucideIcon::SkipBack => "skip-back",
            LucideIcon::Music => "music",
            LucideIcon::Volume2 => "volume-2",
            LucideIcon::Power => "power",
            LucideIcon::ChevronDown => "chevron-down",
            LucideIcon::ChevronUp => "chevron-up",
        }
    }

    pub fn for_category(category: &CommandCategory) -> Self {
        match category {
            CommandCategory::Calculation => LucideIcon::Calculator,
            CommandCategory::Application => LucideIcon::AppWindow,
            CommandCategory::File => LucideIcon::File,
            CommandCategory::BuiltIn => LucideIcon::Settings,
            CommandCategory::Web => LucideIcon::Globe,
            CommandCategory::Help => LucideIcon::CircleQuestionMark,
            CommandCategory::Note => LucideIcon::StickyNote,
            CommandCategory::Focus => LucideIcon::Target,
            CommandCategory::Clipboard => LucideIcon::Clipboard,
            CommandCategory::WindowManagement => LucideIcon::Columns2,
            CommandCategory::Snippet => LucideIcon::TextQuote,
            CommandCategory::Quicklink => LucideIcon::Link,
            CommandCategory::Calendar => LucideIcon::Calendar,
            CommandCategory::System => LucideIcon::Monitor,
            CommandCategory::Emoji => LucideIcon::Smile,
            CommandCategory::Context => LucideIcon::Clock,
            CommandCategory::DevTools => LucideIcon::Zap,
            CommandCategory::Git => LucideIcon::Command,
            CommandCategory::Package => LucideIcon::AppWindow,
            CommandCategory::Lookup => LucideIcon::BookOpen,
            CommandCategory::Media => LucideIcon::Clock,
            CommandCategory::Network => LucideIcon::Globe,
        }
    }
}

pub fn render_lucide_icon(
    icon: LucideIcon,
    size: f32,
    color: Rgba,
    filled: bool,
) -> gpui::AnyElement {
    let path = colored_icon_path(icon, color, filled);
    img(path)
        .w(px(size))
        .h(px(size))
        .into_any_element()
}

pub fn render_hoverable_lucide_icon(
    icon: LucideIcon,
    size: f32,
    color: Rgba,
    filled: bool,
) -> gpui::AnyElement {
    let path = colored_icon_path(icon, color, filled);
    img(path)
        .w(px(size))
        .h(px(size))
        .hover(move |style| style.w(px(size * 1.25)).h(px(size * 1.25)))
        .into_any_element()
}

pub fn render_lucide_badge(icon: LucideIcon, size: f32, color: Rgba) -> gpui::AnyElement {
    let icon_size = (size * 0.58).clamp(12., 24.);
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgba(0xffffff10))
        .child(render_lucide_icon(icon, icon_size, color, false))
        .into_any_element()
}

pub fn render_lucide_button_icon(
    icon: LucideIcon,
    size: f32,
    color: Rgba,
) -> gpui::AnyElement {
    render_lucide_icon(icon, size, color, false)
}

fn colored_icon_path(icon: LucideIcon, color: Rgba, filled: bool) -> PathBuf {
    let cache_dir = resolved_lucide_cache_dir();
    let cache_name = format!(
        "{}_{:08x}{}.svg",
        icon.file_name(),
        rgba_cache_key(color),
        if filled { "_filled" } else { "" }
    );
    let cache_path = cache_dir.join(cache_name);
    if cache_path.exists() {
        return cache_path;
    }

    let Some(svg_source) = read_lucide_svg(icon.file_name()) else {
        return cache_path;
    };

    let tinted = tint_svg(&svg_source, color, filled);
    let _ = fs::create_dir_all(&cache_dir);
    let _ = fs::write(&cache_path, tinted);
    cache_path
}

fn resolved_lucide_cache_dir() -> PathBuf {
    if let Ok(mut guard) = COLORED_ICON_CACHE.lock() {
        if let Some(path) = guard.as_ref() {
            return path.clone();
        }
        let path = lucide_cache_dir();
        *guard = Some(path.clone());
        return path;
    }
    lucide_cache_dir()
}

fn read_lucide_svg(name: &str) -> Option<String> {
    for source_path in lucide_source_paths(name) {
        if source_path.exists() {
            if let Ok(text) = fs::read_to_string(&source_path) {
                return Some(text);
            }
        }
    }
    lucide_svg_embedded(name).map(str::to_string)
}

fn lucide_source_paths(name: &str) -> Vec<PathBuf> {
    let file_name = format!("{name}.svg");
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("data/lucide").join(&file_name));
    }
    paths.push(config_directory().join("lucide").join(&file_name));
    paths
}

fn rgba_cache_key(color: Rgba) -> u32 {
    let r = (color.r * 255.0).round() as u32;
    let g = (color.g * 255.0).round() as u32;
    let b = (color.b * 255.0).round() as u32;
    (r << 16) | (g << 8) | b
}

fn rgba_to_hex(color: Rgba) -> String {
    let r = (color.r * 255.0).round() as u8;
    let g = (color.g * 255.0).round() as u8;
    let b = (color.b * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn tint_svg(svg: &str, color: Rgba, filled: bool) -> String {
    let hex = rgba_to_hex(color);
    let mut tinted = svg.replace("currentColor", &hex);
    if filled {
        tinted = tinted.replace(r#"fill="none""#, &format!(r#"fill="{hex}""#));
    }
    tinted
}

macro_rules! lucide_embedded_icons {
    ($($name:literal),* $(,)?) => {
        fn lucide_svg_embedded(name: &str) -> Option<&'static str> {
            match name {
                $( $name => Some(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/lucide/", $name, ".svg"))), )*
                _ => None,
            }
        }
    };
}

lucide_embedded_icons!(
    "settings",
    "arrow-left",
    "zap",
    "calculator",
    "app-window",
    "file",
    "globe",
    "circle-question-mark",
    "sticky-note",
    "target",
    "clipboard",
    "columns-2",
    "text-quote",
    "link",
    "calendar",
    "monitor",
    "smile",
    "clock",
    "terminal",
    "heart",
    "heart-off",
    "trash-2",
    "folder-open",
    "refresh-cw",
    "file-cog",
    "keyboard",
    "command",
    "book-open",
    "search",
    "x",
    "play",
    "pause",
    "skip-forward",
    "skip-back",
    "music",
    "volume-2",
    "power",
    "chevron-down",
    "chevron-up",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_lucide_icons_exist() {
        assert!(lucide_svg_embedded("settings").is_some());
        assert!(lucide_svg_embedded("calculator").is_some());
    }

    #[test]
    fn tint_svg_replaces_current_color() {
        let tinted = tint_svg(
            r#"<svg stroke="currentColor"></svg>"#,
            Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            false,
        );
        assert!(tinted.contains("#ff0000"));
    }
}
