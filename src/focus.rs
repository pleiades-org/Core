use crate::command::{CommandCategory, CommandResult, FeatureAction};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs, io,
    path::PathBuf,
    process::{Command, Stdio},
};

const FOCUS_SESSION_FILE_NAME: &str = "focus_session.toml";
const HOSTS_BEGIN_MARKER: &str = "# Core Launcher focus block begin";
const HOSTS_END_MARKER: &str = "# Core Launcher focus block end";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusSession {
    goal: String,
    started_at: i64,
    ends_at: i64,
    paused_until: Option<i64>,
    blocked_categories: Vec<String>,
    blocked_apps: Vec<String>,
    blocked_sites: Vec<String>,
}

#[derive(Clone, Copy)]
struct FocusCategory {
    name: &'static str,
    sites: &'static [&'static str],
    apps: &'static [&'static str],
}

pub fn search_focus_commands(search_text: &str) -> Vec<CommandResult> {
    let trimmed_search_text = search_text.trim();

    if trimmed_search_text.is_empty() {
        return focus_home_results();
    }

    let normalized_search_text = trimmed_search_text.to_lowercase();
    if normalized_search_text == "status" || normalized_search_text == "session" {
        return active_focus_status_result().into_iter().collect();
    }

    if normalized_search_text.starts_with("pause") {
        return vec![CommandResult::feature(
            "Pause focus session",
            "Temporarily allow blocked apps and sites",
            CommandCategory::Focus,
            FeatureAction::PauseFocusSession,
            95,
        )];
    }

    if normalized_search_text.starts_with("resume") {
        return vec![CommandResult::feature(
            "Resume focus session",
            "Re-enable focus blocking",
            CommandCategory::Focus,
            FeatureAction::ResumeFocusSession,
            95,
        )];
    }

    if normalized_search_text.starts_with("end") || normalized_search_text.starts_with("stop") {
        return vec![CommandResult::feature(
            "End focus session",
            "Clear active app and website blocks",
            CommandCategory::Focus,
            FeatureAction::EndFocusSession,
            95,
        )];
    }

    if normalized_search_text.starts_with("snooze") {
        let minutes = parse_first_duration_minutes(trimmed_search_text).unwrap_or(5);
        return vec![CommandResult::feature(
            format!("Snooze focus for {minutes} minutes"),
            "Blocks will resume automatically",
            CommandCategory::Focus,
            FeatureAction::SnoozeFocusSession { minutes },
            94,
        )];
    }

    vec![focus_start_result(trimmed_search_text)]
}

pub fn start_focus_session(
    duration_minutes: u32,
    goal: String,
    categories: Vec<String>,
) -> io::Result<()> {
    let now = Local::now().timestamp();
    let selected_categories = selected_focus_categories(&categories);
    let blocked_apps = selected_categories
        .iter()
        .flat_map(|category| category.apps.iter().copied())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let blocked_sites = selected_categories
        .iter()
        .flat_map(|category| category.sites.iter().copied())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let session = FocusSession {
        goal,
        started_at: now,
        ends_at: now + i64::from(duration_minutes) * 60,
        paused_until: None,
        blocked_categories: selected_categories
            .iter()
            .map(|category| category.name.to_string())
            .collect(),
        blocked_apps,
        blocked_sites,
    };

    save_focus_session(&session)?;
    let _ = apply_hosts_blocks(&session);
    Ok(())
}

pub fn pause_focus_session() -> io::Result<()> {
    let Some(mut session) = load_focus_session() else {
        return Ok(());
    };

    session.paused_until = Some(session.ends_at);
    save_focus_session(&session)?;
    let _ = clear_hosts_blocks();
    Ok(())
}

pub fn resume_focus_session() -> io::Result<()> {
    let Some(mut session) = load_focus_session() else {
        return Ok(());
    };

    session.paused_until = None;
    save_focus_session(&session)?;
    let _ = apply_hosts_blocks(&session);
    Ok(())
}

pub fn snooze_focus_session(minutes: u32) -> io::Result<()> {
    let Some(mut session) = load_focus_session() else {
        return Ok(());
    };

    session.paused_until = Some(Local::now().timestamp() + i64::from(minutes) * 60);
    save_focus_session(&session)?;
    let _ = clear_hosts_blocks();
    Ok(())
}

pub fn end_focus_session() -> io::Result<()> {
    let session_path = focus_session_file_path();
    if session_path.exists() {
        fs::remove_file(session_path)?;
    }

    let _ = clear_hosts_blocks();
    Ok(())
}

