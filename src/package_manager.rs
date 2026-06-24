use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    search_text::normalize_search_text,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageManagerKind {
    Winget,
    Scoop,
    Choco,
}

#[derive(Clone, Copy)]
struct PackageShortcut {
    title: &'static str,
    subtitle: &'static str,
    arguments: &'static [&'static str],
    keywords: &'static str,
    confidence: u8,
}

const WINGET_SHORTCUTS: &[PackageShortcut] = &[
    PackageShortcut {
        title: "Winget search",
        subtitle: "Search packages: winget search <query>",
        arguments: &["search"],
        keywords: "search find package",
        confidence: 90,
    },
    PackageShortcut {
        title: "Winget install",
        subtitle: "Install a package: winget install <query>",
        arguments: &["install"],
        keywords: "install add package",
        confidence: 89,
    },
    PackageShortcut {
        title: "Winget upgrade all",
        subtitle: "Upgrade every installed package",
        arguments: &["upgrade", "--all"],
        keywords: "upgrade update all",
        confidence: 88,
    },
    PackageShortcut {
        title: "Winget list",
        subtitle: "List installed packages",
        arguments: &["list"],
        keywords: "list installed packages",
        confidence: 87,
    },
];

const SCOOP_SHORTCUTS: &[PackageShortcut] = &[
    PackageShortcut {
        title: "Scoop search",
        subtitle: "Search packages: scoop search <query>",
        arguments: &["search"],
        keywords: "search find package",
        confidence: 90,
    },
    PackageShortcut {
        title: "Scoop install",
        subtitle: "Install a package: scoop install <query>",
        arguments: &["install"],
        keywords: "install add package",
        confidence: 89,
    },
    PackageShortcut {
        title: "Scoop update",
        subtitle: "Update Scoop and installed apps",
        arguments: &["update", "*"],
        keywords: "update upgrade all",
        confidence: 88,
    },
];

const CHOCO_SHORTCUTS: &[PackageShortcut] = &[
    PackageShortcut {
        title: "Choco search",
        subtitle: "Search packages: choco search <query>",
        arguments: &["search"],
        keywords: "search find package",
        confidence: 90,
    },
    PackageShortcut {
        title: "Choco install",
        subtitle: "Install a package: choco install <query>",
        arguments: &["install", "-y"],
        keywords: "install add package",
        confidence: 89,
    },
    PackageShortcut {
        title: "Choco upgrade all",
        subtitle: "Upgrade every installed package",
        arguments: &["upgrade", "all", "-y"],
        keywords: "upgrade update all",
        confidence: 88,
    },
];

pub fn detect_package_manager(scope_tag: &str, search_text: &str) -> PackageManagerKind {
    let normalized_tag = normalize_search_text(scope_tag);
    if normalized_tag == "scoop" {
        return PackageManagerKind::Scoop;
    }
    if normalized_tag == "choco" || normalized_tag == "chocolatey" {
        return PackageManagerKind::Choco;
    }

    let normalized_search = normalize_search_text(search_text);
    if normalized_search.starts_with("scoop ") {
        return PackageManagerKind::Scoop;
    }
    if normalized_search.starts_with("choco ") || normalized_search.starts_with("chocolatey ") {
        return PackageManagerKind::Choco;
    }

    PackageManagerKind::Winget
}

pub fn search_packages(search_text: &str, scope_tag: &str) -> Vec<CommandResult> {
    let manager = detect_package_manager(scope_tag, search_text);
    let normalized_search_text = strip_manager_prefix(normalize_search_text(search_text), manager);

    if normalized_search_text.is_empty() {
        return shortcuts_for_manager(manager)
            .iter()
            .map(|shortcut| package_result(manager, shortcut, None))
            .collect();
    }

    if let Some(query) = normalized_search_text.strip_prefix("search ") {
        return vec![package_query_result(manager, "search", query, 91)];
    }

    if let Some(query) = normalized_search_text.strip_prefix("install ") {
        return vec![package_query_result(manager, "install", query, 90)];
    }

    let mut results = Vec::new();
    results.push(package_query_result(
        manager,
        "search",
        &normalized_search_text,
        89,
    ));
    results.push(package_query_result(
        manager,
        "install",
        &normalized_search_text,
        88,
    ));
    results.extend(
        shortcuts_for_manager(manager)
            .iter()
            .filter(|shortcut| {
                normalize_search_text(shortcut.title).contains(&normalized_search_text)
                    || normalize_search_text(shortcut.subtitle).contains(&normalized_search_text)
                    || normalize_search_text(shortcut.keywords).contains(&normalized_search_text)
            })
            .map(|shortcut| package_result(manager, shortcut, Some(&normalized_search_text))),
    );
    results
}

