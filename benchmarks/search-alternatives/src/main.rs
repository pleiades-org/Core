mod corpus;
mod matching;
mod measurement;
mod postings;
mod quality;

#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "fixtures/arena.rs"]
mod arena;
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "fixtures/index.rs"]
mod index;
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "fixtures/filename.rs"]
mod upstream;
#[cfg(test)]
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "fixtures/deletion.rs"]
mod upstream_deletion;
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "../../constructs/src/fixtures/legacy_search_text.rs"]
mod legacy_search_text;

use matching::{Catalog, Method, Scratch, Selector};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: measurement::Allocator = measurement::Allocator;

fn benchmark_queries(catalog: &Catalog, run: usize) {
    let count = catalog.arena.files.len();
    let mut methods = Method::ALL.to_vec();
    let method_count = methods.len();
    methods.rotate_left(run % method_count);
    for method in methods {
        let mut scratch = Scratch::default();
        measurement::latency(
            &format!("{count}/{}/mixed_queries", method.label()),
            1_020,
            |sample| {
                let query = corpus::QUERIES[(sample * 7) % corpus::QUERIES.len()].1;
                matching::search(
                    black_box(catalog),
                    black_box(query),
                    method,
                    Selector::Partial,
                    &mut scratch,
                );
                black_box(&scratch.ranked);
            },
        );
        for &(category, query) in corpus::QUERIES {
            let label = format!("{count}/{}/{category}/{query}", method.label());
            measurement::latency(&label, 101, |_| {
                matching::search(
                    black_box(catalog),
                    black_box(query),
                    method,
                    Selector::Partial,
                    &mut scratch,
                );
                black_box(&scratch.ranked);
            });
        }
    }
    for &(category, query) in corpus::QUERIES {
        measurement::latency(
            &format!("{count}/upstream_complete/{category}/{query}"),
            51,
            |_| {
                black_box(upstream::middle_out_search(
                    black_box(&catalog.arena),
                    black_box(query),
                    matching::LIMIT,
                ));
            },
        );
    }
}

fn benchmark_selectors(catalog: &Catalog) {
    let count = catalog.arena.files.len();
    for (label, selector) in [
        ("partial", Selector::Partial),
        ("heap", Selector::Heap),
        ("fixed", Selector::Fixed),
    ] {
        let mut scratch = Scratch::default();
        for query in ["a", "document", "needle_7fd9"] {
            measurement::latency(&format!("{count}/selector_{label}/{query}"), 101, |_| {
                matching::search(
                    black_box(catalog),
                    black_box(query),
                    Method::Rarest,
                    selector,
                    &mut scratch,
                );
                black_box(&scratch.ranked);
            });
        }
    }
}

fn benchmark_core_and_typing(catalog: &Catalog) {
    let count = catalog.arena.files.len();
    for query in ["a", "report", "code visual", "zzqxyw_no_such_file"] {
        measurement::latency(&format!("{count}/core_legacy/{query}"), 101, |_| {
            black_box(matching::legacy_search(
                black_box(catalog),
                black_box(query),
            ));
        });
        for indexed in [false, true] {
            let mut scratch = Scratch::default();
            measurement::latency(
                &format!("{count}/core_borrowed_indexed_{indexed}/{query}"),
                101,
                |_| {
                    matching::borrowed_core_search(
                        black_box(catalog),
                        black_box(query),
                        indexed,
                        &mut scratch,
                    );
                    black_box(&scratch.ranked);
                },
            );
        }
    }
    let sequence = [
        "r", "re", "rep", "repo", "repor", "report", "repor", "re", "a", "au", "aud", "audio",
        "zzq",
    ];
    let mut incremental = matching::Incremental::default();
    measurement::latency(
        &format!("{count}/typing/incremental_all_matches"),
        101,
        |_| {
            for query in sequence {
                black_box(incremental.update(black_box(&catalog.arena), black_box(query)));
            }
        },
    );
    let mut matches = Vec::new();
    measurement::latency(
        &format!("{count}/typing/full_scan_all_matches"),
        101,
        |_| {
            for query in sequence {
                matches.clear();
                matches.extend(
                    catalog
                        .arena
                        .files
                        .iter()
                        .enumerate()
                        .filter(|(_, file)| file.name_lower.contains(black_box(query)))
                        .map(|(identifier, _)| identifier as u32),
                );
                black_box(&matches);
            }
        },
    );
}

