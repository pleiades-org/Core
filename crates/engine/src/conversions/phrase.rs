//! Tokens for sentence-like questions (`20% off 80`, `tip 15% on 80 split 4`).
use super::quantity::parse_scaled_number;

#[derive(Clone, Debug, PartialEq)]
pub enum Token<'text> {
    Number(f64),
    Percent(f64),
    Word(&'text str),
}

impl Token<'_> {
    pub fn is(&self, word: &str) -> bool {
        matches!(self, Token::Word(text) if *text == word)
    }
}

/// Lowercases, joins `percent`/`per cent`/`%` onto numbers, and drops currency symbols and `?`.
/// Words borrow the lowercased text kept in `buffer`, so one call serves every converter.
/// `None` when a token looks like a percentage but is not a number.
pub fn tokenize<'text>(input: &str, buffer: &'text mut String) -> Option<Vec<Token<'text>>> {
    *buffer = input.to_lowercase();
    if buffer.contains("per") {
        *buffer = buffer
            .replace("per cent", "%")
            .replace("percentage", "%")
            .replace("percent", "%");
    }
    if buffer.contains('?') {
        buffer.retain(|character| character != '?');
    }
    let text: &'text str = buffer;
    let mut tokens: Vec<Token> = Vec::new();
    for raw in text.split_whitespace() {
        let raw = raw.trim_start_matches(['$', '£', '€']);
        if raw == "%" {
            if let Some(&Token::Number(value)) = tokens.last() {
                *tokens.last_mut().expect("last token") = Token::Percent(value);
            } else {
                tokens.push(Token::Word("%"));
            }
        } else if let Some(number) = raw.strip_suffix('%') {
            tokens.push(Token::Percent(parse_scaled_number(number)?));
        } else if let Some(number) = parse_scaled_number(raw) {
            tokens.push(Token::Number(number));
        } else if raw == "->" || raw == "→" {
            tokens.push(Token::Word("to"));
        } else if !raw.is_empty() {
            tokens.push(Token::Word(raw));
        }
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::{Token::*, *};

    #[test]
    fn percent_words_join_numbers_and_symbols_are_dropped() {
        let mut text = String::new();
        assert_eq!(
            tokenize("Tip 15 percent on $80?", &mut text).unwrap(),
            [Word("tip"), Percent(15.), Word("on"), Number(80.)]
        );
        assert_eq!(
            tokenize("what % of 1.2k", &mut text).unwrap(),
            [Word("what"), Word("%"), Word("of"), Number(1_200.)]
        );
        assert_eq!(tokenize("abc%", &mut text), None);
    }
}
