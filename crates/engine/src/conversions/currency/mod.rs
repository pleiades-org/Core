//! Currency conversion with ECB reference rates: `100 usd to eur`, `£50 in dollars`, `$1.5k`.
mod catalog;
mod rates;

use super::{
    format::{grouped, plain, readable},
    quantity::{parse_scaled_number, split_number_prefix},
    Conversion, Outcome,
};
use crate::calculator::Calculation;
use catalog::{attached_symbols, by_code, find_currency, Currency};
pub use rates::ExchangeRates;
#[cfg(test)]
pub(crate) use rates::SAMPLE_ECB_XML;

const SEPARATORS: &[&str] = &["to", "in", "into", "as", "="];
const MESSAGE: &str = "Enter to copy amount · European Central Bank reference rate";

/// A currency named in a query, and whether the name could mean something else.
type Named = (&'static Currency, bool);

pub fn convert_currency(input: &str, rates: Option<&ExchangeRates>) -> Outcome {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let separator = tokens
        .iter()
        .skip(1)
        .position(|token| {
            SEPARATORS
                .iter()
                .any(|word| token.eq_ignore_ascii_case(word))
        })
        .map(|index| index + 1);
    let (left, right) = match separator {
        Some(index) => (&tokens[..index], Some(&tokens[index + 1..])),
        None => (&tokens[..], None),
    };
    let (amount, (source, source_ambiguous)) = parse_money(left)?;
    let destination = match right {
        Some([]) => return Some(Err("Add a destination currency, such as usd to eur")),
        Some(words) => {
            let (currency, ambiguous) = find_currency(&words.join(" "))?;
            // `10 pounds to kg` is mass; `10 pounds to euros` is money.
            if ambiguous && source_ambiguous {
                return None;
            }
            currency
        }
        None if source_ambiguous => return None,
        None => default_destination(source, rates),
    };
    Some(convert(amount, source, destination, rates))
}

fn default_destination(source: &Currency, rates: Option<&ExchangeRates>) -> &'static Currency {
    let local = rates.and_then(ExchangeRates::local).and_then(by_code);
    match local {
        Some(local) if local.code != source.code => local,
        _ if source.code == "USD" => by_code("EUR").expect("euro"),
        _ => by_code("USD").expect("US dollar"),
    }
}

/// An amount and currency: `$100`, `100$`, `100usd`, `100 usd`, `1.5k us dollars`, `usd 100`.
fn parse_money(tokens: &[&str]) -> Option<(f64, Named)> {
    match tokens {
        [single] => parse_attached(single),
        [first, rest @ ..] => {
            if let Some(amount) = parse_scaled_number(first) {
                return Some((amount, find_currency(&rest.join(" "))?));
            }
            let (last, names) = rest.split_last()?;
            let amount = parse_scaled_number(last)?;
            let name = std::iter::once(*first)
                .chain(names.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            Some((amount, find_currency(&name)?))
        }
        [] => None,
    }
}

fn parse_attached(token: &str) -> Option<(f64, Named)> {
    let lower = token.to_lowercase();
    for (symbol, currency) in attached_symbols() {
        if let Some(amount) = lower.strip_prefix(symbol) {
            return Some((parse_scaled_number(amount)?, (currency, false)));
        }
    }
    let (number, name) = split_number_prefix(token)?;
    if name.is_empty() {
        return None;
    }
    // A magnitude suffix may sit between the number and the currency: `5k€`, `1.5kusd`.
    let (number, name) = match name.strip_prefix(['k', 'K', 'm', 'M']) {
        Some(rest) if find_currency(rest).is_some() && find_currency(name).is_none() => {
            (&token[..number.len() + 1], rest)
        }
        _ => (number, name),
    };
    Some((parse_scaled_number(number)?, find_currency(name)?))
}

fn convert(
    amount: f64,
    source: &Currency,
    destination: &Currency,
    rates: Option<&ExchangeRates>,
) -> Result<Conversion, &'static str> {
    let rates = rates.ok_or(
        "Exchange rates are not available yet · show Core while online to download ECB rates",
    )?;
    let (Some(from), Some(to)) = (
        rates.per_euro(source.code),
        rates.per_euro(destination.code),
    ) else {
        return Err("The European Central Bank publishes no rate for that currency");
    };
    let rate = to / from;
    let value = amount * rate;
    if !value.is_finite() {
        return Err("Result is outside the supported numeric range");
    }
    Ok(Conversion::single(
        Calculation {
            title: display_amount(destination, value),
            detail: format!(
                "{} → {} · 1 {} = {} {} · ECB {}",
                display_amount(source, amount),
                destination.code,
                source.code,
                readable(rate),
                destination.code,
                rates.date()
            ),
            copy: plain(value, destination.decimals),
        },
        MESSAGE,
    ))
}

fn display_amount(currency: &Currency, value: f64) -> String {
    let number = grouped(value.abs(), currency.decimals);
    let sign = if value < 0. && number.bytes().any(|byte| (b'1'..=b'9').contains(&byte)) {
        "-"
    } else {
        ""
    };
    match currency.symbol {
        Some(symbol) => format!("{sign}{symbol}{number}"),
        None => format!("{sign}{number} {}", currency.code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates() -> ExchangeRates {
        ExchangeRates::from_ecb_xml(SAMPLE_ECB_XML)
            .unwrap()
            .with_local_currency("GBP")
    }

    fn answer(query: &str) -> Calculation {
        convert_currency(query, Some(&rates()))
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .remove(0)
    }

    #[test]
    fn codes_symbols_and_names_convert_through_the_euro() {
        // USD 1.10, GBP 0.85, JPY 160 per euro in the sample file.
        for (query, title, copy) in [
            ("110 usd to eur", "€100.00", "100.00"),
            ("100 eur in usd", "$110.00", "110.00"),
            ("$110 to £", "£85.00", "85.00"),
            ("110$ to gbp", "£85.00", "85.00"),
            ("€1.5k to yen", "¥240,000", "240000"),
            ("1,000 euros to dollars", "$1,100.00", "1100.00"),
            ("85 pounds to euros", "€100.00", "100.00"),
            ("100 swiss francs to eur", "€106.38", "106.38"),
            ("usd 110 to eur", "€100.00", "100.00"),
            ("100usd in chf", "85.45 CHF", "85.45"),
        ] {
            let result = answer(query);
            assert_eq!(
                (result.title.as_str(), result.copy.as_str()),
                (title, copy),
                "{query}"
            );
        }
        assert!(answer("110 usd to eur").detail.contains("ECB 2026-09-21"));
    }

    #[test]
    fn a_bare_amount_converts_to_the_local_currency() {
        assert_eq!(answer("100 eur").title, "£85.00");
        assert_eq!(answer("$110").title, "£85.00");
        // Already local: show dollars instead.
        assert_eq!(answer("85 gbp").title, "$110.00");
    }

    #[test]
    fn mass_and_other_units_are_left_to_unit_conversion() {
        for query in [
            "10 pounds to kg",
            "10 pounds",
            "100cm to m",
            "10 kg to lb",
            "2 days from now",
            "Visual Studio Code",
            "9pm et to uk",
        ] {
            assert!(convert_currency(query, Some(&rates())).is_none(), "{query}");
        }
    }

    #[test]
    fn missing_rates_and_unpublished_currencies_explain_themselves() {
        assert!(convert_currency("100 usd to eur", None).unwrap().is_err());
        assert!(convert_currency("100 usd to sek", Some(&rates()))
            .unwrap()
            .is_err());
        assert!(convert_currency("100 usd to", Some(&rates()))
            .unwrap()
            .is_err());
    }
}
