use crate::{
    command::{CommandCategory, CommandResult},
    search_text::normalize_search_text,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HslColor {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

pub fn search_color_tools(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    if normalized_search_text.is_empty() {
        return color_catalog();
    }

    let mut results = execute_color_query(search_text);
    results.extend(
        color_catalog()
            .into_iter()
            .filter(|result| {
                normalize_search_text(&result.title).contains(&normalized_search_text)
                    || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
            }),
    );
    results
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    execute_color_query(query)
}

fn color_catalog() -> Vec<CommandResult> {
    vec![
        hint_result(
            "#ff5500 to rgb",
            "Convert hex color to rgb(...)",
            "#ff5500 to rgb",
            84,
        ),
        hint_result(
            "rgb(255,85,0) to hex",
            "Convert rgb to #RRGGBB",
            "rgb(255,85,0) to hex",
            83,
        ),
        hint_result(
            "hsl(20, 100%, 50%) to hex",
            "Convert hsl to hex",
            "hsl(20, 100%, 50%) to hex",
            82,
        ),
    ]
}

fn hint_result(title: &str, subtitle: &str, copy_text: &str, confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(
        title,
        subtitle,
        copy_text,
        CommandCategory::DevTools,
        confidence,
    )
}

fn execute_color_query(query: &str) -> Vec<CommandResult> {
    if let Some(result) = try_color_conversion(query) {
        return vec![result];
    }

    Vec::new()
}

fn try_color_conversion(query: &str) -> Option<CommandResult> {
    let trimmed = query.trim();
    let normalized = normalize_search_text(trimmed);

    if normalized.ends_with(" to rgb") {
        let source = trimmed[..trimmed.len() - " to rgb".len()].trim();
        if let Some(rgb) = parse_hex_color(source) {
            let copy_text = format!("rgb({}, {}, {})", rgb.r, rgb.g, rgb.b);
            return Some(copy_result(
                copy_text.clone(),
                format!("Hex {source}"),
                copy_text,
                90,
            ));
        }
    }

    if normalized.ends_with(" to hex") {
        let source = trimmed[..trimmed.len() - " to hex".len()].trim();
        if let Some(rgb) = parse_rgb_color(source).or_else(|| parse_hsl_color(source).map(hsl_to_rgb)) {
            let copy_text = rgb_to_hex(rgb);
            return Some(copy_result(
                copy_text.clone(),
                format!("From {source}"),
                copy_text,
                90,
            ));
        }
    }

    if normalized.ends_with(" to hsl") {
        let source = trimmed[..trimmed.len() - " to hsl".len()].trim();
        if let Some(rgb) = parse_hex_color(source).or_else(|| parse_rgb_color(source)) {
            let hsl = rgb_to_hsl(rgb);
            let copy_text = format!(
                "hsl({}, {}%, {}%)",
                hsl.h.round() as i32,
                (hsl.s * 100.0).round() as i32,
                (hsl.l * 100.0).round() as i32
            );
            return Some(copy_result(
                copy_text.clone(),
                format!("From {source}"),
                copy_text,
                88,
            ));
        }
    }

    None
}

fn copy_result(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    copy_text: impl Into<String>,
    confidence: u8,
) -> CommandResult {
    CommandResult::copyable_feature(title, subtitle, copy_text, CommandCategory::DevTools, confidence)
}

pub fn parse_hex_color(value: &str) -> Option<RgbColor> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(RgbColor { r, g, b })
}

pub fn parse_rgb_color(value: &str) -> Option<RgbColor> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(trimmed);
    let parts: Vec<_> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }

    let r = parts[0].parse().ok()?;
    let g = parts[1].parse().ok()?;
    let b = parts[2].parse().ok()?;
    Some(RgbColor { r, g, b })
}

pub fn parse_hsl_color(value: &str) -> Option<HslColor> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix("hsl(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(trimmed);
    let parts: Vec<_> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }

    let h = parts[0].trim_end_matches("deg").parse::<f64>().ok()?;
    let s = parse_percentage(parts[1])?;
    let l = parse_percentage(parts[2])?;
    Some(HslColor { h, s, l })
}

fn parse_percentage(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if let Some(number) = trimmed.strip_suffix('%') {
        let percent = number.parse::<f64>().ok()?;
        return Some((percent / 100.0).clamp(0.0, 1.0));
    }

    let number = trimmed.parse::<f64>().ok()?;
    if number > 1.0 {
        Some((number / 100.0).clamp(0.0, 1.0))
    } else {
        Some(number.clamp(0.0, 1.0))
    }
}

pub fn rgb_to_hex(color: RgbColor) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

pub fn hsl_to_rgb(color: HslColor) -> RgbColor {
    let h = color.h / 360.0;
    let s = color.s;
    let l = color.l;

    if s == 0.0 {
        let channel = (l * 255.0).round() as u8;
        return RgbColor {
            r: channel,
            g: channel,
            b: channel,
        };
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    RgbColor {
        r: (hue_to_rgb(p, q, h + 1.0 / 3.0) * 255.0).round() as u8,
        g: (hue_to_rgb(p, q, h) * 255.0).round() as u8,
        b: (hue_to_rgb(p, q, h - 1.0 / 3.0) * 255.0).round() as u8,
    }
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }

    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

pub fn rgb_to_hsl(color: RgbColor) -> HslColor {
    let r = color.r as f64 / 255.0;
    let g = color.g as f64 / 255.0;
    let b = color.b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let mut h = 0.0;
    if delta != 0.0 {
        h = if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
    }
    if h < 0.0 {
        h += 360.0;
    }

    let l = (max + min) / 2.0;
    let s = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs())
    };

    HslColor { h, s, l }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_hex_to_rgb() {
        let rgb = parse_hex_color("#ff5500").expect("rgb");
        assert_eq!(rgb, RgbColor { r: 255, g: 85, b: 0 });
    }

    #[test]
    fn converts_rgb_to_hex() {
        assert_eq!(
            rgb_to_hex(RgbColor {
                r: 255,
                g: 85,
                b: 0
            }),
            "#ff5500"
        );
    }

    #[test]
    fn converts_hsl_to_rgb() {
        let rgb = hsl_to_rgb(HslColor {
            h: 20.0,
            s: 1.0,
            l: 0.5,
        });
        assert_eq!(rgb, RgbColor { r: 255, g: 85, b: 0 });
    }

    #[test]
    fn inline_hex_to_rgb_query() {
        let results = search_inline("#ff5500 to rgb");
        assert_eq!(results[0].copy_text, "rgb(255, 85, 0)");
    }

    #[test]
    fn scoped_color_catalog_filters() {
        let results = search_color_tools("hex");
        assert!(!results.is_empty());
    }
}