pub fn enforce_active_focus_session() {
    let Some(mut session) = load_focus_session() else {
        return;
    };

    let now = Local::now().timestamp();
    if now >= session.ends_at {
        let _ = end_focus_session();
        return;
    }

    if let Some(paused_until) = session.paused_until {
        if now < paused_until {
            return;
        }

        session.paused_until = None;
        let _ = save_focus_session(&session);
        let _ = apply_hosts_blocks(&session);
    }

    for process_name in &session.blocked_apps {
        terminate_process_by_name(process_name);
    }
}

fn focus_home_results() -> Vec<CommandResult> {
    let mut results = Vec::new();
    if let Some(active_status_result) = active_focus_status_result() {
        results.push(active_status_result);
        results.extend([
            CommandResult::feature(
                "Pause focus session",
                "Temporarily allow blocked apps and sites",
                CommandCategory::Focus,
                FeatureAction::PauseFocusSession,
                88,
            ),
            CommandResult::feature(
                "End focus session",
                "Clear active app and website blocks",
                CommandCategory::Focus,
                FeatureAction::EndFocusSession,
                86,
            ),
        ]);
        return results;
    }

    results.extend([
        focus_start_result("25 social"),
        focus_start_result("50 social shopping"),
        focus_start_result("90 deep work social shopping video"),
    ]);
    results
}

fn active_focus_status_result() -> Option<CommandResult> {
    let session = load_focus_session()?;
    let now = Local::now().timestamp();
    if now >= session.ends_at {
        let _ = end_focus_session();
        return None;
    }

    let remaining_minutes = ((session.ends_at - now) / 60).max(1);
    let paused_label = session
        .paused_until
        .filter(|paused_until| *paused_until > now)
        .map(|paused_until| format!("Snoozed for {} more minutes", (paused_until - now) / 60))
        .unwrap_or_else(|| "Blocking is active".to_string());

    Some(CommandResult::informational(
        format!("Focus: {} minutes left", remaining_minutes),
        format!(
            "{} - {} - {}",
            session.goal,
            session.blocked_categories.join(", "),
            paused_label
        ),
    ))
}

fn focus_start_result(search_text: &str) -> CommandResult {
    let duration_minutes = parse_first_duration_minutes(search_text).unwrap_or(25);
    let categories = parse_focus_categories(search_text);
    let selected_category_names = if categories.is_empty() {
        vec!["social".to_string()]
    } else {
        categories
    };
    let goal = focus_goal(search_text, duration_minutes, &selected_category_names);

    CommandResult::feature(
        format!("Start {duration_minutes} minute focus session"),
        format!("{} - blocking {}", goal, selected_category_names.join(", ")),
        CommandCategory::Focus,
        FeatureAction::StartFocusSession {
            duration_minutes,
            goal,
            categories: selected_category_names,
        },
        92,
    )
}

fn parse_first_duration_minutes(search_text: &str) -> Option<u32> {
    for word in search_text.split_whitespace() {
        let normalized_word =
            word.trim_matches(|character: char| !character.is_ascii_alphanumeric());
        if let Some(hour_text) = normalized_word
            .strip_suffix('h')
            .or_else(|| normalized_word.strip_suffix("hr"))
            .or_else(|| normalized_word.strip_suffix("hrs"))
        {
            return hour_text.parse::<u32>().ok().map(|hours| hours * 60);
        }

        if let Some(minute_text) = normalized_word
            .strip_suffix('m')
            .or_else(|| normalized_word.strip_suffix("min"))
            .or_else(|| normalized_word.strip_suffix("mins"))
        {
            return minute_text.parse::<u32>().ok();
        }

        if let Ok(minutes) = normalized_word.parse::<u32>() {
            return Some(minutes);
        }
    }

    None
}

fn parse_focus_categories(search_text: &str) -> Vec<String> {
    let normalized_search_text = search_text.to_lowercase();
    focus_categories()
        .iter()
        .filter(|category| normalized_search_text.contains(category.name))
        .map(|category| category.name.to_string())
        .collect()
}

fn selected_focus_categories(category_names: &[String]) -> Vec<FocusCategory> {
    let selected_names = if category_names.is_empty() {
        vec!["social".to_string()]
    } else {
        category_names.to_vec()
    };

    focus_categories()
        .iter()
        .copied()
        .filter(|category| {
            selected_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(category.name))
        })
        .collect()
}

fn focus_goal(search_text: &str, duration_minutes: u32, categories: &[String]) -> String {
    let duration_text = duration_minutes.to_string();
    let mut goal_words = Vec::new();

    for word in search_text.split_whitespace() {
        let normalized_word = word
            .trim_matches(|character: char| !character.is_ascii_alphanumeric())
            .to_lowercase();
        if normalized_word == duration_text
            || normalized_word.ends_with('m')
            || normalized_word.ends_with("min")
            || normalized_word.ends_with('h')
            || categories
                .iter()
                .any(|category| category.eq_ignore_ascii_case(&normalized_word))
        {
            continue;
        }

        goal_words.push(word);
    }

    let goal = goal_words.join(" ");
    if goal.trim().is_empty() {
        "Focused work".to_string()
    } else {
        goal
    }
}

