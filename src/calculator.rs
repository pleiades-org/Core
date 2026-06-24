#[cfg(not(test))]
use crate::paths::currency_rates_file;
use crate::{command::CommandResult, settings::LauncherSettings, timezone_resolver};
use chrono::{
    DateTime, Datelike, Duration, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use regex::Regex;
#[cfg(not(test))]
use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use std::{fs, time::Duration as StdDuration};

#[cfg(not(test))]
const CURRENCY_RATE_CACHE_TTL_SECONDS: i64 = 60 * 60 * 6;
#[cfg(not(test))]
const LIVE_RATE_REQUEST_TIMEOUT_MS: u64 = 700;

#[derive(Clone, Debug)]
pub struct CalculationContext {
    pub now: DateTime<Tz>,
    pub settings: LauncherSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimePrecision {
    Minute,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateTimeDisplayMode {
    TimeOnlyForSameDay,
    AlwaysShowDate,
}

impl CalculationContext {
    pub fn from_settings(settings: LauncherSettings) -> Self {
        let local_timezone = timezone_resolver::local_timezone(&settings);

        Self {
            now: chrono::Utc::now().with_timezone(&local_timezone),
            settings,
        }
    }
}

pub fn evaluate_calculation(input_text: &str, context: &CalculationContext) -> Vec<CommandResult> {
    crate::calculator_dispatch::dispatch_calculation(input_text, context)
}

#[derive(Clone, Copy, Debug)]
struct Currency {
    code: &'static str,
    symbol: &'static str,
    decimal_places: usize,
    usd_value: f64,
}

#[cfg(not(test))]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct CurrencyRateCache {
    fetched_at: i64,
    rates: Vec<CurrencyRateEntry>,
}

#[cfg(not(test))]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct CurrencyRateEntry {
    code: String,
    usd_value: f64,
}

#[derive(Clone, Copy, Debug)]
struct ParsedCurrencyAmount {
    value: f64,
    currency: Option<Currency>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum UnitDimension {
    Length,
    Mass,
    Data,
    DataRate,
    Duration,
    Temperature,
    Volume,
    Speed,
    Pressure,
    Area,
    Energy,
    Power,
    Angle,
    Frequency,
}

#[derive(Clone, Copy, Debug)]
struct UnitDefinition {
    code: &'static str,
    label: &'static str,
    dimension: UnitDimension,
    scale_to_base: f64,
}

pub(crate) fn evaluate_unit_or_currency_conversion(input_text: &str) -> Option<CommandResult> {
    let (amount, source_text, target_text) = parse_conversion_query(input_text)?;

    if let Some(currency_result) =
        evaluate_currency_conversion(input_text, amount, source_text, target_text)
    {
        return Some(currency_result);
    }

    if let Some(duration_result) =
        evaluate_duration_unit_conversion(input_text, amount, source_text, target_text)
    {
        return Some(duration_result);
    }

    evaluate_unit_conversion(input_text, amount, source_text, target_text)
}

fn parse_conversion_query(input_text: &str) -> Option<(f64, &str, &str)> {
    let conversion_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<amount>-?\d+(?:\.\d+)?)
        \s*
        (?P<source>[a-z°$£€¥][a-z0-9°$£€¥./_-]{0,18})
        \s+
        (?:to|in)
        \s+
        (?P<target>[a-z°$£€¥][a-z0-9°$£€¥./_-]{0,18})
        \s*\??\s*$
        ",
    )
    .ok()?;
    let captures = conversion_regex.captures(input_text)?;
    let amount = captures.name("amount")?.as_str().parse::<f64>().ok()?;
    let source_text = captures.name("source")?.as_str();
    let target_text = captures.name("target")?.as_str();
    Some((amount, source_text, target_text))
}

fn evaluate_duration_unit_conversion(
    input_text: &str,
    amount: f64,
    source_text: &str,
    target_text: &str,
) -> Option<CommandResult> {
    let source_unit = duration_unit_from_text(source_text)?;
    let target_unit = duration_unit_from_text(target_text)?;
    let converted_amount = amount * source_unit.scale_to_base / target_unit.scale_to_base;
    let formatted_amount = format_converted_unit_value(converted_amount, target_unit);

    Some(CommandResult::calculation_with_display(
        formatted_amount.clone(),
        input_text,
        formatted_amount.clone(),
        format!(
            "{source_amount} is {formatted_amount}.",
            source_amount = format_converted_unit_value(amount, source_unit),
        ),
        "Duration",
        target_unit.label,
        94,
    ))
}

fn evaluate_currency_conversion(
    input_text: &str,
    amount: f64,
    source_text: &str,
    target_text: &str,
) -> Option<CommandResult> {
    let source_currency = currency_with_current_rate(currency_from_text(source_text)?);
    let target_currency = currency_with_current_rate(currency_from_text(target_text)?);
    let converted_amount = amount * source_currency.usd_value / target_currency.usd_value;
    let formatted_amount = format_currency_value(converted_amount, target_currency);

    Some(CommandResult::calculation_with_display(
        formatted_amount.clone(),
        input_text,
        formatted_amount.clone(),
        format!(
            "{} {} is approximately {formatted_amount}.",
            format_number(amount),
            source_currency.code
        ),
        "Currency",
        target_currency.code,
        94,
    ))
}

pub(crate) fn evaluate_quick_currency_amount(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let parsed_amount = parse_currency_amount(input_text)?;
    let source_currency = currency_with_current_rate(parsed_amount.currency?);
    let local_currency = currency_with_current_rate(local_currency_for_context(context)?);
    let target_currency = quick_currency_target(source_currency, local_currency)?;
    let converted_amount =
        parsed_amount.value * source_currency.usd_value / target_currency.usd_value;
    let formatted_amount = format_currency_value(converted_amount, target_currency);

    Some(CommandResult::calculation_with_display(
        formatted_amount.clone(),
        input_text,
        formatted_amount.clone(),
        format!(
            "Using {} as local context, {} {} is approximately {formatted_amount}.",
            context.settings.local_timezone,
            format_number(parsed_amount.value),
            source_currency.code
        ),
        "Currency",
        target_currency.code,
        93,
    ))
}

fn evaluate_unit_conversion(
    input_text: &str,
    amount: f64,
    source_text: &str,
    target_text: &str,
) -> Option<CommandResult> {
    let source_unit = unit_from_text(source_text)?;
    let target_unit = unit_from_text(target_text)?;
    if source_unit.dimension != target_unit.dimension {
        return None;
    }

    let converted_amount = if source_unit.dimension == UnitDimension::Temperature {
        convert_temperature(amount, source_unit.code, target_unit.code)?
    } else {
        amount * source_unit.scale_to_base / target_unit.scale_to_base
    };
    let formatted_amount = format_converted_unit_value(converted_amount, target_unit);

    Some(CommandResult::calculation_with_display(
        formatted_amount.clone(),
        input_text,
        formatted_amount.clone(),
        format!(
            "{source_amount} is {formatted_amount}.",
            source_amount = format_converted_unit_value(amount, source_unit),
        ),
        "Unit",
        target_unit.label,
        94,
    ))
}

fn format_converted_unit_value(value: f64, unit: UnitDefinition) -> String {
    if unit.dimension == UnitDimension::Duration {
        let rounded = value.round();
        if (value - rounded).abs() < 0.000_5 {
            return format!("{}{}", format_integer(rounded as i64), unit.label);
        }
        return format!("{}{}", format_unit_number(value), unit.label);
    }

    format!("{} {}", format_unit_number(value), unit.label)
}

fn format_integer(value: i64) -> String {
    value.to_string()
}

fn duration_unit_from_text(unit_text: &str) -> Option<UnitDefinition> {
    let normalized_unit = unit_text
        .trim()
        .trim_start_matches('.')
        .to_lowercase();

    match normalized_unit.as_str() {
        "ns" | "nanosecond" | "nanoseconds" => Some(linear_unit("ns", "ns", UnitDimension::Duration, 1e-9)),
        "us" | "μs" | "microsecond" | "microseconds" => {
            Some(linear_unit("us", "us", UnitDimension::Duration, 1e-6))
        }
        "ms" | "millisecond" | "milliseconds" => {
            Some(linear_unit("ms", "ms", UnitDimension::Duration, 0.001))
        }
        "s" | "sec" | "secs" | "second" | "seconds" => {
            Some(linear_unit("s", "s", UnitDimension::Duration, 1.0))
        }
        "m" | "min" | "mins" | "minute" | "minutes" => {
            Some(linear_unit("min", "m", UnitDimension::Duration, 60.0))
        }
        "h" | "hr" | "hrs" | "hour" | "hours" => {
            Some(linear_unit("h", "h", UnitDimension::Duration, 3_600.0))
        }
        "d" | "day" | "days" => Some(linear_unit("d", "d", UnitDimension::Duration, 86_400.0)),
        "w" | "week" | "weeks" => Some(linear_unit(
            "w",
            "w",
            UnitDimension::Duration,
            604_800.0,
        )),
        "fn" | "fortnight" | "fortnights" => Some(linear_unit(
            "fn",
            "fn",
            UnitDimension::Duration,
            1_209_600.0,
        )),
        "mo" | "mos" | "month" | "months" => Some(linear_unit(
            "mo",
            "mo",
            UnitDimension::Duration,
            2_592_000.0,
        )),
        "y" | "yr" | "yrs" | "year" | "years" => Some(linear_unit(
            "y",
            "y",
            UnitDimension::Duration,
            31_557_600.0,
        )),
        _ => None,
    }
}

pub(crate) fn evaluate_market_quote(input_text: &str) -> Option<CommandResult> {
    let trimmed_input = input_text.trim();
    let quote_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:
            quote|price|stock|crypto
        )
        \s+
        (?P<symbol>\$?[a-z0-9.-]{2,12})
        \s*\??\s*$
        |
        ^\s*\$(?P<ticker>[a-z]{2,8})\s*$
        ",
    )
    .ok()?;
    let captures = quote_regex.captures(trimmed_input)?;
    let symbol = captures
        .name("symbol")
        .or_else(|| captures.name("ticker"))?
        .as_str()
        .trim_start_matches('$')
        .to_uppercase();
    let quote = market_quote_for_symbol(&symbol)?;
    let formatted_price = format_currency_value(quote.price_usd, currency_from_text("usd")?);
    let freshness_label = if quote.is_live { "Live" } else { "Fallback" };

    Some(CommandResult::calculation_with_display(
        formatted_price.clone(),
        trimmed_input,
        formatted_price.clone(),
        format!(
            "{freshness_label} {} quote for {} is {formatted_price}.",
            quote.asset_kind, quote.symbol
        ),
        "Quote",
        quote.symbol,
        88,
    ))
}

struct MarketQuote {
    symbol: String,
    asset_kind: &'static str,
    price_usd: f64,
    is_live: bool,
}

fn market_quote_for_symbol(symbol: &str) -> Option<MarketQuote> {
    fetch_live_market_quote(symbol).or_else(|| fallback_market_quote(symbol))
}

#[cfg(test)]
fn fetch_live_market_quote(_symbol: &str) -> Option<MarketQuote> {
    None
}

#[cfg(not(test))]
fn fetch_live_market_quote(symbol: &str) -> Option<MarketQuote> {
    if is_crypto_symbol(symbol) {
        return fetch_live_crypto_quote(symbol);
    }

    fetch_live_stock_quote(symbol)
}

