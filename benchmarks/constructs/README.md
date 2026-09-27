**Core construct benchmarks — isolated experiments, not the application**

The expanded suite adds four equivalent prefix-parser alternatives on mixed and uppercase/miss corpora, plus borrowed/owned/reused-buffer normalization on canonical ASCII, changing ASCII and Unicode. These cases are described in [the Search follow-up report](C:/Users/Robert/code/Pleiades/Core/CORE_SEARCH_EXPERIMENTS.md). The ASCII fast paths now explicitly include vertical tab to preserve `split_whitespace` semantics; tests check all 128 ASCII bytes. The original review preserves its first measurement snapshot; this directory's CSV files reflect the latest expanded run. Re-running the script overwrites those files.

Run `./run-benchmarks.ps1` from this directory. It builds in release mode and writes three raw CSV files, an aggregate CSV and environment metadata under `results/`. The only dependency is the same `regex` version and transitive versions already locked in the existing Core project; no new library choice or installation is required. Offline reproduction requires those versions in the local Cargo cache.

For correctness, run `cargo test --release --offline --locked`. For checks, run `cargo fmt -- --check` and `cargo clippy --offline --locked --all-targets -- -D warnings`.

**What is measured**

| Group | Workload |
| --- | --- |
| Routing | Exact extracted legacy parser versus a four-family, prefix-only borrowed parser. Ten queries rotate; tests establish equivalent outputs on this corpus only. New parsing deliberately excludes embedded scopes, punctuation cleanup, arbitrary extension tags and legacy whitespace collapsing. |
| Command lookup | Rust string `match`, linear static alias table and a prebuilt standard `HashMap`; 20 aliases, eight rotating hit/miss probes. Construction costs are excluded. |
| Normalization | Original normalization-cache function, the original uncached algorithm, and an ASCII borrowed fast path with original Unicode fallback. Both repeated-input hits and a 512-key churn workload are measured. |
| Regex | The exact named-day pattern from `calculator.rs:1933`, compiled per operation versus compiled once before timing. Both call `is_match`; this is not the full calculator, capture extraction or a general tokenizer. |
| Ranking | Original `take_top_scored` with cloned string comparison keys, borrowed partial selection, full sort, bounded heap and fixed sorted buffer. Keep 12 results from pre-scored synthetic candidates, excluding matching and rendering. Include mixed, best-first and worst-first input. |

Timing uses `black_box` at observable inputs/outputs and 31 batches per case. The median and p95 in raw CSVs are **per-operation averages within batches**; they are not individual-operation latency percentiles. Aggregate values are medians of the three run medians. No confidence intervals, universal speed claims or whole-launcher speedups are implied. Short lookup measurements are sensitive to compiler decisions, branch prediction and harness overhead.

The allocator counts `alloc`, `alloc_zeroed` and `realloc` requests in a separate pass after timing. Requested bytes are cumulative traffic, counting the full new size on realloc; they are **not peak live allocations, private bytes, working set or GPU memory**. Deallocation is performed but not counted as an allocation call. Cold setup, retained regex state, catalog construction, map construction and global-cache capacity are excluded from per-operation counters. Timing still includes the allocator wrapper's disabled counting check.

Ranking fixtures contain unique names and fixed scores. The legacy helper includes general scoring/tuple materialization while optimized selectors read pre-scored records, so improvements include representation and allocation changes, not just the selection algorithm. The no-clone full sort is a standard alternative, not a claim about the legacy implementation. Best/worst arrival order demonstrates why the fastest method depends on input. The 50k/150k rows are selection kernels, **not file-search benchmarks**.

Borrowing does not eliminate ownership across threads: a real search request still needs owned query storage. The prototype parser borrows that storage only for the duration of processing. `None` is sufficient for this benchmark; production parsing must distinguish normal search, incomplete command, unknown command and invalid input.

**Source provenance**

`prepare.ps1` copies `search_text.rs` verbatim and extracts routing/type helpers from the current old source. `source-manifest.json` records SHA-256 hashes of the original files. The generated fixture retains the old parser and helpers; only a wrapper exposes command/payload results. Rustfmt skips copied modules, and one legacy Clippy `single_match` warning is locally allowed to preserve the measured construct.

Do not re-run extraction after source changes without reviewing its guarded line ranges. The checked-in fixtures permit reproduction without modifying or compiling the old application. No benchmark touches user data or calls providers, Windows launch actions, network endpoints or clipboard APIs.
