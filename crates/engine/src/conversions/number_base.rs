//! Number bases and Roman numerals: `255 to hex`, `0xff`, `0b1010 to dec`, `2024 to roman`,
//! `MMXXIV to number`.
use super::{quantity::ungroup, Conversion, Outcome};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy · Esc to hide";
const SEPARATORS: &[&str] = &["to", "in", "as", "into"];
const ROMAN: &[(u64, &str)] = &[
    (1000, "M"),
    (900, "CM"),
    (500, "D"),
    (400, "CD"),
    (100, "C"),
    (90, "XC"),
    (50, "L"),
    (40, "XL"),
    (10, "X"),
    (9, "IX"),
    (5, "V"),
    (4, "IV"),
    (1, "I"),
];
const MAX_ROMAN: u64 = 3_999;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Base {
    Binary,
    Octal,
    Decimal,
    Hexadecimal,
    Roman,
}

impl Base {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "hex" | "hexadecimal" | "base16" => Self::Hexadecimal,
            "bin" | "binary" | "base2" => Self::Binary,
            "oct" | "octal" | "base8" => Self::Octal,
            "dec" | "decimal" | "base10" | "number" | "arabic" | "int" | "integer" => Self::Decimal,
            "roman" | "roman numeral" | "roman numerals" | "numerals" => Self::Roman,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Octal => "octal",
            Self::Decimal => "decimal",
            Self::Hexadecimal => "hexadecimal",
            Self::Roman => "Roman numerals",
        }
    }
}

pub fn convert_number_base(input: &str) -> Outcome {
    let lower = input.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if let [value] = words.as_slice() {
        // A bare prefixed literal shows the other representations.
        let (number, base) = parse_prefixed(value)?;
        return Some(Ok(all_bases(number, base)));
    }
    if let ["roman", numeral] = words.as_slice() {
        return Some(from_roman(numeral).map(|number| single(number, Base::Decimal, Base::Roman)));
    }
    let separator = words.iter().position(|word| SEPARATORS.contains(word))?;
    let [value] = &words[..separator] else {
        return None;
    };
    let target = Base::parse(&words[separator + 1..].join(" "))?;
    if let Some((number, source)) = parse_prefixed(value).or_else(|| parse_decimal(value)) {
        return Some(convert(number, source, target));
    }
    // `MMXXIV to number`: Roman numerals are recognised only with an explicit target.
    let number = from_roman(value).ok()?;
    Some(Ok(single(number, target, Base::Roman)))
}

fn convert(number: u64, source: Base, target: Base) -> Result<Conversion, &'static str> {
    if target == Base::Roman && !(1..=MAX_ROMAN).contains(&number) {
        return Err("Roman numerals cover 1 to 3999");
    }
    Ok(single(number, target, source))
}

fn single(number: u64, target: Base, source: Base) -> Conversion {
    Conversion::single(answer(number, target, source), MESSAGE)
}

fn all_bases(number: u64, source: Base) -> Conversion {
    let answers = [Base::Decimal, Base::Hexadecimal, Base::Binary, Base::Octal]
        .into_iter()
        .filter(|base| *base != source)
        .map(|base| answer(number, base, source))
        .collect();
    Conversion {
        answers,
        message: MESSAGE,
    }
}

fn answer(number: u64, target: Base, source: Base) -> Calculation {
    let copy = match target {
        Base::Binary => format!("0b{number:b}"),
        Base::Octal => format!("0o{number:o}"),
        Base::Decimal => number.to_string(),
        Base::Hexadecimal => format!("0x{number:X}"),
        Base::Roman => to_roman(number),
    };
    let title = if target == Base::Decimal {
        grouped_integer(number)
    } else {
        copy.clone()
    };
    Calculation {
        title,
        detail: format!(
            "{} from {} · {}",
            target.name(),
            source.name(),
            grouped_integer(number)
        ),
        copy,
    }
}

