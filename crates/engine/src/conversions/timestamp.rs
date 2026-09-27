//! Unix timestamps: `unix 1700000000`, `1700000000000 unix` (milliseconds), `unix now`.
use super::{Conversion, Outcome};
use crate::calculator::{calendar::CalendarClock, Calculation};

const MESSAGE: &str = "Enter to copy · Esc to hide";
const KEYWORDS: &[&str] = &["unix", "epoch", "timestamp", "posix"];
const NOW_WORDS: &[&str] = &["now", "current", "time"];
/// Larger magnitudes are milliseconds: 1e11 seconds would be the year 5138.
const MILLISECOND_THRESHOLD: u64 = 100_000_000_000;
/// 1900-01-01 to 9999-12-31 23:59:59 UTC.
const EARLIEST: i64 = -2_208_988_800;
const LATEST: i64 = 253_402_300_799;
const SECONDS_PER_DAY: i64 = 86_400;

pub fn convert_timestamp(input: &str, clock: Option<&dyn CalendarClock>) -> Outcome {
    let lower = input.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let keyword = |word: &&str| KEYWORDS.contains(word);
    let now_word = |word: &&str| NOW_WORDS.contains(word);
    // `unix now`, `now unix`, `now to unix`, `current unix time`, `unix timestamp`.
    let about_now = !words.is_empty()
        && words.iter().any(keyword)
        && words
            .iter()
            .all(|word| keyword(word) || now_word(word) || *word == "to" || *word == "in")
        && (words.iter().any(now_word) || words.len() >= 2);
    if about_now {
        return Some(now(clock));
    }
    let number = match words.as_slice() {
        [first, number] if keyword(first) => number,
        [number, last] if keyword(last) => number,
        [number, to, target]
            if (*to == "to" || *to == "in")
                && matches!(*target, "date" | "datetime" | "utc" | "local") =>
        {
            number
        }
        _ => return None,
    };
    let value: i64 = number.parse().ok()?;
    let (seconds, milliseconds) = if value.unsigned_abs() >= MILLISECOND_THRESHOLD {
        (value.div_euclid(1_000), true)
    } else {
        (value, false)
    };
    Some(from_unix(seconds, milliseconds, clock))
}

fn now(clock: Option<&dyn CalendarClock>) -> Result<Conversion, &'static str> {
    let seconds = clock
        .ok_or("The current time is unavailable")?
        .unix_now()
        .map_err(|error| error.message())?;
    Ok(Conversion {
        answers: vec![
            Calculation {
                title: seconds.to_string(),
                detail: format!("Unix seconds · {}", utc_text(seconds)),
                copy: seconds.to_string(),
            },
            Calculation {
                title: (seconds * 1_000).to_string(),
                detail: "Unix milliseconds".into(),
                copy: (seconds * 1_000).to_string(),
            },
        ],
        message: MESSAGE,
    })
}

fn from_unix(
    seconds: i64,
    milliseconds: bool,
    clock: Option<&dyn CalendarClock>,
) -> Result<Conversion, &'static str> {
    if !(EARLIEST..=LATEST).contains(&seconds) {
        return Err("Unix times from 1900 to 9999 are supported");
    }
    let unit = if milliseconds {
        "milliseconds"
    } else {
        "seconds"
    };
    let mut answers = Vec::with_capacity(2);
    if let Some(local) = clock.and_then(|clock| clock.local_from_unix(seconds).ok()) {
        // Time-zone offsets are whole minutes, so local seconds equal UTC seconds.
        let text = format!(
            "{} {:02}:{:02}:{:02}",
            local.date,
            local.time.hour,
            local.time.minute,
            seconds.rem_euclid(60)
        );
        answers.push(Calculation {
            title: text.clone(),
            detail: format!("Local time · Unix {unit}"),
            copy: text,
        });
    }
    let utc = utc_text(seconds);
    answers.push(Calculation {
        title: utc.clone(),
        detail: format!("UTC · ISO 8601 · Unix {unit}"),
        copy: utc,
    });
    Ok(Conversion {
        answers,
        message: MESSAGE,
    })
}

/// `YYYY-MM-DDTHH:MM:SSZ` computed without the operating system.
fn utc_text(seconds: i64) -> String {
    let days = seconds.div_euclid(SECONDS_PER_DAY);
    let time = seconds.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3_600,
        time % 3_600 / 60,
        time % 60
    )
}

/// Proleptic Gregorian date for days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        calculator::calendar::{CalendarError, LocalMoment},
        time_conversion::{ClockTime, Date},
    };

    /// A clock fixed one hour ahead of UTC.
    struct PlusOneHour;
    impl CalendarClock for PlusOneHour {
        fn local_after_minutes(&self, _: i64) -> Result<LocalMoment, CalendarError> {
            Err(CalendarError::Unavailable)
        }
        fn unix_now(&self) -> Result<i64, CalendarError> {
            Ok(1_700_000_000)
        }
        fn local_from_unix(&self, seconds: i64) -> Result<LocalMoment, CalendarError> {
            let shifted = seconds + 3_600;
            let (year, month, day) = civil_from_days(shifted.div_euclid(SECONDS_PER_DAY));
            let time = shifted.rem_euclid(SECONDS_PER_DAY);
            Ok(LocalMoment {
                date: Date {
                    year: year as u16,
                    month: month as u16,
                    day: day as u16,
                },
                time: ClockTime {
                    hour: (time / 3_600) as u16,
                    minute: (time % 3_600 / 60) as u16,
                },
            })
        }
    }

    fn titles(query: &str, clock: Option<&dyn CalendarClock>) -> Vec<String> {
        convert_timestamp(query, clock)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn unix_times_show_local_and_utc() {
        assert_eq!(
            titles("unix 1700000000", Some(&PlusOneHour)),
            ["2023-11-14 23:13:20", "2023-11-14T22:13:20Z"]
        );
        assert_eq!(titles("1700000000000 unix", None), ["2023-11-14T22:13:20Z"]);
        assert_eq!(titles("0 to date", None), ["1970-01-01T00:00:00Z"]);
        assert_eq!(titles("timestamp -86400", None), ["1969-12-31T00:00:00Z"]);
        assert_eq!(titles("epoch 951782400", None), ["2000-02-29T00:00:00Z"]);
    }

    #[test]
    fn the_current_time_needs_a_clock() {
        assert_eq!(
            titles("unix now", Some(&PlusOneHour)),
            ["1700000000", "1700000000000"]
        );
        assert_eq!(titles("now to unix", Some(&PlusOneHour))[0], "1700000000");
        assert_eq!(
            titles("unix timestamp", Some(&PlusOneHour))[0],
            "1700000000"
        );
        assert!(convert_timestamp("unix now", None).unwrap().is_err());
    }

    #[test]
    fn out_of_range_and_other_text() {
        for query in ["unix -9223372036854775808", "unix 9223372036854775807"] {
            assert!(convert_timestamp(query, None).unwrap().is_err());
        }
        assert!(convert_timestamp("unix 999999999999999", None)
            .unwrap()
            .is_err());
        for query in ["unix", "2024 to roman", "xbox", "unix abc", "now"] {
            assert!(convert_timestamp(query, None).is_none(), "{query}");
        }
    }
}
