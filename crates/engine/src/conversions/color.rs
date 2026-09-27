//! Colour notation: `#ff8800`, `#f80`, `rgb(255, 136, 0)`, `hsl(32, 100%, 50%)`, optionally
//! followed by `to hex|rgb|hsl` to put that form first.
use super::{Conversion, Outcome};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy colour · Esc to hide";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Notation {
    Hex,
    Rgb,
    Hsl,
}

pub fn convert_color(input: &str) -> Outcome {
    let lower = input.trim().to_ascii_lowercase();
    let (color_text, target) = match lower
        .rsplit_once(" to ")
        .or_else(|| lower.rsplit_once(" in "))
    {
        Some((color, target)) => (color.trim(), Some(parse_notation(target.trim())?)),
        None => (lower.as_str(), None),
    };
    let (rgb, source) = parse_color(color_text)?;
    let Ok(rgb) = rgb else {
        return Some(Err(
            "Colour channels must be 0–255 (RGB) or 0–360 and 0–100% (HSL)",
        ));
    };
    let mut order = vec![Notation::Hex, Notation::Rgb, Notation::Hsl];
    if let Some(target) = target {
        order.retain(|notation| *notation != target);
        order.insert(0, target);
    } else {
        // Without a target, the input's own notation moves last.
        order.retain(|notation| *notation != source);
        order.push(source);
    }
    let answers = order
        .into_iter()
        .map(|notation| answer(rgb, notation))
        .collect();
    Some(Ok(Conversion {
        answers,
        message: MESSAGE,
    }))
}

fn parse_notation(text: &str) -> Option<Notation> {
    match text {
        "hex" | "html" | "css" => Some(Notation::Hex),
        "rgb" => Some(Notation::Rgb),
        "hsl" => Some(Notation::Hsl),
        _ => None,
    }
}

type Rgb = [u8; 3];

/// Recognised colour text, with `Err` when the notation is right but a channel is out of range.
fn parse_color(text: &str) -> Option<(Result<Rgb, ()>, Notation)> {
    if let Some(hex) = text.strip_prefix('#') {
        return parse_hex(hex).map(|rgb| (Ok(rgb), Notation::Hex));
    }
    let (notation, arguments) = if let Some(rest) = text.strip_prefix("rgb") {
        (Notation::Rgb, rest)
    } else if let Some(rest) = text.strip_prefix("hsl") {
        (Notation::Hsl, rest)
    } else {
        return None;
    };
    let arguments = arguments.trim();
    let arguments = arguments
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(arguments);
    let values: Vec<f64> = arguments
        .split([',', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_end_matches(['%', '°']).parse().ok())
        .collect::<Option<_>>()?;
    let [first, second, third] = values.as_slice() else {
        return None;
    };
    let rgb = match notation {
        Notation::Rgb => [*first, *second, *third]
            .iter()
            .all(|channel| (0. ..=255.).contains(channel))
            .then(|| [*first, *second, *third].map(|channel| channel.round() as u8)),
        _ => ((0. ..=360.).contains(first)
            && (0. ..=100.).contains(second)
            && (0. ..=100.).contains(third))
        .then(|| hsl_to_rgb(*first, *second / 100., *third / 100.)),
    };
    Some((rgb.ok_or(()), notation))
}

fn parse_hex(hex: &str) -> Option<Rgb> {
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expanded: String = match hex.len() {
        3 => hex.chars().flat_map(|digit| [digit, digit]).collect(),
        6 => hex.to_owned(),
        _ => return None,
    };
    let channel = |index: usize| u8::from_str_radix(&expanded[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn answer(rgb: Rgb, notation: Notation) -> Calculation {
    let [red, green, blue] = rgb;
    let text = match notation {
        Notation::Hex => format!("#{red:02X}{green:02X}{blue:02X}"),
        Notation::Rgb => format!("rgb({red}, {green}, {blue})"),
        Notation::Hsl => {
            let (hue, saturation, lightness) = rgb_to_hsl(rgb);
            format!("hsl({hue}, {saturation}%, {lightness}%)")
        }
    };
    Calculation {
        title: text.clone(),
        detail: match notation {
            Notation::Hex => "HEX · CSS and design tools",
            Notation::Rgb => "RGB · red, green, blue 0–255",
            Notation::Hsl => "HSL · hue°, saturation, lightness",
        }
        .into(),
        copy: text,
    }
}

fn rgb_to_hsl([red, green, blue]: Rgb) -> (u32, u32, u32) {
    let [red, green, blue] = [red, green, blue].map(|channel| f64::from(channel) / 255.);
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.;
    let delta = max - min;
    if delta == 0. {
        return (0, 0, (lightness * 100.).round() as u32);
    }
    let saturation = delta / (1. - (2. * lightness - 1.).abs());
    let hue = if max == red {
        60. * ((green - blue) / delta).rem_euclid(6.)
    } else if max == green {
        60. * ((blue - red) / delta + 2.)
    } else {
        60. * ((red - green) / delta + 4.)
    };
    (
        hue.round() as u32 % 360,
        (saturation * 100.).round() as u32,
        (lightness * 100.).round() as u32,
    )
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> Rgb {
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let sector = hue / 60.;
    let second = chroma * (1. - (sector.rem_euclid(2.) - 1.).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.),
        1 => (second, chroma, 0.),
        2 => (0., chroma, second),
        3 => (0., second, chroma),
        4 => (second, 0., chroma),
        _ => (chroma, 0., second),
    };
    let offset = lightness - chroma / 2.;
    [red, green, blue].map(|channel| ((channel + offset) * 255.).round().clamp(0., 255.) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        convert_color(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn colours_convert_between_notations() {
        assert_eq!(
            titles("#ff8800"),
            ["rgb(255, 136, 0)", "hsl(32, 100%, 50%)", "#FF8800"]
        );
        assert_eq!(titles("#f80")[0], "rgb(255, 136, 0)");
        assert_eq!(titles("rgb(255, 136, 0) to hex")[0], "#FF8800");
        assert_eq!(titles("rgb 18 18 18 to hsl")[0], "hsl(0, 0%, 7%)");
        assert_eq!(titles("hsl(210, 50%, 40%) to rgb")[0], "rgb(51, 102, 153)");
        assert_eq!(titles("hsl 0 100 50")[0], "#FF0000");
    }

    #[test]
    fn invalid_colours_and_other_text() {
        assert!(convert_color("rgb(300, 0, 0)").unwrap().is_err());
        assert!(convert_color("hsl(10, 150%, 50%)").unwrap().is_err());
        for query in ["#zzzzzz", "#1234", "rgb", "#ff8800 to cmyk", "xbox", "hsl"] {
            assert!(convert_color(query).is_none(), "{query}");
        }
    }
}
