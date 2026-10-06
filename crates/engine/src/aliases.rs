//! Aliases: short names the person defines for things they type, such as `d` for `Discord` or
//! `@s` for `@song`. An alias stands for the first word of the query; the rest is kept, so
//! `@s basorexia` becomes `@song basorexia`. What an alias stands for is not looked up again.
use std::{borrow::Cow, sync::Arc};

pub const MAX_ALIASES: usize = 200;
/// In UTF-16 units, as the settings fields count them.
pub const MAX_NAME_LENGTH: usize = 32;
pub const MAX_EXPANSION_LENGTH: usize = 256;
/// Starts Core's command prompt, which takes what is typed after it unchanged.
const COMMAND_PROMPT: char = '/';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub name: Arc<str>,
    pub expansion: Arc<str>,
}

impl Alias {
    pub fn new(name: &str, expansion: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty()
            || name.encode_utf16().count() > MAX_NAME_LENGTH
            || name
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err("Enter an alias of up to 32 characters without spaces.".into());
        }
        if name.starts_with(COMMAND_PROMPT) {
            return Err("An alias cannot start with /, which starts a command.".into());
        }
        let expansion = expansion.trim();
        if expansion.is_empty()
            || expansion.encode_utf16().count() > MAX_EXPANSION_LENGTH
            || expansion.chars().any(char::is_control)
        {
            return Err("Enter what the alias stands for, in up to 256 characters.".into());
        }
        Ok(Self {
            name: name.into(),
            expansion: expansion.into(),
        })
    }

    /// The form two aliases must not share: names are matched without regard to case.
    pub fn key(&self) -> String {
        self.name.to_lowercase()
    }
}

/// The query Core acts on: `query` with its first word replaced by what that word's alias
/// stands for. Everything after the first word keeps its spelling and spacing.
pub fn expand<'query>(query: &'query str, aliases: &[Alias]) -> Cow<'query, str> {
    if aliases.is_empty() {
        return Cow::Borrowed(query);
    }
    let typed = query.trim_start();
    let end = typed.find(char::is_whitespace).unwrap_or(typed.len());
    let (word, rest) = typed.split_at(end);
    if word.is_empty() {
        return Cow::Borrowed(query);
    }
    let word = word.to_lowercase();
    match aliases.iter().find(|alias| alias.key() == word) {
        Some(alias) => Cow::Owned(format!("{}{rest}", alias.expansion)),
        None => Cow::Borrowed(query),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> Vec<Alias> {
        vec![
            Alias::new("d", "Discord").unwrap(),
            Alias::new("@s", "@song").unwrap(),
            Alias::new("Ip", "/ipconfig /all").unwrap(),
            Alias::new("é", "Référence").unwrap(),
        ]
    }

    #[test]
    fn the_first_word_is_replaced_and_the_rest_is_kept() {
        let aliases = aliases();
        assert_eq!(expand("d", &aliases), "Discord");
        assert_eq!(expand("@s basorexia", &aliases), "@song basorexia");
        assert_eq!(
            expand("  @S   two  words ", &aliases),
            "@song   two  words "
        );
        // Names match without regard to case, including beyond ASCII.
        assert_eq!(expand("IP", &aliases), "/ipconfig /all");
        assert_eq!(expand("É", &aliases), "Référence");
    }

    #[test]
    fn only_a_whole_first_word_is_an_alias() {
        let aliases = aliases();
        for query in ["", "   ", "do", "discord", "x d", "@so", "@song d", "/d"] {
            assert!(
                matches!(expand(query, &aliases), Cow::Borrowed(text) if text == query),
                "{query}"
            );
        }
        assert!(matches!(expand("d", &[]), Cow::Borrowed("d")));
    }

    #[test]
    fn what_an_alias_stands_for_is_not_expanded_again() {
        let aliases = vec![Alias::new("a", "b").unwrap(), Alias::new("b", "a").unwrap()];
        assert_eq!(expand("a", &aliases), "b");
        assert_eq!(expand("b x", &aliases), "a x");
    }

    #[test]
    fn names_are_one_word_and_never_the_command_prompt() {
        let alias = Alias::new("  @S  ", "  @song  ").unwrap();
        assert_eq!((&*alias.name, &*alias.expansion), ("@S", "@song"));
        assert_eq!(alias.key(), "@s");
        for (name, expansion) in [
            ("", "Discord"),
            ("two words", "Discord"),
            ("tab\tbed", "Discord"),
            ("/d", "Discord"),
            ("d", ""),
            ("d", "   "),
            ("d", "bad\u{0}text"),
        ] {
            assert!(
                Alias::new(name, expansion).is_err(),
                "{name:?} {expansion:?}"
            );
        }
        assert!(Alias::new(&"a".repeat(MAX_NAME_LENGTH), "x").is_ok());
        assert!(Alias::new(&"a".repeat(MAX_NAME_LENGTH + 1), "x").is_err());
        assert!(Alias::new("a", &"x".repeat(MAX_EXPANSION_LENGTH)).is_ok());
        assert!(Alias::new("a", &"x".repeat(MAX_EXPANSION_LENGTH + 1)).is_err());
    }
}
