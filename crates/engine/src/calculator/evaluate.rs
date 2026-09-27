use super::math_function::{constant, MathFunction};
use crate::search::MAX_QUERY_BYTES;

const MAX_TOKENS: usize = 256;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcError {
    Incomplete,
    Invalid,
    DivisionByZero,
    NonFinite,
    TooComplex,
    Domain,
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Incomplete => "Finish the expression",
            Self::Invalid => "Use numbers, parentheses, + − * / ^ and %",
            Self::DivisionByZero => "Cannot divide by zero",
            Self::NonFinite => "Result is outside the supported numeric range",
            Self::TooComplex => "Expression is too long or deeply nested",
            Self::Domain => "That function is undefined for this input in real numbers",
        })
    }
}

/// A persistent arithmetic provider. Evaluation borrows input and allocates no token vector.
#[derive(Default)]
pub struct CalculatorEngine;

impl CalculatorEngine {
    pub fn recognizes(&self, input: &str) -> bool {
        let input = input.trim();
        if input.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return false;
        }
        let name_end = input
            .bytes()
            .position(|byte| !byte.is_ascii_alphanumeric())
            .unwrap_or(input.len());
        let named_start = constant(input.as_bytes()).is_some()
            || (MathFunction::parse(&input.as_bytes()[..name_end]).is_some()
                && input[name_end..].trim_start().starts_with('('))
            || constant(&input.as_bytes()[..name_end]).is_some();
        if !input
            .trim_start()
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_digit() || b".+-(".contains(&byte) || named_start)
        {
            return false;
        }
        let mut cursor = 0;
        let bytes = input.as_bytes();
        while cursor < bytes.len() {
            if bytes[cursor].is_ascii_alphabetic() {
                let start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                    cursor += 1;
                }
                if matches!(&bytes[start..cursor], b"log") {
                    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                        cursor += 1;
                    }
                }
                let name = &bytes[start..cursor];
                if !name.eq_ignore_ascii_case(b"of")
                    && constant(name).is_none()
                    && MathFunction::parse(name).is_none()
                {
                    return false;
                }
            } else {
                if !bytes[cursor].is_ascii_digit()
                    && !bytes[cursor].is_ascii_whitespace()
                    && !b".+-*/^()%".contains(&bytes[cursor])
                {
                    return false;
                }
                cursor += 1;
            }
        }
        true
    }

    pub fn evaluate(&self, input: &str) -> Result<f64, CalcError> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(CalcError::TooComplex);
        }
        let mut parser = Parser {
            bytes: input.as_bytes(),
            cursor: 0,
            tokens: 0,
        };
        let result = parser.expression(0, 0)?;
        parser.skip_space();
        if parser.cursor != parser.bytes.len() {
            return Err(CalcError::Invalid);
        }
        finite(result)
    }
}

