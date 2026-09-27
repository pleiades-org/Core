//! Physical and digital unit conversion: `10 kg to lb`, `4.7GB in MiB`, `30 mpg to l/100km`.
mod catalog;

use super::{
    format::{format_number, significant},
    quantity::{parse_number, split_number_prefix},
};
use crate::calculator::Calculation;
use catalog::UNITS;

const SIGNIFICANT_DIGITS: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Length,
    Mass,
    Duration,
    Volume,
    Temperature,
    Area,
    Speed,
    Data,
    DataRate,
    Energy,
    Power,
    Pressure,
    Angle,
    Frequency,
    FuelEconomy,
}

pub struct Unit {
    /// Case-sensitive spellings, checked before any case-insensitive alias.
    exact: &'static [&'static str],
    aliases: &'static [&'static str],
    pub symbol: &'static str,
    pub dimension: Dimension,
    /// Base units per one of this unit; for inverse units, `base = scale / amount`.
    pub scale: f64,
    offset: f64,
    inverse: bool,
}

impl Unit {
    pub fn base_value(&self, amount: f64) -> f64 {
        if self.inverse {
            self.scale / amount
        } else {
            amount * self.scale + self.offset
        }
    }

    pub fn unit_value(&self, base: f64) -> f64 {
        if self.inverse {
            self.scale / base
        } else {
            (base - self.offset) / self.scale
        }
    }
}

pub fn find_unit(text: &str) -> Option<&'static Unit> {
    UNITS
        .iter()
        .find(|unit| unit.exact.contains(&text))
        .or_else(|| {
            UNITS.iter().find(|unit| {
                unit.aliases
                    .iter()
                    .any(|alias| text.eq_ignore_ascii_case(alias))
            })
        })
}

/// A number with a unit, written `4.7GB` or `4.7 GB`. Consumes one or two tokens.
pub fn parse_measure<'text>(
    words: &mut impl Iterator<Item = &'text str>,
) -> Option<(f64, &'static Unit)> {
    let (number, attached) = split_number_prefix(words.next()?)?;
    let amount = parse_number(number)?;
    let unit = find_unit(if attached.is_empty() {
        words.next()?
    } else {
        attached
    })?;
    Some((amount, unit))
}

pub fn convert_units(input: &str) -> Option<Result<Calculation, &'static str>> {
    let mut words = input.split_whitespace();
    let quantity = words.next()?;
    // Compact inputs such as 100cm and spaced inputs share the same conversion path.
    let (quantity, attached_unit) = split_number_prefix(quantity)?;
    let source = find_unit(if attached_unit.is_empty() {
        words.next()?
    } else {
        attached_unit
    })?;
    let separator = words.next()?;
    if !["to", "in", "into", "as"]
        .iter()
        .any(|word| separator.eq_ignore_ascii_case(word))
    {
        return None;
    }
    let result = (|| {
        let amount = parse_number(quantity).ok_or("Enter a valid number before the unit")?;
        let destination = find_unit(
            words
                .next()
                .ok_or("Add a destination unit, such as kg to lb")?,
        )
        .ok_or("Unknown destination unit")?;
        if words.next().is_some() {
            return Err("Use a number and two units, such as 10 kg to lb");
        }
        convert(amount, source, destination)
    })();
    Some(result)
}