#[cfg(not(test))]
fn fetch_live_crypto_quote(symbol: &str) -> Option<MarketQuote> {
    let request_url = format!("https://api.coinbase.com/v2/prices/{symbol}-USD/spot");
    let response_text = ureq::get(&request_url)
        .timeout(StdDuration::from_millis(LIVE_RATE_REQUEST_TIMEOUT_MS))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let response_json: serde_json::Value = serde_json::from_str(&response_text).ok()?;
    let price_usd = response_json
        .get("data")?
        .get("amount")?
        .as_str()?
        .parse::<f64>()
        .ok()?;

    Some(MarketQuote {
        symbol: symbol.to_string(),
        asset_kind: "crypto",
        price_usd,
        is_live: true,
    })
}

#[cfg(not(test))]
fn fetch_live_stock_quote(symbol: &str) -> Option<MarketQuote> {
    let request_url = format!(
        "https://stooq.com/q/l/?s={}.us&f=sd2t2ohlcv&h&e=csv",
        symbol.to_lowercase()
    );
    let response_text = ureq::get(&request_url)
        .timeout(StdDuration::from_millis(LIVE_RATE_REQUEST_TIMEOUT_MS))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let close_text = response_text.lines().nth(1)?.split(',').nth(6)?;
    let price_usd = close_text.parse::<f64>().ok()?;
    if !price_usd.is_finite() || price_usd <= 0.0 {
        return None;
    }

    Some(MarketQuote {
        symbol: symbol.to_string(),
        asset_kind: "stock",
        price_usd,
        is_live: true,
    })
}

fn fallback_market_quote(symbol: &str) -> Option<MarketQuote> {
    let (asset_kind, price_usd) = match symbol.to_uppercase().as_str() {
        "BTC" | "BTC-USD" => ("crypto", 65_000.0),
        "ETH" | "ETH-USD" => ("crypto", 3_200.0),
        "SOL" | "SOL-USD" => ("crypto", 150.0),
        "AAPL" => ("stock", 195.0),
        "MSFT" => ("stock", 420.0),
        "NVDA" => ("stock", 900.0),
        "GOOGL" | "GOOG" => ("stock", 175.0),
        "AMZN" => ("stock", 185.0),
        _ => return None,
    };

    Some(MarketQuote {
        symbol: symbol.to_uppercase(),
        asset_kind,
        price_usd,
        is_live: false,
    })
}

#[cfg(not(test))]
fn is_crypto_symbol(symbol: &str) -> bool {
    matches!(
        symbol.to_uppercase().as_str(),
        "BTC" | "BTC-USD" | "ETH" | "ETH-USD" | "SOL" | "SOL-USD"
    )
}

pub(crate) fn evaluate_commercial_helper(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    evaluate_tip_tax_or_vat(input_text, context)
        .or_else(|| evaluate_discount(input_text, context))
        .or_else(|| evaluate_margin_or_markup(input_text, context))
}

fn evaluate_tip_tax_or_vat(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let helper_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:
            (?P<kind_prefix>tip|tax|vat)\s+
            (?P<rate_prefix>\d+(?:\.\d+)?)\s*%\s+
            (?:on|for)\s+
            (?P<amount_prefix>.+)
            |
            (?P<rate_leading>\d+(?:\.\d+)?)\s*%\s+
            (?P<kind_leading>tip|tax|vat)\s+
            (?:on|for)\s+
            (?P<amount_leading>.+)
        )
        \s*$
        ",
    )
    .ok()?;
    let captures = helper_regex.captures(input_text)?;
    let helper_kind = captures
        .name("kind_prefix")
        .or_else(|| captures.name("kind_leading"))?
        .as_str()
        .to_lowercase();
    let rate = captures
        .name("rate_prefix")
        .or_else(|| captures.name("rate_leading"))?
        .as_str()
        .parse::<f64>()
        .ok()?;
    let amount_text = captures
        .name("amount_prefix")
        .or_else(|| captures.name("amount_leading"))?
        .as_str();
    let amount = parse_amount_with_context(amount_text, context)?;
    let added_amount = amount.value * rate / 100.0;
    let total_amount = amount.value + added_amount;
    let formatted_total = format_amount_for_currency(total_amount, amount.currency);
    let formatted_added = format_amount_for_currency(added_amount, amount.currency);

    Some(CommandResult::calculation_with_display(
        formatted_total.clone(),
        input_text,
        formatted_total.clone(),
        format!(
            "{}% {} adds {formatted_added}; total is {formatted_total}.",
            format_number(rate),
            helper_kind.to_uppercase()
        ),
        "Commerce",
        "Total",
        92,
    ))
}

fn evaluate_discount(input_text: &str, context: &CalculationContext) -> Option<CommandResult> {
    let discount_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:
            discount\s+(?P<rate_prefix>\d+(?:\.\d+)?)\s*%\s+(?:off|on)\s+(?P<amount_prefix>.+)
            |
            (?P<rate_leading>\d+(?:\.\d+)?)\s*%\s+off\s+(?P<amount_leading>.+)
        )
        \s*$
        ",
    )
    .ok()?;
    let captures = discount_regex.captures(input_text)?;
    let rate = captures
        .name("rate_prefix")
        .or_else(|| captures.name("rate_leading"))?
        .as_str()
        .parse::<f64>()
        .ok()?;
    let amount_text = captures
        .name("amount_prefix")
        .or_else(|| captures.name("amount_leading"))?
        .as_str();
    let amount = parse_amount_with_context(amount_text, context)?;
    let saved_amount = amount.value * rate / 100.0;
    let final_amount = amount.value - saved_amount;
    let formatted_final = format_amount_for_currency(final_amount, amount.currency);
    let formatted_saved = format_amount_for_currency(saved_amount, amount.currency);

    Some(CommandResult::calculation_with_display(
        formatted_final.clone(),
        input_text,
        formatted_final.clone(),
        format!(
            "{}% off saves {formatted_saved}; final price is {formatted_final}.",
            format_number(rate)
        ),
        "Discount",
        "Final",
        92,
    ))
}

fn evaluate_margin_or_markup(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let margin_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<kind>margin|markup)\s+
        (?P<rate>\d+(?:\.\d+)?)\s*%\s+
        (?:on|for)\s+
        (?P<amount>.+)
        \s*$
        ",
    )
    .ok()?;
    let captures = margin_regex.captures(input_text)?;
    let helper_kind = captures.name("kind")?.as_str().to_lowercase();
    let rate = captures.name("rate")?.as_str().parse::<f64>().ok()?;
    let amount = parse_amount_with_context(captures.name("amount")?.as_str(), context)?;
    let final_amount = if helper_kind == "margin" {
        if rate >= 100.0 {
            return None;
        }
        amount.value / (1.0 - rate / 100.0)
    } else {
        amount.value * (1.0 + rate / 100.0)
    };
    let formatted_final = format_amount_for_currency(final_amount, amount.currency);

    Some(CommandResult::calculation_with_display(
        formatted_final.clone(),
        input_text,
        formatted_final.clone(),
        format!(
            "{}% {} on {} gives {formatted_final}.",
            format_number(rate),
            helper_kind,
            format_amount_for_currency(amount.value, amount.currency)
        ),
        "Commerce",
        "Price",
        90,
    ))
}

pub(crate) fn evaluate_finance_helper(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    evaluate_loan_payment(input_text, context)
        .or_else(|| evaluate_compound_interest(input_text, context))
        .or_else(|| evaluate_apr_apy_conversion(input_text))
}

fn evaluate_loan_payment(input_text: &str, context: &CalculationContext) -> Option<CommandResult> {
    let loan_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<kind>loan|mortgage)\s+
        (?P<principal>.+?)\s+
        (?:at\s+)?
        (?P<rate>\d+(?:\.\d+)?)\s*%\s+
        (?:for\s+)?
        (?P<years>\d+(?:\.\d+)?)\s*(?:years?|yrs?|y)
        \s*$
        ",
    )
    .ok()?;
    let captures = loan_regex.captures(input_text)?;
    let loan_kind = captures.name("kind")?.as_str();
    let principal = parse_amount_with_context(captures.name("principal")?.as_str(), context)?;
    let annual_rate = captures.name("rate")?.as_str().parse::<f64>().ok()? / 100.0;
    let year_count = captures.name("years")?.as_str().parse::<f64>().ok()?;
    let payment_count = (year_count * 12.0).round();
    let monthly_rate = annual_rate / 12.0;
    let monthly_payment = if monthly_rate.abs() < f64::EPSILON {
        principal.value / payment_count
    } else {
        principal.value * monthly_rate * (1.0 + monthly_rate).powf(payment_count)
            / ((1.0 + monthly_rate).powf(payment_count) - 1.0)
    };
    let formatted_payment = format_amount_for_currency(monthly_payment, principal.currency);
    let total_paid = monthly_payment * payment_count;

    Some(CommandResult::calculation_with_display(
        formatted_payment.clone(),
        input_text,
        formatted_payment.clone(),
        format!(
            "{} payment is {formatted_payment}/month; total paid is {}.",
            capitalize_ascii(loan_kind),
            format_amount_for_currency(total_paid, principal.currency)
        ),
        "Finance",
        "Monthly",
        91,
    ))
}

fn evaluate_compound_interest(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let compound_regex = Regex::new(
        r"(?ix)
        ^\s*
        compound\s+
        (?P<principal>.+?)\s+
        (?:at\s+)?
        (?P<rate>\d+(?:\.\d+)?)\s*%\s+
        (?:for\s+)?
        (?P<years>\d+(?:\.\d+)?)\s*(?:years?|yrs?|y)
        (?:\s+(?P<frequency>daily|monthly|quarterly|yearly|annually))?
        \s*$
        ",
    )
    .ok()?;
    let captures = compound_regex.captures(input_text)?;
    let principal = parse_amount_with_context(captures.name("principal")?.as_str(), context)?;
    let annual_rate = captures.name("rate")?.as_str().parse::<f64>().ok()? / 100.0;
    let year_count = captures.name("years")?.as_str().parse::<f64>().ok()?;
    let compounds_per_year = captures
        .name("frequency")
        .and_then(|frequency| compounds_per_year(frequency.as_str()))
        .unwrap_or(12.0);
    let future_value = principal.value
        * (1.0 + annual_rate / compounds_per_year).powf(compounds_per_year * year_count);
    let formatted_value = format_amount_for_currency(future_value, principal.currency);

    Some(CommandResult::calculation_with_display(
        formatted_value.clone(),
        input_text,
        formatted_value.clone(),
        format!(
            "{} compounded for {} years becomes {formatted_value}.",
            format_amount_for_currency(principal.value, principal.currency),
            format_number(year_count)
        ),
        "Finance",
        "Future value",
        90,
    ))
}

fn evaluate_apr_apy_conversion(input_text: &str) -> Option<CommandResult> {
    let apr_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:
            apy\s+from\s+|apr\s+
        )
        (?P<rate>\d+(?:\.\d+)?)\s*%\s+
        (?P<frequency>daily|monthly|quarterly|yearly|annually)
        (?:\s+to\s+apy)?
        \s*$
        ",
    )
    .ok()?;
    let captures = apr_regex.captures(input_text)?;
    let annual_rate = captures.name("rate")?.as_str().parse::<f64>().ok()? / 100.0;
    let compounds_per_year = compounds_per_year(captures.name("frequency")?.as_str())?;
    let apy = (1.0 + annual_rate / compounds_per_year).powf(compounds_per_year) - 1.0;
    let formatted_apy = format!("{}%", format_number(apy * 100.0));

    Some(CommandResult::calculation_with_display(
        formatted_apy.clone(),
        input_text,
        formatted_apy.clone(),
        format!("{input_text} converts to {formatted_apy} APY."),
        "Finance",
        "APY",
        88,
    ))
}

