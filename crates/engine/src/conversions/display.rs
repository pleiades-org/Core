//! Screen maths: `1920x1080` (aspect ratio), `16:9 1440 wide` (resize), `ppi 2560x1440 27in`.
use super::{
    format::{grouped, readable},
    quantity::parse_number,
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy · Esc to hide";
const ASPECT_WORDS: &[&str] = &["aspect", "ratio", "aspect ratio", "resolution"];
const DENSITY_WORDS: &[&str] = &["ppi", "dpi", "pixel density"];
/// Bare `WxH` is treated as a resolution only from this size, so `2x3` stays arithmetic-like.
const MIN_BARE_EDGE: u64 = 100;
const COMMON_RATIOS: &[(u64, u64)] = &[
    (16, 9),
    (16, 10),
    (4, 3),
    (21, 9),
    (32, 9),
    (3, 2),
    (5, 4),
    (1, 1),
    (9, 16),
];

pub fn calculate_display(input: &str) -> Outcome {
    let lower = input.trim().to_ascii_lowercase().replace('×', "x");
    let lower = lower.replace(" x ", "x");
    let words: Vec<&str> = lower.split_whitespace().collect();
    if let Some(outcome) = density(&words) {
        return Some(outcome);
    }
    if let Some(outcome) = resize(&words) {
        return Some(outcome);
    }
    let (resolution, rest): (Vec<&str>, Vec<&str>) = words
        .iter()
        .copied()
        .partition(|word| parse_resolution(word).is_some());
    let [resolution] = resolution.as_slice() else {
        return None;
    };
    let (width, height) = parse_resolution(resolution)?;
    let keyword = !rest.is_empty() && ASPECT_WORDS.contains(&rest.join(" ").as_str());
    let bare = rest.is_empty() && width >= MIN_BARE_EDGE && height >= MIN_BARE_EDGE;
    (keyword || bare).then(|| Ok(aspect(width, height)))
}

fn parse_resolution(text: &str) -> Option<(u64, u64)> {
    let (width, height) = text.split_once('x')?;
    let parse = |value: &str| {
        value
            .parse::<u64>()
            .ok()
            .filter(|value| (1..=1_000_000).contains(value))
    };
    Some((parse(width)?, parse(height)?))
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn aspect(width: u64, height: u64) -> Conversion {
    let divisor = gcd(width, height);
    let (ratio_width, ratio_height) = (width / divisor, height / divisor);
    let exact = format!("{ratio_width}:{ratio_height}");
    let decimal = width as f64 / height as f64;
    // Near-standard resolutions such as 1366x768 are labelled with the familiar ratio.
    let familiar = COMMON_RATIOS.iter().find(|(w, h)| {
        (*w, *h) != (ratio_width, ratio_height) && (decimal - *w as f64 / *h as f64).abs() < 0.01
    });
    let title = match familiar {
        Some((w, h)) => format!("≈ {w}:{h}"),
        None => exact.clone(),
    };
    let pixels = width * height;
    Conversion {
        answers: vec![
            Calculation {
                title: title.clone(),
                detail: format!(
                    "{width} × {height} · exactly {exact} · {}:1",
                    readable(decimal)
                ),
                copy: title.trim_start_matches("≈ ").to_owned(),
            },
            Calculation {
                title: format!("{} MP", readable(pixels as f64 / 1e6)),
                detail: format!("{} pixels", grouped(pixels as f64, 0)),
                copy: pixels.to_string(),
            },
        ],
        message: MESSAGE,
    }
}

/// `16:9 1440 wide`, `16:9 width 1440`, `16:9 1080 tall`, `16:9 height 1080`.
fn resize(words: &[&str]) -> Option<Result<Conversion, &'static str>> {
    let [ratio, rest @ ..] = words else {
        return None;
    };
    let (ratio_width, ratio_height) = ratio.split_once(':')?;
    let ratio_width = parse_number(ratio_width).filter(|value| *value > 0.)?;
    let ratio_height = parse_number(ratio_height).filter(|value| *value > 0.)?;
    let (size, is_width) = match rest {
        [size, side] | [side, size] if parse_number(size).is_some() => {
            let is_width = match *side {
                "wide" | "width" | "w" => true,
                "tall" | "high" | "height" | "h" => false,
                _ => return None,
            };
            (parse_number(size)?, is_width)
        }
        _ => return None,
    };
    if size <= 0. {
        return Some(Err("Use a positive width or height"));
    }
    let (width, height) = if is_width {
        (size, size * ratio_height / ratio_width)
    } else {
        (size * ratio_width / ratio_height, size)
    };
    let text = format!("{} × {}", readable(width), readable(height));
    Some(Ok(Conversion::single(
        Calculation {
            title: text,
            detail: format!(
                "{}:{} at {} {}",
                readable(ratio_width),
                readable(ratio_height),
                readable(size),
                if is_width { "wide" } else { "tall" }
            ),
            copy: format!("{}x{}", width.round(), height.round()),
        },
        MESSAGE,
    )))
}

/// `ppi 2560x1440 27in`, `2560x1440 27 inch ppi`, `27" 3840x2160 dpi`.
fn density(words: &[&str]) -> Option<Result<Conversion, &'static str>> {
    let joined = words.join(" ");
    let keyword = DENSITY_WORDS.iter().find(|word| joined.contains(**word))?;
    let rest = joined.replace(keyword, " ");
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let (width, height) = tokens.iter().find_map(|token| parse_resolution(token))?;
    let diagonal = tokens.iter().enumerate().find_map(|(index, token)| {
        let number = token
            .trim_end_matches(['"', '″'])
            .trim_end_matches("inches")
            .trim_end_matches("inch")
            .trim_end_matches("in");
        let follows = tokens
            .get(index + 1)
            .is_some_and(|next| matches!(*next, "in" | "inch" | "inches" | "\""));
        let marked = number.len() < token.len() || follows;
        marked.then(|| parse_number(number)).flatten()
    })?;
    if diagonal <= 0. {
        return Some(Err("Use a positive screen diagonal"));
    }
    let pixels_diagonal = ((width * width + height * height) as f64).sqrt();
    let ppi = pixels_diagonal / diagonal;
    let width_inches = width as f64 / ppi;
    let height_inches = height as f64 / ppi;
    Some(Ok(Conversion {
        answers: vec![
            Calculation {
                title: format!("{} PPI", readable(ppi)),
                detail: format!(
                    "{width} × {height} at {}\" · dot pitch {} mm",
                    readable(diagonal),
                    readable(25.4 / ppi)
                ),
                copy: readable(ppi).replace(',', ""),
            },
            Calculation {
                title: format!(
                    "{} × {} in",
                    readable(width_inches),
                    readable(height_inches)
                ),
                detail: format!(
                    "{} × {} cm visible area",
                    readable(width_inches * 2.54),
                    readable(height_inches * 2.54)
                ),
                copy: format!(
                    "{} x {} in",
                    readable(width_inches),
                    readable(height_inches)
                ),
            },
        ],
        message: MESSAGE,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        calculate_display(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn aspect_ratios_resizes_and_density() {
        assert_eq!(titles("1920x1080"), ["16:9", "2.074 MP"]);
        assert_eq!(titles("aspect 1366 x 768")[0], "≈ 16:9");
        assert_eq!(titles("3440×1440 ratio")[0], "43:18");
        assert_eq!(titles("16:9 1440 wide"), ["1,440 × 810"]);
        assert_eq!(titles("16:9 height 1080"), ["1,920 × 1,080"]);
        assert_eq!(titles("ppi 2560x1440 27in")[0], "108.8 PPI");
        assert_eq!(titles("27 inch 3840x2160 dpi")[0], "163.2 PPI");
    }

    #[test]
    fn small_products_and_other_text_are_not_resolutions() {
        for query in [
            "2x3",
            "10x",
            "x1080",
            "xbox",
            "16:9",
            "9pm et to uk",
            "1920x1080 extra words",
        ] {
            assert!(calculate_display(query).is_none(), "{query}");
        }
    }
}