fn convert(amount: f64, source: &Unit, destination: &Unit) -> Result<Calculation, &'static str> {
    if source.dimension != destination.dimension {
        return Err("Those units measure different things");
    }
    let converted = match source.dimension {
        Dimension::Temperature => {
            let kelvin = source.base_value(amount);
            if kelvin < -1e-10 {
                return Err("Temperature cannot be below absolute zero");
            }
            (destination.unit_value(kelvin.max(0.)) * 1e10).round() / 1e10
        }
        _ if source.inverse || destination.inverse => {
            if amount <= 0. {
                return Err("Fuel economy must be greater than zero");
            }
            destination.unit_value(source.base_value(amount))
        }
        _ => amount * (source.scale / destination.scale),
    };
    if !converted.is_finite() {
        return Err("Result is outside the supported numeric range");
    }
    // Twelve significant figures hide binary noise (4.535923700000001) without losing precision.
    let number = format_number(significant(converted, SIGNIFICANT_DIGITS));
    Ok(Calculation {
        title: format!("{number} {}", destination.symbol),
        detail: format!(
            "{} {} → {}",
            format_number(amount),
            source.symbol,
            destination.symbol
        ),
        copy: number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(query: &str) -> f64 {
        convert_units(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .copy
            .parse()
            .unwrap()
    }

    #[test]
    fn converts_dimensions_compact_numbers_and_temperature_offsets() {
        for (query, expected) in [
            ("100cm to m", 1.),
            ("2 days in hours", 48.),
            ("10 kg to lb", 22.046226218487757),
            ("32 f to c", 0.),
            ("100 c to f", 212.),
            ("-40C to F", -40.),
            ("0 K to C", -273.15),
            ("2 l in ml", 2000.),
        ] {
            assert!((value(query) - expected).abs() < 1e-10, "{query}");
        }
    }

    #[test]
    fn new_dimensions_convert_with_reference_values() {
        for (query, expected, tolerance) in [
            ("1 acre to m2", 4_046.8564224, 1e-9),
            ("100 km/h to mph", 62.13711922, 1e-8),
            ("1 knot to km/h", 1.852, 1e-12),
            ("1 kWh to J", 3.6e6, 1e-6),
            ("1 hp to W", 745.69987158, 1e-8),
            ("1 atm to psi", 14.69594878, 1e-8),
            ("180 deg to rad", std::f64::consts::PI, 1e-12),
            ("60 rpm to hz", 1., 1e-12),
            ("1 gal to l", 3.785411784, 1e-12),
            ("1 ukgal to l", 4.54609, 1e-12),
            ("1 cup to ml", 236.5882365, 1e-7),
            ("1 st to lb", 14., 1e-12),
            ("1 year to days", 365.2425, 1e-12),
            ("1 Cal to kJ", 4.184, 1e-12),
            ("1 cal to J", 4.184, 1e-12),
            ("100 sqft to m2", 9.290304, 1e-12),
        ] {
            assert!(
                (value(query) - expected).abs() < tolerance,
                "{query} = {}",
                value(query)
            );
        }
    }

    #[test]
    fn data_units_distinguish_bits_bytes_and_binary_prefixes() {
        assert_eq!(value("1 GB to MB"), 1_000.);
        assert_eq!(value("1 GiB to MiB"), 1_024.);
        assert_eq!(value("1 GB to Gb"), 8.);
        assert_eq!(value("1gb to mb"), 1_000.);
        assert_eq!(value("8 Mb to MB"), 1.);
        assert_eq!(value("1 B to b"), 8.);
        assert!((value("1 TB to GiB") - 931.3225746154785).abs() < 1e-9);
        assert_eq!(value("100 mbps to MB/s"), 12.5);
        assert_eq!(value("1 Gbps to Mbps"), 1_000.);
        assert_eq!(value("1 MB/s to mbps"), 8.);
    }

    #[test]
    fn results_hide_floating_point_noise() {
        let answer = convert_units("10 pounds to kg").unwrap().unwrap();
        assert_eq!(answer.title, "4.5359237 kg");
        let tiny = convert_units("1e-300 m to km").unwrap().unwrap();
        assert_eq!(tiny.title, format!("{} km", tiny.copy));
        assert!((tiny.copy.parse::<f64>().unwrap() / 1e-303 - 1.).abs() < 1e-12);
        assert_eq!(convert_units("0.1 m to cm").unwrap().unwrap().copy, "10");
    }

    #[test]
    fn fuel_economy_converts_through_the_inverse_unit() {
        assert!((value("30 mpg to l/100km") - 7.84049).abs() < 1e-5);
        assert!((value("5 l/100km to mpg") - 47.04292).abs() < 1e-5);
        assert!((value("40 ukmpg to mpg") - 33.306967).abs() < 1e-5);
        assert!(convert_units("0 l/100km to mpg").unwrap().is_err());
    }

    #[test]
    fn invalid_conversions_never_return_answers() {
        for query in [
            "1 kg to m",
            "-1 k to c",
            "1 kg to unknown",
            "1e309 kg to lb",
            "10 kg to lb extra",
            "1 GB to Mbps",
        ] {
            assert!(convert_units(query).unwrap().is_err(), "{query}");
        }
        for query in [
            "7 zip",
            "9pm et to uk",
            "2 days from now",
            "Core",
            "1Password",
        ] {
            assert!(convert_units(query).is_none(), "{query}");
        }
    }
}
