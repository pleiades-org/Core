use std::collections::HashMap;

const MAX_COMMAND_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Applications = 1,
    Calculator = 2,
    Web = 3,
    Quicklinks = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsedCommand<'query> {
    pub kind: CommandKind,
    pub payload: &'query str,
}

pub const COMMAND_ALIASES: &[(&str, CommandKind)] = &[
    ("app", CommandKind::Applications),
    ("apps", CommandKind::Applications),
    ("application", CommandKind::Applications),
    ("applications", CommandKind::Applications),
    ("a", CommandKind::Applications),
    ("calc", CommandKind::Calculator),
    ("calculator", CommandKind::Calculator),
    ("math", CommandKind::Calculator),
    ("calculate", CommandKind::Calculator),
    ("web", CommandKind::Web),
    ("google", CommandKind::Web),
    ("search", CommandKind::Web),
    ("w", CommandKind::Web),
    ("quicklink", CommandKind::Quicklinks),
    ("quicklinks", CommandKind::Quicklinks),
    ("ql", CommandKind::Quicklinks),
    ("link", CommandKind::Quicklinks),
    ("links", CommandKind::Quicklinks),
    ("url", CommandKind::Quicklinks),
    ("urls", CommandKind::Quicklinks),
];

pub fn lookup_match(command: &str) -> Option<CommandKind> {
    match command {
        "app" | "apps" | "application" | "applications" | "a" => Some(CommandKind::Applications),
        "calc" | "calculator" | "math" | "calculate" => Some(CommandKind::Calculator),
        "web" | "google" | "search" | "w" => Some(CommandKind::Web),
        "quicklink" | "quicklinks" | "ql" | "link" | "links" | "url" | "urls" => {
            Some(CommandKind::Quicklinks)
        }
        _ => None,
    }
}

pub fn lookup_linear(command: &str) -> Option<CommandKind> {
    COMMAND_ALIASES
        .iter()
        .find_map(|(alias, kind)| (*alias == command).then_some(*kind))
}

pub fn command_map() -> HashMap<&'static str, CommandKind> {
    COMMAND_ALIASES.iter().copied().collect()
}

/// Prototype of the proposed prefix-only grammar; it is not a legacy-compatible replacement.
pub fn parse_command(query: &str) -> Option<ParsedCommand<'_>> {
    let scoped = query.trim().strip_prefix('@')?;
    let (head, payload) = scoped
        .split_once(char::is_whitespace)
        .unwrap_or((scoped, ""));
    if head.len() > MAX_COMMAND_BYTES || !head.is_ascii() {
        return None;
    }
    let mut normalized = [0_u8; MAX_COMMAND_BYTES];
    for (destination, original) in normalized.iter_mut().zip(head.bytes()) {
        *destination = original.to_ascii_lowercase();
    }
    let command = std::str::from_utf8(&normalized[..head.len()]).ok()?;
    Some(ParsedCommand {
        kind: lookup_match(command)?,
        payload: payload.trim(),
    })
}

pub const QUERY_CORPUS: &[&str] = &[
    "@calc 2 + 2",
    "@CALC 25% of 80",
    "@app visual studio code",
    "@web rust & windows",
    "@quicklink docs",
    "@math (12 / 3) * 9",
    "  @apps notepad",
    "@google rust enums",
    "visual studio code",
    "notepad",
];

fn command_parts(query: &str) -> Option<(&str, &str)> {
    let scoped = query.trim().strip_prefix('@')?;
    let (head, payload) = scoped
        .split_once(char::is_whitespace)
        .unwrap_or((scoped, ""));
    if head.len() > MAX_COMMAND_BYTES || !head.is_ascii() {
        return None;
    }
    Some((head, payload.trim()))
}

/// Try the common canonical spelling before the checked lowercase-buffer fallback.
pub fn parse_canonical_first(query: &str) -> Option<ParsedCommand<'_>> {
    let (head, payload) = command_parts(query)?;
    if let Some(kind) = lookup_match(head) {
        return Some(ParsedCommand { kind, payload });
    }
    parse_command(query)
}

pub fn parse_ascii_comparisons(query: &str) -> Option<ParsedCommand<'_>> {
    let (head, payload) = command_parts(query)?;
    let kind = COMMAND_ALIASES
        .iter()
        .find_map(|(alias, kind)| head.eq_ignore_ascii_case(alias).then_some(*kind))?;
    Some(ParsedCommand { kind, payload })
}

pub fn parse_binary_search<'query>(
    query: &'query str,
    aliases: &[(&str, CommandKind)],
) -> Option<ParsedCommand<'query>> {
    let (head, payload) = command_parts(query)?;
    let mut normalized = [0_u8; MAX_COMMAND_BYTES];
    for (destination, original) in normalized.iter_mut().zip(head.bytes()) {
        *destination = original.to_ascii_lowercase();
    }
    let command = std::str::from_utf8(&normalized[..head.len()]).ok()?;
    let position = aliases
        .binary_search_by_key(&command, |(alias, _)| *alias)
        .ok()?;
    Some(ParsedCommand {
        kind: aliases[position].1,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_prefix_parser_alternatives_preserve_the_same_grammar() {
        let mut sorted = COMMAND_ALIASES.to_vec();
        sorted.sort_by_key(|entry| entry.0);
        let mut queries: Vec<_> = QUERY_CORPUS.iter().map(|query| query.to_string()).collect();
        for (alias, _) in COMMAND_ALIASES {
            queries.push(format!("@{} Body with  Café", alias.to_uppercase()));
            queries.push(format!("\u{2003}@{alias}\t Mixed  Payload "));
        }
        queries.extend(
            [
                "@",
                "@cal",
                "@unknown body",
                "text @calc 2",
                "@calc2+2",
                "@çalc 2",
                "@abcdefghijklmnopq 4",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        for query in queries {
            let expected = parse_command(&query);
            assert_eq!(parse_canonical_first(&query), expected);
            assert_eq!(parse_ascii_comparisons(&query), expected);
            assert_eq!(parse_binary_search(&query, &sorted), expected);
        }
    }

    #[test]
    fn benchmark_corpus_matches_legacy_command_and_payload() {
        for query in QUERY_CORPUS {
            let current =
                parse_command(query).map(|parsed| (parsed.kind as u8, parsed.payload.to_owned()));
            assert_eq!(current, crate::legacy_router::describe(query), "{query}");
        }
    }

    #[test]
    fn command_parser_preserves_payload_case_spacing_and_unicode() {
        let parsed = parse_command("\u{2003}@CaLc\t  2 +  München  ").unwrap();
        assert_eq!(parsed.kind, CommandKind::Calculator);
        assert_eq!(parsed.payload, "2 +  München");
    }

    #[test]
    fn command_lookup_implementations_agree() {
        let lookup = command_map();
        for (alias, expected) in COMMAND_ALIASES {
            assert_eq!(lookup_match(alias), Some(*expected));
            assert_eq!(lookup_linear(alias), Some(*expected));
            assert_eq!(lookup.get(alias).copied(), Some(*expected));
        }
        for unknown in ["", "cal", "unknown", "@calc", "çalc"] {
            assert_eq!(lookup_match(unknown), None);
            assert_eq!(lookup_linear(unknown), None);
        }
    }

    #[test]
    fn incomplete_unknown_embedded_and_oversized_commands_are_not_executed() {
        for query in [
            "@",
            "@cal",
            "@unknown body",
            "text @calc 2+2",
            "@abcdefghijklmnopq value",
            "@çalc 2+2",
        ] {
            assert!(parse_command(query).is_none());
        }
    }
}
