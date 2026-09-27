//! Body mass index: `bmi 70kg 175cm`, `bmi 154 lb 5ft 9in`, `bmi 11st 5'9"`.
use super::{
    format::readable,
    quantity::{parse_number, split_number_prefix},
    units::{find_unit, Dimension},
    Conversion, Outcome,
};
use crate::calculator::Calculation;

const MESSAGE: &str = "Enter to copy · WHO adult categories; not medical advice";

pub fn calculate_bmi(input: &str) -> Outcome {
    let lower = input.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("bmi")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    // `5'9"` becomes `5ft 9in` so feet and inches parse like any other measure.
    let normalized = rest.replace(['′', '\''], "ft ").replace(['″', '"'], "in ");
    let mut kilograms = 0.;
    let mut metres = 0.;
    let mut unitless = Vec::new();
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    let mut index = 0;
    while index < tokens.len() {
        let (number, attached) = split_number_prefix(tokens[index])?;
        let value = parse_number(number)?;
        let unit_text = if attached.is_empty() {
            match tokens.get(index + 1).and_then(|next| find_unit(next)) {
                Some(_) => {
                    index += 1;
                    tokens[index]
                }
                None => "",
            }
        } else {
            attached
        };
        index += 1;
        if unit_text.is_empty() {
            unitless.push(value);
            continue;
        }
        let unit = find_unit(unit_text)?;
        match unit.dimension {
            Dimension::Mass => kilograms += unit.base_value(value),
            Dimension::Length => metres += unit.base_value(value),
            _ => return None,
        }
    }
    // Plain numbers: weight in kilograms, then height in centimetres (or metres when small).
    for value in unitless {
        if kilograms == 0. {
            kilograms = value;
        } else if metres == 0. {
            metres = if value > 3. { value / 100. } else { value };
        } else {
            return None;
        }
    }
    Some(bmi(kilograms, metres))
}

fn bmi(kilograms: f64, metres: f64) -> Result<Conversion, &'static str> {
    if !(1. ..=700.).contains(&kilograms) || !(0.3..=3.).contains(&metres) {
        return Err("Enter weight and height, such as bmi 70kg 175cm or bmi 154lb 5ft 9in");
    }
    let index = kilograms / (metres * metres);
    let category = match index {
        value if value < 18.5 => "Underweight",
        value if value < 25. => "Healthy weight",
        value if value < 30. => "Overweight",
        _ => "Obesity",
    };
    let rounded = format!("{index:.1}");
    Ok(Conversion::single(
        Calculation {
            title: rounded.clone(),
            detail: format!(
                "{category} · {} kg, {} m",
                readable(kilograms),
                readable(metres)
            ),
            copy: rounded,
        },
        MESSAGE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(query: &str) -> Calculation {
        calculate_bmi(query)
            .unwrap_or_else(|| panic!("{query} not recognized"))
            .unwrap_or_else(|error| panic!("{query}: {error}"))
            .answers
            .remove(0)
    }

    #[test]
    fn metric_imperial_and_unitless_inputs() {
        assert_eq!(answer("bmi 70kg 175cm").title, "22.9");
        assert_eq!(answer("bmi 70 kg 1.75 m").title, "22.9");
        assert_eq!(answer("bmi 154 lb 5ft 9in").title, "22.7");
        assert_eq!(answer("BMI 11st 5'9\"").title, "22.7");
        assert_eq!(answer("bmi 70 175").title, "22.9");
        assert!(answer("bmi 95kg 175cm").detail.starts_with("Obesity"));
    }

    #[test]
    fn missing_values_and_other_text() {
        assert!(calculate_bmi("bmi 70kg").unwrap().is_err());
        for query in ["bmi", "bmicalculator", "bmi 70 kg 5 GB", "xbox"] {
            assert!(calculate_bmi(query).is_none(), "{query}");
        }
    }
}
