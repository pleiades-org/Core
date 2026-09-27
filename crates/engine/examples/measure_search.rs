//! Run with `cargo run --release --locked -p core-engine --example measure_search`.
//! The synthetic catalog has production shapes: IDs are `app:` plus four hex digits per UTF-16
//! unit of a Start Menu shortcut path (about 300 bytes), every app has an alias, and the engine
//! holds 48 recent IDs. `ranking` columns time `ApplicationCatalog::search` alone, without
//! query routing, converters or result formatting.
use core_engine::{
    applications::{Application, ApplicationCatalog, SearchScratch},
    quicklinks::{self, Quicklink},
    search::SearchEngine,
    VISIBLE_RESULT_LIMIT,
};
use std::{fmt::Write, hint::black_box, sync::Arc, time::Instant};

const SAMPLE_COUNT: usize = 2_000;
const RECENT_COUNT: usize = 48;
const QUICKLINK_COUNT: usize = 40;
/// Display names and the alias Windows knows each app by.
const NAMES: [(&str, &str); 6] = [
    ("Visual Studio Code", "code"),
    ("Windows Terminal", "wt"),
    ("Firefox Developer Edition", "firefox"),
    ("PowerShell", "pwsh"),
    ("Σίσυφος Editor", "sisyphus"),
    ("東京 Calendar", "tokyo"),
];
/// Typing and empty-scope queries; the second value is the text ranked on its own, if any.
const KEYSTROKES: &[(&str, Option<&str>)] = &[
    ("", None),
    ("v", Some("v")),
    ("vi", Some("vi")),
    ("vis", Some("vis")),
    ("visu", Some("visu")),
    ("visual", Some("visual")),
    ("code", Some("code")),
    ("vsc", Some("vsc")),
    ("visual studio code", Some("visual studio code")),
    ("@app ", None),
    (">", None),
    ("> docs", None),
];

/// Production identity: `app:` and four hex digits per UTF-16 unit of the shortcut path.
fn identity(path: &str) -> Arc<str> {
    let mut identifier = String::from("app:");
    for unit in path.encode_utf16() {
        write!(identifier, "{unit:04x}").expect("writing to String cannot fail");
    }
    identifier.into()
}

fn applications(count: usize) -> Vec<Application> {
    (0..count)
        .map(|index| {
            let (name, alias) = NAMES[index % NAMES.len()];
            let name = format!("{name} {index}");
            Application {
                id: identity(&format!(
                    r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\{name}.lnk"
                )),
                name: name.into(),
                description: "Synthetic Start Menu target".into(),
                pinned: index % 29 == 0,
                launches: (index % 97) as u32,
                aliases: Arc::from([Arc::from(format!("{alias}{index}"))]),
            }
        })
        .collect()
}

/// Spread across the catalog, most recent first, as the launcher passes them.
fn recent(applications: &[Application]) -> Arc<[Arc<str>]> {
    applications
        .iter()
        .step_by((applications.len() / RECENT_COUNT).max(1))
        .take(RECENT_COUNT)
        .map(|application| application.id.clone())
        .collect()
}

