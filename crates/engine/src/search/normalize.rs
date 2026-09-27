use std::borrow::Cow;

/// Borrow canonical ASCII. Lowercase Unicode and fold final sigma for consistent prefix matching.
pub fn normalize(query: &str) -> Cow<'_, str> {
    let trimmed = query.trim();
    if is_canonical(trimmed) {
        return Cow::Borrowed(trimmed);
    }
    let mut result = String::with_capacity(trimmed.len());
    write_normalized(trimmed, &mut result);
    Cow::Owned(result)
}

/// [`normalize`] for every keystroke: text that needs changes is written into reused `buffer`.
pub fn normalize_into<'text>(query: &'text str, buffer: &'text mut String) -> &'text str {
    let trimmed = query.trim();
    if is_canonical(trimmed) {
        return trimmed;
    }
    write_normalized(trimmed, buffer);
    buffer
}

/// Lowercase ASCII words separated by single spaces.
fn is_canonical(trimmed: &str) -> bool {
    let mut previous_space = false;
    for character in trimmed.bytes() {
        let is_space = character == b' ';
        if !character.is_ascii()
            || character.is_ascii_uppercase()
            || ((character.is_ascii_whitespace() || character == b'\x0b') && !is_space)
            || (is_space && previous_space)
        {
            return false;
        }
        previous_space = is_space;
    }
    true
}

/// Per-character lowercasing equals `str::to_lowercase` apart from its final-sigma rule, and
/// every sigma is folded to `σ` anyway, so no intermediate lowercase copy is needed.
fn write_normalized(trimmed: &str, output: &mut String) {
    output.clear();
    for word in trimmed.split_whitespace() {
        if !output.is_empty() {
            output.push(' ');
        }
        for character in word.chars().flat_map(char::to_lowercase) {
            output.push(if character == 'ς' { 'σ' } else { character });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ascii_is_borrowed_and_unicode_case_is_preserved() {
        assert!(matches!(normalize("hello world"), Cow::Borrowed(_)));
        for input in [
            " HeLLo  WORLD ",
            "ΟΣ",
            "İ",
            "MÜNCHEN",
            "\u{2003}hello\u{2003}world",
        ] {
            assert_eq!(
                normalize(input),
                input
                    .to_lowercase()
                    .replace('ς', "σ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }

    #[test]
    fn reused_buffer_matches_owned_normalization() {
        let mut buffer = String::new();
        let canonical = "hello world";
        assert!(std::ptr::eq(
            normalize_into(canonical, &mut buffer),
            canonical
        ));
        for input in [
            " HeLLo  WORLD ",
            "ΟΣ",
            "ΣΊΣΥΦΟΣ ΟΣ",
            "İ",
            "MÜNCHEN",
            "\u{2003}hello\u{2003}world",
            "  ",
            "a",
        ] {
            assert_eq!(
                normalize_into(input, &mut buffer),
                normalize(input),
                "{input}"
            );
        }
    }

    #[test]
    fn every_ascii_whitespace_character_agrees_with_unicode_whitespace_rules() {
        for character in 0_u8..=127 {
            let query = format!("A{}B", character as char);
            assert_eq!(
                normalize(&query),
                query
                    .to_lowercase()
                    .replace('ς', "σ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
}