fn compounds_per_year(frequency_text: &str) -> Option<f64> {
    match frequency_text.trim().to_lowercase().as_str() {
        "daily" => Some(365.0),
        "monthly" => Some(12.0),
        "quarterly" => Some(4.0),
        "yearly" | "annually" => Some(1.0),
        _ => None,
    }
}

fn parse_amount_with_context(
    amount_text: &str,
    context: &CalculationContext,
) -> Option<ParsedCurrencyAmount> {
    let mut amount = parse_currency_amount(amount_text)?;
    if amount.currency.is_none() {
        amount.currency = local_currency_for_context(context);
    }
    amount.currency = amount.currency.map(currency_with_current_rate);
    Some(amount)
}

fn format_amount_for_currency(value: f64, currency: Option<Currency>) -> String {
    currency
        .map(|currency| format_currency_value(value, currency))
        .unwrap_or_else(|| format_number(value))
}

pub(crate) fn evaluate_percentage_expression(input_text: &str) -> Option<CommandResult> {
    let percentage_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:what\s+is\s+)?
        (?P<percentage>-?\d+(?:\.\d+)?)
        \s*(?:%|percent|percentage)
        \s+of\s+
        (?P<amount>.+?)
        (?:\s+in\s+(?P<target_currency>[a-z]{3}|[$£€¥]))?
        \s*\??\s*$
        ",
    )
    .ok()?;
    let captures = percentage_regex.captures(input_text)?;
    let percentage = captures.name("percentage")?.as_str().parse::<f64>().ok()?;
    let parsed_amount = parse_currency_amount(captures.name("amount")?.as_str())?;
    let requested_target_currency = captures
        .name("target_currency")
        .and_then(|currency_match| currency_from_text(currency_match.as_str()));
    let percentage_value = parsed_amount.value * percentage / 100.0;
    let converted_value = convert_percentage_value(
        percentage_value,
        parsed_amount.currency,
        requested_target_currency,
    );
    let display_currency = requested_target_currency.or(parsed_amount.currency);
    let formatted_value = display_currency
        .map(|currency| format_currency_value(converted_value, currency))
        .unwrap_or_else(|| format_number(converted_value));
    let formatted_percentage = format_number(percentage);

    Some(CommandResult::calculation_with_display(
        formatted_value.clone(),
        input_text,
        formatted_value.clone(),
        format!(
            "{formatted_percentage}% of {} is {formatted_value}.",
            captures.name("amount")?.as_str().trim()
        ),
        "Percentage",
        "Result",
        96,
    ))
}

pub(crate) fn evaluate_math_expression(input_text: &str) -> Option<CommandResult> {
    if !looks_like_math_expression(input_text) {
        return None;
    }

    let calculated_value = meval::eval_str(input_text).ok()?;
    if !calculated_value.is_finite() {
        return None;
    }

    let formatted_value = format_number(calculated_value);
    Some(CommandResult::calculation_with_display(
        formatted_value.clone(),
        input_text,
        formatted_value.clone(),
        format!("{input_text} = {formatted_value}"),
        "Math",
        "Result",
        90,
    ))
}

pub(crate) fn evaluate_current_time_lookup(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let timezone_text = parse_current_time_lookup_timezone(input_text)?;
    let timezone_resolution =
        timezone_resolver::resolve_timezone(timezone_text, &context.settings)?;
    let target_datetime = context.now.with_timezone(&timezone_resolution.timezone);
    let reference_datetime = context.now.with_timezone(&timezone_resolution.timezone);
    let formatted_datetime = format_datetime_for_reference(
        target_datetime,
        reference_datetime,
        TimePrecision::Minute,
        DateTimeDisplayMode::AlwaysShowDate,
    );
    let readable_datetime = single_line_datetime(&formatted_datetime);

    Some(CommandResult::calculation_with_display(
        formatted_datetime.clone(),
        input_text,
        formatted_datetime,
        format!(
            "The current time in {} is {readable_datetime}.",
            timezone_resolution.display_name
        ),
        "Time",
        timezone_resolution.display_name,
        97,
    ))
}

fn parse_current_time_lookup_timezone(input_text: &str) -> Option<&str> {
    let lookup_patterns = [
        r"(?ix)^\s*what\s+time\s+is\s+it\s+(?:in|at|for)\s+(?P<timezone>.+?)\??\s*$",
        r"(?ix)^\s*(?:what(?:'s|\s+is)?\s+)?(?:the\s+)?(?:current\s+)?time\s+(?:in|at|for)\s+(?P<timezone>.+?)\??\s*$",
    ];

    lookup_patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(input_text)?
            .name("timezone")
            .map(|timezone_match| timezone_match.as_str().trim())
    })
}

fn parse_currency_amount(amount_text: &str) -> Option<ParsedCurrencyAmount> {
    let amount_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<prefix_symbol>[$£€¥])?
        \s*
        (?P<prefix_code>[a-z]{3})?
        \s*
        (?P<number>-?\d[\d,]*(?:\.\d+)?)
        \s*
        (?P<suffix_code>[a-z]{3})?
        \s*$
        ",
    )
    .ok()?;
    let captures = amount_regex.captures(amount_text)?;
    let value = captures
        .name("number")?
        .as_str()
        .replace(',', "")
        .parse::<f64>()
        .ok()?;
    let currency = captures
        .name("prefix_symbol")
        .and_then(|currency_match| currency_from_text(currency_match.as_str()))
        .or_else(|| {
            captures
                .name("prefix_code")
                .and_then(|currency_match| currency_from_text(currency_match.as_str()))
        })
        .or_else(|| {
            captures
                .name("suffix_code")
                .and_then(|currency_match| currency_from_text(currency_match.as_str()))
        });

    Some(ParsedCurrencyAmount { value, currency })
}

fn convert_percentage_value(
    percentage_value: f64,
    source_currency: Option<Currency>,
    target_currency: Option<Currency>,
) -> f64 {
    match (source_currency, target_currency) {
        (Some(source_currency), Some(target_currency))
            if source_currency.code != target_currency.code =>
        {
            let source_currency = currency_with_current_rate(source_currency);
            let target_currency = currency_with_current_rate(target_currency);
            percentage_value * source_currency.usd_value / target_currency.usd_value
        }
        _ => percentage_value,
    }
}

fn currency_with_current_rate(currency: Currency) -> Currency {
    live_usd_value_for_currency(currency.code)
        .map(|usd_value| Currency {
            usd_value,
            ..currency
        })
        .unwrap_or(currency)
}

#[cfg(test)]
fn live_usd_value_for_currency(_currency_code: &str) -> Option<f64> {
    None
}

#[cfg(not(test))]
fn live_usd_value_for_currency(currency_code: &str) -> Option<f64> {
    let normalized_currency_code = currency_code.trim().to_uppercase();
    if normalized_currency_code == "USD" {
        return Some(1.0);
    }

    let fresh_cache = load_currency_rate_cache()
        .filter(|cache| {
            Utc::now().timestamp() - cache.fetched_at <= CURRENCY_RATE_CACHE_TTL_SECONDS
        })
        .or_else(refresh_currency_rate_cache);
    let cache = fresh_cache.or_else(load_currency_rate_cache)?;

    cache
        .rates
        .into_iter()
        .find(|rate| rate.code.eq_ignore_ascii_case(&normalized_currency_code))
        .map(|rate| rate.usd_value)
}

#[cfg(not(test))]
fn refresh_currency_rate_cache() -> Option<CurrencyRateCache> {
    let response_text =
        ureq::get("https://api.frankfurter.app/latest?from=USD&to=GBP,EUR,JPY,CAD,AUD,CHF,CNY,INR")
            .timeout(StdDuration::from_millis(LIVE_RATE_REQUEST_TIMEOUT_MS))
            .call()
            .ok()?
            .into_string()
            .ok()?;
    let response_json: serde_json::Value = serde_json::from_str(&response_text).ok()?;
    let rates = response_json.get("rates")?.as_object()?;
    let mut rate_entries = vec![CurrencyRateEntry {
        code: "USD".to_string(),
        usd_value: 1.0,
    }];

    for (currency_code, rate_per_usd_value) in rates {
        let Some(rate_per_usd_value) = rate_per_usd_value.as_f64() else {
            continue;
        };
        if rate_per_usd_value <= 0.0 {
            continue;
        }

        rate_entries.push(CurrencyRateEntry {
            code: currency_code.to_uppercase(),
            usd_value: 1.0 / rate_per_usd_value,
        });
    }

    let cache = CurrencyRateCache {
        fetched_at: Utc::now().timestamp(),
        rates: rate_entries,
    };
    let _ = save_currency_rate_cache(&cache);
    Some(cache)
}

#[cfg(not(test))]
fn load_currency_rate_cache() -> Option<CurrencyRateCache> {
    fs::read_to_string(currency_rates_cache_path())
        .ok()
        .and_then(|cache_text| toml::from_str(&cache_text).ok())
}

#[cfg(not(test))]
fn save_currency_rate_cache(cache: &CurrencyRateCache) -> std::io::Result<()> {
    let cache_path = currency_rates_cache_path();
    if let Some(cache_directory) = cache_path.parent() {
        fs::create_dir_all(cache_directory)?;
    }

    let cache_text = toml::to_string_pretty(cache).unwrap_or_default();
    fs::write(cache_path, cache_text)
}

#[cfg(not(test))]
fn currency_rates_cache_path() -> std::path::PathBuf {
    currency_rates_file()
}

fn quick_currency_target(source_currency: Currency, local_currency: Currency) -> Option<Currency> {
    if source_currency.code != local_currency.code {
        return Some(local_currency);
    }

    if local_currency.code == "USD" {
        currency_from_text("gbp")
    } else {
        currency_from_text("usd")
    }
}

fn local_currency_for_context(context: &CalculationContext) -> Option<Currency> {
    context
        .settings
        .home_currency
        .as_deref()
        .and_then(currency_from_text)
        .or_else(|| currency_from_timezone(&context.settings.local_timezone))
        .or_else(|| currency_from_text("usd"))
}

fn currency_from_timezone(timezone_name: &str) -> Option<Currency> {
    let normalized_timezone = timezone_name.trim().to_lowercase();

    match normalized_timezone.as_str() {
        "europe/london" | "europe/guernsey" | "europe/isle_of_man" | "europe/jersey" => {
            currency_from_text("gbp")
        }
        "europe/zurich" | "europe/vaduz" => currency_from_text("chf"),
        "asia/tokyo" => currency_from_text("jpy"),
        "asia/shanghai" | "asia/chongqing" | "asia/harbin" | "asia/urumqi" => {
            currency_from_text("cny")
        }
        "asia/kolkata" | "asia/calcutta" => currency_from_text("inr"),
        "america/toronto"
        | "america/vancouver"
        | "america/edmonton"
        | "america/winnipeg"
        | "america/halifax"
        | "america/st_johns"
        | "america/regina"
        | "america/whitehorse"
        | "america/moncton"
        | "america/yellowknife" => currency_from_text("cad"),
        "australia/sydney"
        | "australia/melbourne"
        | "australia/brisbane"
        | "australia/perth"
        | "australia/adelaide"
        | "australia/darwin"
        | "australia/hobart"
        | "australia/lord_howe" => currency_from_text("aud"),
        _ if normalized_timezone.starts_with("europe/") => currency_from_text("eur"),
        _ if is_us_timezone(&normalized_timezone) => currency_from_text("usd"),
        _ => None,
    }
}

