use crate::{
    calculator::CalculationContext,
    command::CommandResult,
};

pub fn dispatch_calculation(input_text: &str, context: &CalculationContext) -> Vec<CommandResult> {
    let trimmed_input = input_text.trim();

    if trimmed_input.is_empty() {
        return Vec::new();
    }

    if let Some(time_lookup_result) =
        crate::calculator::evaluate_current_time_lookup(trimmed_input, context)
    {
        return vec![time_lookup_result];
    }

    if let Some(timezone_results) =
        crate::calculator::evaluate_timezone_conversion(trimmed_input, context)
    {
        return timezone_results;
    }

    if let Some(programmer_result) =
        crate::calculator::evaluate_programmer_expression(trimmed_input, context)
    {
        return vec![programmer_result];
    }

    if let Some(unix_timestamp_result) =
        crate::calculator::evaluate_unix_timestamp(trimmed_input, context)
    {
        return vec![unix_timestamp_result];
    }

    if conversion_query_has_marker(trimmed_input) {
        if let Some(conversion_result) =
            crate::calculator::evaluate_unit_or_currency_conversion(trimmed_input)
        {
            return vec![conversion_result];
        }
    }

    if let Some(duration_result) = crate::calculator::evaluate_duration_arithmetic(trimmed_input) {
        return vec![duration_result];
    }

    if let Some(date_range_result) =
        crate::calculator::evaluate_date_range(trimmed_input, context)
    {
        return vec![date_range_result];
    }

    if let Some(relative_date_result) =
        crate::calculator::evaluate_relative_datetime(trimmed_input, context)
    {
        return vec![relative_date_result];
    }

    if let Some(weekday_result) =
        crate::calculator::evaluate_weekday_expression(trimmed_input, context)
    {
        return vec![weekday_result];
    }

    if let Some(currency_result) =
        crate::calculator::evaluate_quick_currency_amount(trimmed_input, context)
    {
        return vec![currency_result];
    }

    if let Some(quote_result) = crate::calculator::evaluate_market_quote(trimmed_input) {
        return vec![quote_result];
    }

    if let Some(commercial_result) =
        crate::calculator::evaluate_commercial_helper(trimmed_input, context)
    {
        return vec![commercial_result];
    }

    if let Some(finance_result) =
        crate::calculator::evaluate_finance_helper(trimmed_input, context)
    {
        return vec![finance_result];
    }

    if let Some(percent_result) = crate::calculator::evaluate_percentage_expression(trimmed_input)
    {
        return vec![percent_result];
    }

    if let Some(math_result) = crate::calculator::evaluate_math_expression(trimmed_input) {
        return vec![math_result];
    }

    Vec::new()
}

fn conversion_query_has_marker(input_text: &str) -> bool {
    input_text
        .split_whitespace()
        .any(|word| word.eq_ignore_ascii_case("to") || word.eq_ignore_ascii_case("in"))
}