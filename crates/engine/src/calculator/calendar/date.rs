use super::CalendarError;
use crate::time_conversion::Date;

pub(super) const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub(super) fn days_in_month(year: u16, month: u16) -> u16 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => 0,
    }
}

pub(super) fn validate(date: Date) -> Result<Date, CalendarError> {
    if !(1900..=2100).contains(&date.year)
        || date.day == 0
        || date.day > days_in_month(date.year, date.month)
    {
        return Err(CalendarError::Range);
    }
    Ok(date)
}

fn days_before_year(year: u16) -> i64 {
    let previous = i64::from(year) - 1;
    365 * previous + previous / 4 - previous / 100 + previous / 400
}

pub(super) fn ordinal(date: Date) -> i64 {
    days_before_year(date.year)
        + (1..date.month)
            .map(|month| i64::from(days_in_month(date.year, month)))
            .sum::<i64>()
        + i64::from(date.day)
        - 1
}

pub(super) fn add_days(date: Date, days: i64) -> Result<Date, CalendarError> {
    let target = ordinal(date)
        .checked_add(days)
        .ok_or(CalendarError::Range)?;
    if target < days_before_year(1900) || target >= days_before_year(2101) {
        return Err(CalendarError::Range);
    }
    // A bounded binary search avoids work proportional to the requested date offset.
    let (mut first, mut last) = (1900, 2101);
    while first + 1 < last {
        let middle = first + (last - first) / 2;
        if days_before_year(middle) <= target {
            first = middle;
        } else {
            last = middle;
        }
    }
    let mut remaining = target - days_before_year(first);
    let mut month = 1;
    while remaining >= i64::from(days_in_month(first, month)) {
        remaining -= i64::from(days_in_month(first, month));
        month += 1;
    }
    Ok(Date {
        year: first,
        month,
        day: remaining as u16 + 1,
    })
}

pub(super) fn add_months(date: Date, months: i64) -> Result<Date, CalendarError> {
    let target = (i64::from(date.year) * 12 + i64::from(date.month) - 1)
        .checked_add(months)
        .ok_or(CalendarError::Range)?;
    if !(1900 * 12..2101 * 12).contains(&target) {
        return Err(CalendarError::Range);
    }
    let year = (target / 12) as u16;
    let month = (target % 12 + 1) as u16;
    Ok(Date {
        year,
        month,
        day: date.day.min(days_in_month(year, month)),
    })
}

pub(super) fn weekday(date: Date) -> usize {
    ordinal(date).rem_euclid(7) as usize
}

pub(super) fn friendly(date: Date) -> String {
    format!(
        "{} {} {}",
        date.day,
        MONTHS[usize::from(date.month - 1)],
        date.year
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_supported_date_round_trips_and_weekdays_advance() {
        let mut previous = None;
        for year in 1900..=2100 {
            for month in 1..=12 {
                for day in 1..=days_in_month(year, month) {
                    let date = Date { year, month, day };
                    assert_eq!(add_days(date, 0), Ok(date));
                    if let Some(previous) = previous {
                        assert_eq!(add_days(previous, 1), Ok(date));
                        assert_eq!(weekday(date), (weekday(previous) + 1) % 7);
                    }
                    previous = Some(date);
                }
            }
        }
        assert_eq!(
            weekday(Date {
                year: 2026,
                month: 9,
                day: 21
            }),
            0
        );
        assert!(add_days(
            Date {
                year: 1900,
                month: 1,
                day: 1
            },
            -1
        )
        .is_err());
        assert!(add_days(
            Date {
                year: 2100,
                month: 12,
                day: 31
            },
            1
        )
        .is_err());
    }
}
