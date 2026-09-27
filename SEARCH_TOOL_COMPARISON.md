**Search versus ripgrep, Git grep and GNU grep — measured on this machine**

19 September 2026. The current Search can beat a normal ripgrep directory scan for selective queries across many files, but it uses a prepared filename catalog and spends more CPU. It loses substantially when returning many lines. The experimental direct scanner also has wins and losses. These results support choosing different paths for different workloads; they do not establish a universal winner.

This compares **line/content search**, not filename ranking, fuzzy matching, regex or the complete Core launcher. Core and the supplied Search implementation remain unchanged.

**What was actually tested**

- Unchanged [Search revision fb1d7969](https://github.com/pleiades-org/Search/tree/fb1d7969450357955e23c344912c7856478716db), release build, Windows MFT feature disabled, existing filename database loaded on each invocation.
- Experimental single-thread scan: read one file at a time, reuse a `memmem` literal finder, skip to the end of a matched line, calculate line numbers and buffer output. No content index. This is a benchmark-only prototype with a narrower feature set than ripgrep.
- ripgrep **15.1.0**, automatic workers and one worker; Git **2.54.0.windows.1** using `git grep --no-index`; installed Git-for-Windows GNU grep **3.0**. These Windows/MSYS results are not measurements of current Linux grep.
- Source: **73 files / 938,302 bytes**. Synthetic many-file case: **1,280 files / 55,707,698 bytes (53.1 MiB)**. Single-file case: exactly the same synthetic bytes concatenated into one file.
- Ryzen 7 7800X3D, 16 logical processors, Windows 26200, Rust 1.95.0. One benchmark child at a time, tool order rotated, 21 samples per main case, 2,646 measured invocations. No affinity pinning or exclusive machine access.

Each invocation starts a fresh process with a warm filesystem cache. All tools search identical valid UTF-8 text with case-sensitive single-line literal matching, and print **every matching line with filename, line number and text**. Timers include process creation, initialization, file access, output and draining/decoding stdout; exclude correctness sorting/hashing and terminal rendering. All-results queries avoid confusing Search's global limit with ripgrep's per-file `-m` limit.

Relative paths have comparable lengths. Full result tuples are normalized and compared, including duplicate counts. Exit codes are checked. No shell pipelines terminate a tool after partial output. Search's filename cache is outside the searched directory; it is **not a content trigram index**. Standard directory scans include file discovery. A separate explicit-file-list experiment examines that difference.

**Main results: median milliseconds, lower is better**

| Workload | Matching lines | Current Search | Scan prototype | rg auto | rg one worker | git grep via normal launcher | GNU grep |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Real source: `normalize_search_text` | 109 | 29.1 | **25.6** | 30.4 | 28.4 | 47.9 | 37.4 |
| Many files: rare marker | 1 | **52.7** | 92.4 | 76.4 | 126.4 | 68.5 | 196.9 |
| Many files: absent long literal | 0 | **50.3** | 102.7 | 73.5 | 121.8 | 67.2 | 198.3 |
| Many files: `return` | 81,920 | 226.8 | 164.1 | **86.9** | 210.2 | 94.0 | 250.8 |
| One large file: rare marker | 1 | 48.8 | 35.8 | 38.9 | **34.0** | 69.5 | 71.9 |
| One large file: absent long literal | 0 | 50.7 | 36.3 | 39.2 | **33.9** | 68.3 | 74.9 |
| One large file: `return` | 81,920 | 224.0 | **58.4** | 61.4 | 65.2 | 115.9 | 123.0 |

Search was about **1.45× faster** than rg-auto for the many-file rare marker, but rg-auto was **2.61× faster** for many-file `return` and **3.65× faster** for single-file `return`. The small prototype/rg gap on the large-file broad query is not a robust universal win: their tails overlap. That prototype also reads the entire large file into RAM and lacks the standards' feature coverage.

For context, many-file rare-marker median/p95 was **52.7/63.1 ms** for Search and **76.4/88.6 ms** for rg-auto. Many-file `return` was **226.8/255.3 ms** versus **86.9/99.9 ms**. Single-file `return` was **58.4/73.0 ms** for the prototype and **61.4/97.5 ms** for rg-auto. These 21-sample p95 estimates are coarse observations, not service-level guarantees.

**Tuning and resource follow-up**

An interleaved 11-sample follow-up used direct native Git (bypassing the Windows launcher), `rg --mmap`, and ripgrep supplied a prepared file list. Many-file rare-marker medians were **44.1 ms Search, 45.5 ms native Git, 66.4 ms rg-auto and 62.5 ms rg with the file list**. Treat Search/native Git as approximately tied. For many-file `return`, native Git took **70.5 ms**, rg-auto **78.4 ms**, and Search **209.0 ms**. Forcing memory mapping did not help these cases. Controls changed between runs, so small differences across the main and tuning passes are not decisive.

For an absent long literal across many files, main-run median CPU time was **187.5 ms Search, 109.4 ms rg-auto, 78.1 ms prototype**. Lower latency can consume more CPU through parallel work. CPU accounting is quantized at approximately 15.625 ms; the normal Git launcher's parent-only CPU is not comparable with native Git.

A separate five-sample pass polled each live process's OS working-set high-water mark. Observed median peaks for the same absent query were **8.2 MiB Search / 7.6 MiB rg-auto / 4.7 MiB prototype** across many files, and **62.2 / 7.4 / 57.6 MiB** for one large file. These are sampled lower bounds, exclude filesystem cache and the harness, and are not private heap measurements. Whole-file allocation makes both Search and the prototype considerably less memory-efficient on the large file. See [tuning data](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/results/tuning-summary.csv) and [memory data](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/results/memory-summary.csv).

An independent line-by-line oracle agreed with all 126 main cases. Additional CRLF, Unicode, metacharacter, no-final-newline and case probes confirmed the original Search's **uppercase-query defect**: `CamelCase` searches for `camelcase`, returning the wrong line despite an equal result count. The benchmark-only prototype passed those checks. Formatting, strict Clippy and the existing three line-search tests passed.

**Why this changes the design**

The existing implementation reads bodies and visits lines even when a literal is absent. It materializes owned path/content strings for matches and emits them using repeated `println!` calls. The experimental scanner changes several of those operations together, so its speedup does not isolate the effect of any one change. Nevertheless, whole-line result creation/output is a material part of the end-to-end contract and cannot be omitted from a fair comparison.

Keeping a prepared catalog and using parallel reads helps the existing Search on many files. It does not prove the midpoint or filename-trigram matching logic accelerates content search. A body-only query still needs all eligible bodies scanned until a real content index can reject candidates.

The single-thread scan is a useful small-corpus/large-file baseline, not a blanket replacement: it loses badly to parallel search across 1,280 files. Worker count should reflect eligible bytes, file count and the CPU budget, with bounded concurrency and cancellation.

**Resident rarest-trigram experiments remain a separate comparison**

The earlier [line-search study](C:/Users/Robert/code/Pleiades/Core/LINE_SEARCH_DESIGN.md) measured **2.1 µs** for line-rarest filtering versus **23.4 µs** for an in-memory buffer scan on the real-source `normalize_search_text` query. Both returned 109 line IDs with content already resident; neither included process creation, file access or formatted output. The line index cost approximately **3.39 MB of additional requested heap and 17.4 ms to build**, excluding shared content/line metadata and process overhead.

Do **not** divide a 30 ms ripgrep CLI measurement by that 2.1 µs kernel measurement and claim a four-order-of-magnitude engine win. A fair resident comparison would embed an equivalent standard engine with prepared input and the same output contract, or compare complete indexed services including build/load/update costs. Zoekt, livegrep, regex, cold storage, multi-gigabyte data, top-k ranking and changing files were not benchmarked in this turn.

**Evidence and reproduction**

The [benchmark instructions](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/README.md) record the contract and build/run commands. See [main summary](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/results/summary.csv), [all samples](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/results/samples.csv), [exact tool arguments](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/tools.ps1), and [environment](C:/Users/Robert/code/Pleiades/Core/benchmarks/search-tool-comparison/results/environment.json).
