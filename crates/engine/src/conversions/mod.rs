//! Smart conversions and everyday calculations recognised in ordinary search text.
//! Each converter returns `None` for text it does not recognise, so the next one can try;
//! `Some(Err)` explains recognised but unusable input without offering a stale action.
mod bmi;
mod color;
mod currency;
mod display;
mod duration;
mod finance;
mod format;
mod number_base;
mod percentage;
mod phrase;
mod quantity;
mod timestamp;
mod tip;
mod transfer;
mod units;

use crate::calculator::{calendar::CalendarClock, Calculation};
pub use currency::ExchangeRates;
#[cfg(test)]
pub(crate) use currency::SAMPLE_ECB_XML;
pub use units::convert_units;

/// One or more answers; each becomes a result row whose Enter copies `copy`.
#[derive(Debug)]
pub struct Conversion {
    pub answers: Vec<Calculation>,
    pub message: &'static str,
}

impl Conversion {
    fn single(answer: Calculation, message: &'static str) -> Self {
        Self {
            answers: vec![answer],
            message,
        }
    }
}

pub(crate) type Outcome = Option<Result<Conversion, &'static str>>;

/// Inputs from outside the engine. Both are optional: conversions that need them explain why
/// they are unavailable instead of guessing.
#[derive(Clone, Copy, Default)]
pub struct ConversionContext<'a> {
    pub rates: Option<&'a ExchangeRates>,
    pub clock: Option<&'a dyn CalendarClock>,
}

const UNIT_MESSAGE: &str = "Enter to copy number · Esc to hide";

/// Keyword-led formats run first; generic `<number> <unit> to <unit>` runs last.
pub fn convert(input: &str, context: ConversionContext) -> Outcome {
    let input = input.trim();
    timestamp::convert_timestamp(input, context.clock)
        .or_else(|| color::convert_color(input))
        .or_else(|| number_base::convert_number_base(input))
        .or_else(|| bmi::calculate_bmi(input))
        .or_else(|| finance::calculate_finance(input))
        .or_else(|| tip::calculate_tip(input))
        .or_else(|| percentage::calculate_percentage(input))
        .or_else(|| display::calculate_display(input))
        .or_else(|| transfer::calculate_transfer(input))
        .or_else(|| currency::convert_currency(input, context.rates))
        .or_else(|| {
            convert_units(input)
                .map(|result| result.map(|answer| Conversion::single(answer, UNIT_MESSAGE)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        let rates = ExchangeRates::from_ecb_xml(currency::SAMPLE_ECB_XML).unwrap();
        let context = ConversionContext {
            rates: Some(&rates),
            clock: None,
        };
        convert(query, context)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn each_family_is_routed_to_its_converter() {
        for (query, first_title) in [
            ("10 kg to lb", "22.0462262185 lb"),
            ("110 usd to eur", "€100.00"),
            ("10 GB at 100 Mbps", "13 min 20 s"),
            ("255 to hex", "0xFF"),
            ("#ff8800", "rgb(255, 136, 0)"),
            ("20% off 80", "64"),
            ("1920x1080", "16:9"),
            ("bmi 70kg 175cm", "22.9"),
        ] {
            assert_eq!(titles(query)[0], first_title, "{query}");
        }
    }

    #[test]
    fn ordinary_searches_and_arithmetic_are_not_conversions() {
        for query in [
            "Visual Studio Code",
            "xbox",
            "2 + 2",
            "sqrt(81)",
            "2 days from now",
            "9pm et to uk",
            "1Password",
            "7 zip",
            "",
        ] {
            assert!(
                convert(query, ConversionContext::default()).is_none(),
                "{query}"
            );
        }
    }
}