fn strip_manager_prefix(normalized_search_text: String, manager: PackageManagerKind) -> String {
    match manager {
        PackageManagerKind::Winget => normalized_search_text,
        PackageManagerKind::Scoop => normalized_search_text
            .strip_prefix("scoop ")
            .unwrap_or(&normalized_search_text)
            .to_string(),
        PackageManagerKind::Choco => normalized_search_text
            .strip_prefix("choco ")
            .or_else(|| normalized_search_text.strip_prefix("chocolatey "))
            .unwrap_or(&normalized_search_text)
            .to_string(),
    }
}

fn shortcuts_for_manager(manager: PackageManagerKind) -> &'static [PackageShortcut] {
    match manager {
        PackageManagerKind::Winget => WINGET_SHORTCUTS,
        PackageManagerKind::Scoop => SCOOP_SHORTCUTS,
        PackageManagerKind::Choco => CHOCO_SHORTCUTS,
    }
}

fn manager_program(manager: PackageManagerKind) -> &'static str {
    match manager {
        PackageManagerKind::Winget => "winget",
        PackageManagerKind::Scoop => "scoop",
        PackageManagerKind::Choco => "choco",
    }
}

fn package_result(
    manager: PackageManagerKind,
    shortcut: &PackageShortcut,
    query: Option<&str>,
) -> CommandResult {
    let program = manager_program(manager);
    let mut arguments: Vec<String> = shortcut.arguments.iter().map(|arg| (*arg).to_string()).collect();
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        if matches!(shortcut.arguments.first(), Some(&"search") | Some(&"install")) {
            arguments.push(query.to_string());
        }
    }

    let title = if let Some(query) = query.filter(|value| !value.is_empty()) {
        format!("{} {}", shortcut.title, query)
    } else {
        shortcut.title.to_string()
    };

    let show_in_terminal = shortcut.arguments.first().is_some_and(|arg| {
        matches!(*arg, "search" | "install" | "list" | "upgrade" | "update")
    });

    CommandResult {
        title,
        subtitle: shortcut.subtitle.to_string(),
        copy_text: format!("{program} {}", arguments.join(" ")),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Package,
        action: if show_in_terminal {
            open_terminal_action(program, &arguments)
        } else {
            CommandAction::RunProgram {
                program: program.to_string(),
                arguments,
            }
        },
        confidence: shortcut.confidence,
    }
}

fn package_query_result(
    manager: PackageManagerKind,
    subcommand: &str,
    query: &str,
    confidence: u8,
) -> CommandResult {
    let program = manager_program(manager);
    let mut arguments = vec![subcommand.to_string(), query.to_string()];
    if manager == PackageManagerKind::Choco && subcommand == "install" {
        arguments.push("-y".to_string());
    }

    CommandResult {
        title: format!(
            "{} {} {query}",
            match manager {
                PackageManagerKind::Winget => "Winget",
                PackageManagerKind::Scoop => "Scoop",
                PackageManagerKind::Choco => "Choco",
            },
            subcommand
        ),
        subtitle: format!("{program} {subcommand} {query}"),
        copy_text: format!("{program} {}", arguments.join(" ")),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Package,
        action: open_terminal_action(program, &arguments),
        confidence,
    }
}

fn open_terminal_action(program: &str, arguments: &[String]) -> CommandAction {
    let command_line = std::iter::once(program.to_string())
        .chain(arguments.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");

    CommandAction::RunProgram {
        program: "cmd.exe".to_string(),
        arguments: vec![
            "/c".to_string(),
            "start".to_string(),
            "cmd".to_string(),
            "/k".to_string(),
            command_line,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_shortcuts_for_empty_query() {
        let results = search_packages("", "winget");
        assert_eq!(results.len(), WINGET_SHORTCUTS.len());
        assert!(results
            .iter()
            .all(|result| result.category == CommandCategory::Package));
    }

    #[test]
    fn parses_explicit_search_query() {
        let results = search_packages("search vscode", "winget");
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("vscode"));
        assert!(results[0].copy_text.contains("winget search vscode"));
    }

    #[test]
    fn detects_scoop_from_scope_tag() {
        let manager = detect_package_manager("scoop", "search git");
        assert_eq!(manager, PackageManagerKind::Scoop);
        let results = search_packages("", "scoop");
        assert_eq!(results.len(), SCOOP_SHORTCUTS.len());
    }

    #[test]
    fn detects_choco_from_subcommand_in_winget_scope() {
        let manager = detect_package_manager("winget", "choco install git");
        assert_eq!(manager, PackageManagerKind::Choco);
        let results = search_packages("choco install git", "winget");
        assert_eq!(results.len(), 1);
        assert!(results[0].copy_text.contains("choco install git"));
    }
}