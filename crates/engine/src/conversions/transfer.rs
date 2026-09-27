//! Rate × time problems: `10 GB at 100 Mbps` (download time), `10 GB in 10 min` (speed needed),
//! `100 Mbps for 2 h` (data used) and `5 km in 25 min` (pace and speed).
use super::{
    duration::parse_duration,
    format::{duration, format_number, readable, significant},
    units::{parse_measure, Dimension, Unit},
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const LEADING_WORDS: &[&str] = &[
    "how", "long", "to", "time", "download", "upload", "transfer", "copy",
];
const RATE_WORDS: &[&str] = &["at", "@", "over", "on", "with"];
const DURATION_WORDS: &[&str] = &["in", "within", "for"];
const DOWNLOAD_MESSAGE: &str = "Enter to copy · ideal time; real transfers add protocol overhead";
const SPEED_MESSAGE: &str = "Enter to copy number · Esc to hide";
const DECIMAL_SIZES: &[(&str, f64)] = &[
    ("PB", 1e15),
    ("TB", 1e12),
    ("GB", 1e9),
    ("MB", 1e6),
    ("kB", 1e3),
    ("B", 1.),
];
const BINARY_SIZES: &[(&str, f64)] = &[
    ("PiB", 1_125_899_906_842_624.),
    ("TiB", 1_099_511_627_776.),
    ("GiB", 1_073_741_824.),
    ("MiB", 1_048_576.),
    ("KiB", 1_024.),
    ("B", 1.),
];
const BIT_RATES: &[(&str, f64)] = &[
    ("Tbps", 1.25e11),
    ("Gbps", 1.25e8),
    ("Mbps", 1.25e5),
    ("kbps", 125.),
    ("bps", 0.125),
];

pub fn calculate_transfer(input: &str) -> Outcome {
    let mut words = input.split_whitespace().skip_while(|word| {
        LEADING_WORDS
            .iter()
            .any(|known| word.eq_ignore_ascii_case(known))
    });
    let (amount, unit) = parse_measure(&mut words)?;
    let connector = words.next()?.to_ascii_lowercase();
    let rest: Vec<&str> = words.collect();
    if rest.is_empty() || amount <= 0. {
        return None;
    }
    let is = |list: &[&str]| list.contains(&connector.as_str());
    match unit.dimension {
        Dimension::Data if is(RATE_WORDS) => {
            let (rate, rate_unit) = parse_measure(&mut rest.iter().copied())
                .filter(|(_, unit)| unit.dimension == Dimension::DataRate)?;
            Some(download_time(amount, unit, rate, rate_unit))
        }
        Dimension::Data if is(DURATION_WORDS) => {
            let seconds = parse_duration(&rest.join(" "))?;
            Some(Ok(required_speed(amount, unit, seconds)))
        }
        Dimension::DataRate if is(DURATION_WORDS) => {
            let seconds = parse_duration(&rest.join(" "))?;
            Some(Ok(data_for(amount, unit, seconds)))
        }
        Dimension::Length if is(DURATION_WORDS) => {
            let seconds = parse_duration(&rest.join(" "))?;
            Some(Ok(pace(amount, unit, seconds)))
        }
        _ => None,
    }
}

fn download_time(
    amount: f64,
    unit: &Unit,
    rate: f64,
    rate_unit: &Unit,
) -> Result<Conversion, &'static str> {
    if rate <= 0. {
        return Err("The transfer speed must be greater than zero");
    }
    let seconds = amount * unit.scale / (rate * rate_unit.scale);
    Ok(Conversion::single(
        Calculation {
            title: duration(seconds),
            detail: format!(
                "{} {} at {} {} · {} s",
                format_number(amount),
                unit.symbol,
                format_number(rate),
                rate_unit.symbol,
                readable(seconds)
            ),
            copy: duration(seconds),
        },
        DOWNLOAD_MESSAGE,
    ))
}

fn required_speed(amount: f64, unit: &Unit, seconds: f64) -> Conversion {
    let bytes_per_second = amount * unit.scale / seconds;
    let summary = format!(
        "{} {} in {}",
        format_number(amount),
        unit.symbol,
        duration(seconds)
    );
    let (bits_name, bits_scale) = scaled(bytes_per_second, BIT_RATES);
    let megabytes = bytes_per_second / 1e6;
    Conversion {
        answers: vec![
            answer(
                bytes_per_second / bits_scale,
                bits_name,
                format!("{summary} · speed needed"),
            ),
            answer(
                megabytes,
                "MB/s",
                format!("{summary} · megabytes per second"),
            ),
        ],
        message: SPEED_MESSAGE,
    }
}

