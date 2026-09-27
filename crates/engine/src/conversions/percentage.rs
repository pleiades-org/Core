//! Everyday percentage questions. Plain `25% of 80` stays with the calculator.
use super::{
    format::{format_number, readable, significant},
    phrase::{tokenize, Token, Token::*},
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy number · Esc to hide";

pub fn calculate_percentage(input: &str) -> Outcome {
    let tokens = tokenize(input)?;
    let answer = match tokens.as_slice() {
        // 20% off 80 · 20% discount on 80
        [Percent(rate), Word(off), Number(base)] if off == "off" => Ok(discount(*rate, *base)),
        [Percent(rate), Word(keyword), Word(on), Number(base)]
            if keyword == "discount" && (on == "on" || on == "off") =>
        {
            Ok(discount(*rate, *base))
        }
        // 20 is what % of 80 · 20 as a % of 80 · what % of 80 is 20
        [Number(part), Word(is), Word(what), Word(pct), Word(of), Number(whole)]
            if is == "is" && what == "what" && pct == "%" && of == "of" =>
        {
            share(*part, *whole)
        }
        [Number(part), Word(r#as), rest @ .., Word(of), Number(whole)]
            if r#as == "as" && of == "of" && is_percent_phrase(rest) =>
        {
            share(*part, *whole)
        }
        [Word(what), Word(pct), Word(of), Number(whole), Word(is), Number(part)]
            if what == "what" && pct == "%" && of == "of" && is == "is" =>
        {
            share(*part, *whole)
        }
        // % change from 50 to 75 · change from 50 to 75
        [rest @ .., Number(from), Word(to), Number(target)]
            if to == "to" && is_change_prefix(rest) =>
        {
            change(*from, *target)
        }
        // increase 50 by 10% · 50 decreased by 10%
        [Word(verb), Number(base), Word(by), Percent(rate)] if by == "by" => {
            Ok(adjust(verb, *base, *rate)?)
        }
        [Number(base), Word(verb), Word(by), Percent(rate)] if by == "by" => {
            Ok(adjust(verb.trim_end_matches('d'), *base, *rate)?)
        }
        // 15 is 20% of what · 20% of what is 15
        [Number(part), Word(is), Percent(rate), Word(of), Word(what)]
            if is == "is" && of == "of" && what == "what" =>
        {
            base_of(*part, *rate)
        }
        [Percent(rate), Word(of), Word(what), Word(is), Number(part)]
            if of == "of" && what == "what" && is == "is" =>
        {
            base_of(*part, *rate)
        }
        _ => return None,
    };
    Some(answer.map(|answer| Conversion::single(answer, MESSAGE)))
}

/// `%` or `a %` between `as` and `of`.
fn is_percent_phrase(tokens: &[Token]) -> bool {
    match tokens {
        [Word(percent)] => percent == "%",
        [Word(article), Word(percent)] => article == "a" && percent == "%",
        _ => false,
    }
}

fn is_change_prefix(tokens: &[Token]) -> bool {
    let words: Vec<&str> = tokens
        .iter()
        .map(|token| match token {
            Word(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Option<_>>()
        .unwrap_or_default();
    matches!(
        words.as_slice(),
        ["%", "change", "from"]
            | ["%", "change"]
            | ["change", "from"]
            | ["%", "difference", "from"]
            | ["%", "increase", "from"]
            | ["%", "decrease", "from"]
    )
}

fn number(value: f64, detail: String) -> Calculation {
    Calculation {
        title: readable(value),
        detail,
        copy: format_number(significant(value, 12)),
    }
}

fn percent(value: f64, detail: String) -> Calculation {
    Calculation {
        title: format!("{}%", readable(value)),
        detail,
        copy: format_number(significant(value, 12)),
    }
}

fn discount(rate: f64, base: f64) -> Calculation {
    let saving = base * rate / 100.;
    number(
        base - saving,
        format!(
            "{} − {}% · you save {}",
            readable(base),
            readable(rate),
            readable(saving)
        ),
    )
}

fn share(part: f64, whole: f64) -> Result<Calculation, &'static str> {
    if whole == 0. {
        return Err("Cannot take a percentage of zero");
    }
    Ok(percent(
        part / whole * 100.,
        format!("{} of {}", readable(part), readable(whole)),
    ))
}

fn change(from: f64, target: f64) -> Result<Calculation, &'static str> {
    if from == 0. {
        return Err("Percentage change from zero is undefined");
    }
    let change = (target - from) / from.abs() * 100.;
    let direction = if change >= 0. { "increase" } else { "decrease" };
    Ok(Calculation {
        title: format!(
            "{}{}%",
            if change > 0. { "+" } else { "" },
            readable(change)
        ),
        detail: format!(
            "{} → {} · {direction} of {}",
            readable(from),
            readable(target),
            readable((target - from).abs())
        ),
        copy: format_number(significant(change, 12)),
    })
}

/// `None` for other verbs (`multiply 50 by 10%`) so another parser may recognise them.
fn adjust(verb: &str, base: f64, rate: f64) -> Option<Calculation> {
    let sign = match verb {
        "increase" | "raise" | "add" => 1.,
        "decrease" | "reduce" | "lower" => -1.,
        _ => return None,
    };
    let delta = base * rate / 100.;
    Some(number(
        base + sign * delta,
        format!(
            "{} {} {}% ({})",
            readable(base),
            if sign > 0. { "+" } else { "−" },
            readable(rate),
            readable(delta)
        ),
    ))
}

fn base_of(part: f64, rate: f64) -> Result<Calculation, &'static str> {
    if rate == 0. {
        return Err("0% of any number is zero");
    }
    Ok(number(
        part / (rate / 100.),
        format!("{} is {}% of this number", readable(part), readable(rate)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(query: &str) -> String {
        calculate_percentage(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .remove(0)
            .title
    }

    #[test]
    fn everyday_percentage_questions() {
        for (query, expected) in [
            ("20% off 80", "64"),
            ("15 percent discount on $200", "170"),
            ("20 is what % of 80", "25%"),
            ("20 is what percent of 80?", "25%"),
            ("20 as a % of 80", "25%"),
            ("what % of 80 is 20", "25%"),
            ("% change from 50 to 75", "+50%"),
            ("percent change 80 to 60", "-25%"),
            ("change from 1.2k to 1.5k", "+25%"),
            ("increase 50 by 10%", "55"),
            ("decrease 50 by 10 percent", "45"),
            ("50 increased by 20%", "60"),
            ("15 is 20% of what", "75"),
            ("20% of what is 15", "75"),
        ] {
            assert_eq!(title(query), expected, "{query}");
        }
    }

    #[test]
    fn calculator_percentages_and_zero_bases_are_handled_elsewhere_or_explained() {
        for query in ["25% of 80", "20 + 10%", "xbox", "50 to 75"] {
            assert!(calculate_percentage(query).is_none(), "{query}");
        }
        assert!(calculate_percentage("% change from 0 to 5")
            .unwrap()
            .is_err());
        assert!(calculate_percentage("5 is what % of 0").unwrap().is_err());
    }
}
