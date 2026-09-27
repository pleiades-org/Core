//! Display formatting shared by conversions. Copied text never contains grouping separators.
pub use crate::calculator::format_number;

/// Fixed decimals with `,` thousands separators, e.g. `1,234.50`.
pub fn grouped(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut output = String::with_capacity(text.len() + whole.len() / 3 + 1);
    // Rounded-away negatives such as -0.001 at 2 decimals print without a sign.
    if value < 0. && text.bytes().any(|byte| (b'1'..=b'9').contains(&byte)) {
        output.push('-');
    }
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(digit);
    }
    if !fraction.is_empty() {
        output.push('.');
        output.push_str(fraction);
    }
    output
}

/// Plain fixed decimals for copying, e.g. `1234.50`; never `-0.00`.
pub fn plain(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text
        .trim_start_matches('-')
        .bytes()
        .all(|byte| byte == b'0' || byte == b'.')
    {
        text.trim_start_matches('-').to_owned()
    } else {
        text
    }
}

/// Rounds to `digits` significant figures, e.g. 133.3333 → 133.3 for 4 digits.
pub fn significant(value: f64, digits: i32) -> f64 {
    if value == 0. || !value.is_finite() {
        return value;
    }
    let magnitude = value.abs().log10().floor() as i32;
    let exponent = digits - 1 - magnitude;
    // Powers of ten above 1e22 are inexact, so tiny values would round to noise such as
    // 1.0000000000000001e-303; decimal formatting rounds them exactly instead.
    if exponent > 22 {
        let precision = (digits - 1).max(0) as usize;
        return format!("{value:.precision$e}").parse().unwrap_or(value);
    }
    let factor = 10_f64.powi(exponent);
    if !factor.is_finite() {
        return value;
    }
    (value * factor).round() / factor
}

/// A readable approximate number: 4 significant figures, grouped, trailing zeros removed.
pub fn readable(value: f64) -> String {
    let rounded = significant(value, 4);
    // log10(0) is -∞; zero has no magnitude to size the decimals by.
    if rounded == 0. {
        return "0".into();
    }
    if rounded.abs() >= 1e15 || rounded.abs() < 1e-4 {
        return format_number(rounded);
    }
    let decimals = (3 - rounded.abs().log10().floor() as i32).clamp(0, 6) as usize;
    let text = grouped(rounded, decimals);
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Human duration with at most three parts: `2 h 13 min 20 s`, `3 days 4 h`, `0.35 s`.
pub fn duration(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "∞".into();
    }
    if seconds < 1. {
        return format!("{} s", readable(seconds));
    }
    let mut remaining = seconds.round() as u64;
    // Average Gregorian year; each later part uses what the previous part left over.
    let parts = [
        (31_556_952, "year", "years"),
        (86_400, "day", "days"),
        (3_600, "h", "h"),
        (60, "min", "min"),
        (1, "s", "s"),
    ]
    .map(|(size, one, many)| {
        let amount = remaining / size;
        remaining %= size;
        (amount, one, many)
    });
    let first = parts
        .iter()
        .position(|(amount, ..)| *amount > 0)
        .unwrap_or(4);
    parts[first..]
        .iter()
        .take(3)
        .filter(|(amount, ..)| *amount > 0)
        .map(|(amount, one, many)| format!("{amount} {}", if *amount == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_rounding_and_readable_numbers() {
        assert_eq!(grouped(1_234_567.891, 2), "1,234,567.89");
        assert_eq!(grouped(-1234.6, 0), "-1,235");
        assert_eq!(grouped(999., 0), "999");
        assert_eq!(grouped(-0.001, 2), "0.00");
        assert_eq!(plain(-0.001, 2), "0.00");
        assert_eq!(plain(12.345, 2), "12.35");
        assert_eq!(significant(133.3333, 4), 133.3);
        for value in [1e-300, -1e-300, f64::from_bits(1)] {
            assert_eq!(significant(value, 12), value);
            assert_ne!(readable(value), "NaN");
        }
        assert_eq!(readable(133.3333), "133.3");
        assert_eq!(readable(1_234_567.), "1,235,000");
        assert_eq!(readable(0.012345), "0.01235");
        assert_eq!(readable(2.), "2");
        assert_eq!(readable(0.), "0");
        assert_eq!(readable(-0.0), "0");
    }

    #[test]
    fn durations_use_at_most_three_parts() {
        assert_eq!(duration(800.), "13 min 20 s");
        assert_eq!(duration(7_380.), "2 h 3 min");
        assert_eq!(duration(90_061.), "1 day 1 h 1 min");
        assert_eq!(duration(0.25), "0.25 s");
        assert_eq!(duration(3.), "3 s");
        assert_eq!(duration(40_000_000.), "1 year 97 days 17 h");
    }
}
