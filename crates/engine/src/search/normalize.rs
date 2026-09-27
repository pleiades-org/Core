use std::borrow::Cow;

/// Borrow canonical ASCII. Lowercase Unicode and fold final sigma for consistent prefix matching.
pub fn normalize(query: &str) -> Cow<'_, str> {
    let trimmed = query.trim();
    let mut previous_space = false;
    for character in trimmed.bytes() {
        let is_space = character == b' ';
        if !character.is_ascii()
            || character.is_ascii_uppercase()
            || ((character.is_ascii_whitespace() || character == b'\x0b') && !is_space)
            || (is_space && previous_space)
        {
            let lowercase = trimmed.to_lowercase();
            let mut result = String::with_capacity(lowercase.len());
            for word in lowercase.split_whitespace() {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.extend(
                    word.chars()
                        .map(|character| if character == 'ς' { 'σ' } else { character }),
                );
            }
            return Cow::Owned(result);
        }
        previous_space = is_space;
    }
    Cow::Borrowed(trimmed)
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
