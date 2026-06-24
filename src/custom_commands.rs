use crate::{
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction},
    search_text::normalize_search_text,
    settings::{CommandAliasSetting, CommandHotkeySetting, CustomCommandSetting, LauncherSettings},
};

const MAX_ALIAS_EXPANSION_DEPTH: usize = 5;

pub fn expand_alias_query(query: &str, settings: &LauncherSettings) -> Option<String> {
    expand_alias_query_with_depth(query.trim(), settings, 0)
}

pub fn search_custom_commands(query: &str, settings: &LauncherSettings) -> Vec<CommandResult> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return custom_command_home_results(settings);
    }

    ranked_custom_commands(trimmed_query, settings)
        .into_iter()
        .map(custom_command_result)
        .collect()
}

pub fn search_aliases(query: &str, settings: &LauncherSettings) -> Vec<CommandResult> {
    let normalized_query = normalize_search_text(query);
    let aliases = settings
        .aliases
        .iter()
        .filter(|alias| !alias.keyword.trim().is_empty() && !alias.expands_to.trim().is_empty());

    if normalized_query.is_empty() {
        return aliases.cloned().map(alias_result).collect();
    }

    aliases
        .filter(|alias| {
            normalize_search_text(&alias.keyword).contains(&normalized_query)
                || normalize_search_text(&alias.expands_to).contains(&normalized_query)
        })
        .cloned()
        .map(alias_result)
        .collect()
}

pub fn search_hotkeys(query: &str, settings: &LauncherSettings) -> Vec<CommandResult> {
    let normalized_query = normalize_search_text(query);
    let mut hotkey_results = Vec::new();

    if settings.hotkey_enabled && !settings.hotkey.trim().is_empty() {
        hotkey_results.push(CommandResult::copyable_feature(
            "Launcher hotkey",
            settings.hotkey.clone(),
            settings.hotkey.clone(),
            CommandCategory::BuiltIn,
            82,
        ));
    }

    hotkey_results.extend(
        configured_query_hotkeys(settings)
            .into_iter()
            .map(|hotkey| {
                CommandResult::copyable_feature(
                    hotkey.title,
                    format!("{} -> {}", hotkey.hotkey, hotkey.query),
                    hotkey.query,
                    CommandCategory::BuiltIn,
                    80,
                )
            }),
    );

    if normalized_query.is_empty() {
        return hotkey_results;
    }

    hotkey_results
        .into_iter()
        .filter(|result| {
            normalize_search_text(&result.title).contains(&normalized_query)
                || normalize_search_text(&result.subtitle).contains(&normalized_query)
        })
        .collect()
}