fn focus_categories() -> &'static [FocusCategory] {
    &[
        FocusCategory {
            name: "social",
            sites: &[
                "facebook.com",
                "instagram.com",
                "reddit.com",
                "tiktok.com",
                "x.com",
                "twitter.com",
            ],
            apps: &["Discord.exe", "Slack.exe", "Teams.exe"],
        },
        FocusCategory {
            name: "shopping",
            sites: &["amazon.com", "ebay.com", "etsy.com", "walmart.com"],
            apps: &[],
        },
        FocusCategory {
            name: "video",
            sites: &["youtube.com", "netflix.com", "twitch.tv", "hulu.com"],
            apps: &["vlc.exe", "obs64.exe"],
        },
        FocusCategory {
            name: "news",
            sites: &["news.google.com", "cnn.com", "bbc.com", "nytimes.com"],
            apps: &[],
        },
        FocusCategory {
            name: "games",
            sites: &["steampowered.com", "epicgames.com"],
            apps: &["Steam.exe", "EpicGamesLauncher.exe", "Battle.net.exe"],
        },
    ]
}

fn terminate_process_by_name(process_name: &str) {
    let _ = Command::new("taskkill")
        .args(["/IM", process_name, "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn load_focus_session() -> Option<FocusSession> {
    fs::read_to_string(focus_session_file_path())
        .ok()
        .and_then(|session_text| toml::from_str(&session_text).ok())
}

fn save_focus_session(session: &FocusSession) -> io::Result<()> {
    let session_path = focus_session_file_path();
    if let Some(session_directory) = session_path.parent() {
        fs::create_dir_all(session_directory)?;
    }

    let session_text = toml::to_string_pretty(session).unwrap_or_default();
    fs::write(session_path, session_text)
}

fn focus_session_file_path() -> PathBuf {
    crate::paths::data_file(FOCUS_SESSION_FILE_NAME)
}

#[cfg(target_os = "windows")]
fn apply_hosts_blocks(session: &FocusSession) -> io::Result<()> {
    let hosts_path = windows_hosts_file_path();
    let existing_hosts_text = fs::read_to_string(&hosts_path).unwrap_or_default();
    let mut hosts_text = remove_focus_hosts_section(&existing_hosts_text);
    hosts_text.push('\n');
    hosts_text.push_str(HOSTS_BEGIN_MARKER);
    hosts_text.push('\n');
    for blocked_site in &session.blocked_sites {
        hosts_text.push_str(&format!("0.0.0.0 {blocked_site}\n"));
        hosts_text.push_str(&format!("0.0.0.0 www.{blocked_site}\n"));
    }
    hosts_text.push_str(HOSTS_END_MARKER);
    hosts_text.push('\n');

    fs::write(hosts_path, hosts_text)
}

#[cfg(not(target_os = "windows"))]
fn apply_hosts_blocks(_session: &FocusSession) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn clear_hosts_blocks() -> io::Result<()> {
    let hosts_path = windows_hosts_file_path();
    let existing_hosts_text = fs::read_to_string(&hosts_path).unwrap_or_default();
    fs::write(hosts_path, remove_focus_hosts_section(&existing_hosts_text))
}

#[cfg(not(target_os = "windows"))]
fn clear_hosts_blocks() -> io::Result<()> {
    Ok(())
}

fn remove_focus_hosts_section(hosts_text: &str) -> String {
    let mut filtered_lines = Vec::new();
    let mut is_inside_focus_section = false;

    for line in hosts_text.lines() {
        if line.trim() == HOSTS_BEGIN_MARKER {
            is_inside_focus_section = true;
            continue;
        }

        if line.trim() == HOSTS_END_MARKER {
            is_inside_focus_section = false;
            continue;
        }

        if !is_inside_focus_section {
            filtered_lines.push(line);
        }
    }

    filtered_lines.join("\n")
}

#[cfg(target_os = "windows")]
fn windows_hosts_file_path() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hour_duration() {
        assert_eq!(parse_first_duration_minutes("2h social"), Some(120));
    }

    #[test]
    fn removes_hosts_section() {
        let hosts_text =
            "a\n# Core Launcher focus block begin\nb\n# Core Launcher focus block end\nc";

        assert_eq!(remove_focus_hosts_section(hosts_text), "a\nc");
    }
}
