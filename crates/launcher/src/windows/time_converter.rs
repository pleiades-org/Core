use core_engine::calculator::calendar::{CalendarClock, CalendarError, LocalMoment};
use core_engine::time_conversion::{
    ClockTime, ConvertedTime, Date, TimeConverter, TimeError, TimeRequest, TimeZone,
};
use windows::Win32::{
    Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME, SYSTEMTIME},
    System::{
        SystemInformation::{GetSystemTime, GetSystemTimeAsFileTime},
        Time::*,
    },
};

const ZONE_NAMES: &[(TimeZone, &str)] = &[
    (TimeZone::Eastern, "Eastern Standard Time"),
    (TimeZone::Central, "Central Standard Time"),
    (TimeZone::Mountain, "Mountain Standard Time"),
    (TimeZone::Pacific, "Pacific Standard Time"),
    (TimeZone::London, "GMT Standard Time"),
    (TimeZone::Berlin, "W. Europe Standard Time"),
    (TimeZone::Sydney, "AUS Eastern Standard Time"),
    (TimeZone::Tokyo, "Tokyo Standard Time"),
    (TimeZone::India, "India Standard Time"),
];

pub struct WindowsCalendarClock;

/// 100-nanosecond FILETIME ticks between 1601-01-01 and the Unix epoch.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;

impl CalendarClock for WindowsCalendarClock {
    fn unix_now(&self) -> Result<i64, CalendarError> {
        let now = unsafe { GetSystemTimeAsFileTime() };
        let ticks = (i64::from(now.dwHighDateTime) << 32) | i64::from(now.dwLowDateTime);
        Ok((ticks - UNIX_EPOCH_TICKS).div_euclid(TICKS_PER_SECOND))
    }

    fn local_from_unix(&self, seconds: i64) -> Result<LocalMoment, CalendarError> {
        let ticks = seconds
            .checked_mul(TICKS_PER_SECOND)
            .and_then(|ticks| ticks.checked_add(UNIX_EPOCH_TICKS))
            .filter(|ticks| *ticks >= 0)
            .ok_or(CalendarError::Range)?;
        let file_time = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.map_err(|_| CalendarError::Range)?;
        let mut local = SYSTEMTIME::default();
        unsafe { SystemTimeToTzSpecificLocalTimeEx(None, &utc, &mut local) }
            .map_err(|_| CalendarError::Unavailable)?;
        Ok(LocalMoment {
            date: date_of(&local),
            time: ClockTime {
                hour: local.wHour,
                minute: local.wMinute,
            },
        })
    }

    fn local_after_minutes(&self, minutes: i64) -> Result<LocalMoment, CalendarError> {
        let minutes = i32::try_from(minutes).map_err(|_| CalendarError::Range)?;
        let utc =
            shift_minutes(unsafe { GetSystemTime() }, minutes).map_err(|_| CalendarError::Range)?;
        let mut local = SYSTEMTIME::default();
        unsafe { SystemTimeToTzSpecificLocalTimeEx(None, &utc, &mut local) }
            .map_err(|_| CalendarError::Unavailable)?;
        Ok(LocalMoment {
            date: date_of(&local),
            time: ClockTime {
                hour: local.wHour,
                minute: local.wMinute,
            },
        })
    }
}

/// Created cheaply with the search worker; only the first regional conversion loads zone keys.
#[derive(Default)]
pub struct WindowsTimeConverter {
    zones: Option<Vec<(TimeZone, DYNAMIC_TIME_ZONE_INFORMATION)>>,
}

impl TimeConverter for WindowsTimeConverter {
    fn convert(&mut self, request: TimeRequest) -> Result<ConvertedTime, TimeError> {
        self.convert_at(request, unsafe { GetSystemTime() })
    }
}

impl WindowsTimeConverter {
    fn convert_at(
        &mut self,
        request: TimeRequest,
        now: SYSTEMTIME,
    ) -> Result<ConvertedTime, TimeError> {
        let source_date = match request.date {
            Some(date) => date,
            None => date_of(&self.local_time(request.source, now)?),
        };
        let local = SYSTEMTIME {
            wYear: source_date.year,
            wMonth: source_date.month,
            wDay: source_date.day,
            wHour: request.time.hour,
            wMinute: request.time.minute,
            ..Default::default()
        };
        let utc = self.universal_time(request.source, local)?;
        let destination = self.local_time(request.destination, utc)?;
        Ok(ConvertedTime {
            source_date,
            destination_date: date_of(&destination),
            time: ClockTime {
                hour: destination.wHour,
                minute: destination.wMinute,
            },
        })
    }

    fn zone(&mut self, zone: TimeZone) -> Result<&DYNAMIC_TIME_ZONE_INFORMATION, TimeError> {
        if self.zones.is_none() {
            self.zones = Some(load_zones()?);
        }
        self.zones
            .as_ref()
            .expect("zone keys loaded")
            .iter()
            .find(|(identifier, _)| *identifier == zone)
            .map(|(_, information)| information)
            .ok_or(TimeError::Unavailable)
    }

