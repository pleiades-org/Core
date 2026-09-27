use super::CalcError;

#[derive(Clone, Copy)]
pub(super) enum MathFunction {
    Sqrt,
    Cbrt,
    Abs,
    Round,
    Floor,
    Ceil,
    Ln,
    Log10,
    Log2,
    Sin,
    Cos,
    Tan,
}

impl MathFunction {
    pub(super) fn parse(name: &[u8]) -> Option<Self> {
        use MathFunction::*;
        const FUNCTIONS: &[(&[u8], MathFunction)] = &[
            (b"sqrt", Sqrt),
            (b"cbrt", Cbrt),
            (b"abs", Abs),
            (b"round", Round),
            (b"floor", Floor),
            (b"ceil", Ceil),
            (b"ln", Ln),
            (b"log", Log10),
            (b"log10", Log10),
            (b"log2", Log2),
            (b"sin", Sin),
            (b"cos", Cos),
            (b"tan", Tan),
        ];
        FUNCTIONS
            .iter()
            .find(|(alias, _)| name.eq_ignore_ascii_case(alias))
            .map(|(_, function)| *function)
    }

    pub(super) fn apply(self, number: f64) -> Result<f64, CalcError> {
        use MathFunction::*;
        if matches!(self, Sqrt) && number < 0. || matches!(self, Ln | Log10 | Log2) && number <= 0.
        {
            return Err(CalcError::Domain);
        }
        Ok(match self {
            Sqrt => number.sqrt(),
            Cbrt => number.cbrt(),
            Abs => number.abs(),
            Round => number.round(),
            Floor => number.floor(),
            Ceil => number.ceil(),
            Ln => number.ln(),
            Log10 => number.log10(),
            Log2 => number.log2(),
            Sin => number.sin(),
            Cos => number.cos(),
            Tan => number.tan(),
        })
    }
}

pub(super) fn constant(name: &[u8]) -> Option<f64> {
    [
        (b"pi".as_slice(), std::f64::consts::PI),
        (b"e".as_slice(), std::f64::consts::E),
        (b"tau".as_slice(), std::f64::consts::TAU),
    ]
    .into_iter()
    .find(|(alias, _)| name.eq_ignore_ascii_case(alias))
    .map(|(_, number)| number)
}