fn is_us_timezone(normalized_timezone: &str) -> bool {
    matches!(
        normalized_timezone,
        "america/new_york"
            | "america/detroit"
            | "america/kentucky/louisville"
            | "america/kentucky/monticello"
            | "america/indiana/indianapolis"
            | "america/indiana/vincennes"
            | "america/indiana/winamac"
            | "america/indiana/marengo"
            | "america/indiana/petersburg"
            | "america/indiana/vevay"
            | "america/chicago"
            | "america/indiana/tell_city"
            | "america/indiana/knox"
            | "america/menominee"
            | "america/north_dakota/center"
            | "america/north_dakota/new_salem"
            | "america/north_dakota/beulah"
            | "america/denver"
            | "america/boise"
            | "america/phoenix"
            | "america/los_angeles"
            | "america/anchorage"
            | "america/juneau"
            | "america/sitka"
            | "america/metlakatla"
            | "america/yakutat"
            | "america/nome"
            | "america/adak"
            | "pacific/honolulu"
    )
}

fn currency_from_text(currency_text: &str) -> Option<Currency> {
    let normalized_currency = currency_text.trim().trim_start_matches('.').to_lowercase();

    match normalized_currency.as_str() {
        "$" | "usd" => Some(Currency {
            code: "USD",
            symbol: "$",
            decimal_places: 2,
            usd_value: 1.0,
        }),
        "£" | "gbp" => Some(Currency {
            code: "GBP",
            symbol: "£",
            decimal_places: 2,
            usd_value: 1.27,
        }),
        "€" | "eur" => Some(Currency {
            code: "EUR",
            symbol: "€",
            decimal_places: 2,
            usd_value: 1.09,
        }),
        "¥" | "jpy" => Some(Currency {
            code: "JPY",
            symbol: "¥",
            decimal_places: 0,
            usd_value: 0.0065,
        }),
        "cad" => Some(Currency {
            code: "CAD",
            symbol: "C$",
            decimal_places: 2,
            usd_value: 0.73,
        }),
        "aud" => Some(Currency {
            code: "AUD",
            symbol: "A$",
            decimal_places: 2,
            usd_value: 0.66,
        }),
        "chf" => Some(Currency {
            code: "CHF",
            symbol: "CHF ",
            decimal_places: 2,
            usd_value: 1.12,
        }),
        "cny" => Some(Currency {
            code: "CNY",
            symbol: "CN¥",
            decimal_places: 2,
            usd_value: 0.14,
        }),
        "inr" => Some(Currency {
            code: "INR",
            symbol: "₹",
            decimal_places: 2,
            usd_value: 0.012,
        }),
        _ => None,
    }
}

fn unit_from_text(unit_text: &str) -> Option<UnitDefinition> {
    let normalized_unit = unit_text
        .trim()
        .trim_start_matches('.')
        .to_lowercase()
        .replace("degrees", "")
        .replace("degree", "");
    let normalized_unit = normalized_unit.trim();

    match normalized_unit {
        "m" | "meter" | "meters" | "metre" | "metres" => {
            Some(linear_unit("m", "m", UnitDimension::Length, 1.0))
        }
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => {
            Some(linear_unit("km", "km", UnitDimension::Length, 1_000.0))
        }
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => {
            Some(linear_unit("cm", "cm", UnitDimension::Length, 0.01))
        }
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => {
            Some(linear_unit("mm", "mm", UnitDimension::Length, 0.001))
        }
        "mi" | "mile" | "miles" => Some(linear_unit("mi", "mi", UnitDimension::Length, 1_609.344)),
        "ft" | "foot" | "feet" => Some(linear_unit("ft", "ft", UnitDimension::Length, 0.3048)),
        "in" | "inch" | "inches" => Some(linear_unit("in", "in", UnitDimension::Length, 0.0254)),
        "yd" | "yard" | "yards" => Some(linear_unit("yd", "yd", UnitDimension::Length, 0.9144)),
        "kg" | "kilogram" | "kilograms" => {
            Some(linear_unit("kg", "kg", UnitDimension::Mass, 1_000.0))
        }
        "g" | "gram" | "grams" => Some(linear_unit("g", "g", UnitDimension::Mass, 1.0)),
        "mg" | "milligram" | "milligrams" => {
            Some(linear_unit("mg", "mg", UnitDimension::Mass, 0.001))
        }
        "lb" | "lbs" | "pound" | "pounds" => {
            Some(linear_unit("lb", "lb", UnitDimension::Mass, 453.59237))
        }
        "oz" | "ounce" | "ounces" => {
            Some(linear_unit("oz", "oz", UnitDimension::Mass, 28.349523125))
        }
        "bit" | "bits" => Some(linear_unit("bit", "bits", UnitDimension::Data, 0.125)),
        "b" | "byte" | "bytes" => Some(linear_unit("B", "B", UnitDimension::Data, 1.0)),
        "kb" | "kilobyte" | "kilobytes" => {
            Some(linear_unit("KB", "KB", UnitDimension::Data, 1_000.0))
        }
        "mb" | "megabyte" | "megabytes" => {
            Some(linear_unit("MB", "MB", UnitDimension::Data, 1_000_000.0))
        }
        "gb" | "gigabyte" | "gigabytes" => Some(linear_unit(
            "GB",
            "GB",
            UnitDimension::Data,
            1_000_000_000.0,
        )),
        "tb" | "terabyte" | "terabytes" => Some(linear_unit(
            "TB",
            "TB",
            UnitDimension::Data,
            1_000_000_000_000.0,
        )),
        "kib" => Some(linear_unit("KiB", "KiB", UnitDimension::Data, 1_024.0)),
        "mib" => Some(linear_unit("MiB", "MiB", UnitDimension::Data, 1_048_576.0)),
        "gib" => Some(linear_unit(
            "GiB",
            "GiB",
            UnitDimension::Data,
            1_073_741_824.0,
        )),
        "tib" => Some(linear_unit(
            "TiB",
            "TiB",
            UnitDimension::Data,
            1_099_511_627_776.0,
        )),
        "s" | "sec" | "second" | "seconds" => {
            Some(linear_unit("s", "s", UnitDimension::Duration, 1.0))
        }
        "min" | "mins" | "minute" | "minutes" => {
            Some(linear_unit("min", "min", UnitDimension::Duration, 60.0))
        }
        "h" | "hr" | "hrs" | "hour" | "hours" => {
            Some(linear_unit("h", "h", UnitDimension::Duration, 3_600.0))
        }
        "day" | "days" => Some(linear_unit(
            "day",
            "days",
            UnitDimension::Duration,
            86_400.0,
        )),
        "week" | "weeks" => Some(linear_unit(
            "week",
            "weeks",
            UnitDimension::Duration,
            604_800.0,
        )),
        "tsp" | "teaspoon" | "teaspoons" => Some(linear_unit(
            "tsp",
            "tsp",
            UnitDimension::Volume,
            4.928_921_593_75,
        )),
        "tbsp" | "tablespoon" | "tablespoons" => Some(linear_unit(
            "tbsp",
            "tbsp",
            UnitDimension::Volume,
            14.786_764_781_25,
        )),
        "cup" | "cups" => Some(linear_unit(
            "cup",
            "cups",
            UnitDimension::Volume,
            236.588_236_5,
        )),
        "floz" | "fl oz" | "fluid ounce" | "fluid ounces" => Some(linear_unit(
            "fl oz",
            "fl oz",
            UnitDimension::Volume,
            29.573_529_562,
        )),
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => {
            Some(linear_unit("ml", "ml", UnitDimension::Volume, 1.0))
        }
        "l" | "liter" | "liters" | "litre" | "litres" => {
            Some(linear_unit("l", "l", UnitDimension::Volume, 1_000.0))
        }
        "c" | "celsius" | "°c" => Some(linear_unit("c", "°C", UnitDimension::Temperature, 1.0)),
        "f" | "fahrenheit" | "°f" => Some(linear_unit("f", "°F", UnitDimension::Temperature, 1.0)),
        "k" | "kelvin" | "°k" => Some(linear_unit("k", "K", UnitDimension::Temperature, 1.0)),
        "nm" | "nmi" | "nautical mile" | "nautical miles" => Some(linear_unit(
            "nmi",
            "nmi",
            UnitDimension::Length,
            1_852.0,
        )),
        "stone" | "stones" | "st" => Some(linear_unit("st", "st", UnitDimension::Mass, 6_350.293_18)),
        "ton" | "tons" | "tonne" | "tonnes" | "t" => {
            Some(linear_unit("t", "t", UnitDimension::Mass, 1_000_000.0))
        }
        "mph" => Some(linear_unit("mph", "mph", UnitDimension::Speed, 0.447_04)),
        "kph" | "kmh" | "km/h" => Some(linear_unit("kph", "kph", UnitDimension::Speed, 0.277_778)),
        "mps" | "m/s" => Some(linear_unit("m/s", "m/s", UnitDimension::Speed, 1.0)),
        "knot" | "knots" | "kt" => Some(linear_unit("kt", "kt", UnitDimension::Speed, 0.514_444)),
        "psi" => Some(linear_unit("psi", "psi", UnitDimension::Pressure, 6_894.757)),
        "bar" => Some(linear_unit("bar", "bar", UnitDimension::Pressure, 100_000.0)),
        "kpa" => Some(linear_unit("kPa", "kPa", UnitDimension::Pressure, 1_000.0)),
        "pa" => Some(linear_unit("Pa", "Pa", UnitDimension::Pressure, 1.0)),
        "atm" => Some(linear_unit("atm", "atm", UnitDimension::Pressure, 101_325.0)),
        "acre" | "acres" => Some(linear_unit("acre", "acres", UnitDimension::Area, 4_046.856_422_4)),
        "ha" | "hectare" | "hectares" => {
            Some(linear_unit("ha", "ha", UnitDimension::Area, 10_000.0))
        }
        "sqft" | "ft2" | "sq ft" => Some(linear_unit("sqft", "sqft", UnitDimension::Area, 0.092_903_04)),
        "sqm" | "m2" | "sq m" => Some(linear_unit("sqm", "sqm", UnitDimension::Area, 1.0)),
        "gal" | "gallon" | "gallons" => {
            Some(linear_unit("gal", "gal", UnitDimension::Volume, 3_785.411_784))
        }
        "qt" | "quart" | "quarts" => {
            Some(linear_unit("qt", "qt", UnitDimension::Volume, 946.352_946))
        }
        "pt" | "pint" | "pints" => Some(linear_unit("pt", "pt", UnitDimension::Volume, 473.176_473)),
        "j" | "joule" | "joules" => Some(linear_unit("J", "J", UnitDimension::Energy, 1.0)),
        "kj" | "kilojoule" | "kilojoules" => {
            Some(linear_unit("kJ", "kJ", UnitDimension::Energy, 1_000.0))
        }
        "cal" | "calorie" | "calories" => Some(linear_unit("cal", "cal", UnitDimension::Energy, 4.184)),
        "kcal" => Some(linear_unit("kcal", "kcal", UnitDimension::Energy, 4_184.0)),
        "wh" | "watt-hour" | "watt-hours" => {
            Some(linear_unit("Wh", "Wh", UnitDimension::Energy, 3_600.0))
        }
        "kwh" | "kilowatt-hour" | "kilowatt-hours" => {
            Some(linear_unit("kWh", "kWh", UnitDimension::Energy, 3_600_000.0))
        }
        "w" | "watt" | "watts" => Some(linear_unit("W", "W", UnitDimension::Power, 1.0)),
        "kw" | "kilowatt" | "kilowatts" => {
            Some(linear_unit("kW", "kW", UnitDimension::Power, 1_000.0))
        }
        "hp" | "horsepower" => Some(linear_unit("hp", "hp", UnitDimension::Power, 745.699_872)),
        "deg" | "degree" | "degrees" => Some(linear_unit("deg", "deg", UnitDimension::Angle, 1.0)),
        "rad" | "radian" | "radians" => {
            Some(linear_unit("rad", "rad", UnitDimension::Angle, 57.295_779_513))
        }
        "hz" | "hertz" => Some(linear_unit("Hz", "Hz", UnitDimension::Frequency, 1.0)),
        "khz" | "kilohertz" => Some(linear_unit("kHz", "kHz", UnitDimension::Frequency, 1_000.0)),
        "mhz" | "megahertz" => {
            Some(linear_unit("MHz", "MHz", UnitDimension::Frequency, 1_000_000.0))
        }
        "ghz" | "gigahertz" => {
            Some(linear_unit("GHz", "GHz", UnitDimension::Frequency, 1_000_000_000.0))
        }
        "bps" | "bit/s" | "bits/s" => Some(linear_unit("bps", "bps", UnitDimension::DataRate, 1.0)),
        "kbps" | "kbit/s" | "kb/s" if !unit_text.contains('B') => {
            Some(linear_unit("kbps", "kbps", UnitDimension::DataRate, 1_000.0))
        }
        "mbps" | "mbit/s" => Some(linear_unit("Mbps", "Mbps", UnitDimension::DataRate, 1_000_000.0)),
        "gbps" | "gbit/s" => {
            Some(linear_unit("Gbps", "Gbps", UnitDimension::DataRate, 1_000_000_000.0))
        }
        "b/s" | "bytes/s" | "bytes/sec" if unit_text.contains('B') => {
            Some(linear_unit("B/s", "B/s", UnitDimension::DataRate, 8.0))
        }
        "kb/s" if unit_text.contains('B') => {
            Some(linear_unit("KB/s", "KB/s", UnitDimension::DataRate, 8_000.0))
        }
        "mb/s" => Some(linear_unit("MB/s", "MB/s", UnitDimension::DataRate, 8_000_000.0)),
        "gb/s" => Some(linear_unit("GB/s", "GB/s", UnitDimension::DataRate, 8_000_000_000.0)),
        _ => None,
    }
}

