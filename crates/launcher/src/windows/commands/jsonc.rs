//! A small JSON parser that accepts the comments and trailing commas Windows Terminal allows in
//! `settings.json`. Only used to read the default terminal profile.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// Deep structures are rejected rather than risking a stack overflow on a hostile file.
const MAX_DEPTH: usize = 64;

pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
        text,
    };
    let value = parser.value(0)?;
    parser.skip_trivia()?;
    if parser.position != parser.bytes.len() {
        return Err(format!("unexpected text at byte {}", parser.position));
    }
    Ok(value)
}

struct Parser<'text> {
    text: &'text str,
    bytes: &'text [u8],
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn error(&self, what: &str) -> String {
        format!("{what} at byte {}", self.position)
    }

    fn skip_trivia(&mut self) -> Result<(), String> {
        loop {
            match (self.peek(), self.bytes.get(self.position + 1)) {
                (Some(byte), _) if byte.is_ascii_whitespace() => self.position += 1,
                (Some(b'/'), Some(b'/')) => {
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.position += 1;
                    }
                }
                (Some(b'/'), Some(b'*')) => {
                    let end = self.text[self.position + 2..]
                        .find("*/")
                        .ok_or_else(|| self.error("unterminated comment"))?;
                    self.position += end + 4;
                }
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        self.skip_trivia()?;
        match self.peek().ok_or_else(|| self.error("unexpected end"))? {
            b'{' => self
                .sequence(b'}', depth, |parser, depth| {
                    let key = match parser.value(depth)? {
                        Value::Text(key) => key,
                        _ => return Err(parser.error("object keys must be strings")),
                    };
                    parser.skip_trivia()?;
                    if parser.peek() != Some(b':') {
                        return Err(parser.error("expected ':'"));
                    }
                    parser.position += 1;
                    Ok(Some((key, parser.value(depth)?)))
                })
                .map(Value::Object),
            b'[' => self
                .sequence(b']', depth, |parser, depth| Ok(Some(parser.value(depth)?)))
                .map(Value::Array),
            b'"' => self.string().map(Value::Text),
            _ => self.literal(),
        }
    }

    /// `{…}` or `[…]` items separated by commas, allowing a trailing comma.
    fn sequence<T>(
        &mut self,
        close: u8,
        depth: usize,
        mut item: impl FnMut(&mut Self, usize) -> Result<Option<T>, String>,
    ) -> Result<Vec<T>, String> {
        self.position += 1;
        let mut items = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(close) {
                self.position += 1;
                return Ok(items);
            }
            items.extend(item(self, depth + 1)?);
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(byte) if byte == close => {}
                _ => return Err(self.error("expected ',' or closing bracket")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.position += 1;
        let mut output = String::new();
        loop {
            let rest = &self.text[self.position..];
            let stop = rest
                .find(['"', '\\'])
                .ok_or_else(|| self.error("unterminated string"))?;
            output.push_str(&rest[..stop]);
            self.position += stop;
            if self.peek() == Some(b'"') {
                self.position += 1;
                return Ok(output);
            }
            let escape = *self
                .bytes
                .get(self.position + 1)
                .ok_or_else(|| self.error("unterminated escape"))?;
            self.position += 2;
            output.push(match escape {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let digits = self
                        .text
                        .get(self.position..self.position + 4)
                        .ok_or_else(|| self.error("short unicode escape"))?;
                    self.position += 4;
                    let unit = u32::from_str_radix(digits, 16)
                        .map_err(|_| self.error("bad unicode escape"))?;
                    // Surrogate pairs are not needed for profile commands; replace them.
                    char::from_u32(unit).unwrap_or('\u{fffd}')
                }
                _ => return Err(self.error("unknown escape")),
            });
        }
    }

    fn literal(&mut self) -> Result<Value, String> {
        let rest = &self.text[self.position..];
        let end = rest
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || "+-.".contains(character))
            })
            .unwrap_or(rest.len());
        let word = &rest[..end];
        self.position += end;
        match word {
            "null" => Ok(Value::Null),
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => word
                .parse()
                .map(Value::Number)
                .map_err(|_| self.error("unexpected value")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_settings_with_comments_and_trailing_commas_parse() {
        let value = parse(
            r#"// Windows Terminal settings
            {
                "defaultProfile": "{574e775e-4f2a-5b96-ac1e-a2962a402336}", /* PowerShell 7 */
                "profiles": { "list": [
                    { "guid": "{0}", "commandline": "%SystemRoot%\\System32\\cmd.exe", "hidden": false, },
                    { "guid": "{1}", "source": "Windows.Terminal.Wsl", "name": "Ubuntu A" },
                ], },
                "number": -1.5e2,
                "nothing": null,
            }"#,
        )
        .unwrap();
        assert_eq!(
            value.get("defaultProfile").and_then(Value::as_str),
            Some("{574e775e-4f2a-5b96-ac1e-a2962a402336}")
        );
        let list = value
            .get("profiles")
            .and_then(|profiles| profiles.get("list"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(
            list[0].get("commandline").and_then(Value::as_str),
            Some("%SystemRoot%\\System32\\cmd.exe")
        );
        assert_eq!(
            list[1].get("name").and_then(Value::as_str),
            Some("Ubuntu A")
        );
        assert_eq!(value.get("number"), Some(&Value::Number(-150.)));
    }

    #[test]
    fn malformed_and_hostile_files_are_rejected() {
        for text in [
            "",
            "{",
            "{\"a\" 1}",
            "[1 2]",
            "\"open",
            "/* never closed",
            "{1: 2}",
            "tru",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert!(parse(&"[".repeat(1_000)).is_err());
    }
}
