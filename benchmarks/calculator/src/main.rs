mod instruction;
mod lexer;
mod shunting_yard;
mod stack;

// Share Core's exact function implementations and the existing allocation/timing harness.
#[path = "../../../crates/engine/src/calculator/math_function.rs"]
mod math_function;
#[path = "../../constructs/src/measurement.rs"]
mod measurement;

use core_engine::calculator::{CalcError, CalculatorEngine};
use instruction::{Program, Values};
use shunting_yard::ShuntingYard;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: measurement::CountingAllocator = measurement::CountingAllocator;

const SHORT: &[&str] = &[
    "14+4",
    "2+3*4",
    "8/2*2",
    "18-6+2",
    "(2+3)*4",
    "-2^2",
    "2^-2",
    "25% of 80",
    "1e3/2",
    "0.1+0.2",
    ".5*8",
];
const FUNCTIONS: &[&str] = &[
    "sqrt(81)+round(2.6)",
    "sin(pi/2)^2+cos(pi/2)^2",
    "2^(3+2)-4/2",
    "-(-3+2)^2",
    "50%*200+10",
    "abs(-7)+floor(8.9)-ceil(1.1)",
    "(2+3)*(4+5)-(6/2)",
];
const TYPING: &[&str] = &[
    "", "1", "14", "14 +", "14 + ", "14 + 4", "sqrt", "sqrt(", "sqrt(8", "sqrt(81", "sqrt(81)",
    "2 ^", "2 ^ -", "2 ^ -2", "(2+3", "1e", "1e-", "1e-3",
];

fn benchmark_group(name: &str, expressions: &[&str]) {
    let mut parser = ShuntingYard::new();
    let mut program = Program::new();
    let mut values = Values::new();
    let mut methods = ["pratt", "direct_stack", "postfix_compile_and_run"];
    if std::env::args().any(|argument| argument == "--reverse") {
        methods.reverse();
    }
    // Dispatch outside the timed loop; each candidate gets the same input/black_box overhead.
    for method in methods {
        match method {
            "pratt" => measure_inputs(name, method, expressions, |input| {
                CalculatorEngine.evaluate(input)
            }),
            "direct_stack" => measure_inputs(name, method, expressions, |input| {
                parser
                    .parse(input, &mut values)
                    .and_then(|()| values.result())
            }),
            _ => measure_inputs(name, method, expressions, |input| {
                parser
                    .parse(input, &mut program)
                    .and_then(|()| program.run(&mut values))
            }),
        }
    }
}

fn measure_inputs(
    group: &str,
    method: &str,
    expressions: &[&str],
    mut evaluate: impl FnMut(&str) -> Result<f64, CalcError>,
) {
    let mut index = 0;
    measurement::measure(&format!("{group}/{method}"), 30_000, || {
        let input = black_box(expressions[index % expressions.len()]);
        index += 1;
        let _ = black_box(evaluate(input));
    });
}

fn benchmark_repeated_program() {
    let expression = "sqrt(81)+round(2.6)";
    let mut parser = ShuntingYard::new();
    let mut program = Program::new();
    let mut values = Values::new();
    parser.parse(expression, &mut program).unwrap();
    measurement::measure("unchanged_program/postfix_run_only", 30_000, || {
        let _ = black_box(black_box(&program).run(&mut values));
    });
    measurement::measure("unchanged_program/pratt_parse_and_run", 30_000, || {
        let _ = black_box(CalculatorEngine.evaluate(black_box(expression)));
    });
}

fn main() {
    println!("case,iterations_per_batch,batches,median_batch_ns_per_op,p95_batch_ns_per_op,allocation_calls_per_op,cumulative_requested_bytes_per_op");
    benchmark_group("short", SHORT);
    benchmark_group("functions", FUNCTIONS);
    benchmark_group("typing", TYPING);
    let long = (1..=64)
        .map(|number| number.to_string())
        .collect::<Vec<_>>()
        .join("+");
    let nested = format!("{}2{}", "(".repeat(20), ")".repeat(20));
    benchmark_group("long_and_nested", &[&long, &nested]);
    benchmark_repeated_program();
    eprintln!("Reusable scratch bytes: direct_stack={}, postfix_compile_and_run={} (excludes input, call frames and benchmark harness)",
        std::mem::size_of::<ShuntingYard>() + std::mem::size_of::<Values>(),
        std::mem::size_of::<ShuntingYard>() + std::mem::size_of::<Program>() + std::mem::size_of::<Values>());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(expression: &str) {
        let expected = CalculatorEngine.evaluate(expression);
        let mut parser = ShuntingYard::new();
        let mut values = Values::new();
        let direct = parser
            .parse(expression, &mut values)
            .and_then(|()| values.result());
        assert_eq!(direct, expected, "direct stack: {expression}");
        let mut program = Program::new();
        let postfix = parser
            .parse(expression, &mut program)
            .and_then(|()| program.run(&mut values));
        assert_eq!(postfix, expected, "postfix: {expression}");
    }

    #[test]
    fn candidates_preserve_bodmas_unary_power_functions_and_percent_on_corpus() {
        for expression in SHORT.iter().chain(FUNCTIONS).chain(TYPING) {
            compare(expression);
        }
        for expression in [
            "2^3^2", "(-2)^2", "2^-2^2", "-2^-2", "--2", "2%%", "2^3%", "(2^3)%", "2+3)", "()",
            "1/0", "1e309", "sqrt(-1)", "ln(0)", "1.2.3", "2*(3+)",
        ] {
            compare(expression);
        }
    }

    #[test]
    fn generated_small_expressions_agree_with_production() {
        for left in 1..=12 {
            for right in 1..=12 {
                for operator in ["+", "-", "*", "/", "^"] {
                    for expression in [
                        format!("{left}{operator}{right}*2+3"),
                        format!("-({left}{operator}{right})^2"),
                        format!("2^{left}%{operator}({right}+1)"),
                    ] {
                        compare(&expression);
                    }
                }
            }
        }
    }

    #[test]
    fn candidate_workspaces_reset_after_errors_and_bound_memory() {
        let mut parser = ShuntingYard::new();
        let mut program = Program::new();
        let mut values = Values::new();
        for invalid in [
            "2+",
            "(1",
            "1/0",
            &"1+".repeat(300),
            &"(".repeat(40),
            &"0".repeat(4097),
        ] {
            assert!(parser
                .parse(invalid, &mut values)
                .and_then(|()| values.result())
                .is_err());
            parser.parse("2+3*4", &mut values).unwrap();
            assert_eq!(values.result(), Ok(14.));
            assert!(parser
                .parse(invalid, &mut program)
                .and_then(|()| program.run(&mut values))
                .is_err());
            parser.parse("2+3*4", &mut program).unwrap();
            assert_eq!(program.run(&mut values), Ok(14.));
        }
    }

    #[test]
    fn postfix_prototype_reports_syntax_before_earlier_arithmetic_errors() {
        // This intentional difference must be resolved before replacing live calculator behavior.
        let mut parser = ShuntingYard::new();
        let mut program = Program::new();
        assert_eq!(
            CalculatorEngine.evaluate("1/0 +"),
            Err(CalcError::DivisionByZero)
        );
        assert_eq!(
            parser.parse("1/0 +", &mut program),
            Err(CalcError::Incomplete)
        );
    }
}
