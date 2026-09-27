//! Run with `cargo run --release -p core-engine --example measure_search`.
use core_engine::{
    applications::{Application, ApplicationCatalog},
    search::SearchEngine,
};
use std::{hint::black_box, time::Instant};

const SAMPLE_COUNT: usize = 2_000;

fn catalog(count: usize) -> ApplicationCatalog {
    let names = [
        "Visual Studio Code",
        "Windows Terminal",
        "Firefox Developer Edition",
        "PowerShell",
        "Σίσυφος Editor",
        "東京 Calendar",
    ];
    ApplicationCatalog::new(
        (0..count)
            .map(|index| Application {
                id: format!("app:{index}").into(),
                name: format!("{} {index}", names[index % names.len()]).into(),
                description: "Synthetic Start Menu target".into(),
                pinned: index % 29 == 0,
                launches: (index % 97) as u32,
                aliases: Default::default(),
            })
            .collect(),
    )
}

fn queries() -> Vec<String> {
    let cases = [
        "",
        "v",
        "vs",
        "vsc",
        "terminal",
        "windows term",
        "ΣΊΣ",
        "東京",
        "not-found",
        "@calc 25% of 80",
        "@web Rust & Windows",
        "@unknown",
    ];
    (0..SAMPLE_COUNT)
        .map(|index| match index % 7 {
            0 => format!(
                "{} {index}",
                ["code", "powershell", "terminal"][(index / 7) % 3]
            ),
            _ => cases[index % cases.len()].to_string(),
        })
        .collect()
}

fn measure(count: usize, queries: &[String], reuse_engine: bool) {
    let catalog = catalog(count);
    let mut engine = SearchEngine::default();
    for query in queries.iter().take(100) {
        black_box(engine.search(query, &catalog));
    }
    let mut durations = Vec::with_capacity(queries.len());
    for query in queries {
        let start = Instant::now();
        if reuse_engine {
            black_box(engine.search(black_box(query), &catalog));
        } else {
            black_box(SearchEngine::default().search(black_box(query), &catalog));
        }
        durations.push(start.elapsed().as_nanos());
    }
    durations.sort_unstable();
    let percentile =
        |percent: usize| durations[(durations.len() * percent).div_ceil(100) - 1] as f64 / 1000.;
    println!(
        "{count},{reuse_engine},{},{},{:.3},{:.3},{:.3},{:.3}",
        catalog.index_memory_bytes(),
        queries.len(),
        percentile(50),
        percentile(95),
        percentile(99),
        durations.last().unwrap().to_owned() as f64 / 1000.
    );
}

fn main() {
    let queries = queries();
    println!("applications,reused_engine,index_bytes,samples,p50_us,p95_us,p99_us,max_us");
    for count in [200, 2_000, 10_000] {
        measure(count, &queries, true);
        measure(count, &queries, false);
    }
}
