//! Tips and bill splitting: `tip 15% on 80`, `80 tip 20% split 4`, `split 120 3 ways`.
use super::{
    format::{grouped, plain},
    phrase::{Token, Token::*},
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy amount · Esc to hide";
const MAX_PEOPLE: f64 = 1_000.;

/// `tokens` come from [`super::phrase::tokenize`], shared with the other sentence converters.
pub fn calculate_tip(tokens: &[Token]) -> Outcome {
    if let Some(outcome) = split_only(tokens) {
        return Some(outcome);
    }
    let (core, people) = split_clause(tokens);
    let (bill, rate) = match core {
        [tip, Percent(rate), on, Number(bill)] if tip.is("tip") && (on.is("on") || on.is("of")) => {
            (*bill, *rate)
        }
        [Percent(rate), tip, on, Number(bill)] if tip.is("tip") && (on.is("on") || on.is("of")) => {
            (*bill, *rate)
        }
        [Number(bill), tip, Percent(rate)] if tip.is("tip") => (*bill, *rate),
        [Number(bill), with, Percent(rate), tip]
            if (with.is("with") || with.is("+") || with.is("plus")) && tip.is("tip") =>
        {
            (*bill, *rate)
        }
        _ => return None,
    };
    let Some(people) = people else {
        return Some(Err("Split between 1 and 1000 people"));
    };
    Some(tip(bill, rate, people))
}

/// Removes a trailing `split 4`, `split 4 ways`, `for 4 people`, `between 4` or `4 ways`.
/// The count is `None` when present but not a sensible number of people.
fn split_clause<'a>(tokens: &'a [Token<'a>]) -> (&'a [Token<'a>], Option<f64>) {
    let people_word = |token: &Token| token.is("ways") || token.is("people") || token.is("persons");
    let (core, count) = match tokens {
        [core @ .., intro, Number(count), tail] if is_split_word(intro) && people_word(tail) => {
            (core, *count)
        }
        [core @ .., intro, Number(count)] if is_split_word(intro) => (core, *count),
        [core @ .., Number(count), tail] if tail.is("ways") => (core, *count),
        _ => return (tokens, Some(1.)),
    };
    (core, valid_people(count).then_some(count))
}

/// A whole number of people from 1 to 1000.
fn valid_people(count: f64) -> bool {
    (1. ..=MAX_PEOPLE).contains(&count) && count.fract() == 0.
}

fn is_split_word(token: &Token) -> bool {
    ["split", "for", "between", "among", "by"]
        .iter()
        .any(|word| token.is(word))
}

/// `split 120 3 ways`, `split 120 by 4`, `120 split 4`.
fn split_only(tokens: &[Token]) -> Option<Result<Conversion, &'static str>> {
    let (bill, rest) = match tokens {
        [split, Number(bill), rest @ ..] if split.is("split") => (*bill, rest),
        [Number(bill), split, rest @ ..] if split.is("split") => (*bill, rest),
        _ => return None,
    };
    let count = match rest {
        [Number(count)] => *count,
        [Number(count), tail] if tail.is("ways") || tail.is("people") => *count,
        [by, Number(count), ..] if is_split_word(by) || by.is("into") => *count,
        _ => return None,
    };
    if !valid_people(count) {
        return Some(Err("Split between 1 and 1000 people"));
    }
    Some(Ok(Conversion::single(
        money(
            bill / count,
            format!("{} split {} ways", grouped(bill, 2), count),
        ),
        MESSAGE,
    )))
}

fn tip(bill: f64, rate: f64, people: f64) -> Result<Conversion, &'static str> {
    if bill < 0. || rate < 0. {
        return Err("Use a positive bill and tip percentage");
    }
    let tip = bill * rate / 100.;
    let total = bill + tip;
    let mut answers = vec![
        money(
            total,
            format!("{} + {}% tip · total", grouped(bill, 2), rate),
        ),
        money(tip, format!("{rate}% tip on {}", grouped(bill, 2))),
    ];
    if people > 1. {
        answers.insert(
            0,
            money(
                total / people,
                format!("{} including tip · each of {people}", grouped(total, 2)),
            ),
        );
    }
    Ok(Conversion {
        answers,
        message: MESSAGE,
    })
}

fn money(value: f64, detail: String) -> Calculation {
    Calculation {
        title: grouped(value, 2),
        detail,
        copy: plain(value, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokenizes like `convert()` does, then calls the converter.
    fn calculate_tip(input: &str) -> Outcome {
        let mut text = String::new();
        super::calculate_tip(&super::super::phrase::tokenize(input, &mut text)?)
    }

    fn titles(query: &str) -> Vec<String> {
        calculate_tip(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn tips_totals_and_shares() {
        assert_eq!(titles("tip 15% on 80"), ["92.00", "12.00"]);
        assert_eq!(titles("20% tip on $1,250"), ["1,500.00", "250.00"]);
        assert_eq!(titles("80 tip 15%"), ["92.00", "12.00"]);
        assert_eq!(titles("tip 15% on 80 split 4"), ["23.00", "92.00", "12.00"]);
        assert_eq!(
            titles("80 with 20% tip for 3 people"),
            ["32.00", "96.00", "16.00"]
        );
        assert_eq!(titles("split 120 3 ways"), ["40.00"]);
        assert_eq!(titles("split 100 by 3"), ["33.33"]);
        assert_eq!(titles("90 split 4"), ["22.50"]);
    }

    #[test]
    fn unusual_counts_and_other_text() {
        assert!(calculate_tip("split 120 0 ways").unwrap().is_err());
        assert!(calculate_tip("tip 15% on 80 split 2.5").unwrap().is_err());
        for query in ["tip", "20% off 80", "xbox", "split screen"] {
            assert!(calculate_tip(query).is_none(), "{query}");
        }
    }
}
