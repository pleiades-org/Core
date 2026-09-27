#[path = "../line_search/corpus.rs"]
mod corpus;
#[path = "../line_search/index.rs"]
mod index;
#[path = "../measurement.rs"]
#[allow(dead_code)]
mod measurement;
#[path = "../line_search/search.rs"]
mod search;

use corpus::Corpus;
use search::{Indexes, Method, Scratch};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: measurement::Allocator = measurement::Allocator;

const QUERIES: &[&str] = &[
    "a",
    "::",
    "return",
    "let ",
    "configure",
    "needle_tag_7fd9",
    "zzqxyw_no_such_content",
    "abcd",
    "café",
    "normalize_search_text",
];

fn verify(corpus: &Corpus, indexes: &Indexes) {
    let mut expected = Scratch::default();
    let mut actual = Scratch::default();
    for query in QUERIES {
        for limit in [0, 1, 12, usize::MAX] {
            search::search(
                corpus,
                indexes,
                query.as_bytes(),
                Method::LineScan,
                limit,
                &mut expected,
            );
            for method in Method::ALL {
                search::search(
                    corpus,
                    indexes,
                    query.as_bytes(),
                    method,
                    limit,
                    &mut actual,
                );
                assert_eq!(
                    actual.results, expected.results,
                    "{method:?}/{query}/{limit}"
                );
            }
        }
    }
}

fn benchmark(label: &str, corpus: &Corpus, run: usize) {
    eprintln!(
        "{label}: {} files / {} lines / {} bytes",
        corpus.files.len(),
        corpus.lines.len(),
        corpus.bytes.len()
    );
    let indexes = Indexes::build(corpus);
    verify(corpus, &indexes);
    let mut methods = Method::ALL;
    let method_count = methods.len();
    methods.rotate_left(run % method_count);
    for method in methods {
        let mut scratch = Scratch::default();
        for (limit_name, limit) in [("first12", 12), ("all", usize::MAX)] {
            for query in QUERIES {
                measurement::latency(
                    &format!("{label}/{}/{limit_name}/{query}", method.label()),
                    101,
                    |_| {
                        search::search(
                            black_box(corpus),
                            &indexes,
                            black_box(query.as_bytes()),
                            method,
                            limit,
                            &mut scratch,
                        );
                        black_box(&scratch.results);
                    },
                );
            }
        }
    }
    for (name, granularity) in [
        ("file", index::Granularity::File),
        ("block64", index::Granularity::Block),
        ("line", index::Granularity::Line),
    ] {
        measurement::build(&format!("{label}/{name}"), || {
            index::UnitIndex::build(corpus, granularity)
        });
    }
    measurement::build(&format!("{label}/positional"), || {
        index::build_positions(corpus)
    });
}

fn main() -> std::io::Result<()> {
    let arguments: Vec<_> = std::env::args().collect();
    let run = arguments
        .get(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(0);
    println!("kind,case,samples,median_ns,p95_ns,allocation_calls,requested_bytes,retained_bytes,peak_bytes");
    benchmark("synthetic", &corpus::synthetic(128), run);
    if let Some(manifest) = arguments.get(2) {
        benchmark(
            "local_source",
            &Corpus::from_manifest(std::path::Path::new(manifest))?,
            run,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_methods_preserve_results_and_limits() {
        let corpus = corpus::synthetic(3);
        verify(&corpus, &Indexes::build(&corpus));
    }

    #[test]
    fn content_boundaries_duplicate_hits_case_and_unicode() {
        let mut corpus = Corpus::default();
        corpus.append("abc\r\nbcd\nabc abc\nCAFÉ\ncafé\n東京\n");
        corpus.append("\nabc\n");
        let indexes = Indexes::build(&corpus);
        for query in [
            "", "abc", "abcd", "bc\nb", "CAFÉ", "café", "東京", "a", "bc", "abc abc",
        ] {
            let mut expected = Scratch::default();
            search::search(
                &corpus,
                &indexes,
                query.as_bytes(),
                Method::LineScan,
                usize::MAX,
                &mut expected,
            );
            for method in Method::ALL {
                let mut actual = Scratch::default();
                search::search(
                    &corpus,
                    &indexes,
                    query.as_bytes(),
                    method,
                    usize::MAX,
                    &mut actual,
                );
                assert_eq!(actual.results, expected.results, "{method:?}/{query}");
            }
        }
    }

    #[test]
    fn no_false_negatives_at_block_boundaries_or_in_long_lines() {
        let mut corpus = Corpus::default();
        let mut lines = vec!["ordinary".to_string(); 129];
        lines[63] = "needle_boundary".to_string();
        lines[64] = format!("{}needle_boundary", "a".repeat(70_000));
        lines[128] = "needle_boundary".to_string();
        corpus.append(&lines.join("\n"));
        let indexes = Indexes::build(&corpus);
        for method in Method::ALL {
            let mut actual = Scratch::default();
            search::search(
                &corpus,
                &indexes,
                b"needle_boundary",
                method,
                usize::MAX,
                &mut actual,
            );
            assert_eq!(actual.results, vec![63, 64, 128]);
        }
    }
}
