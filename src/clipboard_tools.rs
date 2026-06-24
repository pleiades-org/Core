use crate::{
    color_tools::{parse_hex_color, parse_hsl_color, parse_rgb_color, rgb_to_hex, rgb_to_hsl},
    command::{CommandCategory, CommandResult},
    search_text::normalize_search_text,
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn search_clipboard_color(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);

    if normalized_search_text.is_empty() {
        return vec![CommandResult::copyable_feature(
            "Clipboard color",
            "Read a color from clipboard text and convert formats",
            "clipboard color",
            CommandCategory::Clipboard,
            84,
        )];
    }

    execute_clipboard_color_query(&normalized_search_text)
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(query);
    if matches!(
        normalized.as_str(),
        "clipboard color" | "color from clipboard" | "clip color"
    ) {
        return execute_clipboard_color_query(&normalized);
    }

    if let Some(color_text) = normalized.strip_prefix("clipboard color ") {
        return parse_clipboard_color_text(color_text);
    }

    Vec::new()
}

fn execute_clipboard_color_query(_normalized: &str) -> Vec<CommandResult> {
    match read_clipboard_text() {
        Some(text) => parse_clipboard_color_text(&text),
        None => vec![CommandResult::informational(
            "Clipboard color",
            "Clipboard is empty or could not be read",
        )],
    }
}

pub fn parse_clipboard_color_text(text: &str) -> Vec<CommandResult> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![CommandResult::informational(
            "Clipboard color",
            "No color value found in clipboard",
        )];
    }

    let Some(rgb) = detect_color(trimmed) else {
        return vec![CommandResult::informational(
            "Clipboard color",
            &format!("Could not parse color from \"{trimmed}\""),
        )];
    };

    let color = crate::color_tools::RgbColor {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    };
    let hex = rgb_to_hex(color);
    let rgb_text = format!("rgb({}, {}, {})", rgb.r, rgb.g, rgb.b);
    let hsl = rgb_to_hsl(color);
    let hsl_text = format!(
        "hsl({}, {}%, {}%)",
        hsl.h.round() as i32,
        (hsl.s * 100.0).round() as i32,
        (hsl.l * 100.0).round() as i32
    );

    let combined = format!("{hex}\n{rgb_text}\n{hsl_text}");

    vec![CommandResult::copyable_feature(
        format!("Color {hex}"),
        format!("{rgb_text} · {hsl_text}"),
        combined,
        CommandCategory::Clipboard,
        90,
    )]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParsedColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub fn detect_color(text: &str) -> Option<ParsedColor> {
    let trimmed = text.trim();

    if let Some(rgb) = parse_hex_color(trimmed) {
        return Some(ParsedColor {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        });
    }

    if let Some(rgb) = parse_rgb_color(trimmed) {
        return Some(ParsedColor {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        });
    }

    if let Some(hsl) = parse_hsl_color(trimmed) {
        let rgb = crate::color_tools::hsl_to_rgb(hsl);
        return Some(ParsedColor {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        });
    }

    None
}

fn read_clipboard_text() -> Option<String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-Command",
        "Get-Clipboard -Format Text -ErrorAction SilentlyContinue",
    ]);
    command.stdin(Stdio::null());
    configure_no_console_window(&mut command);

    let child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout?;
    let text = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "windows")]
fn configure_no_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_no_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_hex_from_clipboard_text() {
        let results = parse_clipboard_color_text("#ff5500");
        assert_eq!(results.len(), 1);
        assert!(results[0].copy_text.contains("#ff5500"));
        assert!(results[0].copy_text.contains("rgb(255, 85, 0)"));
    }

    #[test]
    fn parses_rgb_from_clipboard_text() {
        let rgb = detect_color("rgb(255, 85, 0)").expect("rgb");
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 85);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn parses_hsl_from_clipboard_text() {
        let rgb = detect_color("hsl(20, 100%, 50%)").expect("hsl");
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 85);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn inline_clipboard_color_triggers() {
        let results = search_inline("clipboard color");
        assert!(!results.is_empty());
    }
}