mod measurement;
mod normalization;
mod ranking;
mod routing;

#[path = "fixtures/legacy_router.rs"]
#[rustfmt::skip]
// Preserve the original source construct in the measured fixture.
#[allow(clippy::single_match)]
mod legacy_router;
#[allow(dead_code)]
#[path = "fixtures/legacy_search_text.rs"]
#[rustfmt::skip]
mod legacy_search_text;

use measurement::measure;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: measurement::CountingAllocator = measurement::CountingAllocator;

fn benchmark_routing() {
    let corpus = routing::QUERY_CORPUS;
    let mut query_index = 0;
    measure("route/legacy_owned", 10_000, || {
        black_box(legacy_router::describe(black_box(
            corpus[query_index % corpus.len()],
        )));
        query_index += 1;
    });
    measure("route/borrowed_prefix", 10_000, || {
        black_box(routing::parse_command(black_box(
            corpus[query_index % corpus.len()],
        )));
        query_index += 1;
    });
    let aliases = [
        "app",
        "calc",
        "web",
        "quicklinks",
        "unknown",
        "applications",
        "math",
        "url",
    ];
    let table = routing::command_map();
    for (label, lookup) in [
        (
            "lookup/match",
            routing::lookup_match as fn(&str) -> Option<routing::CommandKind>,
        ),
        ("lookup/linear", routing::lookup_linear),
    ] {
        measure(label, 100_000, || {
            black_box(lookup(black_box(aliases[query_index % aliases.len()])));
            query_index += 1;
        });
    }
    measure("lookup/hashmap", 100_000, || {
        black_box(
            table
                .get(black_box(aliases[query_index % aliases.len()]))
                .copied(),
        );
        query_index += 1;
    });
}

fn benchmark_normalization() {
    let mut iteration = 0;
    let churn: Vec<_> = (0..512)
        .map(|index| format!("application name {index}"))
        .collect();
    for (label, normalizer) in [
        (
            "normalize/cache_hit",
            legacy_search_text::normalize_search_text as fn(&str) -> String,
        ),
        ("normalize/direct", normalization::uncached),
    ] {
        measure(label, 10_000, || {
            black_box(normalizer(black_box("visual studio code")));
        });
    }
    measure("normalize/borrow_ascii", 10_000, || {
        black_box(normalization::borrow_when_normalized(black_box(
            "visual studio code",
        )));
    });
    measure("normalize/cache_churn_512", 10_000, || {
        black_box(legacy_search_text::normalize_search_text(black_box(
            &churn[iteration % churn.len()],
        )));
        iteration += 1;
    });
    measure("normalize/direct_churn_512", 10_000, || {
        black_box(normalization::uncached(black_box(
            &churn[iteration % churn.len()],
        )));
        iteration += 1;
    });
}

fn benchmark_additional_alternatives() {
    let mut sorted_aliases = routing::COMMAND_ALIASES.to_vec();
    sorted_aliases.sort_by_key(|entry| entry.0);
    for (corpus_name, corpus) in [
        ("mixed", routing::QUERY_CORPUS.to_vec()),
        (
            "uppercase_misses",
            vec![
                "@CALCULATOR 2",
                "@APPLICATIONS editor",
                "@QUICKLINKS docs",
                "@unknown x",
                "@CALC 4",
                "@WHAT x",
            ],
        ),
    ] {
        for (label, parser) in [
            (
                "stack_lower_match",
                routing::parse_command as fn(&str) -> Option<routing::ParsedCommand<'_>>,
            ),
            ("canonical_first", routing::parse_canonical_first),
            ("ascii_comparisons", routing::parse_ascii_comparisons),
        ] {
            let mut position = 0;
            measure(&format!("route_more/{corpus_name}/{label}"), 50_000, || {
                black_box(parser(black_box(corpus[position % corpus.len()])));
                position += 1;
            });
        }
        let mut position = 0;
        measure(
            &format!("route_more/{corpus_name}/binary_search"),
            50_000,
            || {
                black_box(routing::parse_binary_search(
                    black_box(corpus[position % corpus.len()]),
                    &sorted_aliases,
                ));
                position += 1;
            },
        );
    }
    for (label, query) in [
        ("canonical", "visual studio code"),
        ("mixed_ascii", "  Visual\t STUDIO   code  "),
        ("unicode", "  ΟΣ  MÜNCHEN  "),
    ] {
        measure(&format!("normalize_more/{label}/owned"), 20_000, || {
            black_box(normalization::uncached(black_box(query)));
        });
        measure(
            &format!("normalize_more/{label}/borrow_or_owned"),
            20_000,
            || {
                black_box(normalization::borrow_when_normalized(black_box(query)));
            },
        );
        let mut buffer = String::with_capacity(128);
        measure(
            &format!("normalize_more/{label}/reused_buffer"),
            20_000,
            || {
                black_box(normalization::normalize_reused(
                    black_box(query),
                    &mut buffer,
                ));
            },
        );
    }
}

fn benchmark_regex() {
    // Exact pattern from calculator.rs:1933. Measures one predicate, not the full calculator.
    const PATTERN: &str =
        r"(?i)^\s*(today|tomorrow|yesterday)\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)\s*$";
    let compiled = regex::Regex::new(PATTERN).expect("legacy regex is valid");
    measure("regex/compile_and_match", 10, || {
        let expression = regex::Regex::new(black_box(PATTERN)).expect("legacy regex is valid");
        black_box(expression.is_match(black_box("tomorrow 9am")));
    });
    measure("regex/reused_match", 10_000, || {
        black_box(compiled.is_match(black_box("tomorrow 9am")));
    });
}

fn benchmark_ranking() {
    type Selector = fn(&[ranking::Record]) -> Vec<usize>;
    let selectors: [(&str, Selector); 5] = [
        ("legacy_clone_partial", ranking::legacy),
        ("borrowed_partial", ranking::borrowed_partial),
        ("full_sort", ranking::full_sort),
        ("bounded_heap", ranking::bounded_heap),
        ("fixed_buffer", ranking::fixed_buffer),
    ];
    for count in [8, 2_000, 50_000, 150_000] {
        let records = ranking::make_records(count);
        let iterations = match count {
            0..=12 => 10_000,
            13..=2_000 => 30,
            _ => 2,
        };
        for (label, select) in selectors {
            measure(&format!("top12/{count}/{label}"), iterations, || {
                black_box(select(black_box(&records)));
            });
        }
    }
    for (order_name, reverse) in [("best_first", false), ("worst_first", true)] {
        let mut records = ranking::make_records(2_000);
        records.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.name.cmp(&right.name))
        });
        if reverse {
            records.reverse();
        }
        for (label, select) in selectors {
            measure(&format!("top12/2000_{order_name}/{label}"), 30, || {
                black_box(select(black_box(&records)));
            });
        }
    }
}

fn main() {
    println!("case,iterations_per_batch,batches,median_batch_ns_per_op,p95_batch_ns_per_op,allocation_calls_per_op,cumulative_requested_bytes_per_op");
    benchmark_routing();
    benchmark_normalization();
    benchmark_regex();
    benchmark_ranking();
    benchmark_additional_alternatives();
}