    fn local_time(&mut self, zone: TimeZone, utc: SYSTEMTIME) -> Result<SYSTEMTIME, TimeError> {
        if let Some(minutes) = zone.fixed_minutes() {
            return shift_minutes(utc, minutes);
        }
        local_with_rules(self.zone(zone)?, utc)
    }

    fn universal_time(
        &mut self,
        zone: TimeZone,
        local: SYSTEMTIME,
    ) -> Result<SYSTEMTIME, TimeError> {
        if let Some(minutes) = zone.fixed_minutes() {
            return shift_minutes(local, -minutes);
        }
        let information = self.zone(zone)?;
        let mut rules = TIME_ZONE_INFORMATION::default();
        unsafe { GetTimeZoneInformationForYear(local.wYear, Some(information), &mut rules) }
            .map_err(|_| TimeError::Unavailable)?;
        let standard_bias = rules.Bias + rules.StandardBias;
        let daylight_bias = rules.Bias + rules.DaylightBias;
        let mut match_found = None;
        // Round-tripping both offsets detects spring gaps and autumn overlaps explicitly.
        for bias in [
            Some(standard_bias),
            (rules.DaylightDate.wMonth != 0 && daylight_bias != standard_bias)
                .then_some(daylight_bias),
        ]
        .into_iter()
        .flatten()
        {
            let candidate = shift_minutes(local, bias)?;
            let round_trip = local_with_rules(information, candidate)?;
            if same_minute(&round_trip, &local) {
                if match_found.is_some() {
                    return Err(TimeError::Ambiguous);
                }
                match_found = Some(candidate);
            }
        }
        match_found.ok_or(TimeError::Nonexistent)
    }
}

fn load_zones() -> Result<Vec<(TimeZone, DYNAMIC_TIME_ZONE_INFORMATION)>, TimeError> {
    let mut zones = Vec::with_capacity(ZONE_NAMES.len());
    for index in 0.. {
        let mut information = DYNAMIC_TIME_ZONE_INFORMATION::default();
        match unsafe { EnumDynamicTimeZoneInformation(index, &mut information) } {
            status if status == ERROR_NO_MORE_ITEMS.0 => return Ok(zones),
            status if status == ERROR_SUCCESS.0 => {}
            _ => return Err(TimeError::Unavailable),
        }
        let length = information
            .TimeZoneKeyName
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(information.TimeZoneKeyName.len());
        if let Some((zone, _)) = ZONE_NAMES.iter().find(|(_, name)| {
            name.encode_utf16()
                .eq(information.TimeZoneKeyName[..length].iter().copied())
        }) {
            zones.push((*zone, information));
            if zones.len() == ZONE_NAMES.len() {
                return Ok(zones);
            }
        }
    }
    unreachable!("time zone enumeration ends with a Windows status")
}

fn local_with_rules(
    information: &DYNAMIC_TIME_ZONE_INFORMATION,
    utc: SYSTEMTIME,
) -> Result<SYSTEMTIME, TimeError> {
    let mut local = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(information), &utc, &mut local) }
        .map_err(|_| TimeError::Unavailable)?;
    Ok(local)
}

fn shift_minutes(time: SYSTEMTIME, minutes: i32) -> Result<SYSTEMTIME, TimeError> {
    const TICKS_PER_MINUTE: i64 = 600_000_000;
    let mut file_time = FILETIME::default();
    unsafe { SystemTimeToFileTime(&time, &mut file_time) }.map_err(|_| TimeError::InvalidDate)?;
    let ticks = (u64::from(file_time.dwHighDateTime) << 32) | u64::from(file_time.dwLowDateTime);
    let shifted = ticks
        .checked_add_signed(i64::from(minutes) * TICKS_PER_MINUTE)
        .ok_or(TimeError::InvalidDate)?;
    let file_time = FILETIME {
        dwLowDateTime: shifted as u32,
        dwHighDateTime: (shifted >> 32) as u32,
    };
    let mut result = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&file_time, &mut result) }.map_err(|_| TimeError::InvalidDate)?;
    Ok(result)
}

fn date_of(time: &SYSTEMTIME) -> Date {
    Date {
        year: time.wYear,
        month: time.wMonth,
        day: time.wDay,
    }
}

