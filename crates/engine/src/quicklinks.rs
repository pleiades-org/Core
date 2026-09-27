use crate::applications::{Application, ApplicationCatalog};
use std::sync::Arc;

pub const MAX_QUICKLINKS: usize = 1000;
pub const MAX_LINK_LENGTH: usize = 2048;
pub const MAX_NAME_LENGTH: usize = 120;
pub const ID_PREFIX: &str = "quicklink:";
/// App-link schemes that run script, embed content or have a history of abuse. Quicklinks are
/// typed by the local user, so this is defence in depth against pasted links.
const BLOCKED_SCHEMES: &[&str] = &[
    "javascript",
    "vbscript",
    "data",
    "file",
    "about",
    "blob",
    "ms-msdt",
    "search-ms",
    "search",
    "ms-officecmd",
    "ms-word",
    "ms-excel",
    "ms-powerpoint",
    "mk",
    "its",
    "ms-its",
    "hcp",
    "jar",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quicklink {
    pub name: Arc<str>,
    pub link: Arc<str>,
}

impl Quicklink {
    pub fn new(name: &str, link: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty()
            || name.encode_utf16().count() > MAX_NAME_LENGTH
            || name.chars().any(char::is_control)
        {
            return Err("Enter a name of up to 120 characters without control characters.".into());
        }
        Ok(Self {
            name: name.into(),
            link: validate_target(link)?.into(),
        })
    }

    pub fn application(&self) -> Application {
        Application {
            id: format!("{ID_PREFIX}{}\t{}", self.name, self.link).into(),
            name: self.name.clone(),
            description: self.link.clone(),
            pinned: false,
            launches: 0,
            aliases: Default::default(),
        }
    }
}

/// Only explicit websites, absolute Windows paths and app links such as `steam://rungameid/1`;
/// never interpret shell commands.
pub fn validate_target(link: &str) -> Result<String, String> {
    let link = link.trim();
    if link.is_empty() || link.len() > MAX_LINK_LENGTH || link.chars().any(char::is_control) {
        return Err("Enter a link or absolute path of up to 2048 bytes.".into());
    }
    let bytes = link.as_bytes();
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let network_path = link.strip_prefix("\\\\").is_some_and(|rest| {
        !rest.starts_with(['?', '.'])
            && rest
                .split('\\')
                .take(2)
                .filter(|part| !part.is_empty())
                .count()
                == 2
    });
    if (drive_path || network_path) && !link.contains('"') {
        return Ok(link.to_owned());
    }
    let normalized = if link
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        format!("https://{}", &link[8..])
    } else if link
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
    {
        format!("http://{}", &link[7..])
    } else if !link.contains(':')
        && link
            .split('/')
            .next()
            .is_some_and(|host| host.contains('.'))
    {
        format!("https://{link}")
    } else if let Some(scheme) = app_link_scheme(link) {
        return validate_app_link(link, scheme);
    } else {
        return Err(
            "Use an HTTP/HTTPS website, an app link such as steam://… or an absolute file/folder path."
                .into(),
        );
    };
    let authority = normalized
        .split_once("://")
        .expect("normalized web scheme")
        .1
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if normalized.len() > MAX_LINK_LENGTH
        || authority.is_empty()
        || authority.contains('@')
        || normalized.chars().any(char::is_whitespace)
        || normalized.contains(['\\', '"', '<', '>'])
    {
        return Err("Enter a website with a host and no spaces or embedded credentials.".into());
    }
    Ok(normalized)
}

/// The scheme of an app link `scheme:rest` such as `steam://rungameid/1`, as written. `None` for
/// websites, paths and anything else. Schemes follow RFC 3986 and need two characters, so a
/// drive letter such as `c:relative` is never a scheme.
pub fn app_link_scheme(link: &str) -> Option<&str> {
    let (scheme, rest) = link.split_once(':')?;
    let mut bytes = scheme.bytes();
    let valid = scheme.len() >= 2
        && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'));
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    (valid && !web && !rest.is_empty()).then_some(scheme)
}

/// Lowercases only the scheme; the rest is passed to the registered app byte-for-byte.
fn validate_app_link(link: &str, scheme: &str) -> Result<String, String> {
    let scheme = scheme.to_ascii_lowercase();
    if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
        return Err("That link type is not allowed.".into());
    }
    if link.chars().any(char::is_whitespace) || link.contains(['\\', '"', '<', '>']) {
        return Err(
            "Enter an app link without spaces, quotes, angle brackets or backslashes.".into(),
        );
    }
    Ok(format!("{scheme}{}", &link[scheme.len()..]))
}

