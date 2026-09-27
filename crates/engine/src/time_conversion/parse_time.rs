use super::{ClockTime, Date, TimeError, TimeRequest, TimeZone};

/// Restrict implicit routing so ordinary application names keep using app search.
pub fn recognizes_time(query: &str) -> bool {
    let mut words = query.split_whitespace();
    let Some(clock) = words.next() else {
        return false;
    };
    if !clock.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    let mut source = words.next().unwrap_or("");
    if is_period(source) {
        source = words.next().unwrap_or("");
    }
    parse_zone(source).is_ok() && words.any(|word| word.eq_ignore_ascii_case("to"))
}

pub fn parse_time(query: &str) -> Result<TimeRequest, TimeError> {
    let mut words = query.split_whitespace().peekable();
    let clock = words.next().ok_or(TimeError::Syntax)?;
    let period = words.peek().copied().filter(|word| is_period(word));
    if period.is_some() {
        words.next();
    }
    let time = parse_clock(clock, period)?;
    let source = parse_zone(words.next().ok_or(TimeError::Syntax)?)?;
    if !words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("to"))
    {
        return Err(TimeError::Syntax);
    }
    let destination = parse_zone(words.next().ok_or(TimeError::Syntax)?)?;
    let date = match words.next() {
        None => None,
        Some(word) if word.eq_ignore_ascii_case("on") => {
            Some(parse_date(words.next().ok_or(TimeError::InvalidDate)?)?)
        }
        _ => return Err(TimeError::Syntax),
    };
    if words.next().is_some() {
        return Err(TimeError::Syntax);
    }
    Ok(TimeRequest {
        time,
        source,
        destination,
        date,
    })
}

fn is_period(word: &str) -> bool {
    word.eq_ignore_ascii_case("am") || word.eq_ignore_ascii_case("pm")
}

fn parse_clock(clock: &str, separate_period: Option<&str>) -> Result<ClockTime, TimeError> {
    let suffix = clock
        .len()
        .checked_sub(2)
        .and_then(|start| clock.get(start..))
        .filter(|text| is_period(text));
    if suffix.is_some() && separate_period.is_some() {
        return Err(TimeError::InvalidTime);
    }
    let digits = if suffix.is_some() {
        &clock[..clock.len() - 2]
    } else {
        clock
    };
    let (hour, minute) = digits.split_once(':').unwrap_or((digits, "00"));
    if hour.is_empty()
        || hour.len() > 2
        || minute.len() != 2
        || !hour
            .bytes()
            .chain(minute.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(TimeError::InvalidTime);
    }
    let mut hour: u16 = hour.parse().map_err(|_| TimeError::InvalidTime)?;
    let minute: u16 = minute.parse().map_err(|_| TimeError::InvalidTime)?;
    if let Some(period) = separate_period.or(suffix) {
        if !(1..=12).contains(&hour) {
            return Err(TimeError::InvalidTime);
        }
        hour = hour % 12
            + if period.eq_ignore_ascii_case("pm") {
                12
            } else {
                0
            };
    }
    if hour > 23 || minute > 59 {
        return Err(TimeError::InvalidTime);
    }
    Ok(ClockTime { hour, minute })
}

fn parse_zone(word: &str) -> Result<TimeZone, TimeError> {
    use TimeZone::*;
    const ALIASES: &[(&str, TimeZone)] = &[
        ("et", Eastern),
        ("eastern", Eastern),
        ("nyc", Eastern),
        ("new_york", Eastern),
        ("america/new_york", Eastern),
        ("ct", Central),
        ("chicago", Central),
        ("america/chicago", Central),
        ("mt", Mountain),
        ("denver", Mountain),
        ("america/denver", Mountain),
        ("pt", Pacific),
        ("pacific", Pacific),
        ("la", Pacific),
        ("america/los_angeles", Pacific),
        ("uk", London),
        ("london", London),
        ("europe/london", London),
        ("berlin", Berlin),
        ("europe/berlin", Berlin),
        ("sydney", Sydney),
        ("australia/sydney", Sydney),
        ("tokyo", Tokyo),
        ("jst", Tokyo),
        ("asia/tokyo", Tokyo),
        ("india", India),
        ("kolkata", India),
        ("asia/kolkata", India),
        ("utc", Utc),
        ("gmt", Utc),
        ("est", Est),
        ("edt", Edt),
        ("cst", Cst),
        ("cdt", Cdt),
        ("mst", Mst),
        ("mdt", Mdt),
        ("pst", Pst),
        ("pdt", Pdt),
        ("bst", Bst),
    ];
    ALIASES
        .iter()
        .find(|(alias, _)| word.eq_ignore_ascii_case(alias))
        .map(|(_, zone)| *zone)
        .ok_or(TimeError::UnknownZone)
}

pub fn parse_date(text: &str) -> Result<Date, TimeError> {
    let bytes = text.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return Err(TimeError::InvalidDate);
    }
    let year: u16 = text[..4].parse().map_err(|_| TimeError::InvalidDate)?;
    let month: u16 = text[5..7].parse().map_err(|_| TimeError::InvalidDate)?;
    let day: u16 = text[8..].parse().map_err(|_| TimeError::InvalidDate)?;
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        1..=12 => 31,
        _ => 0,
    };
    if !(1900..=2100).contains(&year) || day == 0 || day > days {
        return Err(TimeError::InvalidDate);
    }
    Ok(Date { year, month, day })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_twelve_and_twenty_four_hour_times_case_and_dates() {
        for query in [
            "9pm et to uk",
            "9 PM ET TO London",
            "21:00 America/New_York to Europe/London",
        ] {
            let request = parse_time(query).unwrap();
            assert_eq!(
                request.time,
                ClockTime {
                    hour: 21,
                    minute: 0
                }
            );
            assert_eq!(request.source, TimeZone::Eastern);
            assert_eq!(request.destination, TimeZone::London);
            assert_eq!(request.date, None);
        }
        assert_eq!(
            parse_time("12am utc to india on 2028-02-29")
                .unwrap()
                .time
                .hour,
            0
        );
        assert_eq!(parse_time("12:30pm uk to et").unwrap().time.hour, 12);
        assert_eq!(parse_time("23:59 utc to uk").unwrap().time.minute, 59);
    }

    #[test]
    fn rejects_invalid_times_dates_unknown_zones_and_trailing_text() {
        for clock in [
            "24:00", "0pm", "13am", "9:60pm", "9:3pm", "9pmpm", "9pm pm", "-1", "９pm",
        ] {
            assert!(parse_time(&format!("{clock} et to uk")).is_err(), "{clock}");
        }
        for date in [
            "2026-02-29",
            "1900-02-29",
            "2026-04-31",
            "2026-13-01",
            "2026-00-01",
            "2026-01-00",
            "2026-1-01",
            "2101-01-01",
            "２０２６-01-01",
        ] {
            assert_eq!(
                parse_time(&format!("9pm et to uk on {date}")),
                Err(TimeError::InvalidDate),
                "{date}"
            );
        }
        assert_eq!(parse_time("9pm ist to uk"), Err(TimeError::UnknownZone));
        assert_eq!(parse_time("9pm et to uk tomorrow"), Err(TimeError::Syntax));
        assert_eq!(
            parse_time("9pm et to uk on 2026-01-01 extra"),
            Err(TimeError::Syntax)
        );
    }

    #[test]
    fn implicit_detection_preserves_application_queries() {
        assert!(recognizes_time("9pm et to uk"));
        for query in ["7 zip", "Go To Meeting", "2+2", "", "@app 9pm et to uk"] {
            assert!(!recognizes_time(query));
        }
    }
}
