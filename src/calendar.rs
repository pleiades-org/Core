use crate::{
    command::{CommandAction, CommandCategory, CommandResult, FeatureAction},
    search_text::normalize_search_text,
};
use chrono::{Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

const CALENDAR_FILE_NAME: &str = "calendar.toml";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CalendarEventRecord {
    pub title: String,
    pub start_text: String,
    pub duration_minutes: u32,
    pub meeting_url: Option<String>,
    pub attendees: Vec<String>,
    pub created_at: i64,
}

#[derive(Default, Deserialize, Serialize)]
struct CalendarStore {
    events: Vec<CalendarEventRecord>,
}

pub fn search_calendar(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();
    if trimmed_search_text.is_empty()
        || trimmed_search_text.eq_ignore_ascii_case("my schedule")
        || trimmed_search_text.eq_ignore_ascii_case("schedule")
    {
        return schedule_results();
    }

    if let Some(event_text) = trimmed_search_text
        .strip_prefix("add ")
        .or_else(|| trimmed_search_text.strip_prefix("new "))
        .or_else(|| trimmed_search_text.strip_prefix("block "))
    {
        if let Some(event) = parse_calendar_event(event_text) {
            return vec![CommandResult::feature(
                format!("Add calendar event {}", event.title),
                calendar_event_summary(&event),
                CommandCategory::Calendar,
                FeatureAction::SaveCalendarEvent {
                    title: event.title,
                    start_text: event.start_text,
                    duration_minutes: event.duration_minutes,
                    meeting_url: event.meeting_url,
                    attendees: event.attendees,
                },
                96,
            )];
        }
    }

    let normalized_search_text = normalize_search_text(trimmed_search_text);
    let mut results = upcoming_events()
        .into_iter()
        .filter(|event| {
            normalize_search_text(&event.title).contains(&normalized_search_text)
                || normalize_search_text(&event.start_text).contains(&normalized_search_text)
                || event
                    .meeting_url
                    .as_ref()
                    .is_some_and(|url| normalize_search_text(url).contains(&normalized_search_text))
        })
        .flat_map(calendar_event_results)
        .collect::<Vec<_>>();

    if normalized_search_text.contains("join") {
        results.retain(|result| result.title.to_lowercase().contains("join"));
    }
    if normalized_search_text.contains("email") {
        results.retain(|result| result.title.to_lowercase().contains("email"));
    }

    results
}

pub fn next_event_prompt_result() -> Option<CommandResult> {
    upcoming_events()
        .into_iter()
        .next()
        .map(|event| CommandResult {
            title: format!("Next event: {}", event.title),
            subtitle: calendar_event_summary(&event),
            copy_text: calendar_event_details(&event),
            explanation: event
                .meeting_url
                .clone()
                .map(|_| "Press Enter to join".to_string()),
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Calendar,
            action: event
                .meeting_url
                .clone()
                .map(CommandAction::OpenUrl)
                .unwrap_or_else(|| CommandAction::CopyToClipboard(calendar_event_details(&event))),
            confidence: 48,
        })
}

pub fn save_calendar_event(
    title: String,
    start_text: String,
    duration_minutes: u32,
    meeting_url: Option<String>,
    attendees: Vec<String>,
) -> io::Result<()> {
    let mut store = load_calendar_store();
    store.events.push(CalendarEventRecord {
        title,
        start_text,
        duration_minutes,
        meeting_url,
        attendees,
        created_at: Local::now().timestamp(),
    });
    store.events.sort_by_key(calendar_sort_key);
    save_calendar_store(&store)
}

fn schedule_results() -> Vec<CommandResult> {
    let events = upcoming_events();
    if events.is_empty() {
        return vec![CommandResult::informational(
            "My Schedule",
            "Use @calendar add Title | 2026-05-21 09:00 | 30 | meeting-url",
        )];
    }

    events
        .into_iter()
        .take(6)
        .flat_map(calendar_event_results)
        .collect()
}

fn calendar_event_results(event: CalendarEventRecord) -> Vec<CommandResult> {
    let mut results = vec![CommandResult::copyable_feature(
        format!("Copy details for {}", event.title),
        calendar_event_summary(&event),
        calendar_event_details(&event),
        CommandCategory::Calendar,
        84,
    )];

    if let Some(meeting_url) = event.meeting_url.clone() {
        results.insert(
            0,
            CommandResult {
                title: format!("Join {}", event.title),
                subtitle: meeting_url.clone(),
                copy_text: meeting_url.clone(),
                explanation: Some("Opens Zoom, Google Meet, Teams, or any meeting URL".to_string()),
                icon_path: None,
                calculation_display: None,
                category: CommandCategory::Calendar,
                action: CommandAction::OpenUrl(meeting_url),
                confidence: 92,
            },
        );
    }

    if !event.attendees.is_empty() {
        let mailto_url = format!(
            "mailto:{}?subject={}",
            event.attendees.join(","),
            url_encode(&event.title)
        );
        results.push(CommandResult {
            title: format!("Email attendees for {}", event.title),
            subtitle: event.attendees.join(", "),
            copy_text: mailto_url.clone(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Calendar,
            action: CommandAction::OpenUrl(mailto_url),
            confidence: 80,
        });
    }

    results
}

fn upcoming_events() -> Vec<CalendarEventRecord> {
    let now = Local::now().timestamp();
    let mut events = load_calendar_store()
        .events
        .into_iter()
        .filter(|event| calendar_sort_key(event) >= now)
        .collect::<Vec<_>>();
    events.sort_by_key(calendar_sort_key);
    events
}

fn parse_calendar_event(event_text: &str) -> Option<CalendarEventRecord> {
    let parts = event_text
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }

    let title = parts[0].to_string();
    let start_text = parts[1].to_string();
    let duration_minutes = parts
        .get(2)
        .and_then(|duration_text| parse_duration_minutes(duration_text))
        .unwrap_or(30);
    let meeting_url = parts
        .get(3)
        .filter(|url| looks_like_url(url))
        .map(|url| (*url).to_string());
    let attendees = parts
        .get(4)
        .map(|attendees_text| {
            attendees_text
                .split(',')
                .map(str::trim)
                .filter(|attendee| !attendee.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(CalendarEventRecord {
        title,
        start_text,
        duration_minutes,
        meeting_url,
        attendees,
        created_at: Local::now().timestamp(),
    })
}

fn parse_duration_minutes(duration_text: &str) -> Option<u32> {
    let normalized_duration_text = duration_text.trim().to_lowercase();
    if let Some(hour_text) = normalized_duration_text
        .strip_suffix('h')
        .or_else(|| normalized_duration_text.strip_suffix("hr"))
        .or_else(|| normalized_duration_text.strip_suffix("hrs"))
    {
        return hour_text.trim().parse::<u32>().ok().map(|hours| hours * 60);
    }

    normalized_duration_text
        .trim_end_matches("minutes")
        .trim_end_matches("minute")
        .trim_end_matches("mins")
        .trim_end_matches("min")
        .trim_end_matches('m')
        .trim()
        .parse::<u32>()
        .ok()
}

fn calendar_sort_key(event: &CalendarEventRecord) -> i64 {
    parse_event_start_timestamp(&event.start_text).unwrap_or(i64::MAX)
}

fn parse_event_start_timestamp(start_text: &str) -> Option<i64> {
    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %I:%M%p"] {
        if let Ok(datetime) = NaiveDateTime::parse_from_str(start_text, format) {
            return Local
                .from_local_datetime(&datetime)
                .single()
                .map(|datetime| datetime.timestamp());
        }
    }

    None
}

fn calendar_event_summary(event: &CalendarEventRecord) -> String {
    format!(
        "{} for {} minutes{}",
        event.start_text,
        event.duration_minutes,
        event
            .meeting_url
            .as_ref()
            .map(|_| " - meeting link")
            .unwrap_or("")
    )
}

fn calendar_event_details(event: &CalendarEventRecord) -> String {
    let mut details = vec![
        event.title.clone(),
        format!("When: {}", event.start_text),
        format!("Duration: {} minutes", event.duration_minutes),
    ];

    if let Some(meeting_url) = event.meeting_url.as_ref() {
        details.push(format!("Join: {meeting_url}"));
    }
    if !event.attendees.is_empty() {
        details.push(format!("Attendees: {}", event.attendees.join(", ")));
    }

    details.join("\n")
}

fn load_calendar_store() -> CalendarStore {
    fs::read_to_string(calendar_file_path())
        .ok()
        .and_then(|store_text| toml::from_str(&store_text).ok())
        .unwrap_or_default()
}

fn save_calendar_store(store: &CalendarStore) -> io::Result<()> {
    let calendar_path = calendar_file_path();
    if let Some(calendar_directory) = calendar_path.parent() {
        fs::create_dir_all(calendar_directory)?;
    }

    let calendar_text = toml::to_string_pretty(store).unwrap_or_default();
    fs::write(calendar_path, calendar_text)
}

fn calendar_file_path() -> PathBuf {
    crate::paths::data_file(CALENDAR_FILE_NAME)
}

fn looks_like_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => (byte as char).to_string(),
            b' ' => "%20".to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calendar_event() {
        let event =
            parse_calendar_event("Review | 2026-05-21 09:00 | 45 | https://meet.test").unwrap();

        assert_eq!(event.title, "Review");
        assert_eq!(event.duration_minutes, 45);
        assert_eq!(event.meeting_url.as_deref(), Some("https://meet.test"));
    }
}
