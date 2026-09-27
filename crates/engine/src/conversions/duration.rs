//! Durations written the way people type them: `90 min`, `1h 30m`, `1:30:00`, `25:00`.
use super::quantity::parse_number;

const UNITS: &[(&[&str], f64)] = &[
    (&["ms", "millisecond", "milliseconds"], 0.001),
    (&["s", "sec", "secs", "second", "seconds"], 1.),
    (&["m", "min", "mins", "minute", "minutes"], 60.),
    (&["h", "hr", "hrs", "hour", "hours"], 3_600.),
    (&["d", "day", "days"], 86_400.),
    (&["w", "wk", "week", "weeks"], 604_800.),
];

/// Seconds for a whole duration expression, or `None` if any part is not a duration.
pub fn parse_duration(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.contains(':') {
        return parse_clock(text);
    }
    let mut total = 0.;
    let mut pending: Option<f64> = None;
    let mut matched = false;
    for token in text.split_whitespace() {
        let mut rest = token;
        while !rest.is_empty() {
            let digits = rest
                .find(|character: char| !character.is_ascii_digit() && character != '.')
                .unwrap_or(rest.len());
            if digits > 0 {
                if pending.is_some() {
                    return None;
                }
                pending = Some(parse_number(&rest[..digits])?);
                rest = &rest[digits..];
                continue;
            }
            let letters = rest
                .find(|character: char| character.is_ascii_digit() || character == '.')
                .unwrap_or(rest.len());
            let seconds = unit_seconds(&rest[..letters])?;
            total += pending.take()? * seconds;
            matched = true;
            rest = &rest[letters..];
        }
    }
    (matched && pending.is_none() && total.is_finite() && total > 0.).then_some(total)
}

fn unit_seconds(name: &str) -> Option<f64> {
    UNITS
        .iter()
        .find(|(aliases, _)| aliases.iter().any(|alias| name.eq_ignore_ascii_case(alias)))
        .map(|(_, seconds)| *seconds)
}

/// `h:mm:ss` or `mm:ss`.
fn parse_clock(text: &str) -> Option<f64> {
    let parts: Vec<_> = text.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
        })
    {
        return None;
    }
    let values: Vec<f64> = parts
        .iter()
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    if values[1..].iter().any(|value| *value >= 60.) {
        return None;
    }
    let seconds = values.iter().fold(0., |total, value| total * 60. + value);
    (seconds > 0.).then_some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_units_compounds_and_clock_forms() {
        for (text, expected) in [
            ("90 min", 5_400.),
            ("1h 30m", 5_400.),
            ("1h30min", 5_400.),
            ("1 hour 30 minutes", 5_400.),
            ("2 days", 172_800.),
            ("1:30:00", 5_400.),
            ("25:00", 1_500.),
            ("0.5 h", 1_800.),
            ("45s", 45.),
        ] {
            assert_eq!(parse_duration(text), Some(expected), "{text}");
        }
        for text in ["", "min", "10", "10 GB", "1:75", "5 km", "1h 30", "0 min"] {
            assert_eq!(parse_duration(text), None, "{text}");
        }
    }
}
