use super::{date::WEEKDAYS, CalendarError, CalendarRequest, CalendarUnit};
use crate::time_conversion::{parse_date, Date};

pub fn parse_calendar(input: &str) -> Option<Result<CalendarRequest, CalendarError>> {
    let first = input.split_whitespace().next()?;
    let numeric = first
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_digit() || b"+-".contains(byte));
    let starts_date = first.len() == 10 && first.as_bytes().get(4) == Some(&b'-');
    let named = [
        "date",
        "time",
        "today",
        "tomorrow",
        "yesterday",
        "now",
        "next",
        "last",
        "in",
    ]
    .iter()
    .any(|word| first.eq_ignore_ascii_case(word));
    if !numeric && !named {
        return None;
    }
    let relative = numeric
        && input.split_whitespace().any(|word| {
            ["ago", "from", "after", "before"]
                .iter()
                .any(|marker| word.eq_ignore_ascii_case(marker))
        });
    if !starts_date && !named && !relative {
        return None;
    }
    // A fixed token buffer caps parsing work without a token-vector allocation.
    let mut words = [""; 8];
    let mut count = 0;
    for word in input.split_whitespace() {
        if count == words.len() {
            return starts_date.then_some(Err(CalendarError::Syntax));
        }
        words[count] = word;
        count += 1;
    }
    // Do not steal app names such as "Nextcloud",
            "next time i open the launcher please show me my apps", "In Design" or "Days Gone".
    if !starts_date && !is_calendar_shape(&words[..count]) {
        return None;
    }
    Some(parse_words(&words[..count]))
}

fn is_calendar_shape(words: &[&str]) -> bool {
    match words {
        [single] => ["date", "time", "today", "tomorrow", "yesterday", "now"]
            .iter()
            .any(|word| single.eq_ignore_ascii_case(word)),
        [direction, weekday]
            if direction.eq_ignore_ascii_case("next") || direction.eq_ignore_ascii_case("last") =>
        {
            weekday_number(weekday).is_some()
        }
        [prefix, _, unit] if prefix.eq_ignore_ascii_case("in") => parse_unit(unit).is_some(),
        [_, unit, ..] => parse_unit(unit).is_some(),
        _ => false,
    }
}

fn parse_words(words: &[&str]) -> Result<CalendarRequest, CalendarError> {
    use CalendarRequest::*;
    match words {
        [single] if single.eq_ignore_ascii_case("time") || single.eq_ignore_ascii_case("now") => {
            offset(0, CalendarUnit::Minute, None)
        }
        [single] if single.eq_ignore_ascii_case("date") || single.eq_ignore_ascii_case("today") => {
            offset(0, CalendarUnit::Day, None)
        }
        [single] if single.eq_ignore_ascii_case("tomorrow") => offset(1, CalendarUnit::Day, None),
        [single] if single.eq_ignore_ascii_case("yesterday") => offset(-1, CalendarUnit::Day, None),
        [direction, weekday] => Ok(Weekday {
            weekday: weekday_number(weekday).ok_or(CalendarError::Syntax)?,
            forward: direction.eq_ignore_ascii_case("next"),
        }),
        [prefix, amount, unit] if prefix.eq_ignore_ascii_case("in") => {
            offset(number(amount)?, unit_of(unit)?, None)
        }
        [amount, unit, suffix] if suffix.eq_ignore_ascii_case("ago") => offset(
            number(amount)?.checked_neg().ok_or(CalendarError::Range)?,
            unit_of(unit)?,
            None,
        ),
        [amount, unit, relation, base]
            if relation.eq_ignore_ascii_case("from")
                || relation.eq_ignore_ascii_case("after")
                || relation.eq_ignore_ascii_case("before") =>
        {
            let amount = number(amount)?;
            let amount = if relation.eq_ignore_ascii_case("before") {
                amount.checked_neg().ok_or(CalendarError::Range)?
            } else {
                amount
            };
            let base = if base.eq_ignore_ascii_case("now") || base.eq_ignore_ascii_case("today") {
                None
            } else {
                Some(date_of(base)?)
            };
            offset(amount, unit_of(unit)?, base)
        }
        [base, operator, amount, unit] if matches!(*operator, "+" | "-") => {
            let amount = number(amount)?;
            offset(
                if *operator == "-" {
                    amount.checked_neg().ok_or(CalendarError::Range)?
                } else {
                    amount
                },
                unit_of(unit)?,
                Some(date_of(base)?),
            )
        }
        [end, "-", start] => Ok(Difference {
            end: date_of(end)?,
            start: date_of(start)?,
        }),
        _ => Err(CalendarError::Syntax),
    }
}