pub fn configured_query_hotkeys(settings: &LauncherSettings) -> Vec<ConfiguredQueryHotkey> {
    let mut configured_hotkeys = settings
        .hotkeys
        .iter()
        .filter(|hotkey| !hotkey.hotkey.trim().is_empty() && !hotkey.query.trim().is_empty())
        .map(|hotkey| ConfiguredQueryHotkey {
            hotkey: hotkey.hotkey.trim().to_string(),
            query: hotkey.query.trim().to_string(),
            title: hotkey_title_from_setting(hotkey),
        })
        .collect::<Vec<_>>();

    configured_hotkeys.extend(
        settings
            .custom_commands
            .iter()
            .filter_map(|custom_command| {
                Some(ConfiguredQueryHotkey {
                    hotkey: custom_command.hotkey.as_ref()?.trim().to_string(),
                    query: format!("@custom {}", custom_command.name.trim()),
                    title: custom_command_title(custom_command),
                })
            })
            .filter(|hotkey| !hotkey.hotkey.is_empty() && !hotkey.query.trim().is_empty()),
    );

    configured_hotkeys
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredQueryHotkey {
    pub hotkey: String,
    pub query: String,
    pub title: String,
}

fn expand_alias_query_with_depth(
    query: &str,
    settings: &LauncherSettings,
    depth: usize,
) -> Option<String> {
    if depth >= MAX_ALIAS_EXPANSION_DEPTH {
        return None;
    }

    let (first_word, remaining_query) =
        query.split_once(char::is_whitespace).unwrap_or((query, ""));
    let alias = settings.aliases.iter().find(|alias| {
        !alias.keyword.trim().is_empty()
            && alias.keyword.trim().eq_ignore_ascii_case(first_word.trim())
            && !alias.expands_to.trim().is_empty()
    })?;

    let expanded_query = join_query_parts(alias.expands_to.trim(), remaining_query.trim());
    if expanded_query.eq_ignore_ascii_case(query) {
        return None;
    }

    expand_alias_query_with_depth(&expanded_query, settings, depth + 1).or(Some(expanded_query))
}

fn custom_command_home_results(settings: &LauncherSettings) -> Vec<CommandResult> {
    if settings.custom_commands.is_empty() {
        return vec![CommandResult::informational(
            "Custom commands",
            "Add [[custom_commands]] entries in config.toml to run shell commands",
        )];
    }

    settings
        .custom_commands
        .iter()
        .filter(|custom_command| is_valid_custom_command(custom_command))
        .cloned()
        .map(custom_command_result)
        .collect()
}

fn ranked_custom_commands(query: &str, settings: &LauncherSettings) -> Vec<CustomCommandSetting> {
    let normalized_query = normalize_search_text(query);
    let mut scored_commands = settings
        .custom_commands
        .iter()
        .filter(|custom_command| is_valid_custom_command(custom_command))
        .filter_map(|custom_command| {
            score_custom_command(custom_command, &normalized_query)
                .map(|score| (score, custom_command.clone()))
        })
        .collect::<Vec<_>>();

    scored_commands.sort_by_key(|(score, custom_command)| {
        (
            std::cmp::Reverse(*score),
            custom_command.name.to_lowercase(),
        )
    });

    scored_commands
        .into_iter()
        .map(|(_, custom_command)| custom_command)
        .collect()
}

fn custom_command_result(custom_command: CustomCommandSetting) -> CommandResult {
    let title = custom_command_title(&custom_command);
    let subtitle = if custom_command.description.trim().is_empty() {
        custom_command.command.clone()
    } else {
        custom_command.description.clone()
    };

    CommandResult::feature(
        title,
        subtitle,
        CommandCategory::BuiltIn,
        FeatureAction::RunCustomCommand {
            command: custom_command.command,
            working_directory: custom_command.working_directory,
        },
        92,
    )
}

fn alias_result(alias: CommandAliasSetting) -> CommandResult {
    CommandResult {
        title: format!("Alias {}", alias.keyword.trim()),
        subtitle: alias.expands_to.trim().to_string(),
        copy_text: alias.expands_to.trim().to_string(),
        explanation: Some("Searches are expanded before routing.".to_string()),
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::BuiltIn,
        action: CommandAction::CopyToClipboard(alias.expands_to.trim().to_string()),
        confidence: 78,
    }
}

fn score_custom_command(
    custom_command: &CustomCommandSetting,
    normalized_query: &str,
) -> Option<u8> {
    let name = normalize_search_text(&custom_command.name);
    let description = normalize_search_text(&custom_command.description);
    let command = normalize_search_text(&custom_command.command);

    if name == normalized_query
        || custom_command
            .aliases
            .iter()
            .any(|alias| normalize_search_text(alias) == normalized_query)
    {
        return Some(96);
    }

    if name.starts_with(normalized_query)
        || custom_command
            .aliases
            .iter()
            .any(|alias| normalize_search_text(alias).starts_with(normalized_query))
    {
        return Some(88);
    }

    if name.contains(normalized_query) || description.contains(normalized_query) {
        return Some(78);
    }

    command.contains(normalized_query).then_some(66)
}

fn is_valid_custom_command(custom_command: &CustomCommandSetting) -> bool {
    !custom_command.name.trim().is_empty() && !custom_command.command.trim().is_empty()
}

fn hotkey_title_from_setting(hotkey: &CommandHotkeySetting) -> String {
    if hotkey.description.trim().is_empty() {
        format!("Hotkey {}", hotkey.hotkey.trim())
    } else {
        hotkey.description.trim().to_string()
    }
}

fn custom_command_title(custom_command: &CustomCommandSetting) -> String {
    format!("Run {}", custom_command.name.trim())
}

fn join_query_parts(first_part: &str, second_part: &str) -> String {
    if second_part.is_empty() {
        first_part.to_string()
    } else {
        format!("{first_part} {second_part}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_alias_with_remaining_query() {
        let settings = LauncherSettings {
            aliases: vec![CommandAliasSetting {
                keyword: "g".to_string(),
                expands_to: "@web".to_string(),
            }],
            ..LauncherSettings::default()
        };

        assert_eq!(
            expand_alias_query("g rust gpui", &settings),
            Some("@web rust gpui".to_string())
        );
    }

    #[test]
    fn finds_custom_command_by_alias() {
        let settings = LauncherSettings {
            custom_commands: vec![CustomCommandSetting {
                name: "List files".to_string(),
                description: "Show files".to_string(),
                command: "dir".to_string(),
                aliases: vec!["ls".to_string()],
                hotkey: None,
                working_directory: None,
            }],
            ..LauncherSettings::default()
        };

        let results = search_custom_commands("ls", &settings);

        assert_eq!(results[0].title, "Run List files");
    }
}
