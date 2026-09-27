# Calculator evaluation and BODMAS

Core already evaluates ordinary infix maths with a Pratt parser. Keep that implementation for live search: the tested postfix and direct-stack alternatives used more time per edited expression. This change adds comparison implementations and regression coverage, without changing production calculator behavior or requiring a new launcher binary.

## Existing precedence

| Expression | Result | Rule |
| --- | ---: | --- |
| `2+3*4` | 14 | Multiplication before addition |
| `(2+3)*4` | 20 | Brackets first |
| `8/2*2` | 8 | Division and multiplication have equal priority; evaluate left to right |
| `18-6+2` | 14 | Subtraction and addition have equal priority; evaluate left to right |
| `2^3^2` | 512 | Powers associate right to left: `2^(3^2)` |
| `-2^2` | -4 | Power before unary minus |
| `(-2)^2` | 4 | Brackets apply the sign first |
| `2^-2` | 0.25 | Negative exponents work |
| `100*(1+20%)` | 120 | Postfix percent divides its operand by 100 |

Functions evaluate their bracketed arguments. Percent binds more tightly than powers; `2^3%` means `2^(3/100)`, while `(2^3)%` means `8/100`. The word `of` has multiplication precedence. Implicit multiplication such as `2(3+4)` remains unsupported; use `2*(3+4)`.

Postfix/RPN can encode the same ordering: infix `2+3*4` becomes `2 3 4 * +`. It is one internal representation; BODMAS does not require conversion to postfix. This investigation treats postfix as an internal evaluator, rather than adding a typed RPN command.

## Measured comparison, 21 September 2026

Values below are nanoseconds per operation, taking the median of the three per-process median batch averages. Lower is better. Each process measures 31 batches of 30,000 evaluations per case; the middle process reverses candidate order. Inputs and results cross `black_box`, scratch storage is reused, and method selection occurs outside the timed loop. Allocation counting runs separately. These are local calculator microbenchmarks, excluding recognition, formatting, the search worker, painting and keyboard latency.

| Input group | Current Pratt | Direct shunting-yard evaluation | Shunting-yard → postfix → evaluation |
| --- | ---: | ---: | ---: |
| Short arithmetic | **48.24 ns** | 68.14 ns | 73.10 ns |
| Functions and mixed expressions | **129.47 ns** | 190.55 ns | 176.45 ns |
| Complete and incomplete typing prefixes | **26.12 ns** | 35.77 ns | 35.48 ns |
| Long sums and nested brackets | **738.51 ns** | 1,010.60 ns | 1,138.84 ns |

In this implementation, postfix conversion plus evaluation took about 52% longer for short arithmetic. Pratt won each edited-input group in all three processes. Absolute timings varied with desktop activity: short-expression Pratt medians ranged from 44.15 to 57.38 ns, and postfix medians from 70.91 to 82.62 ns. This supports the local implementation choice, not a claim that one parsing algorithm wins on every machine or implementation.

All candidates recorded **zero heap allocation calls and zero requested heap bytes per evaluation**. Persistent scratch storage is 2,584 bytes for the direct-stack candidate and 6,688 bytes for the postfix candidate, excluding input and call frames. The current calculator provider is stateless; its parser uses a borrowed input cursor and bounded recursive calls. Its total peak stack use was not measured, so zero heap allocation must not be read as zero memory use.

Executing an already compiled postfix program took **18.03 ns**, versus **73.67 ns** to parse and evaluate the same expression with Pratt. This replay comparison excludes compilation and uses the single expression `sqrt(81)+round(2.6)`. Core's input changes as the user types, so the relevant figures include compilation. For a future feature that repeatedly evaluates one formula with changing variable values, a compiled program would be worth measuring; these prototypes do not implement variables. An unchanged constant expression could instead reuse its final result.

## Correctness and scope

The two alternatives share a bounded tokenizer and shunting-yard operator handling, using typed operator/instruction enums and reusable fixed-capacity stacks. Both reuse Core's actual function and constant implementations. They live only in `benchmarks/calculator` and are absent from the launcher build.

Four candidate tests pass, covering the fixed expression/typing corpus, 2,160 generated expressions, precedence, powers, unary signs, functions, percentages, error recovery and bounded workspaces. Two new production tests explicitly cover BODMAS associativity and exact token/depth limits; all 25 engine tests pass. Strict Clippy and formatting checks pass.

The prototypes are not drop-in replacements. Postfix compilation validates the full syntax before execution: `1/0 +` reports incomplete input instead of Core's earlier division-by-zero error. Their nesting/token accounting also differs at some complexity boundaries: Core limits recursive parser depth, while the iterative candidates limit syntactic bracket depth and total tokens. These differences are documented and tested where applicable; they must be reconciled before considering a production switch.

## Reproduction

Run `benchmarks/calculator/run-benchmarks.ps1` from PowerShell. It uses the existing Rust toolchain and local engine dependency, with no new third-party packages. Build settings: Rust 1.95.0, Windows x64 MSVC, optimization level 3, thin LTO, one codegen unit. The generated environment record includes processor information, the benchmark executable hash and source hashes.

Raw evidence: [run 1](measurements/calculator-alternatives-1.csv), [run 2, reversed order](measurements/calculator-alternatives-2.csv), [run 3](measurements/calculator-alternatives-3.csv), [environment and source hashes](measurements/calculator-alternatives-environment.json), [scratch storage](measurements/calculator-alternatives-1-scratch.txt).