fn benchmark_memory(catalog: &Catalog) {
    let count = catalog.arena.files.len();
    measurement::build(&format!("{count}/upstream_search_index"), || {
        index::SearchIndex::build_from_files(&catalog.arena.files, catalog.arena.global_midpoint)
    });
    measurement::build(&format!("{count}/compact_hash"), || {
        postings::compact_hash(&catalog.arena.files)
    });
    measurement::build(&format!("{count}/flat_postings"), || {
        postings::FlatPostings::from_sorted_pairs(&catalog.arena.files)
    });
    measurement::build(&format!("{count}/byte_masks"), || {
        catalog
            .arena
            .files
            .iter()
            .map(|file| postings::byte_mask(&file.name_lower))
            .collect::<Vec<_>>()
    });
    measurement::build(&format!("{count}/lexical_ordinals"), || {
        corpus::lexical_ranks(&catalog.arena)
    });
    for query in ["a", "report", "needle_7fd9", "zzqxyw_no_such_file"] {
        measurement::query_allocations(&format!("{count}/upstream_complete/{query}"), || {
            black_box(upstream::middle_out_search(
                &catalog.arena,
                query,
                matching::LIMIT,
            ));
        });
        for method in [
            Method::Scan,
            Method::OriginalCandidates,
            Method::Rarest,
            Method::Flat,
        ] {
            let mut warm_scratch = Scratch::default();
            matching::search(catalog, query, method, Selector::Partial, &mut warm_scratch);
            measurement::query_allocations(
                &format!("{count}/{}/warm_scratch/{query}", method.label()),
                || {
                    matching::search(catalog, query, method, Selector::Partial, &mut warm_scratch);
                    black_box(&warm_scratch.ranked);
                },
            );
            measurement::query_allocations(
                &format!("{count}/{}/cold_scratch/{query}", method.label()),
                || {
                    let mut scratch = Scratch::default();
                    matching::search(catalog, query, method, Selector::Partial, &mut scratch);
                    black_box(&scratch.ranked);
                },
            );
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("followup") {
        let run = std::env::args()
            .nth(2)
            .and_then(|argument| argument.parse().ok())
            .unwrap_or(0);
        followup(run);
        return;
    }
    let run = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(0);
    println!("kind,case,samples,median_ns,p95_ns,allocation_calls,requested_bytes,retained_bytes,peak_bytes");
    for count in [2_000, 50_000, 150_000] {
        eprintln!("Benchmarking {count} synthetic names, run {run}");
        let catalog = Catalog::new(corpus::arena(count));
        quality::verify(&catalog);
        benchmark_queries(&catalog, run);
        benchmark_selectors(&catalog);
        benchmark_core_and_typing(&catalog);
        benchmark_memory(&catalog);
    }
}

fn followup(run: usize) {
    println!("kind,case,samples,median_ns,p95_ns,allocation_calls,requested_bytes,retained_bytes,peak_bytes");
    for count in [32, 128, 512, 2_000, 150_000] {
        let catalog = Catalog::new(corpus::arena(count));
        crate::quality::verify(&catalog);
        let mut methods = [Method::Scan, Method::Rarest, Method::PrefixRarest];
        let method_count = methods.len();
        methods.rotate_left(run % method_count);
        for method in methods {
            let mut scratch = Scratch::default();
            measurement::latency(
                &format!("{count}/{}/mixed_queries", method.label()),
                1_020,
                |sample| {
                    let query = corpus::QUERIES[(sample * 7) % corpus::QUERIES.len()].1;
                    matching::search(
                        black_box(&catalog),
                        black_box(query),
                        method,
                        Selector::Partial,
                        &mut scratch,
                    );
                    black_box(&scratch.ranked);
                },
            );
            for query in ["a", "re", "document", "needle_7fd9"] {
                measurement::latency(&format!("{count}/{}/{query}", method.label()), 201, |_| {
                    matching::search(
                        black_box(&catalog),
                        black_box(query),
                        method,
                        Selector::Partial,
                        &mut scratch,
                    );
                    black_box(&scratch.ranked);
                });
            }
        }
        if count <= 512 {
            benchmark_memory(&catalog);
        }
        measurement::build(&format!("{count}/prefix_order_only"), || {
            catalog.lexical_order.clone()
        });
    }
}
