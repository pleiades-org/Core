pub mod calendar;
mod evaluate;
mod math_function;
pub use evaluate::{CalcError, CalculatorEngine};

#[derive(Debug)]
pub struct Calculation {
    pub title: String,
    pub detail: String,
    pub copy: String,
}

/// Keep the shortest round-trippable f64 value, using scientific notation for very large/small results.
pub fn format_number(number: f64) -> String {
    if number == 0. {
        return "0".into();
    }
    if number.abs() >= 1e15 || number.abs() < 1e-6 {
        format!("{number:e}")
    } else {
        number.to_string()
    }
}
