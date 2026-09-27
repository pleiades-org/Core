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
    let keyword_led = timestamp::convert_timestamp(input, context.clock)
        .or_else(|| color::convert_color(input))
        .or_else(|| number_base::convert_number_base(input));
    // Every later converter needs a parsed number, so app names stop here without allocating.
    // A leading sign or point may still be a unit error hint such as `.5x kg to lb`.
    let might_be_numeric =
        input.bytes().any(|byte| byte.is_ascii_digit()) || input.starts_with(['+', '-', '.']);
    if keyword_led.is_some() || !might_be_numeric {
        return keyword_led;
    }
    bmi::calculate_bmi(input)
        .or_else(|| {
            let mut text = String::new();
            let tokens = phrase::tokenize(input, &mut text)?;
            finance::calculate_finance(&tokens)
                .or_else(|| tip::calculate_tip(&tokens))
                .or_else(|| percentage::calculate_percentage(&tokens))
        })
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

    #[test]
    fn digit_free_text_reaches_only_keyword_converters() {
        for query in ["code", "vsc", "visual studio code", "-", "+", "."] {
            assert!(
                convert(query, ConversionContext::default()).is_none(),
                "{query}"
            );
        }
        assert_eq!(
            convert("unix now", ConversionContext::default()).map(|outcome| outcome.err()),
            Some(Some("The current time is unavailable"))
        );
        assert_eq!(
            titles("#ff8800"),
            ["rgb(255, 136, 0)", "hsl(32, 100%, 50%)", "#FF8800"]
        );
        assert_eq!(
            titles("rgb(1,2,3)"),
            ["#010203", "hsl(210, 50%, 1%)", "rgb(1, 2, 3)"]
        );
        assert_eq!(titles("roman xlii"), ["42"]);
        // A leading point or sign still reaches units, including its error hint.
        assert_eq!(titles(".5 kg to lb"), ["1.10231131092 lb"]);
        assert_eq!(
            convert(".kg to lb", ConversionContext::default()).map(|outcome| outcome.err()),
            Some(Some("Enter a valid number before the unit"))
        );
    }

    #[test]
    fn shared_tokens_and_symbols_keep_sentence_and_currency_answers() {
        assert_eq!(titles("tip 15 percent on 80?"), ["92.00", "12.00"]);
        assert_eq!(titles("20 per cent off 80"), ["64"]);
        assert_eq!(titles("loan 10000 at 7% over 36 months")[0], "308.77");
        for query in ["$5", "US$5", "us$5"] {
            assert_eq!(titles(query), ["€4.55"], "{query}");
        }
        assert_eq!(titles("5K€"), ["$5,500.00"]);
        assert_eq!(titles("1.5K usd"), ["€1,363.64"]);
    }
}