fn linear_unit(
    code: &'static str,
    label: &'static str,
    dimension: UnitDimension,
    scale_to_base: f64,
) -> UnitDefinition {
    UnitDefinition {
        code,
        label,
        dimension,
        scale_to_base,
    }
}

fn convert_temperature(value: f64, source_code: &str, target_code: &str) -> Option<f64> {
    let celsius = match source_code {
        "c" => value,
        "f" => (value - 32.0) * 5.0 / 9.0,
        "k" => value - 273.15,
        _ => return None,
    };

    Some(match target_code {
        "c" => celsius,
        "f" => celsius * 9.0 / 5.0 + 32.0,
        "k" => celsius + 273.15,
        _ => return None,
    })
}

fn format_currency_value(value: f64, currency: Currency) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let absolute_value = value.abs();

    format!(
        "{sign}{}{}",
        currency.symbol,
        format_grouped_number(absolute_value, currency.decimal_places)
    )
}

fn format_grouped_number(value: f64, decimal_places: usize) -> String {
    let rounded_text = format!("{:.*}", decimal_places, value);
    let (integer_text, decimal_text) = rounded_text
        .split_once('.')
        .map(|(integer_text, decimal_text)| (integer_text, Some(decimal_text)))
        .unwrap_or((rounded_text.as_str(), None));
    let mut grouped_reversed = String::new();

    for (digit_index, character) in integer_text.chars().rev().enumerate() {
        if digit_index > 0 && digit_index % 3 == 0 {
            grouped_reversed.push(',');
        }
        grouped_reversed.push(character);
    }

    let grouped_integer = grouped_reversed.chars().rev().collect::<String>();
    match decimal_text {
        Some(decimal_text) if decimal_places > 0 => format!("{grouped_integer}.{decimal_text}"),
        _ => grouped_integer,
    }
}

pub(crate) fn evaluate_duration_arithmetic(input_text: &str) -> Option<CommandResult> {
    let normalized_input = input_text.trim();
    if !(normalized_input.contains('+') || normalized_input.contains('-')) {
        return None;
    }

    let duration_regex = Regex::new(
        r"(?ix)
        (?P<sign>[+-])?
        \s*
        (?P<amount>\d+(?:\.\d+)?)
        \s*
        (?P<unit>
            weeks?|w|
            days?|d|
            hours?|hrs?|hr|h|
            minutes?|mins?|min|m|
            seconds?|secs?|sec|s
        )
        ",
    )
    .ok()?;
    let mut total_seconds = 0.0;
    let mut term_count = 0;

    for captures in duration_regex.captures_iter(normalized_input) {
        let sign = captures
            .name("sign")
            .map(|sign| sign.as_str())
            .unwrap_or("+");
        let amount = captures.name("amount")?.as_str().parse::<f64>().ok()?;
        let unit_seconds = duration_unit_seconds(captures.name("unit")?.as_str())?;
        let signed_amount = if sign == "-" { -amount } else { amount };
        total_seconds += signed_amount * unit_seconds;
        term_count += 1;
    }

    if term_count < 2 || !total_seconds.is_finite() {
        return None;
    }

    let formatted_duration = format_duration_seconds(total_seconds);
    Some(CommandResult::calculation_with_display(
        formatted_duration.clone(),
        input_text,
        formatted_duration.clone(),
        format!("{input_text} is {formatted_duration}."),
        "Duration",
        "Total",
        92,
    ))
}

fn duration_unit_seconds(unit_text: &str) -> Option<f64> {
    match unit_text.trim().to_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(1.0),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(60.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(3_600.0),
        "d" | "day" | "days" => Some(86_400.0),
        "w" | "week" | "weeks" => Some(604_800.0),
        _ => None,
    }
}

fn format_duration_seconds(total_seconds: f64) -> String {
    let sign = if total_seconds.is_sign_negative() {
        "-"
    } else {
        ""
    };
    let mut remaining_seconds = total_seconds.abs().round() as i64;
    let weeks = remaining_seconds / 604_800;
    remaining_seconds %= 604_800;
    let days = remaining_seconds / 86_400;
    remaining_seconds %= 86_400;
    let hours = remaining_seconds / 3_600;
    remaining_seconds %= 3_600;
    let minutes = remaining_seconds / 60;
    let seconds = remaining_seconds % 60;
    let parts = [
        (weeks, "w"),
        (days, "d"),
        (hours, "h"),
        (minutes, "m"),
        (seconds, "s"),
    ]
    .into_iter()
    .filter(|(value, _)| *value > 0)
    .map(|(value, label)| format!("{value}{label}"))
    .collect::<Vec<_>>();

    if parts.is_empty() {
        "0s".to_string()
    } else {
        format!("{sign}{}", parts.join(" "))
    }
}

pub(crate) fn evaluate_date_range(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let range_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:(?P<unit>days?|weeks?|months?)\s+)?
        (?:between|from)\s+
        (?P<start>.+?)\s+
        (?:and|to)\s+
        (?P<end>.+?)
        \s*$
        ",
    )
    .ok()?;
    let captures = range_regex.captures(input_text)?;
    let start_date = parse_flexible_date(captures.name("start")?.as_str(), context)?;
    let mut end_date = parse_flexible_date(captures.name("end")?.as_str(), context)?;
    if end_date < start_date {
        end_date = end_date.checked_add_months(Months::new(12))?;
    }

    let day_count = (end_date - start_date).num_days();
    let unit = captures
        .name("unit")
        .map(|unit| unit.as_str().to_lowercase())
        .unwrap_or_else(|| "days".to_string());
    let (copy_text, result_label) = if unit.starts_with("week") {
        (format_number(day_count as f64 / 7.0), "Weeks")
    } else if unit.starts_with("month") {
        (format_number(day_count as f64 / 30.4375), "Months")
    } else {
        (day_count.to_string(), "Days")
    };

    Some(CommandResult::calculation_with_display(
        copy_text.clone(),
        input_text,
        copy_text.clone(),
        format!(
            "{} to {} is {copy_text} {}.",
            format_date_for_reference(start_date, context.now.date_naive()),
            format_date_for_reference(end_date, context.now.date_naive()),
            result_label.to_lowercase()
        ),
        "Date range",
        result_label,
        91,
    ))
}

pub(crate) fn evaluate_unix_timestamp(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let timestamp_regex =
        Regex::new(r"(?ix)^\s*(?:unix|timestamp)\s+(?P<timestamp>-?\d{9,13})\s*$").ok()?;
    let captures = timestamp_regex.captures(input_text)?;
    let mut timestamp = captures.name("timestamp")?.as_str().parse::<i64>().ok()?;
    if timestamp.abs() > 99_999_999_999 {
        timestamp /= 1_000;
    }

    let datetime =
        DateTime::<Utc>::from_timestamp(timestamp, 0)?.with_timezone(&context.now.timezone());
    let formatted_datetime = format_datetime_for_reference(
        datetime,
        context.now,
        TimePrecision::Second,
        DateTimeDisplayMode::AlwaysShowDate,
    );

    Some(CommandResult::calculation_with_display(
        formatted_datetime.clone(),
        input_text,
        formatted_datetime.clone(),
        format!(
            "{timestamp} resolves to {}.",
            single_line_datetime(&formatted_datetime)
        ),
        "Timestamp",
        "Local time",
        92,
    ))
}

pub(crate) fn evaluate_programmer_expression(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    evaluate_programmer_conversion(input_text)
        .or_else(|| evaluate_bitwise_expression(input_text, context))
}

fn evaluate_programmer_conversion(input_text: &str) -> Option<CommandResult> {
    let trimmed_input = input_text.trim();

    if let Some(value) = parse_integer_literal(trimmed_input) {
        return Some(programmer_value_result(trimmed_input, value, "Decimal", 90));
    }

    let conversion_regex =
        Regex::new(r"(?ix)^\s*(?P<target>hex|bin|binary|dec|decimal)\s+(?P<value>.+?)\s*$").ok()?;
    let captures = conversion_regex.captures(trimmed_input)?;
    let target = captures.name("target")?.as_str().to_lowercase();
    let value = parse_integer_literal(captures.name("value")?.as_str())?;
    let formatted_value = match target.as_str() {
        "hex" => format!("0x{:X}", value),
        "bin" | "binary" => format!("0b{:b}", value),
        "dec" | "decimal" => value.to_string(),
        _ => return None,
    };

    Some(CommandResult::calculation_with_display(
        formatted_value.clone(),
        input_text,
        formatted_value.clone(),
        format!("{input_text} converts to {formatted_value}."),
        "Programmer",
        target.to_uppercase(),
        91,
    ))
}