struct Parser<'input> {
    bytes: &'input [u8],
    cursor: usize,
    tokens: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn token(&mut self) -> Result<(), CalcError> {
        self.tokens += 1;
        if self.tokens > MAX_TOKENS {
            return Err(CalcError::TooComplex);
        }
        Ok(())
    }

    fn expression(&mut self, minimum_binding: u8, depth: usize) -> Result<f64, CalcError> {
        if depth >= MAX_DEPTH {
            return Err(CalcError::TooComplex);
        }
        let mut left = self.prefix(depth)?;
        loop {
            self.skip_space();
            let Some(&operator) = self.bytes.get(self.cursor) else {
                break;
            };
            if operator == b'%' {
                self.cursor += 1;
                self.token()?;
                left /= 100.;
                continue;
            }
            let (left_binding, right_binding, width) = match operator.to_ascii_lowercase() {
                b'+' | b'-' => (1, 2, 1),
                b'*' | b'/' => (3, 4, 1),
                b'^' => (6, 6, 1),
                b'o' if self
                    .bytes
                    .get(self.cursor..self.cursor + 2)
                    .is_some_and(|name| name.eq_ignore_ascii_case(b"of")) =>
                {
                    (3, 4, 2)
                }
                _ => break,
            };
            if left_binding < minimum_binding {
                break;
            }
            self.cursor += width;
            self.token()?;
            let right = self.expression(right_binding, depth + 1)?;
            left = apply(operator, left, right)?;
        }
        finite(left)
    }

    fn prefix(&mut self, depth: usize) -> Result<f64, CalcError> {
        self.skip_space();
        self.token()?;
        match self
            .bytes
            .get(self.cursor)
            .copied()
            .ok_or(CalcError::Incomplete)?
        {
            b'+' | b'-' => {
                let negative = self.bytes[self.cursor] == b'-';
                self.cursor += 1;
                let number = self.expression(5, depth + 1)?;
                Ok(if negative { -number } else { number })
            }
            b'(' => {
                self.cursor += 1;
                let number = self.expression(0, depth + 1)?;
                self.skip_space();
                if self.bytes.get(self.cursor) != Some(&b')') {
                    return Err(CalcError::Incomplete);
                }
                self.cursor += 1;
                self.token()?;
                Ok(number)
            }
            b'0'..=b'9' | b'.' => self.number(),
            b'a'..=b'z' | b'A'..=b'Z' => self.named(depth),
            _ => Err(CalcError::Invalid),
        }
    }

    fn named(&mut self, depth: usize) -> Result<f64, CalcError> {
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_alphanumeric)
        {
            self.cursor += 1;
        }
        let name = &self.bytes[start..self.cursor];
        if let Some(number) = constant(name) {
            return Ok(number);
        }
        let function = MathFunction::parse(name).ok_or(CalcError::Invalid)?;
        self.skip_space();
        if self.bytes.get(self.cursor) != Some(&b'(') {
            return Err(CalcError::Incomplete);
        }
        self.cursor += 1;
        let number = self.expression(0, depth + 1)?;
        self.skip_space();
        if self.bytes.get(self.cursor) != Some(&b')') {
            return Err(CalcError::Incomplete);
        }
        self.cursor += 1;
        self.token()?;
        finite(function.apply(number)?)
    }

    fn number(&mut self) -> Result<f64, CalcError> {
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
        }
        if self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            self.cursor += 1;
            if self
                .bytes
                .get(self.cursor)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.cursor += 1;
            }
            let exponent_start = self.cursor;
            while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
            if self.cursor == exponent_start {
                return Err(CalcError::Incomplete);
            }
        }
        let text =
            std::str::from_utf8(&self.bytes[start..self.cursor]).map_err(|_| CalcError::Invalid)?;
        finite(text.parse().map_err(|_| CalcError::Invalid)?)
    }
}

fn finite(number: f64) -> Result<f64, CalcError> {
    if number.is_finite() {
        Ok(number)
    } else {
        Err(CalcError::NonFinite)
    }
}

