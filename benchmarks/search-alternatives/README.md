**Isolated Search alternatives — no launcher implementation**

Read [the experiment report](C:/Users/Robert/code/Pleiades/Core/CORE_SEARCH_EXPERIMENTS.md) for decisions, measured values, semantics and limitations.

The separate [line-search design and experiments](C:/Users/Robert/code/Pleiades/Core/LINE_SEARCH_DESIGN.md) add nine content-search strategies. Run `./run-line-search.ps1` for the synthetic and local-source comparisons. Its source-root paths are explicit in the script; the generated manifest and file hashes record exactly what was read. Results are under `results/line-search/`. `cargo test --bin line-search --release --offline --locked` runs its three dedicated tests. The original filename harness is unchanged by this additional binary.

Run from this directory:

```powershell
cargo test --release --offline --locked
cargo fmt -- --check
cargo clippy --all-targets --offline --locked -- -D warnings
.\run-benchmarks.ps1
.\run-followup.ps1
.\run-quality.ps1
```

The sole Cargo dependency is `memchr` 2.8.0, already used by Core/Search. Offline reproduction requires its cached source. The separate quality experiment uses only the Rust standard library. `run-quality.ps1` compiles and runs its two tests before producing output. `rustfmt --check --edition 2021 launcher-quality.rs` checks that standalone source.

Scripts overwrite their own `results` subdirectory's named CSV outputs when rerun. Preserve a dated copy first if historical snapshots matter. Three raw runs, aggregates, CPU/OS/compiler metadata (main suite), source manifests and source hashes are retained. The initial main timing snapshot predates the prefix follow-up; running the current main suite also measures that ninth method. The later deletion regression fixture only changes tests. Historical source hashes document those stages; reproduction is expected to vary with machine load, randomized hash-table layout and compiler decisions.

**Contracts**

- Common-ranking methods return exact/prefix/substring, alphabetical name and ID top 12. Inputs are already normalized. `Scratch` retains capacity. Catalog construction is excluded from query timing.
- `upstream_complete` runs the actual extracted Search filename pipeline, returning allocated paths in its own traversal order. It is not a same-ranking comparison. The complete-result membership test uses a limit equal to the corpus size.
- `core_legacy` uses Core's extracted selection helper and its score rules. `core_borrowed_indexed_*` retain its all-words semantics. Empty query is handled before production search; benchmark shared queries are nonempty.
- `prefix_then_rarest` early exit is valid only for the common alphabetical tie-break. It does not implement usage/pin ranking.
- Typing measurements time the entire 13-query sequence. Both variants collect all matches; neither ranks/formats visible results. Incremental reuse assumes an unchanged arena and compatible substring semantics.
- The separate relevance fixture tests 20 authored intents on 30 names. Timing expands names to 2,000 synthetic editions. Prepared names, words, initials and normalized queries are excluded from those timers. Generalization beyond this fixture is unproven.

**Measurement**

`latency` times one warm query/sample, with 32 warmups. Mixed-query cases rotate 17 query strings and use 1,020 samples. The per-query sample count is 101, upstream 51, follow-up 201. Aggregates are medians of the three run medians and medians of run p95 values, not a pooled percentile or a confidence interval. Very short operations approach the roughly 100 ns timer quantization; zero means below measurable resolution. The main methods rotate order across process runs, not on every individual sample. No affinity pinning or exclusive CPU access is used.

Index build measurements time five constructions with destruction outside each timer; a separate construction records allocation metrics. Index builders borrow pre-existing file records. The allocator wrapper forwards to `System`; timing includes a disabled tracking check on allocations. Allocation-count intervals run serially and must not free anything allocated before the interval. Query results are dropped within the counting interval; a warmed scratch check expects no net retained allocation change. Global counting is not intended for parallel production use.

`requested_bytes` is cumulative allocator traffic, counting the full new size on realloc. `retained_bytes` is requested live heap at the end of an index build. `peak_bytes` is peak requested live memory during the interval, without allocator internals/overhead. None is RSS, working set, private bytes, GPU memory or total application RAM. Warm zero-allocation queries still own previously allocated scratch and catalog storage. Timing CSV rows have zero allocation fields because allocation measurement is separate; those zeros are **not** measured zero allocation counts. Read `allocation` rows for that evidence.

The comparison `Catalog` intentionally holds the upstream index, compact hash, flat postings, masks and alphabetical metadata together. Do not use its process memory as a proposed Core footprint. Production would select a layout. A global source corpus is generated deterministically; no filesystem index, user content or Windows launch action is accessed.

**Provenance and defect fixtures**

`prepare.ps1` reads `../reference/Search` at the recorded commit. Arena/index source is copied verbatim. Filename search is cut at the `LineMatch` declaration, removing two unused imports. Upstream style warnings and rustfmt are disabled only on the preserved fixtures.

The deletion fixture copies `rebuild_subtree` and `path_within`, with a collector stub that panics. Its test first verifies the virtual directory does not exist, ensuring no real scanning occurs. The test documents the existing stale-ID bug; it intentionally expects the defect. Normal filename and index tests do not invoke this fixture.

Core's selection helper is shared from `../constructs/src/fixtures/legacy_search_text.rs`; its provenance is in that harness's `source-manifest.json`. The benchmark's legacy score function reproduces `app_index.rs::score_application`. Keep fixture extraction guarded and inspect diffs if either reference changes. Never replace a fixture with an optimized version and continue labeling it upstream.
