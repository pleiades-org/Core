//! Loans and savings: `mortgage 250k at 4.5% for 25 years`, `loan 10000 at 7% over 36 months`,
//! `compound 1000 at 5% for 10 years monthly`.
use super::{
    format::{grouped, plain, readable},
    phrase::{tokenize, Token, Token::*},
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy amount · estimates exclude fees and taxes";
const MAX_YEARS: f64 = 100.;

pub fn calculate_finance(input: &str) -> Outcome {
    let tokens = tokenize(input)?;
    let (kind, rest) = match tokens.as_slice() {
        [first, rest @ ..] if first.is("loan") || first.is("mortgage") || first.is("repayment") => {
            (Kind::Loan, rest)
        }
        [first, rest @ ..] if first.is("compound") || first.is("savings") || first.is("invest") => {
            (Kind::Compound, rest)
        }
        // `1000 at 5% for 10 years` without a keyword means compound growth.
        [Number(_), at, ..] if at.is("at") => (Kind::Compound, tokens.as_slice()),
        _ => return None,
    };
    let terms = parse_terms(rest)?;
    Some(match kind {
        Kind::Loan => loan(terms),
        Kind::Compound => compound(terms),
    })
}

enum Kind {
    Loan,
    Compound,
}

struct Terms {
    principal: f64,
    annual_rate: f64,
    years: f64,
    periods_per_year: f64,
}

/// `<principal> at <rate>% (for|over) <n> (years|months) [compounded] [monthly|…]`.
fn parse_terms(tokens: &[Token]) -> Option<Terms> {
    let [Number(principal), at, Percent(rate), over, Number(length), unit, frequency @ ..] = tokens
    else {
        return None;
    };
    if !at.is("at") || !(over.is("for") || over.is("over")) {
        return None;
    }
    let years = match unit {
        unit if ["year", "years", "yr", "yrs", "y"]
            .iter()
            .any(|word| unit.is(word)) =>
        {
            *length
        }
        unit if ["month", "months", "mo", "mos"]
            .iter()
            .any(|word| unit.is(word)) =>
        {
            length / 12.
        }
        _ => return None,
    };
    let periods_per_year = match frequency {
        [] => None,
        [word] | [_, word] => Some(match () {
            _ if word.is("annually") || word.is("yearly") => 1.,
            _ if word.is("quarterly") => 4.,
            _ if word.is("monthly") => 12.,
            _ if word.is("weekly") => 52.,
            _ if word.is("daily") => 365.,
            _ => return None,
        }),
        _ => return None,
    };
    Some(Terms {
        principal: *principal,
        annual_rate: *rate / 100.,
        years,
        periods_per_year: periods_per_year.unwrap_or(0.),
    })
}

fn validate(terms: &Terms) -> Result<(), &'static str> {
    if terms.principal <= 0. || terms.annual_rate < 0. || terms.annual_rate > 1. {
        return Err("Use a positive amount and an interest rate from 0% to 100%");
    }
    if terms.years <= 0. || terms.years > MAX_YEARS {
        return Err("Use a term of up to 100 years");
    }
    Ok(())
}

/// Monthly repayment of an amortising loan.
fn loan(terms: Terms) -> Result<Conversion, &'static str> {
    validate(&terms)?;
    let months = (terms.years * 12.).round();
    if months < 1. {
        return Err("Use a term of at least one month");
    }
    let monthly_rate = terms.annual_rate / 12.;
    let payment = if monthly_rate == 0. {
        terms.principal / months
    } else {
        terms.principal * monthly_rate / (1. - (1. + monthly_rate).powf(-months))
    };
    let total = payment * months;
    let summary = format!(
        "{} at {}% over {} months",
        grouped(terms.principal, 2),
        readable(terms.annual_rate * 100.),
        months
    );
    Ok(Conversion {
        answers: vec![
            money(payment, format!("Monthly payment · {summary}")),
            money(total - terms.principal, "Total interest".into()),
            money(total, "Total repaid".into()),
        ],
        message: MESSAGE,
    })
}

/// Future value with compounding (annual unless stated).
fn compound(terms: Terms) -> Result<Conversion, &'static str> {
    validate(&terms)?;
    let periods = if terms.periods_per_year == 0. {
        1.
    } else {
        terms.periods_per_year
    };
    let value = terms.principal * (1. + terms.annual_rate / periods).powf(periods * terms.years);
    if !value.is_finite() {
        return Err("Result is outside the supported numeric range");
    }
    let frequency = match periods as u32 {
        1 => "annually",
        4 => "quarterly",
        12 => "monthly",
        52 => "weekly",
        _ => "daily",
    };
    Ok(Conversion {
        answers: vec![
            money(
                value,
                format!(
                    "{} at {}% for {} years, compounded {frequency}",
                    grouped(terms.principal, 2),
                    readable(terms.annual_rate * 100.),
                    readable(terms.years)
                ),
            ),
            money(value - terms.principal, "Interest earned".into()),
        ],
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

    fn titles(query: &str) -> Vec<String> {
        calculate_finance(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn loan_repayments_match_the_amortisation_formula() {
        assert_eq!(
            titles("mortgage 250k at 4.5% for 25 years"),
            ["1,389.58", "166,874.36", "416,874.36"]
        );
        assert_eq!(titles("loan 12,000 at 0% over 24 months")[0], "500.00");
        assert_eq!(titles("loan 10000 at 7% for 36 months")[0], "308.77");
    }

    #[test]
    fn compound_growth_with_stated_frequency() {
        assert_eq!(
            titles("compound 1000 at 5% for 10 years"),
            ["1,628.89", "628.89"]
        );
        assert_eq!(
            titles("1000 at 5% for 10 years compounded monthly")[0],
            "1,647.01"
        );
        assert_eq!(
            titles("savings 5k at 3% for 18 months quarterly")[0],
            "5,229.26"
        );
    }

    #[test]
    fn invalid_terms_and_other_text() {
        assert!(calculate_finance("loan 1000 at 150% for 2 years")
            .unwrap()
            .is_err());
        assert!(calculate_finance("loan 1000 at 5% for 0 years")
            .unwrap()
            .is_err());
        for query in [
            "loan",
            "10 GB at 100 Mbps",
            "compound interest",
            "1000 at 5%",
            "xbox",
        ] {
            assert!(calculate_finance(query).is_none(), "{query}");
        }
    }
}