fn apply(operator: u8, left: f64, right: f64) -> Result<f64, CalcError> {
    finite(match operator.to_ascii_lowercase() {
        b'+' => left + right,
        b'-' => left - right,
        b'*' | b'o' => left * right,
        b'/' if right == 0. => return Err(CalcError::DivisionByZero),
        b'/' => left / right,
        b'^' => left.powf(right),
        _ => return Err(CalcError::Invalid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn functions_constants_and_domain_errors_use_the_same_bounded_parser() {
        let calculator = CalculatorEngine;
        for (query, expected) in [
            ("sqrt(81)", 9.),
            ("round(2.6)", 3.),
            ("abs(-7)", 7.),
            ("log10(1000)", 3.),
            ("floor(2.9)+ceil(1.1)", 4.),
            ("ln(e)", 1.),
            ("cos(0)", 1.),
            ("log2(8)", 3.),
        ] {
            assert!(calculator.recognizes(query), "{query}");
            assert!((calculator.evaluate(query).unwrap() - expected).abs() < 1e-12);
        }
        for query in ["pi*2", "e^2", "2*pi", "sqrt(81)"] {
            assert!(calculator.recognizes(query), "{query}");
        }
        for query in ["e", "E", "pi", "tau", " PI ", "TAU"] {
            assert!(!calculator.recognizes(query), "{query}");
        }
        assert_eq!(calculator.evaluate("sqrt(-1)"), Err(CalcError::Domain));
        assert_eq!(calculator.evaluate("log(0)"), Err(CalcError::Domain));
        assert_eq!(calculator.evaluate("sqrt("), Err(CalcError::Incomplete));
        assert_eq!(
            calculator.evaluate(&format!("{}1{}", "sqrt(".repeat(40), ")".repeat(40))),
            Err(CalcError::TooComplex)
        );
        assert!(!calculator.recognizes("Sin City"));
    }
    #[test]
    fn arithmetic_precedence_percentages_and_scientific_notation() {
        let calculator = CalculatorEngine;
        for (expression, expected) in [
            ("2+3*4", 14.),
            ("(2+3)*4", 20.),
            ("-2^2", -4.),
            ("2^3^2", 512.),
            ("25% of 80", 20.),
            ("25% OF 80", 20.),
            ("25% oF 80", 20.),
            ("1e3 / 2", 500.),
            ("2^-2", 0.25),
        ] {
            assert_eq!(
                calculator.evaluate(expression),
                Ok(expected),
                "{expression}"
            );
        }
    }
    #[test]
    fn incomplete_invalid_zero_overflow_and_complexity_are_distinct() {
        let calculator = CalculatorEngine;
        for expression in ["", "2 +", "(2+3", "1e-"] {
            assert_eq!(calculator.evaluate(expression), Err(CalcError::Incomplete));
        }
        assert_eq!(calculator.evaluate("1/0"), Err(CalcError::DivisionByZero));
        assert_eq!(calculator.evaluate("1e309"), Err(CalcError::NonFinite));
        assert_eq!(calculator.evaluate("1.2.3"), Err(CalcError::Invalid));
        assert_eq!(
            calculator.evaluate(&format!("{}1{}", "(".repeat(40), ")".repeat(40))),
            Err(CalcError::TooComplex)
        );
        assert!(!calculator.recognizes("1Password"));
        assert!(!calculator.recognizes("foo2"));
    }

    #[test]
    fn bodmas_equal_precedence_associativity_signs_and_postfix_percent_are_explicit() {
        for (expression, expected) in [
            ("8/2*2", 8.),
            ("24/3/2", 4.),
            ("18-6+2", 14.),
            ("18-6-2", 10.),
            ("18/(3*2)", 3.),
            ("(2^3)^2", 64.),
            ("(-2)^2", 4.),
            ("2^-2^2", 0.0625),
            ("-2^-2", -0.25),
            ("-(-3+2)^2", -1.),
            ("(2+3)*(4-1)^2", 45.),
            ("sqrt(16)+2^3*3-12/4", 25.),
            ("100*(1+20%)", 120.),
            ("100+20%", 100.2),
            ("50% of (20+20)", 20.),
            ("(2^3)%", 0.08),
            ("100%%", 0.01),
        ] {
            assert_eq!(
                CalculatorEngine.evaluate(expression),
                Ok(expected),
                "{expression}"
            );
        }
    }

    #[test]
    fn token_and_recursive_depth_limits_accept_the_boundary_then_reject_excess() {
        let accepted_sum = format!("{}1", "1+".repeat(127));
        let rejected_sum = format!("{}1", "1+".repeat(128));
        assert_eq!(CalculatorEngine.evaluate(&accepted_sum), Ok(128.));
        assert_eq!(
            CalculatorEngine.evaluate(&rejected_sum),
            Err(CalcError::TooComplex)
        );
        for (depth, expected) in [(31, Ok(1.)), (32, Err(CalcError::TooComplex))] {
            let parentheses = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
            let unary = format!("{}1", "+".repeat(depth));
            let powers = format!("{}1", "1^".repeat(depth));
            for expression in [parentheses, unary, powers] {
                assert_eq!(
                    CalculatorEngine.evaluate(&expression),
                    expected,
                    "{expression}"
                );
            }
        }
    }
}
