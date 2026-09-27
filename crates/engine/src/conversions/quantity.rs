//! Number parsing shared by every conversion.

/// Parses a plain number: optional sign, optional `,` thousands groups, decimals and exponent.
/// Rejects malformed grouping such as `1,23` so a typo never becomes a different amount.
pub fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| b"0123456789.,+-eE".contains(&byte)) {
        return None;
    }
    let plain = if text.contains(',') {
        ungroup(text)?
    } else {
        text.to_owned()
    };
    plain.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Like [`parse_number`], also accepting `k`, `m`/`mm`, `b`/`bn` magnitude suffixes (`1.5k`).
/// Only for money and plain counts: in unit conversions `2m` means metres.
pub fn parse_scaled_number(text: &str) -> Option<f64> {
    let lower = text.trim().to_ascii_lowercase();
    for (suffix, factor) in [("bn", 1e9), ("mm", 1e6), ("k", 1e3), ("m", 1e6), ("b", 1e9)] {
        if let Some(number) = lower.strip_suffix(suffix) {
            return parse_number(number)
                .map(|value| value * factor)
                .filter(|value| value.is_finite());
        }
    }
    parse_number(&lower)
}

pub(super) fn ungroup(text: &str) -> Option<String> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' | b'-' => text.split_at(1),
        _ => ("", text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let mut groups = whole.split(',');
    let first = groups.next()?;
    if first.is_empty() || first.len() > 3 || !first.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut plain = format!("{sign}{first}");
    for group in groups {
        if group.len() != 3 || !group.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        plain.push_str(group);
    }
    if fraction.contains(',') {
        return None;
    }
    if !fraction.is_empty() || digits.contains('.') {
        plain.push('.');
        plain.push_str(fraction);
    }
    Some(plain)
}

/// Splits a token such as `4.7GB` into its number and unit text. The unit may be empty.
pub fn split_number_prefix(token: &str) -> Option<(&str, &str)> {
    let first = *token.as_bytes().first()?;
    if !(first.is_ascii_digit() || b"+-.".contains(&first)) {
        return None;
    }
    let mut end = 0;
    let bytes = token.as_bytes();
    while end < bytes.len() {
        let byte = bytes[end];
        let exponent = matches!(byte, b'e' | b'E')
            && bytes
                .get(end + 1)
                .is_some_and(|next| next.is_ascii_digit() || b"+-".contains(next))
            && end > 0;
        let signed_exponent =
            b"+-".contains(&byte) && end > 0 && matches!(bytes[end - 1], b'e' | b'E');
        if byte.is_ascii_digit()
            || b".,".contains(&byte)
            || exponent
            || signed_exponent
            || (end == 0 && b"+-".contains(&byte))
        {
            end += 1;
        } else {
            break;
        }
    }
    Some(token.split_at(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_accept_grouping_signs_and_exponents_only_when_well_formed() {
        for (text, expected) in [
            ("42", 42.),
            ("-3.5", -3.5),
            ("1,234,567.25", 1_234_567.25),
            ("1e3", 1000.),
            ("+2.5E-2", 0.025),
            (".5", 0.5),
        ] {
            assert_eq!(parse_number(text), Some(expected), "{text}");
        }
        for text in [
            "", "1,23", "12,3456", "1.2.3", "abc", "1e309", ",100", "1,000,00",
        ] {
            assert_eq!(parse_number(text), None, "{text}");
        }
    }

    #[test]
    fn magnitude_suffixes_scale_counts() {
        assert_eq!(parse_scaled_number("1.5k"), Some(1500.));
        assert_eq!(parse_scaled_number("2M"), Some(2e6));
        assert_eq!(parse_scaled_number("3bn"), Some(3e9));
        assert_eq!(parse_scaled_number("250"), Some(250.));
        assert_eq!(parse_scaled_number("k"), None);
    }

    #[test]
    fn number_prefixes_split_from_attached_units() {
        assert_eq!(split_number_prefix("4.7GB"), Some(("4.7", "GB")));
        assert_eq!(split_number_prefix("1e3km"), Some(("1e3", "km")));
        assert_eq!(split_number_prefix("100"), Some(("100", "")));
        assert_eq!(split_number_prefix("-40C"), Some(("-40", "C")));
        assert_eq!(split_number_prefix("1,000mb"), Some(("1,000", "mb")));
        assert_eq!(split_number_prefix("GB"), None);
    }
}
