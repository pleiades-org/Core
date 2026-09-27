mod date;
mod parse;
use super::Calculation;
use crate::time_conversion::{ClockTime, Date};
pub use parse::parse_calendar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarError {
    Syntax,
    Range,
    Unavailable,
}

impl CalendarError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Syntax => "Try 2 days from now, next Friday, or 2026-12-25 - 2026-09-21",
            Self::Range => "Use whole date offsets and valid dates between 1900 and 2100",
            Self::Unavailable => "The local date and time could not be read",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalMoment {
    pub date: Date,
    pub time: ClockTime,
}

/// Evaluated only for relative-date queries. Elapsed minutes must be added in UTC before converting locally.
pub trait CalendarClock: Send {
    fn local_after_minutes(&self, minutes: i64) -> Result<LocalMoment, CalendarError>;

    /// Seconds since 1970-01-01 00:00 UTC.
    fn unix_now(&self) -> Result<i64, CalendarError> {
        Err(CalendarError::Unavailable)
    }

    /// Local wall-clock date and minute for a Unix time, using the zone rules in effect then.
    fn local_from_unix(&self, _seconds: i64) -> Result<LocalMoment, CalendarError> {
        Err(CalendarError::Unavailable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarUnit {
    Day,
    Week,
    Month,
    Year,
    Hour,
    Minute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarRequest {
    Offset {
        amount: i64,
        unit: CalendarUnit,
        base: Option<Date>,
    },
    Weekday {
        weekday: usize,
        forward: bool,
    },
    Difference {
        end: Date,
        start: Date,
    },
}

impl CalendarRequest {
    pub fn evaluate(self, clock: Option<&dyn CalendarClock>) -> Result<Calculation, CalendarError> {
        match self {
            Self::Difference { end, start } => {
                let days = date::ordinal(end) - date::ordinal(start);
                Ok(Calculation {
                    title: format!("{days} {}", if days.abs() == 1 { "day" } else { "days" }),
                    detail: format!("{start} → {end} · signed calendar difference"),
                    copy: days.to_string(),
                })
            }
            Self::Weekday { weekday, forward } => {
                let today = local(clock, 0)?.date;
                let distance = if forward {
                    (weekday as i64 - date::weekday(today) as i64).rem_euclid(7)
                } else {
                    (date::weekday(today) as i64 - weekday as i64).rem_euclid(7)
                };
                let distance = if distance == 0 { 7 } else { distance };
                date_answer(date::add_days(
                    today,
                    if forward { distance } else { -distance },
                )?)
            }
            Self::Offset { amount, unit, base } => evaluate_offset(amount, unit, base, clock),
        }
    }
}

fn local(clock: Option<&dyn CalendarClock>, minutes: i64) -> Result<LocalMoment, CalendarError> {
    let moment = clock
        .ok_or(CalendarError::Unavailable)?
        .local_after_minutes(minutes)?;
    date::validate(moment.date)?;
    Ok(moment)
}

fn evaluate_offset(
    amount: i64,
    unit: CalendarUnit,
    base: Option<Date>,
    clock: Option<&dyn CalendarClock>,
) -> Result<Calculation, CalendarError> {
    if matches!(unit, CalendarUnit::Hour | CalendarUnit::Minute) {
        if base.is_some() {
            return Err(CalendarError::Syntax);
        }
        let minutes = amount
            .checked_mul(if unit == CalendarUnit::Hour { 60 } else { 1 })
            .ok_or(CalendarError::Range)?;
        let moment = local(clock, minutes)?;
        return Ok(Calculation {
            title: moment.time.to_string(),
            detail: format!(
                "{} · {} · local time",
                date::WEEKDAYS[date::weekday(moment.date)],
                date::friendly(moment.date)
            ),
            copy: format!(
                "{} {:02}:{:02}",
                moment.date, moment.time.hour, moment.time.minute
            ),
        });
    }
    let base = match base {
        Some(base) => base,
        None => local(clock, 0)?.date,
    };
    let result = match unit {
        CalendarUnit::Day => date::add_days(base, amount),
        CalendarUnit::Week => {
            date::add_days(base, amount.checked_mul(7).ok_or(CalendarError::Range)?)
        }
        CalendarUnit::Month => date::add_months(base, amount),
        CalendarUnit::Year => {
            date::add_months(base, amount.checked_mul(12).ok_or(CalendarError::Range)?)
        }
        CalendarUnit::Hour | CalendarUnit::Minute => unreachable!("elapsed units handled above"),
    }?;
    date_answer(result)
}

fn date_answer(date: Date) -> Result<Calculation, CalendarError> {
    Ok(Calculation {
        title: date::friendly(date),
        detail: format!(
            "{} · {} · calendar date",
            date::WEEKDAYS[date::weekday(date)],
            date
        ),
        copy: date.to_string(),
    })
}