fn evaluate_bitwise_expression(
    input_text: &str,
    _context: &CalculationContext,
) -> Option<CommandResult> {
    let bitwise_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<left>0x[0-9a-f]+|0b[01]+|\d+)
        \s*
        (?P<operator><<|>>|&|\||\^)
        \s*
        (?P<right>0x[0-9a-f]+|0b[01]+|\d+)
        \s*$
        ",
    )
    .ok()?;
    let captures = bitwise_regex.captures(input_text)?;
    let left_value = parse_integer_literal(captures.name("left")?.as_str())?;
    let right_value = parse_integer_literal(captures.name("right")?.as_str())?;
    let calculated_value = match captures.name("operator")?.as_str() {
        "&" => left_value & right_value,
        "|" => left_value | right_value,
        "^" => left_value ^ right_value,
        "<<" => left_value.checked_shl(right_value as u32)?,
        ">>" => left_value.checked_shr(right_value as u32)?,
        _ => return None,
    };

    Some(programmer_value_result(
        input_text,
        calculated_value,
        "Result",
        92,
    ))
}

fn programmer_value_result(
    input_text: &str,
    value: i64,
    result_label: &str,
    confidence: u8,
) -> CommandResult {
    let formatted_value = value.to_string();
    CommandResult::calculation_with_display(
        formatted_value.clone(),
        input_text,
        formatted_value.clone(),
        format!(
            "{input_text} = {formatted_value} (0x{:X}, 0b{:b}).",
            value, value
        ),
        "Programmer",
        result_label,
        confidence,
    )
}

fn parse_integer_literal(value_text: &str) -> Option<i64> {
    let trimmed_value = value_text.trim().replace('_', "");
    if trimmed_value.starts_with("0x") || trimmed_value.starts_with("0X") {
        return i64::from_str_radix(&trimmed_value[2..], 16).ok();
    }
    if trimmed_value.starts_with("0b") || trimmed_value.starts_with("0B") {
        return i64::from_str_radix(&trimmed_value[2..], 2).ok();
    }
    trimmed_value.parse::<i64>().ok()
}

pub(crate) fn evaluate_relative_datetime(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    if let Some(simple_day_result) = evaluate_named_day(input_text, context) {
        return Some(simple_day_result);
    }

    if let Some(now_offset_result) = evaluate_now_offset_datetime(input_text, context) {
        return Some(now_offset_result);
    }

    let relative_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:in\s+)?
        (?P<count>\d+(?:\.\d+)?)
        \s*
        (?P<unit>
            milliseconds?|ms|
            seconds?|secs?|s|
            minutes?|mins?|m|
            hours?|hrs?|h|
            days?|d|
            weeks?|w|
            fortnights?|fn|
            months?|mos?|mo|
            years?|yrs?|y
        )
        \s*
        (?P<direction>from\s+now|from\s+today|after\s+now|ago)?
        (?:\s+in\s+(?P<timezone>.+))?
        \s*$
        ",
    )
    .ok()?;
    let captures = relative_regex.captures(input_text)?;
    let unit_count = captures.name("count")?.as_str().parse::<f64>().ok()?;
    let time_unit = normalize_time_unit(captures.name("unit")?.as_str())?;
    let direction = captures
        .name("direction")
        .map(|direction_match| direction_match.as_str().to_lowercase())
        .unwrap_or_else(|| "from now".to_string());
    let signed_count = if direction == "ago" {
        -unit_count
    } else {
        unit_count
    };

    let calculated_datetime = add_time_unit(context.now, time_unit, signed_count)?;
    let timezone_text = captures.name("timezone").map(|value| value.as_str().trim());
    let (display_datetime, reference_datetime, result_label) =
        resolve_relative_datetime_display(calculated_datetime, context, timezone_text)?;

    let copy_text = if matches!(time_unit, "millisecond" | "second" | "minute" | "hour") {
        format_datetime_for_reference(
            display_datetime,
            reference_datetime,
            precision_for_time_unit(time_unit),
            DateTimeDisplayMode::TimeOnlyForSameDay,
        )
    } else {
        format_date_for_reference(
            display_datetime.date_naive(),
            reference_datetime.date_naive(),
        )
    };

    Some(CommandResult::calculation_with_display(
        copy_text.clone(),
        input_text,
        copy_text.clone(),
        format!("{input_text} resolves to {copy_text}."),
        "Date",
        result_label,
        94,
    ))
}

fn evaluate_now_offset_datetime(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let now_offset_regex = Regex::new(
        r"(?ix)
        ^\s*
        now
        \s*\+\s*
        (?P<count>\d+(?:\.\d+)?)
        \s*
        (?P<unit>
            milliseconds?|ms|
            seconds?|secs?|s|
            minutes?|mins?|m|
            hours?|hrs?|h|
            days?|d|
            weeks?|w|
            fortnights?|fn|
            months?|mos?|mo|
            years?|yrs?|y
        )
        (?:\s+in\s+(?P<timezone>.+))?
        \s*$
        ",
    )
    .ok()?;
    let captures = now_offset_regex.captures(input_text)?;
    let unit_count = captures.name("count")?.as_str().parse::<f64>().ok()?;
    let time_unit = normalize_time_unit(captures.name("unit")?.as_str())?;
    let calculated_datetime = add_time_unit(context.now, time_unit, unit_count)?;
    let timezone_text = captures.name("timezone").map(|value| value.as_str().trim());
    let (display_datetime, reference_datetime, result_label) =
        resolve_relative_datetime_display(calculated_datetime, context, timezone_text)?;

    let copy_text = if matches!(time_unit, "millisecond" | "second" | "minute" | "hour") {
        format_datetime_for_reference(
            display_datetime,
            reference_datetime,
            precision_for_time_unit(time_unit),
            DateTimeDisplayMode::TimeOnlyForSameDay,
        )
    } else {
        format_date_for_reference(
            display_datetime.date_naive(),
            reference_datetime.date_naive(),
        )
    };

    Some(CommandResult::calculation_with_display(
        copy_text.clone(),
        input_text,
        copy_text.clone(),
        format!("{input_text} resolves to {copy_text}."),
        "Date",
        result_label,
        94,
    ))
}

fn resolve_relative_datetime_display(
    calculated_datetime: DateTime<Tz>,
    context: &CalculationContext,
    timezone_text: Option<&str>,
) -> Option<(DateTime<Tz>, DateTime<Tz>, String)> {
    let Some(timezone_text) = timezone_text.filter(|value| !value.is_empty()) else {
        return Some((calculated_datetime, context.now, "Result".to_string()));
    };

    let timezone =
        timezone_resolver::resolve_timezone(timezone_text, &context.settings)?;
    let display_datetime = calculated_datetime.with_timezone(&timezone.timezone);
    let reference_datetime = context.now.with_timezone(&timezone.timezone);
    Some((
        display_datetime,
        reference_datetime,
        timezone.display_name.clone(),
    ))
}

fn evaluate_named_day(input_text: &str, context: &CalculationContext) -> Option<CommandResult> {
    let named_day_time_regex =
        Regex::new(r"(?i)^\s*(today|tomorrow|yesterday)\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)\s*$")
            .ok()?;
    if let Some(captures) = named_day_time_regex.captures(input_text) {
        let day_name = captures.get(1)?.as_str().to_lowercase();
        let time_text = captures.get(2)?.as_str();
        let resolved_date = match day_name.as_str() {
            "today" => context.now.date_naive(),
            "tomorrow" => context.now.date_naive() + Duration::days(1),
            "yesterday" => context.now.date_naive() - Duration::days(1),
            _ => return None,
        };
        let resolved_time = parse_time_text(time_text)?;
        return Some(format_date_or_datetime_result(
            input_text,
            resolved_date,
            Some(resolved_time),
            context.now,
            context.now.timezone(),
            92,
        ));
    }

    let normalized_input = input_text.trim().to_lowercase();
    let resolved_date = match normalized_input.as_str() {
        "today" => context.now.date_naive(),
        "tomorrow" => context.now.date_naive() + Duration::days(1),
        "yesterday" => context.now.date_naive() - Duration::days(1),
        _ => return None,
    };
    let formatted_date = format_date_for_reference(resolved_date, context.now.date_naive());

    Some(CommandResult::calculation_with_display(
        formatted_date.clone(),
        input_text,
        formatted_date.clone(),
        format!("{input_text} is {formatted_date}."),
        "Date",
        "Result",
        91,
    ))
}

pub(crate) fn evaluate_weekday_expression(
    input_text: &str,
    context: &CalculationContext,
) -> Option<CommandResult> {
    let next_weekday_regex = Regex::new(r"(?i)^\s*next\s+([a-z]+)(?:\s+at\s+(.+?))?\s*$").ok()?;
    if let Some(captures) = next_weekday_regex.captures(input_text) {
        let weekday = parse_weekday(captures.get(1)?.as_str())?;
        let target_date = next_weekday_after(context.now.date_naive(), weekday);
        let optional_time = captures
            .get(2)
            .and_then(|time_match| parse_time_text(time_match.as_str()));

        return Some(format_date_or_datetime_result(
            input_text,
            target_date,
            optional_time,
            context.now,
            context.now.timezone(),
            92,
        ));
    }

    let weeks_from_weekday_regex =
        Regex::new(r"(?i)^\s*(\d+)\s+weeks?\s+from\s+([a-z]+)(?:\s+at\s+(.+?))?\s*$").ok()?;
    let captures = weeks_from_weekday_regex.captures(input_text)?;
    let week_count = captures.get(1)?.as_str().parse::<i64>().ok()?;
    let weekday = parse_weekday(captures.get(2)?.as_str())?;
    let anchor_date = next_weekday_after(context.now.date_naive(), weekday);
    let target_date = anchor_date + Duration::weeks(week_count);
    let optional_time = captures
        .get(3)
        .and_then(|time_match| parse_time_text(time_match.as_str()));

    Some(format_date_or_datetime_result(
        input_text,
        target_date,
        optional_time,
        context.now,
        context.now.timezone(),
        90,
    ))
}

pub(crate) fn evaluate_timezone_conversion(
    input_text: &str,
    context: &CalculationContext,
) -> Option<Vec<CommandResult>> {
    let conversion_parts = split_timezone_conversion(input_text)?;
    let source_text = conversion_parts.0;
    let destination_timezone_text = conversion_parts.1;

    let conversion_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?:
            (?P<date>next\s+[a-z]+|tomorrow|today|yesterday|[a-z]+|\d{4}-\d{2}-\d{2})
            \s+
        )?
        (?P<time>\d{1,2}(?::\d{2})?\s*(?:am|pm)?)
        \s+
        (?P<source>.+?)
        \s*$
        ",
    )
    .ok()?;
    let captures = conversion_regex.captures(source_text)?;

    let date_phrase = captures.name("date").map(|date_match| date_match.as_str());
    let source_time = parse_time_text(captures.name("time")?.as_str())?;
    let source_timezone =
        timezone_resolver::resolve_timezone(captures.name("source")?.as_str(), &context.settings)?;
    let destination_timezone =
        timezone_resolver::resolve_timezone(destination_timezone_text, &context.settings)?;
    let source_date = resolve_date_phrase(date_phrase, context)?;
    let source_naive_datetime = NaiveDateTime::new(source_date, source_time);

    let source_datetimes = resolve_local_datetime(source_timezone.timezone, source_naive_datetime)?;

    let results = source_datetimes
        .into_iter()
        .map(|source_datetime| {
            let destination_datetime =
                source_datetime.with_timezone(&destination_timezone.timezone);
            let reference_datetime = context.now.with_timezone(&destination_timezone.timezone);
            let title = format_datetime_for_reference(
                destination_datetime,
                reference_datetime,
                TimePrecision::Minute,
                DateTimeDisplayMode::AlwaysShowDate,
            );
            let source_label = format_time(source_datetime);
            let readable_destination = single_line_datetime(&title);

            CommandResult::calculation_with_display(
                title.clone(),
                format!(
                    "{} to {}",
                    source_timezone.display_name, destination_timezone.display_name
                ),
                title,
                format!(
                    "{source_label} in {} is {readable_destination}.",
                    source_timezone.display_name,
                ),
                "Time",
                destination_timezone.display_name.clone(),
                98,
            )
        })
        .collect::<Vec<_>>();

    Some(results)
}

