use std::borrow::Cow;

pub fn uncached(query: &str) -> String {
    query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Borrow already-normalized ASCII; use the original algorithm for all other input.
pub fn borrow_when_normalized(query: &str) -> Cow<'_, str> {
    let trimmed = query.trim();
    let mut previous_space = false;
    for character in trimmed.bytes() {
        let is_space = character == b' ';
        if !character.is_ascii()
            || character.is_ascii_uppercase()
            || ((character.is_ascii_whitespace() || character == b'\x0b') && !is_space)
            || (is_space && previous_space)
        {
            return Cow::Owned(uncached(trimmed));
        }
        previous_space = is_space;
    }
    Cow::Borrowed(trimmed)
}

/// Reuses caller-owned capacity for ASCII; preserves whole-string Unicode lowercasing.
pub fn normalize_reused<'buffer>(query: &str, buffer: &'buffer mut String) -> &'buffer str {
    buffer.clear();
    if !query.is_ascii() {
        buffer.push_str(&uncached(query));
        return buffer;
    }
    let mut pending_space = false;
    for character in query.bytes() {
        if character.is_ascii_whitespace() || character == b'\x0b' {
            pending_space = !buffer.is_empty();
            continue;
        }
        if pending_space {
            buffer.push(' ');
            pending_space = false;
        }
        buffer.push(character.to_ascii_lowercase() as char);
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ascii_byte_preserves_unicode_whitespace_normalization() {
        let mut buffer = String::new();
        for character in 0_u8..=127 {
            let input = format!("A{}B", character as char);
            assert_eq!(borrow_when_normalized(&input), uncached(&input));
            assert_eq!(normalize_reused(&input, &mut buffer), uncached(&input));
        }
    }

    #[test]
    fn borrowed_normalization_retains_unicode_and_whitespace_semantics() {
        let mut reused = String::new();
        for input in [
            "",
            "hello world",
            " HeLLo  WORLD ",
            "\tfoo\nbar\r",
            "ΟΣ",
            "İ",
            "MÜNCHEN",
            "\u{2003}hello\u{2003}world",
        ] {
            assert_eq!(borrow_when_normalized(input), uncached(input));
            assert_eq!(normalize_reused(input, &mut reused), uncached(input));
        }
    }
}