fn same_minute(left: &SYSTEMTIME, right: &SYSTEMTIME) -> bool {
    date_of(left) == date_of(right) && left.wHour == right.wHour && left.wMinute == right.wMinute
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elapsed_hours_cross_dst_in_utc_before_local_conversion() {
        let mut converter = WindowsTimeConverter::default();
        for (month, day, hour, expected) in [(3, 29, 0, 2), (10, 25, 0, 1)] {
            let utc = SYSTEMTIME {
                wYear: 2026,
                wMonth: month,
                wDay: day,
                wHour: hour,
                ..Default::default()
            };
            let shifted = shift_minutes(utc, 60).unwrap();
            let local = converter.local_time(TimeZone::London, shifted).unwrap();
            assert_eq!(local.wHour, expected);
        }
        let mut engine = SearchEngine::with_time_converter(WindowsTimeConverter::default())
            .with_calendar_clock(WindowsCalendarClock);
        for query in [
            "2 days from now",
            "3 hours ago",
            "in 10 minutes",
            "now",
            "time",
            "  TiMe  ",
            "date",
            "  DaTe  ",
            "@calc time",
            "@calc date",
            "@calc tomorrow",
        ] {
            let batch = engine.search(query, &ApplicationCatalog::default());
            assert_eq!(batch.results.len(), 1, "{query}: {}", batch.message);
            assert_eq!(batch.results[0].kind, ResultKind::Date);
        }
    }
    use core_engine::{
        applications::ApplicationCatalog,
        search::{Action, ResultKind, SearchEngine},
        time_conversion::parse_time,
    };

    #[test]
    fn daylight_saving_mismatch_weeks_and_date_rollovers_are_correct() {
        let mut converter = WindowsTimeConverter::default();
        for (query, expected) in [
            ("9pm et to uk on 2026-01-15", "2:00 am 2026-01-16"),
            ("9pm et to uk on 2026-07-15", "2:00 am 2026-07-16"),
            ("9pm et to uk on 2026-03-10", "1:00 am 2026-03-11"),
            ("9pm et to uk on 2026-10-28", "1:00 am 2026-10-29"),
            ("9pm et to uk on 2026-12-31", "2:00 am 2027-01-01"),
            ("1am uk to pt on 2026-01-01", "5:00 pm 2025-12-31"),
            ("9pm utc to india on 2026-01-01", "2:30 am 2026-01-02"),
            ("9pm sydney to uk on 2026-01-01", "10:00 am 2026-01-01"),
            ("9pm tokyo to berlin on 2026-07-01", "2:00 pm 2026-07-01"),
        ] {
            let converted = converter.convert(parse_time(query).unwrap()).unwrap();
            assert_eq!(
                format!("{} {}", converted.time, converted.destination_date),
                expected,
                "{query}"
            );
        }
    }

    #[test]
    fn rejects_nonexistent_and_ambiguous_local_times() {
        let mut converter = WindowsTimeConverter::default();
        for (query, expected) in [
            ("2:30am et to uk on 2026-03-08", TimeError::Nonexistent),
            ("1:30am et to uk on 2026-11-01", TimeError::Ambiguous),
            ("1:30am uk to et on 2026-03-29", TimeError::Nonexistent),
            ("1:30am uk to et on 2026-10-25", TimeError::Ambiguous),
        ] {
            assert_eq!(
                converter.convert(parse_time(query).unwrap()).unwrap_err(),
                expected,
                "{query}"
            );
        }
        let daylight = converter
            .convert(parse_time("1:30am edt to utc on 2026-11-01").unwrap())
            .unwrap();
        let standard = converter
            .convert(parse_time("1:30am est to utc on 2026-11-01").unwrap())
            .unwrap();
        assert_eq!(daylight.time.hour, 5);
        assert_eq!(standard.time.hour, 6);
    }

    #[test]
    fn today_uses_source_date_even_when_utc_has_already_rolled_over() {
        let now = SYSTEMTIME {
            wYear: 2026,
            wMonth: 9,
            wDay: 21,
            wHour: 1,
            ..Default::default()
        };
        let result = WindowsTimeConverter::default()
            .convert_at(parse_time("9pm et to uk").unwrap(), now)
            .unwrap();
        assert_eq!(result.source_date.to_string(), "2026-09-20");
        assert_eq!(result.destination_date.to_string(), "2026-09-21");
        assert_eq!(result.time.to_string(), "2:00 am");
    }

    #[test]
    fn implicit_and_scoped_queries_produce_copyable_dated_results() {
        let mut engine = SearchEngine::with_time_converter(WindowsTimeConverter::default());
        for prefix in ["", "@time ", "@tz ", "@calc "] {
            let batch = engine.search(
                &format!("{prefix}9pm et to uk on 2026-03-10"),
                &ApplicationCatalog::default(),
            );
            assert_eq!(batch.results.len(), 1);
            assert_eq!(batch.results[0].kind, ResultKind::Time);
            assert_eq!(&*batch.results[0].title, "1:00 am UK · next day");
            assert_eq!(
                batch.results[0].action,
                Action::CopyText("1:00 am UK on 2026-03-11".into())
            );
        }
        let invalid = engine.search("9pm et to unknown", &ApplicationCatalog::default());
        assert!(invalid.results.is_empty());
        assert_eq!(invalid.message, TimeError::UnknownZone.message());
    }
}
