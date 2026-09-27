# First implementation milestone — 19 September 2026

Historical baseline. The [20 September styling update](STYLING_UPDATE.md) records the current rounded, animated build and its own measurements; executable hashes and RAM figures below refer to the earlier alpha.

Core v2 now has a working native launcher and a separately testable engine. The implementation covers application discovery/search, calculator, explicit web search, command completion, hotkey/tray, generation-safe input handling and action adapters. This is the start of implementation, not completion of the first-release feature list.

## Decisions backed by measurements

The minimal GPUI probe retained 64.89 MiB private bytes after 60 seconds hidden, before loading the real catalog. The comparable initial native control probe retained 1.24 MiB. The GPUI probe did not leave headroom under the proposed 50 MiB gate, so the planned native fallback was selected. These were early shell experiments in the sandbox session, not an old-Core-versus-new-Core feature-equivalent comparison. The original GPUI and native raw observations remain in `measurements/gpui-hidden-60s.json` and `measurements/native-hidden-60s.json`.

Production command routing follows the tested borrowed-prefix → enum → exhaustive `match` design. A persistent calculator provider handles arithmetic without allocating a token vector. Dedicated modules own discovery, ranking, scheduling and platform actions; there is no large feature-dispatch class or general-purpose plugin runtime.

App matching now uses the Search experiments' rarest-trigram idea with bounded flat postings. Required grams are drawn from individual query terms, then the complete ranker verifies candidates. Acronyms have their own ordered lookup; missing exact grams cannot eliminate acronym matches. A scan remains the oracle and fallback. Generated substring, Unicode, reordered-word, duplicate identity, pin/frequency and different result-limit tests compare full ranked results against that oracle.

The initial scan implementation and the indexed implementation are measured by the same release example, with 2,000 changing query samples at each catalog size. The corpus is synthetic; it includes empty/short/broad queries, initials, Unicode, rare numeric suffixes, misses and command routes. Final row allocation is included; UI, scheduling, filesystem work and index construction are excluded. `engine-scan-search.csv` records the scan baseline; `engine-search.csv` records the latest indexed build. Retained index bytes exclude prepared names, catalog paths, allocator overhead and query scratch. One process run per version is an initial observation, not a statistical proof or a universal speed claim.

| Engine measurement | Initial scan | Current bounded index |
| --- | ---: | ---: |
| 2,000 apps, mixed-query median | 121.9 µs | 3.3 µs |
| 2,000 apps, mixed-query p95 | 193.1 µs | 126.1 µs |
| 2,000 apps, mixed-query p99 | 234.8 µs | 196.0 µs |
| 2,000 apps, retained index storage | 0 | 190,216 bytes (185.8 KiB) |
| 10,000 apps, mixed-query p95 | 933.5 µs | 1,010.6 µs, scan fallback |

The large median gain comes from filtering longer queries. Broad short queries still scan and dominate the tail. At 10,000 entries this fixture exceeds the index build budget, so it deliberately retains no index; the observed timing difference is not evidence of an improvement there. Reused versus fresh scratch timings vary by case; allocation reuse is retained without claiming a universal speedup.

## Validation and remaining release gates

The final release binary is **431,104 bytes (421 KiB)**, SHA-256 `8923D9273A42807B38519A52495C9991E8E563E51AEDD01A70F8E3C4374984DC`. All 13 Rust tests and strict Clippy checks pass. Windows integration and single-instance checks also pass for that binary.

| Final build observation | Measured result |
| --- | ---: |
| Actual discovered Start Menu targets | 294 |
| Private bytes, hidden after warm-up and 60.73-second capture | 2,166,784 bytes (2.07 MiB) |
| Working set at end of capture | 12,369,920 bytes (11.80 MiB) |
| CPU time accrued during capture | 0 at the process counter's resolution |
| Handles / threads, start and end | 180 / 5 |
| Hidden control pipeline, 1,000 calculator queries, p50 / p95 / p99 | 0.584 / 1.060 / 1.448 ms |
| Pipeline stress before/after private bytes | 2,158,592 / 2,158,592 |
| Pipeline stress before/after handles and threads | 180 / 5 in both snapshots |