fn split_timezone_conversion(input_text: &str) -> Option<(&str, &str)> {
    let separator_regex = Regex::new(r"(?i)\s+(?:->|to|-)\s+").ok()?;
    let separator_match = separator_regex.find(input_text)?;
    let source_text = input_text[..separator_match.start()].trim();
    let destination_timezone_text = input_text[separator_match.end()..].trim();

    (!source_text.is_empty() && !destination_timezone_text.is_empty())
        .then_some((source_text, destination_timezone_text))
}

fn resolve_local_datetime(
    timezone: Tz,
    naive_datetime: NaiveDateTime,
) -> Option<Vec<DateTime<Tz>>> {
    match timezone.from_local_datetime(&naive_datetime) {
        LocalResult::Single(datetime) => Some(vec![datetime]),
        LocalResult::Ambiguous(first_datetime, second_datetime) => {
            Some(vec![first_datetime, second_datetime])
        }
        LocalResult::None => None,
    }
}

fn resolve_date_phrase(
    date_phrase: Option<&str>,
    context: &CalculationContext,
) -> Option<NaiveDate> {
    match date_phrase.map(|phrase| phrase.trim().to_lowercase()) {
        None => Some(context.now.date_naive()),
        Some(phrase) if phrase == "today" => Some(context.now.date_naive()),
        Some(phrase) if phrase == "tomorrow" => Some(context.now.date_naive() + Duration::days(1)),
        Some(phrase) if phrase == "yesterday" => Some(context.now.date_naive() - Duration::days(1)),
        Some(phrase) if phrase.starts_with("next ") => {
            let weekday_text = phrase.trim_start_matches("next ").trim();
            let weekday = parse_weekday(weekday_text)?;
            Some(next_weekday_after(context.now.date_naive(), weekday))
        }
        Some(phrase) => parse_weekday(&phrase)
            .map(|weekday| next_or_same_weekday(context.now.date_naive(), weekday))
            .or_else(|| NaiveDate::parse_from_str(&phrase, "%Y-%m-%d").ok()),
    }
}

fn add_time_unit(
    datetime: DateTime<Tz>,
    time_unit: &str,
    signed_count: f64,
) -> Option<DateTime<Tz>> {
    match time_unit {
        "millisecond" => datetime.checked_add_signed(Duration::milliseconds(
            (signed_count * 1_000.0).round() as i64,
        )),
        "second" => datetime.checked_add_signed(Duration::milliseconds(
            (signed_count * 1_000.0).round() as i64,
        )),
        "minute" => datetime.checked_add_signed(Duration::milliseconds(
            (signed_count * 60_000.0).round() as i64,
        )),
        "hour" => datetime.checked_add_signed(Duration::milliseconds(
            (signed_count * 3_600_000.0).round() as i64,
        )),
        "day" => datetime.checked_add_signed(Duration::days(signed_count.round() as i64)),
        "week" => datetime.checked_add_signed(Duration::weeks(signed_count.round() as i64)),
        "fortnight" => datetime.checked_add_signed(Duration::weeks(
            (signed_count * 2.0).round() as i64,
        )),
        "month" => add_months(datetime, signed_count.round() as i64),
        "year" => add_months(datetime, (signed_count * 12.0).round() as i64),
        _ => None,
    }
}

fn normalize_time_unit(time_unit_text: &str) -> Option<&'static str> {
    match time_unit_text.trim().to_lowercase().as_str() {
        "ms" | "millisecond" | "milliseconds" => Some("millisecond"),
        "s" | "sec" | "secs" | "second" | "seconds" => Some("second"),
        "m" | "min" | "mins" | "minute" | "minutes" => Some("minute"),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some("hour"),
        "d" | "day" | "days" => Some("day"),
        "w" | "week" | "weeks" => Some("week"),
        "fn" | "fortnight" | "fortnights" => Some("fortnight"),
        "mo" | "mos" | "month" | "months" => Some("month"),
        "y" | "yr" | "yrs" | "year" | "years" => Some("year"),
        _ => None,
    }
}

fn precision_for_time_unit(time_unit: &str) -> TimePrecision {
    if matches!(time_unit, "millisecond" | "second") {
        TimePrecision::Second
    } else {
        TimePrecision::Minute
    }
}

fn add_months(datetime: DateTime<Tz>, signed_months: i64) -> Option<DateTime<Tz>> {
    let naive_datetime = datetime.naive_local();
    let date = naive_datetime.date();
    let time = naive_datetime.time();
    let adjusted_date = if signed_months >= 0 {
        date.checked_add_months(Months::new(signed_months as u32))?
    } else {
        date.checked_sub_months(Months::new((-signed_months) as u32))?
    };

    let adjusted_naive_datetime = NaiveDateTime::new(adjusted_date, time);
    resolve_local_datetime(datetime.timezone(), adjusted_naive_datetime)?
        .into_iter()
        .next()
}

fn parse_time_text(time_text: &str) -> Option<NaiveTime> {
    let time_regex = Regex::new(r"(?i)^\s*(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\s*$").ok()?;
    let captures = time_regex.captures(time_text)?;
    let mut hour = captures.get(1)?.as_str().parse::<u32>().ok()?;
    let minute = captures
        .get(2)
        .map(|minute_match| minute_match.as_str().parse::<u32>().ok())
        .unwrap_or(Some(0))?;
    let meridiem = captures
        .get(3)
        .map(|meridiem| meridiem.as_str().to_lowercase());

    match meridiem.as_deref() {
        Some("am") if hour == 12 => hour = 0,
        Some("am") => {}
        Some("pm") if hour < 12 => hour += 12,
        Some("pm") if hour == 12 => {}
        Some("pm") => return None,
        None => {}
        _ => return None,
    }

    NaiveTime::from_hms_opt(hour, minute, 0)
}

