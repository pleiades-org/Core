**Core v2 — further experiments and implementation decisions**

19 September 2026. This extends the [project audit](C:/Users/Robert/code/Pleiades/Core/CURRENT_CORE_AUDIT.md), [construct review](C:/Users/Robert/code/Pleiades/Core/CORE_CONSTRUCT_REVIEW.md), and [main plan](C:/Users/Robert/code/Pleiades/Core/CORE_V2_PLAN.md). Only isolated experiments and planning documents have changed. The launcher and upstream Search source remain unchanged.

**Recommendation: reuse Search's trigram-filtering idea, simplify its query path, and preserve Core's ranking semantics.** Use the command token → enum → `match` design already proposed. The larger gains come from reducing candidates and allocations, keeping prepared data in memory, and avoiding repeated work. There is no single construct that minimizes CPU, retained RAM, build time, and implementation complexity simultaneously.

**What was tested**

A later [line/content-search experiment](C:/Users/Robert/code/Pleiades/Core/LINE_SEARCH_DESIGN.md) compares file/block/line and positional content indexes against direct scans. Filename results below must not be used as content-search benchmarks.

The supplied [Search repository](https://github.com/pleiades-org/Search/tree/fb1d7969450357955e23c344912c7856478716db) was inspected at commit `fb1d7969450357955e23c344912c7856478716db`. The harness copies its arena and index unchanged, and extracts the unchanged filename-search functions, excluding unused content-search imports. It does not run its scanner, MFT integration, watcher, database, or home-directory indexing.

Experiments cover 32, 128, 512, 2,000, 50,000 and 150,000 deterministic synthetic names. The main suite uses the three largest sizes; a follow-up measures the small sizes and a prefix shortcut. Seventeen queries cover short/broad matches, ordinary terms, two rare hits, absent terms, Unicode, a phrase and repeated characters. Filenames mix realistic stems, extensions, IDs and virtual directories; these are **not a captured real app catalog or a representative million-file disk**. Search paths are never opened.

Nine candidate strategies were checked against a scan oracle, with partial selection, a bounded heap and a fixed sorted buffer. Additional checks cover generated substrings, long/empty input, Unicode boundaries, reordered words, incremental typing/backspace, and complete upstream result membership. A separate 20-intent launcher fixture compares substring/word matching, subsequences, and initials plus bounded typo correction.

Machine: Ryzen 7 7800X3D, 16 logical processors, Windows build 26200, Rust 1.95.0, release optimization 3, thin LTO, one codegen unit. Three process runs; candidate-method order rotates between runs. Results below are medians of run medians. Search percentiles are **individual warm-query** percentiles, summarized as the median of each run's p95. Mixed-query cases have 1,020 samples/run; individual cases have 101 samples, upstream complete search 51, follow-up individual cases 201. The earlier construct harness still measures batch averages.

Desktop scheduling was not isolated or pinned. Timer readings are quantized at approximately 100 ns, sometimes zero for tiny operations: treat sub-microsecond results as a range, not precise nanosecond speed claims. There are no confidence intervals. Warm data, common vocabulary and synthetic distribution matter; min/max run medians are retained in the CSVs.

**1. Complete matching and ranking, with equivalent results**

Every row in this table returns the same top 12 IDs: exact filename, then prefix, then substring, followed by normalized alphabetical name and stable ID. Matching, candidate selection and ranking are included. Display formatting, query normalization, scheduling, usage preferences and UI work are excluded. Scratch capacity is reused.

| Candidate strategy | 2,000 names median | 150,000 names median | 150,000 names p95 |
| --- | ---: | ---: | ---: |
| Plain `str::contains` scan | 25.5 µs | 2,098.9 µs | 4,520.9 µs |
| Precomputed 64-bit byte-presence filter, then scan | 8.8 µs | 852.1 µs | 2,918.4 µs |
| Reusable `memmem::Finder` within each query, scanning names | 19.9 µs | 1,472.7 µs | 2,493.8 µs |
| Upstream Search candidate selection, common final ranker | 1.7 µs | 180.8 µs | 3,501.4 µs |
| Rarest trigram posting, verify substring | 1.5 µs | 136.6 µs | 2,053.7 µs |
| Intersect up to three postings using reusable buffers | 1.5 µs | 138.5 µs | 2,046.7 µs |
| Sorted flat postings, binary-search gram lookup | 1.4 µs | 136.8 µs | 2,046.0 µs |
| Compact hash postings, rarest-list lookup | 1.6 µs | 145.4 µs | 2,070.8 µs |

The approximately **15×** large-catalog median improvement from scan to rarest-posting lookup is meaningful here. Differences among the indexed variants are much smaller and overlap desktop variability; do not crown a universal winner from those differences.

The simpler indexed algorithm is:

1. Normalize once; form overlapping three-byte grams from the normalized query.
2. If any required gram is absent, return no substring candidates immediately.
3. Borrow the shortest posting list. Verify the full substring against those names to remove false positives.
4. Score every eligible candidate and select the best results using IDs and reused storage.
5. For fewer than three bytes, scan the small catalog or use a separately justified short-query path.

There is no need to intersect every posting or scan the whole index after verifying this complete candidate superset. Hash collisions in the separate byte-mask experiment only admit extra candidates; the final string check protects correctness.

For Core's existing **all-words** rule, choose the shortest available posting from any required word, then verify **every** word. Do not form one phrase-wide trigram filter for reordered words. Acronyms and typo matches need independent candidate paths; exact trigrams cannot safely exclude them.

**Comparison with Core's actual scoring and allocating selection helper**

This separate experiment preserves the old four score tiers, including reordered-word matching, on the same 2,000 synthetic names. The old selection helper is copied from Core. The optimized path precomputes alphabetical ordinals and reuses typed candidate storage.

| Query | Existing scoring + selection | Borrowed scan | Indexed, borrowed selection |
| --- | ---: | ---: | ---: |
| `a` | 226.1 µs | 29.5 µs | 22.8 µs |
| `report` | 60.4 µs | 42.9 µs | 1.6 µs |
| `code visual` | 84.1 µs | 75.9 µs | 2.2 µs |

These are equivalent-output comparisons for the tested corpus, unlike treating every optimized search as equivalent to upstream Search's different ranking. They do not measure the current running launcher.

**2. What to change in the Search algorithm before reuse**

The upstream complete pipeline has a different contract: graph/filename-length traversal order, early termination and cloned path strings. It can be very fast when it encounters enough broad matches early. At 150,000 names, `a` took **3.8 µs** upstream versus approximately **2.06 ms** for exhaustive relevance ranking. Those outputs are not interchangeable.

Its opposite case is a rare or absent term. `needle_7fd9` took **4.79 ms**, and the absent term took **3.79 ms**, because the filtered search falls back to scanning/sorting the rest when fewer than 12 results exist. Rarest-list verification completes these synthetic cases in **under 1 µs**. Do not report a huge precise ratio at the timer's resolution limit.

| Finding in the supplied source | Consequence / planned treatment |
| --- | --- |
| `candidates_for` ignores missing grams with `filter_map` | Wasted candidate work. A missing required gram proves no exact substring match. |
| Filtered search falls back to the full visit order when it finds fewer than the limit | Rare hits and misses lose the main benefit of indexing. Remove that fallback for exact substring membership. |
| Candidate and directory lists are sorted during queries; a `seen` vector covers every file | Avoid recurring allocation/sorting. Use stable IDs and a documented result comparator. |
| Graph order is based on distance to the global mean filename length | It is not an app relevance score. Do not use it as Core's result order. Children sorted ascending and pushed onto a LIFO stack are visited in the opposite order. |
| Filename normalization uses ASCII lowercase | `CAFÉ.txt` does not match `café`; a regression test reproduces this. Use the same agreed Unicode normalization on indexed names and queries. Preserve native launch paths separately. |
| Deletion compacts `arena.files` without remapping directory file IDs | A regression fixture reproduces a retained `b.txt` becoming attached to the removed directory. Use stable identity or rebuild/remap every affected reference before publishing a snapshot. |
| `CodeSearch::search` drains watcher events and can rebuild/save before searching | Keep mutation, I/O and persistence on the background catalog worker. Search reads an immutable version. |
| Content search lowercases the query but searches original file bytes | Source inspection shows inconsistent case behaviour. Content search remains a deferred feature with its own correctness requirements. |

The deletion test deliberately fingerprints an existing defect; passing it does **not** mean upstream is fixed. No upstream source was edited or issue posted. References: [index](https://github.com/pleiades-org/Search/blob/fb1d7969450357955e23c344912c7856478716db/src/index.rs), [filename/content search](https://github.com/pleiades-org/Search/blob/fb1d7969450357955e23c344912c7856478716db/src/search.rs), [update logic](https://github.com/pleiades-org/Search/blob/fb1d7969450357955e23c344912c7856478716db/src/walker.rs), [engine](https://github.com/pleiades-org/Search/blob/fb1d7969450357955e23c344912c7856478716db/src/engine.rs).

**3. Resource trade-offs: flat versus hash postings**

Allocation instrumentation separately measures requested live heap bytes while building a new index from borrowed, already-built filename records. This includes index-owned containers but **excludes filename/path strings, the existing arena, UI, stacks, allocator overhead, OS/GPU memory and RSS**. Peak is the maximum requested live bytes observed by the wrapper, not transient memory inside the allocator or a process peak.

| Index representation | Retained at 2,000 | Retained at 150,000 | Peak build at 150,000 | Build time at 150,000 |
| --- | ---: | ---: | ---: | ---: |
| Upstream hash `Vec` postings plus length buckets/visit order | 0.433 MiB | 21.98 MiB | 21.98 MiB | 44.34 ms |
| Compact hash → boxed posting slices | 0.266 MiB | 12.88 MiB | 20.59 MiB | 45.24 ms |
| Sorted flat postings with offsets | 0.199 MiB | 12.74 MiB | 38.22 MiB | 80.08 ms |

Compacting the hash layout saves about **41% retained index memory** here. Flat storage is slightly smaller at 150,000, but sorting its temporary gram/ID pairs nearly doubles build time and substantially increases build peak. Prefer the compact hash build for a future large dynamic file catalog; do not choose flat storage solely from its final size.

At 2,000 names the trade-off is different: flat build took **0.87 ms**, compact hash **1.00 ms**, upstream **0.81 ms**. Flat storage saves about 69 KiB versus compact hash with similar query time. At 128 names it uses about 18.2 KiB. This supports a small immutable flat index for the launcher, constructed only when its catalog changes.

For a tiny catalog, a scan uses no posting storage and is already cheap. Follow-up mixed medians were 0.4 µs for 32 names and 1.4 µs for 128 names. Rarest postings were below 1 µs at both sizes. A **128-entry threshold is a proposed RAM/complexity policy**, not a discovered universal speed crossover. A proposed 512 KiB posting budget bounds the initial app index; measure actual catalogs before locking that policy. A scan remains the correctness oracle and fallback.

Byte masks add eight bytes/name: 1.14 MiB at 150,000. Alphabetical ordinals add four bytes/name: 0.57 MiB. A separately stored alphabetical ID order adds another four bytes/name. The harness holds several competing indexes simultaneously for comparison; that is not the proposed production layout.

Upstream rare search at 150,000 requested about **5.29 MiB cumulatively per query**, peaking at **3.29 MiB** of requested live query allocations, with 28 allocation/reallocation calls. The rarest-list path with warmed scratch made **zero measured query allocations**. That does not mean zero retained RAM: its broad-query partial-selection scratch reached **1.5 MiB** in the large corpus. Use a bounded selector for a future large catalog when that scratch cost matters. Final displayed strings and the owned cross-thread query still require storage.

**4. Additional shortcuts, including when they are unsafe to adopt**

**Prefix range + rarest postings.** Binary-search names in alphabetical order; if at least 12 exact/prefix results exist, those first 12 are provably sufficient for the experiment's alphabetical tie-break. Otherwise perform ordinary indexed ranking. Correctness checks matched the scan oracle. At 150,000, broad `a`, `re`, and `document` queries completed below 1 µs; the mixed median was 0.9 µs, but p95 was still **1.30 ms** because some short substring queries need scanning. The rarest-only follow-up median/p95 were 149.6 µs / 2.09 ms.

**Keep this shortcut conditional.** Core's planned pins/frequency ordering within a match class can change which prefix results belong in the top 12. The alphabetical early exit would then be wrong. The default Core path must score all eligible candidates, or maintain a separately validated ranking-aware prefix index. Do not remove personalization to claim the prefix timing.

**Incremental narrowing.** Retaining all previous substring matches reduced a 13-query typing/backspace/replacement sequence from **16.98 ms to 8.16 ms** at 150,000. These are whole-sequence times, not one-keystroke latencies. This does not prove an advantage over trigram lookup. Narrow only after a compatible append on the same catalog and query mode; reset on replacement/backspace/version changes. Never narrow from just the displayed top 12. Keep this deferred for the small launcher, which already has cheap indexed queries.

**Result selection.** With real matching included at 150,000, the heap beat partial selection for `a` (1.58 versus 2.05 ms) and `document` (364 versus 442 µs). The fixed sorted buffer did not win those cases. At 2,000 the absolute differences and run variability are small; the earlier order-sensitive experiments still apply. Keep partial selection plus reused storage for apps; use a bounded heap as the leading large-file candidate. Precomputed ordinals avoid string comparisons but need rebuilding when their relevant name-order version changes.

**5. Better launcher relevance without treating every query as fuzzy**

The separate fixture has 30 named apps and 20 declared intents: exact/prefix, reordered words, initials, four basic edit types, Unicode, no matches and empty input. Timing uses 2,000 synthetic app editions. Query normalization and prepared name/word/initial metadata are outside timing.

| Matching policy | Correct top result or correct empty result on 20 curated intents | `vsc` | `chorme` |
| --- | ---: | ---: | ---: |
| Current exact/prefix/substring/all-words policy | 11/20 | 29.1 µs, misses intent | 46.6 µs, misses intent |
| Add general subsequence fallback per candidate | 17/20 | 50.3 µs | 70.2 µs, misses intent |
| Add initials; if no strict/initial result, allow one bounded edit | 20/20 | 37.0 µs | 123.3 µs |

The bounded prototype handles an ASCII substitution, insertion, deletion or adjacent transposition. It caps typo queries at 64 bytes and four words; exact Unicode lowercase matching is separate. This is a **small, deliberately authored behaviour fixture**, not a statistical relevance score, independent holdout set or proof of general fuzzy quality. Accent equivalence, full Unicode case folding, ambiguity, aliases, compound words and multiple typos need additional specifications.

Recommendation: precompute initials, preserve exact/word match classes, and make bounded typo correction a late path only when strict results are absent. A tiny initials lookup can bypass a full abbreviation scan. Do not require all exact query trigrams in the typo path: doing so would discard the intended correction. The measured prototype scans for typo candidates; typo-aware index alternatives were not measured. Hold off on general subsequence matching and heavy fuzzy dependencies unless broader relevance fixtures justify them.

**6. Further command and normalization alternatives**

All four parser alternatives now have equal prefix-only semantics on shared alias, case, Unicode-payload, unknown, boundary and oversized-input tests. Numbers are batch-average nanoseconds, not individual-query tails.

| Parser | Mixed commands | Uppercase and misses | Query allocations |
| --- | ---: | ---: | ---: |
| Checked stack lowercase → string `match` | 15.98 ns | 21.49 ns | 0 |
| Try canonical spelling, otherwise existing parser | 14.96 ns | 38.18 ns | 0 |
| Static aliases with ASCII-insensitive comparisons | 18.23 ns | 24.74 ns | 0 |
| Lowercase → binary search of aliases | 33.26 ns | 42.74 ns | 0 |

Keep the simple stack lowercase + enum `match`. The canonical-first prototype saves about one nanosecond on the mixed set and loses on uppercase/misses, partly because its fallback reparses. That is not a useful implementation priority. This does not prove that every possible canonical-first parser is slower.

For normalization, borrowing canonical ASCII took **18.27 ns** versus **96.08 ns** for the original owned algorithm. Mixed-case/whitespace ASCII into reused storage took **26.93 ns**, versus **100.89 ns** owned, and eliminated three per-query allocations after capacity initialization. Unicode fallback still took approximately **155–167 ns** with three allocations. Preserve that correct fallback rather than changing case semantics for a tiny saving.

A new exhaustive ASCII test caught a prototype bug: Rust's ASCII-whitespace predicate excludes vertical tab while Unicode `split_whitespace` includes it. Both fast paths now handle it explicitly. Tests also retain whole-string Greek sigma lowercasing. Production recommendation: borrow canonical input, otherwise normalize ASCII into worker-owned reusable storage, and use the established whole-string Unicode fallback. The composed three-way dispatcher itself is not separately timed here.

**7. Updated implementation plan and remaining gates**

| Decision | Selected direction |
| --- | --- |
| Built-in command routing | Borrow prefix/body, checked ASCII normalization, typed enum and exhaustive `match`. Persistent composed calculator engine. |
| App catalog | Contiguous immutable entries, stable launch identity, precomputed names/initials and ordinal metadata. No graph traversal required. |
| App candidate generation | Scan tiny catalogs; test a bounded flat trigram index for larger app catalogs. Rarest posting + final verification; independent initials path. |
| Personalized final ranking | Preserve match class, bounded usage preference, canonical name, stable ID. Reused partial-selection scratch. No unsafe alphabetical early exit. |
| Typo tolerance | Bounded, late fallback; retain as optional until broader fixtures and integration measurements pass. |
| Future file module | Compact hash postings and bounded top-k; background snapshots, correct ID updates, chosen roots. Remains outside the small release. |
| Query scheduling and UI | One running/one pending query, cancellation, notifications rather than polling, lazy icons, no query-path I/O. Existing plan unchanged. |

The experiments are sufficient to choose these starting constructs; further nanosecond contests should not delay integration. Production confidence still needs real installed-app catalogs, duplicate names, usage ordering, cancellation/version replacement, locale/IME boundaries, result formatting and query-to-presented-frame traces.

Effort estimates remain engineering estimates for one developer: roughly **0.5–1 day** for query states/routing, **1–2 days** for catalog/index/selection integration and its correctness gates, and **1–2 days** for scheduling/cancellation, overlapping the existing engine milestone. A hardened typo path adds approximately **1–2 days** if included. Broader time/date/unit arithmetic remains a separate later effort. The main plan's shell, discovery, migration and release work remains necessary; these timings are not a claim that the whole app is nearly implemented.

The **50 MiB hidden RAM, 75 MiB visible RAM, ≤0.1% one-core idle CPU, ≤35 ms query-to-display p95 and ≤500 ms cached startup remain unverified application targets**. Engine and index measurements cannot establish those numbers. The next implementation milestone remains the measured minimal shell and pure engine integration; no implementation has begun in this task.

**Reproduction and evidence**

- [Search harness and methodology](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/README.md), [main results](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/results/summary.csv), [prefix/small-catalog follow-up](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/results/followup/summary.csv).
- [Relevance outcomes](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/results/quality/intent-results.csv), [relevance timings](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/results/quality/summary.csv), [expanded construct timings](C:/Users/Robert/code/Pleiades/Core/benchmarks/constructs/results/summary.csv).
- [Pinned upstream source manifest](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/source-manifest.json), [testable algorithm alternatives](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/src/matching.rs), [regression checks](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-alternatives/src/quality.rs).