The final idle capture is `measurements/native-final-hidden-60s.json`. Zero measured CPU in this short interval is not a guarantee of permanently zero CPU. The final exact binary still needs its own ten-minute and full planned reference-dataset gates. The pipeline result is a separate process and workload, with the limitations below; it is not keystroke-to-displayed-frame latency.

- Rust unit tests cover routing boundaries, normalization, arithmetic, exact/word/initial matching, ranking stability, cancellation, index/scan equivalence, bounded index fallback and URL encoding.
- Windows integration checks cover real Start Menu discovery and exact app queries, calculator results, errors clearing rows, command completion, preserved web payloads, 100 rapid edits followed immediately by Enter, unknown commands rejecting stale actions, search completion while the tray menu's nested message loop is open, Escape and orderly shutdown. A separate test proves that a second production instance exits and activates the existing window, which then hides on Escape.
- A separate 1,000-query test checks the path from `WM_SETTEXT` through the worker to native result-control text. It uses hidden controls and a 2 ms polling interval, includes harness overhead and does **not** measure visible paint, compositor latency or real keyboard input. Its before/after process counters are in `measurements/query-pipeline.json`.
- A screenshot of the app's own window is captured for visual review. No desktop or other-app capture is involved.

GUI action tests use `--dry-run`: app launching, clipboard writes and default-browser navigation are implemented but are not exercised by those automated checks. Manual acceptance must still cover those actions, actual hotkey-to-presented-frame latency, mixed DPI monitors, IME candidate handling, assistive technology, Explorer restart and long-running usage. All planned 2,000-app/200-quicklink/100-history-record memory gates remain provisional because quicklinks/history are not implemented.

The early ten-minute capture is of the complete native launcher **before the final trigram and paint changes**; its binary hash identifies that exact build. The final build receives a separate resource capture. Neither observation should be silently attributed to a different binary. Measurements report private bytes separately from working set, and CPU as a percentage of one logical processor. GPU memory is not captured. Measurements are on the user's desktop, with ordinary background activity rather than an isolated benchmark machine.

The pre-index capture ran for 600.26 seconds after warm-up, discovered 294 actual Start Menu targets, averaged 0.002603% of one logical CPU and ended at 2,281,472 private bytes (2.18 MiB) and 12,578,816 working-set bytes (12.00 MiB). Handles stayed at 180. Threads changed from five to eight and retained private bytes increased by 96 KiB during that sample, so these observations must not be described as absolutely zero growth or zero CPU.

## Reproduce

```powershell
cargo run --release --locked -p core-engine --example measure_search
.\scripts\test-windows.ps1
.\scripts\test-single-instance.ps1
.\scripts\measure-query-pipeline.ps1
.\scripts\measure-resources.ps1 -DurationSeconds 600 -Label native-catalog-hidden-repeat
.\scripts\capture-launcher.ps1
```

Build first and close an existing production Core v2 before resource measurement: production startup enforces one instance. Run desktop scripts in the interactive Windows account, not a service/sandbox account without Explorer. Memory captures warm the process for ten seconds, then sample once per second. The scripts close only their own test process. No system startup settings or existing Core data are changed.

## Next implementation milestones

1. Add explicit catalog refresh and notifications, packaged-app discovery, tray recovery and duplicate-name descriptions.
2. Add quicklinks, favorites and bounded usage persistence; construct settings only when opened.
3. Complete the real-display, IME, accessibility and action-adapter acceptance checks, then repeat the full reference-dataset performance gates.
4. Package/sign the app and provide opt-in startup and migration.

Line search, file indexing and the other deferred modules remain outside this launcher milestone.
