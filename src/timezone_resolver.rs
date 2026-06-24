use crate::settings::LauncherSettings;
use chrono_tz::{Tz, TZ_VARIANTS};
use once_cell::sync::Lazy;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeZoneResolution {
    pub original_text: String,
    pub timezone: Tz,
    pub display_name: String,
}

static TIMEZONE_ALIASES: Lazy<HashMap<&'static str, (&'static str, &'static str)>> =
    Lazy::new(|| {
        HashMap::from([
            ("local", ("", "Local time")),
            ("my time", ("", "Local time")),
            ("pt", ("America/Los_Angeles", "PT")),
            ("pacific", ("America/Los_Angeles", "PT")),
            ("pacific time", ("America/Los_Angeles", "PT")),
            ("pst", ("America/Los_Angeles", "PT")),
            ("pdt", ("America/Los_Angeles", "PT")),
            ("et", ("America/New_York", "ET")),
            ("eastern", ("America/New_York", "ET")),
            ("eastern time", ("America/New_York", "ET")),
            ("est", ("America/New_York", "ET")),
            ("edt", ("America/New_York", "ET")),
            ("ct", ("America/Chicago", "CT")),
            ("central", ("America/Chicago", "CT")),
            ("central time", ("America/Chicago", "CT")),
            ("mt", ("America/Denver", "MT")),
            ("mountain", ("America/Denver", "MT")),
            ("mountain time", ("America/Denver", "MT")),
            ("uk", ("Europe/London", "UK")),
            ("gb", ("Europe/London", "UK")),
            ("london", ("Europe/London", "London")),
            ("gmt", ("Europe/London", "UK")),
            ("bst", ("Europe/London", "UK")),
            ("utc", ("UTC", "UTC")),
            ("zulu", ("UTC", "UTC")),
            ("cet", ("Europe/Paris", "Central European Time")),
            ("paris", ("Europe/Paris", "Paris time")),
            ("berlin", ("Europe/Berlin", "Berlin time")),
            ("tokyo", ("Asia/Tokyo", "Tokyo time")),
            ("jst", ("Asia/Tokyo", "Japan Standard Time")),
            ("india", ("Asia/Kolkata", "India time")),
            ("ist", ("Asia/Kolkata", "India time")),
            ("sydney", ("Australia/Sydney", "Sydney time")),
            ("australia eastern", ("Australia/Sydney", "Sydney time")),
        ])
    });

pub fn resolve_timezone(
    timezone_text: &str,
    settings: &LauncherSettings,
) -> Option<TimeZoneResolution> {
    let normalized_timezone = normalize_timezone_text(timezone_text);

    if let Some((iana_name, display_name)) = TIMEZONE_ALIASES.get(normalized_timezone.as_str()) {
        let timezone_name = if iana_name.is_empty() {
            settings.local_timezone.as_str()
        } else {
            iana_name
        };

        return timezone_name
            .parse::<Tz>()
            .ok()
            .map(|timezone| TimeZoneResolution {
                original_text: timezone_text.trim().to_string(),
                timezone,
                display_name: (*display_name).to_string(),
            });
    }

    timezone_text
        .trim()
        .parse::<Tz>()
        .ok()
        .or_else(|| resolve_timezone_from_known_names(&normalized_timezone))
        .map(|timezone| TimeZoneResolution {
            original_text: timezone_text.trim().to_string(),
            display_name: display_name_from_timezone(timezone),
            timezone,
        })
}

pub fn local_timezone(settings: &LauncherSettings) -> Tz {
    settings
        .local_timezone
        .parse::<Tz>()
        .unwrap_or(chrono_tz::Europe::London)
}

fn normalize_timezone_text(timezone_text: &str) -> String {
    timezone_text
        .trim()
        .to_lowercase()
        .replace(['_', '-', '/'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_timezone_from_known_names(normalized_timezone: &str) -> Option<Tz> {
    TZ_VARIANTS
        .iter()
        .copied()
        .find(|timezone| normalize_timezone_text(timezone.name()) == normalized_timezone)
        .or_else(|| {
            TZ_VARIANTS.iter().copied().find(|timezone| {
                timezone
                    .name()
                    .rsplit('/')
                    .next()
                    .map(normalize_timezone_text)
                    .as_deref()
                    == Some(normalized_timezone)
            })
        })
}

fn display_name_from_timezone(timezone: Tz) -> String {
    let location_name = timezone
        .name()
        .rsplit('/')
        .next()
        .unwrap_or(timezone.name());
    let readable_location = location_name
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ");

    format!("{readable_location} time")
}

fn capitalize_word(word: &str) -> String {
    let mut characters = word.chars();
    let Some(first_character) = characters.next() else {
        return String::new();
    };

    format!(
        "{}{}",
        first_character.to_uppercase(),
        characters.as_str().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_pt_to_los_angeles() {
        let settings = LauncherSettings::default();
        let resolved_timezone = resolve_timezone("pt", &settings).unwrap();

        assert_eq!(resolved_timezone.timezone, chrono_tz::America::Los_Angeles);
    }

    #[test]
    fn resolves_uk_to_london() {
        let settings = LauncherSettings::default();
        let resolved_timezone = resolve_timezone("uk", &settings).unwrap();

        assert_eq!(resolved_timezone.timezone, chrono_tz::Europe::London);
    }

    #[test]
    fn resolves_city_name_from_known_timezone_database() {
        let settings = LauncherSettings::default();
        let resolved_timezone = resolve_timezone("new york", &settings).unwrap();

        assert_eq!(resolved_timezone.timezone, chrono_tz::America::New_York);
    }

    #[test]
    fn resolves_full_timezone_name_case_insensitively() {
        let settings = LauncherSettings::default();
        let resolved_timezone = resolve_timezone("asia dubai", &settings).unwrap();

        assert_eq!(resolved_timezone.timezone, chrono_tz::Asia::Dubai);
    }
}
