use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    search_text::normalize_search_text,
};
use std::{env, path::PathBuf};

#[derive(Clone, Copy)]
struct GitCommand {
    title: &'static str,
    subtitle: &'static str,
    args: &'static [&'static str],
    keywords: &'static str,
    confidence: u8,
}

const GIT_COMMANDS: &[GitCommand] = &[
    GitCommand {
        title: "Git status",
        subtitle: "Show working tree status",
        args: &["status"],
        keywords: "status changes staged",
        confidence: 92,
    },
    GitCommand {
        title: "Git pull",
        subtitle: "Fetch and merge remote changes",
        args: &["pull"],
        keywords: "pull fetch merge remote",
        confidence: 91,
    },
    GitCommand {
        title: "Git push",
        subtitle: "Push commits to remote",
        args: &["push"],
        keywords: "push upload remote",
        confidence: 90,
    },
    GitCommand {
        title: "Git fetch",
        subtitle: "Download objects and refs from remote",
        args: &["fetch"],
        keywords: "fetch remote update",
        confidence: 89,
    },
    GitCommand {
        title: "Git stash",
        subtitle: "Stash tracked changes",
        args: &["stash"],
        keywords: "stash save changes",
        confidence: 88,
    },
    GitCommand {
        title: "Git log",
        subtitle: "Show recent commits (oneline)",
        args: &["log", "--oneline", "-10"],
        keywords: "log history commits oneline",
        confidence: 87,
    },
    GitCommand {
        title: "Git branch",
        subtitle: "List local branches",
        args: &["branch"],
        keywords: "branch branches list",
        confidence: 86,
    },
    GitCommand {
        title: "Git diff",
        subtitle: "Show unstaged changes",
        args: &["diff"],
        keywords: "diff changes patch",
        confidence: 85,
    },
    GitCommand {
        title: "Git add all",
        subtitle: "Stage all changes",
        args: &["add", "-A"],
        keywords: "add stage all",
        confidence: 84,
    },
    GitCommand {
        title: "Git commit",
        subtitle: "Create a commit (opens terminal)",
        args: &["commit"],
        keywords: "commit message save",
        confidence: 83,
    },
];

pub fn search_git_commands(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    let (filter_text, repo_path) = split_repo_path(search_text);

    if normalized_search_text.is_empty() && repo_path.is_none() {
        return GIT_COMMANDS.iter().map(|command| git_result(command, None)).collect();
    }

    let normalized_filter = normalize_search_text(&filter_text);

    GIT_COMMANDS
        .iter()
        .filter(|command| {
            if normalized_filter.is_empty() {
                return true;
            }

            normalize_search_text(command.title).contains(&normalized_filter)
                || normalize_search_text(command.subtitle).contains(&normalized_filter)
                || normalize_search_text(command.keywords).contains(&normalized_filter)
                || command
                    .args
                    .iter()
                    .any(|arg| normalize_search_text(arg).contains(&normalized_filter))
        })
        .map(|command| git_result(command, repo_path.as_deref()))
        .collect()
}

fn git_result(command: &GitCommand, repo_path: Option<&str>) -> CommandResult {
    let working_directory = resolve_working_directory(repo_path);
    let mut arguments = Vec::new();

    if let Some(path) = repo_path {
        arguments.push("-C".to_string());
        arguments.push(path.to_string());
    }

    if command.title == "Git commit" {
        return commit_result(command, &arguments, &working_directory);
    }

    arguments.extend(command.args.iter().map(|arg| (*arg).to_string()));

    CommandResult {
        title: command.title.to_string(),
        subtitle: format!("{} · {}", command.subtitle, working_directory.display()),
        copy_text: format!("git {}", arguments.join(" ")),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Git,
        action: CommandAction::RunProgram {
            program: "git".to_string(),
            arguments,
        },
        confidence: command.confidence,
    }
}

fn commit_result(
    command: &GitCommand,
    repo_prefix_args: &[String],
    working_directory: &PathBuf,
) -> CommandResult {
    let mut arguments = repo_prefix_args.to_vec();
    arguments.push("commit".to_string());

    CommandResult {
        title: command.title.to_string(),
        subtitle: format!(
            "{} · {} (opens terminal)",
            command.subtitle,
            working_directory.display()
        ),
        copy_text: format!("git {}", arguments.join(" ")),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Git,
        action: CommandAction::RunProgram {
            program: "cmd.exe".to_string(),
            arguments: vec![
                "/c".to_string(),
                "start".to_string(),
                "cmd".to_string(),
                "/k".to_string(),
                format!(
                    "cd /d \"{}\" && git {}",
                    working_directory.display(),
                    arguments.join(" ")
                ),
            ],
        },
        confidence: command.confidence,
    }
}

fn split_repo_path(search_text: &str) -> (String, Option<String>) {
    let trimmed = search_text.trim();
    let mut parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return (trimmed.to_string(), None);
    }

    let last = parts.pop().expect("last token");
    if looks_like_repo_path(last) {
        (parts.join(" "), Some(last.to_string()))
    } else {
        (trimmed.to_string(), None)
    }
}

fn looks_like_repo_path(value: &str) -> bool {
    value.starts_with("~/")
        || value.starts_with("~\\")
        || value.contains('\\')
        || value.contains('/')
        || value.contains(':')
}

fn resolve_working_directory(repo_path: Option<&str>) -> PathBuf {
    if let Some(path_text) = repo_path {
        return expand_path(path_text);
    }

    env::current_dir().unwrap_or_else(|_| default_home_directory())
}

fn expand_path(path_text: &str) -> PathBuf {
    if path_text == "~" {
        return default_home_directory();
    }

    if let Some(relative) = path_text
        .strip_prefix("~/")
        .or_else(|| path_text.strip_prefix("~\\"))
    {
        return default_home_directory().join(relative);
    }

    PathBuf::from(path_text)
}

fn default_home_directory() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_git_commands_by_search_text() {
        let results = search_git_commands("stash");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Git stash");
        assert_eq!(results[0].category, CommandCategory::Git);
    }

    #[test]
    fn splits_repo_path_from_query_suffix() {
        let (filter, repo_path) = split_repo_path("status C:\\code\\core");
        assert_eq!(filter, "status");
        assert_eq!(repo_path.as_deref(), Some("C:\\code\\core"));
    }

    #[test]
    fn returns_all_commands_for_empty_query() {
        let results = search_git_commands("");
        assert_eq!(results.len(), GIT_COMMANDS.len());
    }
}