fn offset(
    amount: i64,
    unit: CalendarUnit,
    base: Option<Date>,
) -> Result<CalendarRequest, CalendarError> {
    Ok(CalendarRequest::Offset { amount, unit, base })
}
fn number(text: &str) -> Result<i64, CalendarError> {
    text.parse().map_err(|_| CalendarError::Range)
}
fn date_of(text: &str) -> Result<Date, CalendarError> {
    parse_date(text).map_err(|_| CalendarError::Range)
}
fn unit_of(text: &str) -> Result<CalendarUnit, CalendarError> {
    parse_unit(text).ok_or(CalendarError::Syntax)
}
fn weekday_number(text: &str) -> Option<usize> {
    WEEKDAYS.iter().position(|weekday| {
        text.eq_ignore_ascii_case(weekday) || text.eq_ignore_ascii_case(&weekday[..3])
    })
}
fn parse_unit(text: &str) -> Option<CalendarUnit> {
    use CalendarUnit::*;
    [
        ("day", Day),
        ("week", Week),
        ("month", Month),
        ("year", Year),
        ("hour", Hour),
        ("minute", Minute),
    ]
    .into_iter()
    .find_map(|(name, unit)| {
        let singular = text.strip_suffix(['s', 'S']).unwrap_or(text);
        singular.eq_ignore_ascii_case(name).then_some(unit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        calculator::calendar::{CalendarClock, LocalMoment},
        time_conversion::ClockTime,
    };
    struct FixedClock;
    impl CalendarClock for FixedClock {
        fn local_after_minutes(&self, minutes: i64) -> Result<LocalMoment, CalendarError> {
            assert_eq!(minutes, 0);
            Ok(LocalMoment {
                date: date_of("2026-09-21")?,
                time: ClockTime {
                    hour: 12,
                    minute: 30,
                },
            })
        }
    }
    #[test]
    fn relative_dates_weekdays_month_clamping_and_differences() {
        for (query, expected) in [
            ("time", "2026-09-21 12:30"),
            ("  TiMe  ", "2026-09-21 12:30"),
            ("now", "2026-09-21 12:30"),
            ("date", "2026-09-21"),
            ("  DaTe  ", "2026-09-21"),
            ("today", "2026-09-21"),
            ("2 days from now", "2026-09-23"),
            ("IN 2 WEEKS", "2026-10-05"),
            ("3 days ago", "2026-09-18"),
            ("tomorrow", "2026-09-22"),
            ("next Monday", "2026-09-28"),
            ("last Mon", "2026-09-14"),
            ("next Friday", "2026-09-25"),
            ("2028-01-31 + 1 month", "2028-02-29"),
            ("1 year after 2028-02-29", "2029-02-28"),
            ("2 days before 2026-01-01", "2025-12-30"),
            ("2026-12-25 - 2026-09-21", "95"),
            ("2026-09-21 - 2026-12-25", "-95"),
        ] {
            assert_eq!(
                parse_calendar(query)
                    .unwrap()
                    .unwrap()
                    .evaluate(Some(&FixedClock))
                    .unwrap()
                    .copy,
                expected,
                "{query}"
            );
        }
    }
    #[test]
    fn malformed_offsets_and_dates_fail_without_wrapping_or_stealing_apps() {
        for query in [
            "1.5 days from now",
            "2026-09-21 + 1 day and another and another day",
            "2026-02-30 + 1 day",
            "9223372036854775807 weeks from now",
            "-9223372036854775808 days ago",
            "2100-12-31 + 1 day",
            "1900-01-01 - 1 month",
        ] {
            assert!(
                parse_calendar(query)
                    .unwrap()
                    .and_then(|request| request.evaluate(Some(&FixedClock)))
                    .is_err(),
                "{query}"
            );
        }
        for query in [
            "Nextcloud",
            "next time i open the launcher please show me my apps",
            "In Design",
            "2 days in hours",
            "Microsoft To Do",
            "After Effects",
            "Time Tracker",
            "Date Planner",
            "timeless",
            "database",
        ] {
            assert!(parse_calendar(query).is_none(), "{query}");
        }
    }
}