pub fn catalogs(
    applications: &ApplicationCatalog,
    links: &[Quicklink],
) -> (ApplicationCatalog, ApplicationCatalog) {
    let quicklinks: Vec<_> = links.iter().map(Quicklink::application).collect();
    let combined = applications
        .entries()
        .cloned()
        .chain(quicklinks.iter().cloned())
        .collect();
    (
        ApplicationCatalog::new(quicklinks),
        ApplicationCatalog::new(combined),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quicklinks_share_ranking_but_explicit_app_scope_excludes_them() {
        use crate::search::{Action, ResultKind, SearchEngine};
        let apps = ApplicationCatalog::new(vec![Application {
            id: "app:test".into(),
            name: "Docs editor".into(),
            description: "Application".into(),
            pinned: false,
            launches: 0,
            aliases: Default::default(),
        }]);
        let links = vec![
            Quicklink::new("Docs", "https://example.com/docs").unwrap(),
            Quicklink::new("Project files", "C:\\Projects").unwrap(),
        ];
        let (quicklinks, combined) = catalogs(&apps, &links);
        let mut engine = SearchEngine::default();
        let mixed = engine.search_catalogs("docs", &apps, &combined, &quicklinks, &|| false);
        assert_eq!(mixed.results[0].title.as_ref(), "Docs");
        assert_eq!(mixed.results[0].kind, ResultKind::Quicklink);
        assert_eq!(
            mixed.results[0].action,
            Action::OpenQuicklink("https://example.com/docs".into())
        );
        assert_eq!(
            engine
                .search_catalogs("@app docs", &apps, &combined, &quicklinks, &|| false)
                .results
                .len(),
            1
        );
        for query in [">", "@quicklink", "@links"] {
            assert_eq!(
                engine
                    .search_catalogs(query, &apps, &combined, &quicklinks, &|| false)
                    .results
                    .len(),
                2
            );
        }
        assert_eq!(
            engine
                .search_catalogs("> pf", &apps, &combined, &quicklinks, &|| false)
                .results[0]
                .title
                .as_ref(),
            "Project files"
        );
        assert!(engine
            .search_catalogs("> missing", &apps, &combined, &quicklinks, &|| false)
            .results
            .is_empty());
    }
    #[test]
    fn targets_accept_websites_and_paths_without_shell_interpretation() {
        assert_eq!(
            validate_target("example.com/a?q=1&b=2").unwrap(),
            "https://example.com/a?q=1&b=2"
        );
        for target in [
            "HTTPS://example.com",
            "C:\\My Folder\\file.txt",
            "\\\\server\\share\\file",
        ] {
            assert!(validate_target(target).is_ok(), "{target}");
        }
        for target in [
            "",
            "javascript:alert(1)",
            "data:text/html,test",
            "https://",
            "https://user@host",
            "https://a\nb",
            "cmd /c test",
            "\\\\.\\device",
            "relative.txt/../bad:thing",
        ] {
            assert!(validate_target(target).is_err(), "{target}");
        }
    }
    #[test]
    fn app_links_keep_their_target_and_lowercase_only_the_scheme() {
        for (target, expected) in [
            ("steam://rungameid/2379780", "steam://rungameid/2379780"),
            ("STEAM://rungameid/1", "steam://rungameid/1"),
            (
                "com.epicgames.launcher://apps/fn?action=launch",
                "com.epicgames.launcher://apps/fn?action=launch",
            ),
            ("spotify:track:abc", "spotify:track:abc"),
            ("Spotify:Track:ABC", "spotify:Track:ABC"),
            ("mailto:a@b.c", "mailto:a@b.c"),
            ("ms-settings:display", "ms-settings:display"),
            ("shell:startup", "shell:startup"),
        ] {
            assert_eq!(validate_target(target).as_deref(), Ok(expected), "{target}");
            assert!(Quicklink::new("Game", target).is_ok(), "{target}");
        }
        assert_eq!(app_link_scheme("steam://rungameid/1"), Some("steam"));
        for target in [
            "https://example.com",
            "C:\\Games",
            "example.com",
            "c:relative",
        ] {
            assert_eq!(app_link_scheme(target), None, "{target}");
        }
    }
    #[test]
    fn app_links_reject_unsafe_schemes_and_malformed_links() {
        for target in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "vbscript:x",
            "data:text/html,x",
            "file:///C:/x",
            "search-ms:query=x",
            "ms-msdt:/id",
        ] {
            assert_eq!(
                validate_target(target),
                Err("That link type is not allowed.".into()),
                "{target}"
            );
        }
        for target in [
            "steam://run game",
            "steam://run\tgame",
            "steam://\"x\"",
            "steam://<x>",
            "steam:\\\\x",
            "c:relative",
            "1abc:x",
            "steam:",
            "http:example.com",
            "-steam:x",
            "st_eam:x",
        ] {
            assert!(validate_target(target).is_err(), "{target}");
        }
        let long = format!("steam://{}", "a".repeat(MAX_LINK_LENGTH));
        assert!(validate_target(&long).is_err());
        let longest = format!("steam:{}", "a".repeat(MAX_LINK_LENGTH - "steam:".len()));
        assert_eq!(validate_target(&longest), Ok(longest.clone()));
    }
}