fn parse_weekday(weekday_text: &str) -> Option<Weekday> {
    match weekday_text.trim().to_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn parse_flexible_date(date_text: &str, context: &CalculationContext) -> Option<NaiveDate> {
    let normalized_date = date_text.trim().trim_matches(',').to_lowercase();
    if let Ok(date) = NaiveDate::parse_from_str(&normalized_date, "%Y-%m-%d") {
        return Some(date);
    }

    let month_day_regex = Regex::new(
        r"(?ix)
        ^\s*
        (?P<month>[a-z]+)
        \s+
        (?P<day>\d{1,2})
        (?:st|nd|rd|th)?
        (?:,?\s+(?P<year>\d{4}))?
        \s*$
        ",
    )
    .ok()?;
    let captures = month_day_regex.captures(&normalized_date)?;
    let month = parse_month_number(captures.name("month")?.as_str())?;
    let day = captures.name("day")?.as_str().parse::<u32>().ok()?;
    let year = captures
        .name("year")
        .and_then(|year| year.as_str().parse::<i32>().ok())
        .unwrap_or_else(|| context.now.year());

    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_month_number(month_text: &str) -> Option<u32> {
    match month_text.trim().to_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn next_weekday_after(start_date: NaiveDate, target_weekday: Weekday) -> NaiveDate {
    let current_weekday_number = start_date.weekday().num_days_from_monday() as i64;
    let target_weekday_number = target_weekday.num_days_from_monday() as i64;
    let mut days_until_target = target_weekday_number - current_weekday_number;

    if days_until_target <= 0 {
        days_until_target += 7;
    }

    start_date + Duration::days(days_until_target)
}

fn next_or_same_weekday(start_date: NaiveDate, target_weekday: Weekday) -> NaiveDate {
    let current_weekday_number = start_date.weekday().num_days_from_monday() as i64;
    let target_weekday_number = target_weekday.num_days_from_monday() as i64;
    let mut days_until_target = target_weekday_number - current_weekday_number;

    if days_until_target < 0 {
        days_until_target += 7;
    }

    start_date + Duration::days(days_until_target)
}

fn format_date_or_datetime_result(
    input_text: &str,
    target_date: NaiveDate,
    optional_time: Option<NaiveTime>,
    reference_datetime: DateTime<Tz>,
    timezone: Tz,
    confidence: u8,
) -> CommandResult {
    if let Some(target_time) = optional_time {
        let naive_datetime = NaiveDateTime::new(target_date, target_time);
        let formatted_datetime = resolve_local_datetime(timezone, naive_datetime)
            .and_then(|datetimes| datetimes.into_iter().next())
            .map(|datetime| {
                format_datetime_for_reference(
                    datetime,
                    reference_datetime,
                    TimePrecision::Minute,
                    DateTimeDisplayMode::AlwaysShowDate,
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "{}\n{}",
                    format_date_with_year(target_date),
                    format_plain_time(target_time, TimePrecision::Minute)
                )
            });

        return CommandResult::calculation_with_display(
            formatted_datetime.clone(),
            input_text,
            formatted_datetime.clone(),
            format!("{input_text} resolves to {formatted_datetime}."),
            "Date",
            "Result",
            confidence,
        );
    }

    let formatted_date = format_date_for_reference(target_date, reference_datetime.date_naive());
    CommandResult::calculation_with_display(
        formatted_date.clone(),
        input_text,
        formatted_date.clone(),
        format!("{input_text} resolves to {formatted_date}."),
        "Date",
        "Result",
        confidence,
    )
}

fn looks_like_math_expression(input_text: &str) -> bool {
    let has_digit = input_text
        .chars()
        .any(|character| character.is_ascii_digit());
    let has_math_operator = input_text
        .chars()
        .any(|character| matches!(character, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')'));
    let contains_only_math_characters = input_text.chars().all(|character| {
        character.is_ascii_digit()
            || character.is_ascii_whitespace()
            || matches!(
                character,
                '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.' | ',' | '_'
            )
            || character.is_ascii_alphabetic()
    });

    has_digit && has_math_operator && contains_only_math_characters
}

fn format_number(value: f64) -> String {
    let rounded_value = (value * 1_000_000_000.0).round() / 1_000_000_000.0;
    if (rounded_value.fract()).abs() < f64::EPSILON {
        format!("{}", rounded_value as i64)
    } else {
        let mut formatted_value = format!("{rounded_value:.9}");
        while formatted_value.contains('.') && formatted_value.ends_with('0') {
            formatted_value.pop();
        }
        if formatted_value.ends_with('.') {
            formatted_value.pop();
        }
        formatted_value
    }
}

fn format_unit_number(value: f64) -> String {
    let decimal_places = if value.abs() >= 100.0 { 2 } else { 4 };
    let rounded_value = 10_f64.powi(decimal_places) * value;
    trim_decimal_text(&format!(
        "{:.*}",
        decimal_places as usize,
        rounded_value.round() / 10_f64.powi(decimal_places)
    ))
}

fn trim_decimal_text(value_text: &str) -> String {
    let mut trimmed_text = value_text.to_string();
    while trimmed_text.contains('.') && trimmed_text.ends_with('0') {
        trimmed_text.pop();
    }
    if trimmed_text.ends_with('.') {
        trimmed_text.pop();
    }
    trimmed_text
}

fn capitalize_ascii(text: &str) -> String {
    let mut characters = text.chars();
    let Some(first_character) = characters.next() else {
        return String::new();
    };

    format!(
        "{}{}",
        first_character.to_ascii_uppercase(),
        characters.as_str()
    )
}

fn format_time(datetime: DateTime<Tz>) -> String {
    format!(
        "{} {}",
        format_plain_time(datetime.time(), TimePrecision::Minute),
        datetime.format("%Z")
    )
}

fn format_datetime_for_reference(
    datetime: DateTime<Tz>,
    reference_datetime: DateTime<Tz>,
    time_precision: TimePrecision,
    display_mode: DateTimeDisplayMode,
) -> String {
    let date = datetime.date_naive();
    let reference_date = reference_datetime.date_naive();
    let formatted_time = format!(
        "{} {}",
        format_plain_time(datetime.time(), time_precision),
        datetime.format("%Z")
    );

    if display_mode == DateTimeDisplayMode::TimeOnlyForSameDay && date == reference_date {
        return formatted_time;
    }

    format!(
        "{}\n{}",
        format_date_for_reference(date, reference_date),
        formatted_time
    )
}

fn single_line_datetime(datetime_text: &str) -> String {
    datetime_text.replace('\n', " at ")
}

fn format_plain_time(time: NaiveTime, time_precision: TimePrecision) -> String {
    let hour = time.hour();
    let minute = time.minute();
    let second = time.second();
    let meridiem = if hour < 12 { "am" } else { "pm" };
    let hour_12 = match hour % 12 {
        0 => 12,
        value => value,
    };

    match time_precision {
        TimePrecision::Minute => format!("{hour_12}:{minute:02}{meridiem}"),
        TimePrecision::Second => format!("{hour_12}:{minute:02}:{second:02}{meridiem}"),
    }
}

fn format_date_for_reference(date: NaiveDate, reference_date: NaiveDate) -> String {
    if date.year() == reference_date.year() {
        format!(
            "{}, {} {}",
            weekday_name(date.weekday()),
            month_name(date.month()),
            date.day()
        )
    } else {
        format_date_with_year(date)
    }
}

fn format_date_with_year(date: NaiveDate) -> String {
    format_date_parts(date, true)
}

fn format_date_parts(date: NaiveDate, should_include_year: bool) -> String {
    let date_without_year = format!(
        "{}, {} {}",
        weekday_name(date.weekday()),
        month_name(date.month()),
        date.day()
    );

    if should_include_year {
        format!("{date_without_year}, {}", date.year())
    } else {
        date_without_year
    }
}

fn weekday_name(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn month_name(month_number: u32) -> &'static str {
    match month_number {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_context() -> CalculationContext {
        context_at(2026, 5, 17, 19, 0, 0)
    }

    fn context_at(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> CalculationContext {
        let settings = LauncherSettings {
            local_timezone: "Europe/London".to_string(),
            ..LauncherSettings::default()
        };
        let timezone = chrono_tz::Europe::London;
        let now = timezone
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .unwrap();

        CalculationContext { now, settings }
    }

    #[test]
    fn evaluates_two_days_from_now() {
        let results = evaluate_calculation("2 days from now", &fixed_context());

        assert_eq!(results[0].copy_text, "Tuesday, May 19");
    }

    #[test]
    fn evaluates_compact_relative_duration_units() {
        let context = fixed_context();

        let hour_results = evaluate_calculation("2hrs", &context);
        assert_eq!(hour_results[0].copy_text, "9:00pm BST");

        let minute_results = evaluate_calculation("2m from now", &context);
        assert_eq!(minute_results[0].copy_text, "7:02pm BST");

        let second_results = evaluate_calculation("45s from now", &context);
        assert_eq!(second_results[0].copy_text, "7:00:45pm BST");

        let thirty_six_minutes = evaluate_calculation("36m from now", &context);
        assert_eq!(thirty_six_minutes[0].copy_text, "7:36pm BST");
    }

    #[test]
    fn evaluates_duration_unit_conversions() {
        let results = evaluate_calculation("2hrs to m", &fixed_context());
        assert_eq!(results[0].copy_text, "120m");

        let minutes_to_hours = evaluate_calculation("90min to h", &fixed_context());
        assert_eq!(minutes_to_hours[0].copy_text, "1.5h");

        let milliseconds = evaluate_calculation("1000ms to s", &fixed_context());
        assert_eq!(milliseconds[0].copy_text, "1s");
    }

    #[test]
    fn evaluates_fractional_relative_datetime() {
        let context = fixed_context();
        let results = evaluate_calculation("1.5h from now", &context);
        assert_eq!(results[0].copy_text, "8:30pm BST");
    }

    #[test]
    fn includes_year_when_relative_datetime_crosses_year() {
        let context = context_at(2026, 12, 31, 23, 0, 0);
        let results = evaluate_calculation("2hrs", &context);

        assert_eq!(results[0].copy_text, "Friday, January 1, 2027\n1:00am GMT");
    }

    #[test]
    fn evaluates_pt_to_uk_conversion() {
        let results = evaluate_calculation("2pm pt to uk", &fixed_context());

        assert_eq!(results[0].title, "Sunday, May 17\n10:00pm BST");
    }

    #[test]
    fn evaluates_weekday_timezone_conversion_with_dash_separator() {
        let results = evaluate_calculation("Tuesday 2pm ET - UK", &fixed_context());

        assert_eq!(results[0].title, "Tuesday, May 19\n7:00pm BST");
        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.expression.as_str()),
            Some("ET to UK")
        );
    }

    #[test]
    fn evaluates_current_time_lookup() {
        let results = evaluate_calculation("time in london", &fixed_context());

        assert_eq!(results[0].title, "Sunday, May 17\n7:00pm BST");
        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.kind_label.as_str()),
            Some("Time")
        );
    }

    #[test]
    fn evaluates_current_time_lookup_for_database_city() {
        let results = evaluate_calculation("time in dubai", &fixed_context());

        assert_eq!(results[0].title, "Sunday, May 17\n10:00pm +04");
    }

    #[test]
    fn evaluates_next_monday_london_to_tokyo() {
        let results = evaluate_calculation("next monday 9am london to tokyo", &fixed_context());

        assert_eq!(results[0].title, "Monday, May 18\n5:00pm JST");
    }

    #[test]
    fn evaluates_percentage_expression() {
        let results = evaluate_calculation("15% of 89.99", &fixed_context());

        assert_eq!(results[0].copy_text, "13.4985");
    }

    #[test]
    fn evaluates_currency_conversion() {
        let results = evaluate_calculation("10 USD to GBP", &fixed_context());

        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.kind_label.as_str()),
            Some("Currency")
        );
    }

    #[test]
    fn evaluates_symbol_amount_against_local_currency() {
        let results = evaluate_calculation("$2", &fixed_context());

        assert_eq!(results[0].copy_text, "£1.57");
        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.result_label.as_str()),
            Some("GBP")
        );
    }

    #[test]
    fn evaluates_local_symbol_amount_against_usd() {
        let results = evaluate_calculation("£2", &fixed_context());

        assert_eq!(results[0].copy_text, "$2.54");
    }

    #[test]
    fn evaluates_unit_conversion() {
        let results = evaluate_calculation("5 km to mi", &fixed_context());

        assert_eq!(results[0].copy_text, "3.1069 mi");
    }

    #[test]
    fn trims_noisy_unit_conversion_precision() {
        let results = evaluate_calculation("1gb to Mib", &fixed_context());

        assert_eq!(results[0].copy_text, "953.67 MiB");
    }

    #[test]
    fn evaluates_temperature_conversion() {
        let results = evaluate_calculation("32 f to c", &fixed_context());

        assert_eq!(results[0].copy_text, "0 °C");
    }

    #[test]
    fn evaluates_currency_percentage_expression() {
        let results = evaluate_calculation("12% of $321 in jpy", &fixed_context());

        assert!(results[0].title.starts_with('¥'));
        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.kind_label.as_str()),
            Some("Percentage")
        );
    }

    #[test]
    fn evaluates_duration_arithmetic() {
        let results = evaluate_calculation("1h 20m + 45m", &fixed_context());

        assert_eq!(results[0].copy_text, "2h 5m");
    }

    #[test]
    fn evaluates_date_range() {
        let results = evaluate_calculation("days between June 1 and Aug 12", &fixed_context());

        assert_eq!(results[0].copy_text, "72");
    }

    #[test]
    fn evaluates_commerce_helpers() {
        let tip_results = evaluate_calculation("tip 20% on $45", &fixed_context());
        assert_eq!(tip_results[0].copy_text, "$54.00");

        let discount_results = evaluate_calculation("15% off £80", &fixed_context());
        assert_eq!(discount_results[0].copy_text, "£68.00");
    }

    #[test]
    fn evaluates_finance_helpers() {
        let loan_results = evaluate_calculation("loan $10000 at 6% for 5 years", &fixed_context());
        assert_eq!(loan_results[0].copy_text, "$193.33");

        let apy_results = evaluate_calculation("apy from 5% monthly", &fixed_context());
        assert_eq!(apy_results[0].copy_text, "5.116189788%");
    }

    #[test]
    fn evaluates_programmer_helpers() {
        let conversion_results = evaluate_calculation("hex 255", &fixed_context());
        assert_eq!(conversion_results[0].copy_text, "0xFF");

        let bitwise_results = evaluate_calculation("0xff & 0x0f", &fixed_context());
        assert_eq!(bitwise_results[0].copy_text, "15");
    }

    #[test]
    fn evaluates_unix_timestamp() {
        let results = evaluate_calculation("unix 1779292800", &fixed_context());

        assert_eq!(
            results[0]
                .calculation_display
                .as_ref()
                .map(|display| display.kind_label.as_str()),
            Some("Timestamp")
        );
    }

    #[test]
    fn evaluates_quote_fallbacks_and_recipe_units() {
        let quote_results = evaluate_calculation("quote BTC", &fixed_context());
        assert_eq!(quote_results[0].copy_text, "$65,000.00");

        let recipe_results = evaluate_calculation("2 cups to tbsp", &fixed_context());
        assert_eq!(recipe_results[0].copy_text, "32 tbsp");

        let data_results = evaluate_calculation("8 bits to bytes", &fixed_context());
        assert_eq!(data_results[0].copy_text, "1 B");
    }

    #[test]
    fn evaluates_now_offset_datetime() {
        let context = fixed_context();
        let results = evaluate_calculation("now + 36m", &context);
        assert_eq!(results[0].copy_text, "7:36pm BST");

        let hours = evaluate_calculation("now + 2h", &context);
        assert_eq!(hours[0].copy_text, "9:00pm BST");
    }

    #[test]
    fn evaluates_named_day_with_time() {
        let context = fixed_context();
        let results = evaluate_calculation("tomorrow 3pm", &context);
        assert_eq!(results[0].copy_text, "Monday, May 18\n3:00pm BST");
    }

    #[test]
    fn evaluates_relative_datetime_in_timezone() {
        let context = fixed_context();
        let results = evaluate_calculation("36m from now in Tokyo", &context);
        assert_eq!(results[0].copy_text, "3:36am JST");
    }

    #[test]
    fn evaluates_data_rate_conversion() {
        let results = evaluate_calculation("100 Mbps to MB/s", &fixed_context());
        assert_eq!(results[0].copy_text, "12.5 MB/s");
    }
}