fn quicklinks() -> Vec<Quicklink> {
    (0..QUICKLINK_COUNT)
        .map(|index| {
            Quicklink::new(
                &format!("Docs {index}"),
                &format!("https://example.com/docs/{index}"),
            )
            .expect("valid quicklink")
        })
        .collect()
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

/// Individual timings in nanoseconds for each query.
fn time_each(queries: &[String], mut run: impl FnMut(&str)) -> Vec<u128> {
    queries
        .iter()
        .map(|query| {
            let start = Instant::now();
            run(black_box(query));
            start.elapsed().as_nanos()
        })
        .collect()
}

/// p50, p95, p99 and maximum in microseconds.
fn percentiles(mut durations: Vec<u128>) -> [f64; 4] {
    durations.sort_unstable();
    let percentile =
        |percent: usize| durations[(durations.len() * percent).div_ceil(100) - 1] as f64 / 1000.;
    [
        percentile(50),
        percentile(95),
        percentile(99),
        *durations.last().expect("samples") as f64 / 1000.,
    ]
}

fn measure(count: usize, queries: &[String], reuse_engine: bool) {
    let applications = applications(count);
    let recent = recent(&applications);
    let catalog = ApplicationCatalog::new(applications);
    let new_engine = || {
        let mut engine = SearchEngine::default();
        engine.set_recent_applications(recent.clone());
        engine
    };
    let mut engine = new_engine();
    let mut scratch = SearchScratch::default();
    for query in queries.iter().take(100) {
        black_box(engine.search(query, &catalog));
        black_box(catalog.search(query, VISIBLE_RESULT_LIMIT, &mut scratch));
    }
    let search = percentiles(time_each(queries, |query| {
        if reuse_engine {
            black_box(engine.search(query, &catalog));
        } else {
            black_box(new_engine().search(query, &catalog));
        }
    }));
    let ranking = percentiles(time_each(queries, |query| {
        black_box(catalog.search(query, VISIBLE_RESULT_LIMIT, &mut scratch));
    }));
    println!(
        "{count},{reuse_engine},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
        catalog.index_memory_bytes(),
        queries.len(),
        search[0],
        search[1],
        search[2],
        search[3],
        ranking[0],
        ranking[1],
    );
}

/// Median nanoseconds per call. Batches of about 20 µs smooth over the timer's ~100 ns
/// resolution on Windows without making slow queries take minutes.
fn median_ns(mut run: impl FnMut()) -> f64 {
    const BATCHES: usize = 101;
    const BATCH_NANOSECONDS: u128 = 20_000;
    run();
    let start = Instant::now();
    run();
    let batch_size = (BATCH_NANOSECONDS / start.elapsed().as_nanos().max(1)).clamp(1, 100) as u32;
    let mut batches: Vec<f64> = (0..BATCHES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..batch_size {
                run();
            }
            start.elapsed().as_nanos() as f64 / f64::from(batch_size)
        })
        .collect();
    batches.sort_unstable_by(f64::total_cmp);
    batches[BATCHES / 2]
}

/// One keystroke at a time through the launcher's apps and quicklinks, ranked as one list.
fn measure_keystrokes(count: usize) {
    let applications = applications(count);
    let recent = recent(&applications);
    let catalog = ApplicationCatalog::new(applications);
    let (quicklink_catalog, general) = quicklinks::catalogs(&catalog, &quicklinks());
    let mut engine = SearchEngine::default();
    engine.set_recent_applications(recent);
    let mut scratch = SearchScratch::default();
    for &(query, ranked) in KEYSTROKES {
        let search = median_ns(|| {
            black_box(engine.search_catalogs(
                black_box(query),
                &catalog,
                &general,
                &quicklink_catalog,
                &|| false,
            ));
        });
        let ranking = ranked.map(|text| {
            median_ns(|| {
                black_box(general.search_merged(
                    &quicklink_catalog,
                    black_box(text),
                    VISIBLE_RESULT_LIMIT,
                    &mut scratch,
                    &|| false,
                ));
            })
        });
        println!(
            "{query:?},{count},{search:.0},{}",
            ranking.map_or_else(|| "-".to_owned(), |ranking| format!("{ranking:.0}"))
        );
    }
}

fn main() {
    let queries = queries();
    println!(
        "applications,reused_engine,index_bytes,samples,p50_us,p95_us,p99_us,max_us,\
         ranking_p50_us,ranking_p95_us"
    );
    for count in [400, 2_000, 10_000] {
        measure(count, &queries, true);
        measure(count, &queries, false);
    }
    println!();
    println!("query,applications,search_median_ns,ranking_median_ns");
    for count in [2_000, 10_000] {
        measure_keystrokes(count);
    }
}