fn grouped_integer(number: u64) -> String {
    let digits = number.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn parse_prefixed(text: &str) -> Option<(u64, Base)> {
    let (digits, radix, base) = if let Some(digits) = text.strip_prefix("0x") {
        (digits, 16, Base::Hexadecimal)
    } else if let Some(digits) = text.strip_prefix("0b") {
        (digits, 2, Base::Binary)
    } else if let Some(digits) = text.strip_prefix("0o") {
        (digits, 8, Base::Octal)
    } else {
        return None;
    };
    let digits = digits.replace('_', "");
    u64::from_str_radix(&digits, radix)
        .ok()
        .map(|number| (number, base))
}

fn parse_decimal(text: &str) -> Option<(u64, Base)> {
    let digits = if text.contains(',') {
        ungroup(text)?
    } else {
        text.to_owned()
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(|number| (number, Base::Decimal))
}

fn to_roman(mut number: u64) -> String {
    let mut output = String::new();
    for (value, symbol) in ROMAN {
        while number >= *value {
            output.push_str(symbol);
            number -= value;
        }
    }
    output
}

/// Only canonical numerals are accepted: `IIII` and `IC` are rejected by the round trip.
fn from_roman(text: &str) -> Result<u64, &'static str> {
    let upper = text.to_ascii_uppercase();
    let mut rest = upper.as_str();
    let mut number = 0;
    for (value, symbol) in ROMAN {
        while let Some(remaining) = rest.strip_prefix(symbol) {
            number += value;
            rest = remaining;
        }
    }
    if !rest.is_empty() || number == 0 || to_roman(number) != upper {
        return Err("That is not a valid Roman numeral");
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(query: &str) -> Vec<String> {
        convert_number_base(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .into_iter()
            .map(|answer| answer.title)
            .collect()
    }

    #[test]
    fn bases_convert_both_ways() {
        assert_eq!(titles("1,000 to hex"), ["0x3E8"]);
        for query in ["1,00 to hex", ",100 to hex", "1,000,00 to hex"] {
            assert!(convert_number_base(query).is_none(), "{query}");
        }
        let maximum = convert_number_base("0xFFFFFFFFFFFFFFFF").unwrap().unwrap();
        assert_eq!(maximum.answers[0].title, "18,446,744,073,709,551,615");
        assert_eq!(maximum.answers[0].copy, u64::MAX.to_string());
        let large = convert_number_base("9007199254740993 to hex")
            .unwrap()
            .unwrap();
        assert_eq!(
            large.answers[0].detail,
            "hexadecimal from decimal · 9,007,199,254,740,993"
        );
        assert_eq!(grouped_integer(0), "0");
        assert_eq!(titles("255 to hex"), ["0xFF"]);
        assert_eq!(titles("255 in binary"), ["0b11111111"]);
        assert_eq!(titles("0xff to dec"), ["255"]);
        assert_eq!(titles("0b1010 to decimal"), ["10"]);
        assert_eq!(titles("0o777 to hex"), ["0x1FF"]);
        assert_eq!(titles("1,000,000 to hex"), ["0xF4240"]);
        assert_eq!(titles("0xFF"), ["255", "0b11111111", "0o377"]);
        assert_eq!(titles("0b1111_0000"), ["240", "0xF0", "0o360"]);
    }

    #[test]
    fn roman_numerals_are_canonical_and_bounded() {
        assert_eq!(titles("2024 to roman"), ["MMXXIV"]);
        assert_eq!(titles("1994 to roman numerals"), ["MCMXCIV"]);
        assert_eq!(titles("MMXXIV to number"), ["2,024"]);
        assert_eq!(titles("roman xlii"), ["42"]);
        assert!(convert_number_base("4000 to roman").unwrap().is_err());
        assert!(convert_number_base("0 to roman").unwrap().is_err());
        assert!(convert_number_base("roman IIII").unwrap().is_err());
        assert!(convert_number_base("IC to number").is_none());
    }

    #[test]
    fn other_text_is_left_alone() {
        for query in ["255", "hex", "0xyz", "xbox", "10 kg to lb"] {
            assert!(convert_number_base(query).is_none(), "{query}");
        }
    }
}
