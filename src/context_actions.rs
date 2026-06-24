use crate::{
    calculator::CalculationContext,
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction, SystemControlCommand},
    search_text::normalize_search_text,
    settings::LauncherSettings,
    timezone_resolver,
};
use chrono::{Datelike, Timelike};
use chrono_tz::Tz;

pub fn context_home_results(settings: &LauncherSettings) -> Vec<CommandResult> {
    let mut results = vec![
        local_time_result(settings),
        local_date_result(settings),
    ];
    results.extend(media_control_results());
    results
}

pub fn search_context_actions(
    search_text: &str,
    settings: &LauncherSettings,
) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    if normalized_search_text.is_empty() {
        let mut results = context_home_results(settings);
        results.extend(extended_context_results(settings));
        return results;
    }

    let mut results: Vec<CommandResult> = catalog_context_results(settings)
        .into_iter()
        .filter(|result| context_result_matches(&normalized_search_text, result))
        .collect();

    if matches_time_query(&normalized_search_text) {
        results.insert(0, local_time_result(settings));
    }

    results
}

fn catalog_context_results(settings: &LauncherSettings) -> Vec<CommandResult> {
    let mut results = context_home_results(settings);
    results.extend(extended_context_results(settings));
    results
}

fn extended_context_results(settings: &LauncherSettings) -> Vec<CommandResult> {
    let timezone = timezone_resolver::local_timezone(settings);
    let timezone_label = settings.local_timezone.trim();
    let timezone_subtitle = if timezone_label.is_empty() {
        timezone.to_string()
    } else {
        timezone_label.to_string()
    };

    vec![
        CommandResult::copyable_feature(
            "Copy ISO timestamp",
            format!("Full local timestamp in {timezone_subtitle}"),
            current_timestamp_copy_text(settings),
            CommandCategory::Context,
            84,
        ),
        CommandResult::copyable_feature(
            "Copy today's date",
            "YYYY-MM-DD in your local timezone",
            current_date_copy_text(settings),
            CommandCategory::Context,
            82,
        ),
        CommandResult::feature(
            "Mute volume",
            "Toggle system mute",
            CommandCategory::Context,
            FeatureAction::SystemControl(SystemControlCommand::MuteVolume),
            80,
        ),
        CommandResult::feature(
            "Volume up",
            "Increase system volume",
            CommandCategory::Context,
            FeatureAction::SystemControl(SystemControlCommand::VolumeUp),
            78,
        ),
        CommandResult::feature(
            "Volume down",
            "Decrease system volume",
            CommandCategory::Context,
            FeatureAction::SystemControl(SystemControlCommand::VolumeDown),
            76,
        ),
    ]
}

fn local_time_result(settings: &LauncherSettings) -> CommandResult {
    let context = CalculationContext::from_settings(settings.clone());
    let now = context.now;
    let timezone_label = settings.local_timezone.trim();
    let timezone_name = if timezone_label.is_empty() {
        "Local time".to_string()
    } else {
        timezone_label.to_string()
    };
    let title = format_clock_time(now.hour(), now.minute());
    let subtitle = format!("{} · {}", format_long_date(now), timezone_name);
    let copy_text = now.format("%Y-%m-%d %H:%M:%S").to_string();

    CommandResult {
        title,
        subtitle,
        copy_text: copy_text.clone(),
        explanation: Some(format!("Current local time in {timezone_name}.")),
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Context,
        action: CommandAction::CopyToClipboard(copy_text),
        confidence: 100,
    }
}

fn local_date_result(settings: &LauncherSettings) -> CommandResult {
    let context = CalculationContext::from_settings(settings.clone());
    let now = context.now;
    let title = format_long_date(now);
    let subtitle = format!("Week {}", now.iso_week().week());
    let copy_text = now.format("%Y-%m-%d").to_string();

    CommandResult {
        title,
        subtitle,
        copy_text: copy_text.clone(),
        explanation: Some("Today's date in your local timezone.".to_string()),
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::Context,
        action: CommandAction::CopyToClipboard(copy_text),
        confidence: 98,
    }
}

fn media_control_results() -> Vec<CommandResult> {
    [
        (
            "Play / Pause media",
            "Toggle playback for the active media app",
            SystemControlCommand::MediaPlayPause,
            94,
        ),
        (
            "Next track",
            "Skip to the next song or video",
            SystemControlCommand::MediaNext,
            92,
        ),
        (
            "Previous track",
            "Go back to the previous song or video",
            SystemControlCommand::MediaPrevious,
            90,
        ),
        (
            "Stop media",
            "Stop the current media session",
            SystemControlCommand::MediaStop,
            86,
        ),
    ]
    .into_iter()
    .map(|(title, subtitle, command, confidence)| {
        CommandResult::feature(
            title,
            subtitle,
            CommandCategory::Context,
            FeatureAction::SystemControl(command),
            confidence,
        )
    })
    .collect()
}

fn current_timestamp_copy_text(settings: &LauncherSettings) -> String {
    CalculationContext::from_settings(settings.clone())
        .now
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn current_date_copy_text(settings: &LauncherSettings) -> String {
    CalculationContext::from_settings(settings.clone())
        .now
        .format("%Y-%m-%d")
        .to_string()
}

fn context_result_matches(normalized_query: &str, result: &CommandResult) -> bool {
    let title = normalize_search_text(&result.title);
    let subtitle = normalize_search_text(&result.subtitle);
    title.contains(normalized_query)
        || subtitle.contains(normalized_query)
        || normalized_query
            .split_whitespace()
            .all(|word| title.contains(word) || subtitle.contains(word))
}

fn matches_time_query(normalized_query: &str) -> bool {
    ["time", "clock", "now", "today", "date"].contains(&normalized_query)
}

fn format_clock_time(hour: u32, minute: u32) -> String {
    let hour_12 = hour % 12;
    let hour_12 = if hour_12 == 0 { 12 } else { hour_12 };
    let period = if hour < 12 { "AM" } else { "PM" };
    format!("{hour_12}:{minute:02} {period}")
}

fn format_long_date(datetime: chrono::DateTime<Tz>) -> String {
    datetime.format("%A, %B %d, %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LauncherSettings;

    #[test]
    fn context_home_results_include_time_media_and_date() {
        let settings = LauncherSettings::default();
        let results = context_home_results(&settings);
        assert!(results.iter().any(|result| result.title.contains(':')));
        assert!(results.iter().any(|result| result.title.contains("Play / Pause")));
        assert!(results.iter().any(|result| result.title.contains(',')));
    }

    #[test]
    fn search_context_actions_finds_media_controls() {
        let settings = LauncherSettings::default();
        let results = search_context_actions("next track", &settings);
        assert!(results.iter().any(|result| result.title == "Next track"));
    }
}