fn data_for(rate: f64, unit: &Unit, seconds: f64) -> Conversion {
    let bytes = rate * unit.scale * seconds;
    let (name, scale) = scaled(bytes, DECIMAL_SIZES);
    let (binary_name, binary_scale) = scaled(bytes, BINARY_SIZES);
    Conversion::single(
        answer(
            bytes / scale,
            name,
            format!(
                "{} {} for {} · {} {}",
                format_number(rate),
                unit.symbol,
                duration(seconds),
                readable(bytes / binary_scale),
                binary_name
            ),
        ),
        SPEED_MESSAGE,
    )
}

/// Pace per kilometre for metric distances, per mile otherwise.
fn pace(distance: f64, unit: &Unit, seconds: f64) -> Conversion {
    let metres = distance * unit.scale;
    let imperial = matches!(unit.symbol, "mi" | "yd" | "ft" | "in");
    let (per, per_metres) = if imperial {
        ("mi", 1_609.344)
    } else {
        ("km", 1_000.)
    };
    let pace_seconds = seconds / (metres / per_metres);
    let kmh = metres / seconds * 3.6;
    let mph = kmh / 1.609344;
    let summary = format!(
        "{} {} in {}",
        format_number(distance),
        unit.symbol,
        duration(seconds)
    );
    let pace_text = format!("{} /{per}", clock(pace_seconds));
    let (speed, speed_unit, other, other_unit) = if imperial {
        (mph, "mph", kmh, "km/h")
    } else {
        (kmh, "km/h", mph, "mph")
    };
    Conversion {
        answers: vec![
            Calculation {
                title: pace_text.clone(),
                detail: format!("{summary} · pace"),
                copy: pace_text,
            },
            answer(
                speed,
                speed_unit,
                format!("{summary} · {} {other_unit}", readable(other)),
            ),
        ],
        message: SPEED_MESSAGE,
    }
}

fn answer(value: f64, unit: &str, detail: String) -> Calculation {
    let rounded = significant(value, 4);
    Calculation {
        title: format!("{} {unit}", readable(value)),
        detail,
        copy: format_number(rounded),
    }
}

/// The largest unit that keeps the value at least 1.
fn scaled(value: f64, units: &'static [(&'static str, f64)]) -> (&'static str, f64) {
    units
        .iter()
        .copied()
        .find(|(_, scale)| value >= *scale)
        .unwrap_or(*units.last().expect("units"))
}

/// `m:ss` or `h:mm:ss`.
fn clock(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3_600, total % 3_600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        calculate_transfer(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn download_times_use_bits_and_bytes_correctly() {
        assert_eq!(titles("10 GB at 100 Mbps"), ["13 min 20 s"]);
        assert_eq!(titles("download 4.7GB at 50mbps"), ["12 min 32 s"]);
        assert_eq!(
            titles("how long to download 1 TB @ 1 Gbps"),
            ["2 h 13 min 20 s"]
        );
        assert_eq!(titles("100 GiB over 10 MB/s"), ["2 h 58 min 57 s"]);
        assert!(calculate_transfer("1 GB at 0 Mbps").unwrap().is_err());
    }

    #[test]
    fn required_speed_data_used_and_pace() {
        assert_eq!(titles("10 GB in 10 min"), ["133.3 Mbps", "16.67 MB/s"]);
        assert_eq!(titles("100 Mbps for 1 h"), ["45 GB"]);
        assert_eq!(titles("5 km in 25 min"), ["5:00 /km", "12 km/h"]);
        assert_eq!(titles("26.2 mi in 3:30:00"), ["8:01 /mi", "7.486 mph"]);
    }

    #[test]
    fn plain_unit_conversions_and_other_text_are_not_transfers() {
        for query in [
            "10 GB in MB",
            "2 days in hours",
            "10 kg in 5 min",
            "5 in to cm",
            "xbox",
            "10 GB",
        ] {
            assert!(calculate_transfer(query).is_none(), "{query}");
        }
    }
}
