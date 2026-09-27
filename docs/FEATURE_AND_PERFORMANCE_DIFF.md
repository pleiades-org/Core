# Core: feature and performance comparison

This comparison is the historical click-away milestone snapshot. The [21 September release](RELEASE_2026_09_21.md) adds an expanded calculator, power menu, configurable shortcuts, Windows-key interception, startup and display settings, seven positions and packaged apps. Measurements below retain their original build identities; pending-feature statements describe that older milestone.

Updated 20 September 2026. This compares the old project at `C:\Users\Robert\code\Rust\core` (audited HEAD `753fb6b`, including its existing working-tree changes), the v2 release plan, and the implemented native v2 alpha. The old working tree has not been modified. Line search is excluded from the implementation scope.

**V2 is a working smaller launcher, not yet the complete planned first release.** Application search, arithmetic, explicit web search, regional time conversion, app icons, the OLED UI and click-away dismissal are implemented. Saved quicklinks, favorites/history, settings, catalog refresh and release hardening remain unfinished.

## Feature differences

“Implemented” means present in the source and built executable; verification limits are listed below. Old-feature entries describe source coverage, not a certification that every old feature works correctly.

| Area | Old Core | V2 implemented now | Proposed / remaining |
| --- | --- | --- | --- |
| Application search and launch | Application catalog, scoped and general search, Windows launch actions | Background Start Menu discovery; exact, prefix, word-prefix, initials and substring matching; eight rows; stable launch IDs | Direct packaged-app enumeration and fuller duplicate-name disambiguation |
| Application icons | Icon lookup during discovery; rendering also had asset-processing paths | Actual native app icons loaded off the UI/search thread, only for visible results; 64-entry LRU; generic fallback | Source-change/DPI invalidation and complete long-run resource gates |
| Query commands | Typed enums already existed; parser searched/allocated tokens for embedded scopes | Borrowed prefix parsing, stack ASCII command normalization, enum + exhaustive `match`; incomplete-command hints | Configurable aliases; embedded-scope compatibility intentionally not retained |
| Arithmetic | Broader calculator dispatcher with many calculation families | Local arithmetic, parentheses, scientific notation, powers, unary signs, postfix percentages and `% of`; Enter copies | Additional mathematical functions and richer calculation families |
| Time conversion | Broader time/date and natural-language providers | `9pm et to uk`, `@time`, `@tz`, time expressions through `@calc`, optional `on YYYY-MM-DD`; regional DST and invalid/ambiguous time handling | `tomorrow`, arbitrary city/IANA names, numeric offset syntax and broader natural-language dates |
| Time-zone coverage | Existing time-related providers | ET/CT/MT/PT, UK/London, Berlin, Sydney, Tokyo, India, UTC/GMT and documented seasonal abbreviations | Complete world-time database/grammar; historical accuracy remains bounded by Windows rules |
| Units, currency, finance, programmer tools, date arithmetic | Implemented provider families | Not ported into this launcher | Deferred; add separately only when requested and measured |
| Web search | Search scopes and fallback behavior | Explicit `@web`, preserved/encoded query, default-browser action | Configurable search providers; automatic web fallback is not present |
| Saved quicklinks | Persistent quicklinks and `>` entry point | Prefix recognized, with an honest “settings unavailable” response | Quicklink storage, editor, matching and execution: planned first-release work |
| Favorites and recent launches | Pins, launch counts and persisted recent list | Ranking types support pin/count signals, but discovered apps currently have `false` / `0`; no user-facing persistence | Pin/unpin UI, bounded history, clear/disable controls and successful-launch recording |
| Settings and startup | Configurable settings, hotkey and startup integration; default Alt+Space | Fixed Ctrl+Alt+Space; CLI motion options; no automatic startup registration | Settings UI, configurable shortcut/theme and opt-in launch at login |
| Catalog lifecycle | Startup performs indexing before opening the launcher | Shell/hotkey first; one background discovery pass with bounds | Versioned startup cache, manual refresh, directory notifications, overflow recovery and resume reconciliation; currently restart after installing apps |
| Typo tolerance | No general typo engine established by the audit | Initials matching is shipped; the experimental typo fallback is not | Late, bounded typo fallback after broader relevance/latency validation |
| Query scheduling / Enter | Debounced UI update; old cached result handling | Immediate worker submission, one replaceable pending query, cooperative cancellation, generation-safe Enter and stable selection | More soak and platform acceptance testing |
| Hotkey, tray, single instance | Existing features, including polling bridges | Direct Windows messages and single-instance activation | Explorer-restart tray recovery and friendlier hotkey-conflict settings |
| Click away | Existing focus-loss behavior, observer plus 100 ms polling and initial grace period | Added now: foreground-change subscription while visible; outside activation starts the existing hide transition; input/results remain usable | Owned-dialog acceptance if dialogs are added; see test limitations |
| Appearance and motion | GPUI UI, near-black surfaces, borders and optional blur | Rounded, borderless, pure-black surfaces; selected row #121212; 130 ms entrance / 100 ms exit; fades enabled independently of Windows | Theme choice in settings; wider DPI/accessibility acceptance |
| Files and content / line search | File/content modules existed, with issues recorded in the audit | Absent | Outside this task and this launcher release |
| Clipboard history, emoji, notes, snippets, focus, calendar | Broad productivity feature set | Absent | Deliberately deferred rather than silently running disabled workers |
| Terminal, media, system/window controls, Git/GitHub, developer/network/process tools | Broad tool/provider set | Absent | Deliberately deferred; no speculative network/process provider fan-out |
| Data storage and migration | Existing settings/quicklinks/usage stores; synchronous/direct-write concerns | No new quicklink/settings/history store or migration | Versioned stores, atomic replacement, recoverable errors and opt-in import |
| Installer and release readiness | Installer/startup code exists | Local executable and build/test scripts | Signing, installer/update/uninstall, migration, full accessibility/IME/DPI/sleep-resume validation |

Scope details: [old source audit](../CURRENT_CORE_AUDIT.md), [v2 release plan](../CORE_V2_PLAN.md), [current README](../README.md), [time and icon behavior](TIME_AND_ICONS.md). The plan's original deferral of time zones was superseded by the subsequent user request; the rest of its unimplemented milestones remain proposals.

## Performance: implemented changes and evidence

**There is no controlled, feature-equivalent old-Core versus v2 full-application benchmark yet.** Do not interpret the following construct, engine or shell-probe figures as a whole-application speedup or RAM reduction. Earlier benchmark artifacts retain their own dates/build identities.

Measurements were made on this Windows desktop (Ryzen 7 7800X3D, 16 logical processors, Windows build 26200), in release builds with ordinary background activity. Times are observations, not universal bounds. Private bytes, working set and retained index storage are different quantities.

| Construct / workload | Previous or comparison result | Implemented result / proposed change | Status and interpretation |
| --- | --- | --- | --- |
| Command routing microbenchmark | Owned legacy-style parsing: 133.75 ns median, 3.5 allocation calls/op | Borrowed-prefix prototype: 15.63 ns, zero allocations | Borrowed-prefix + enum design implemented; roughly 8.6× in this isolated fixture. Grammar differs, and this is not a benchmark of today's entire search path |
| Fixed command lookup | HashMap 10.28 ns; linear table 2.69 ns | `match` 2.61 ns | Static dispatch implemented. Old Core already used enums; this does **not** establish a previous HashMap bottleneck |
| Already canonical ASCII normalization | Owned transform 96.08 ns, three allocation calls | Borrowed path 18.27 ns, zero allocations | Implemented; approximately 5.3× for this fixture only |
| Mixed ASCII normalization | Owned 100.89 ns | Reused-buffer prototype 26.93 ns | **Tested, not shipped.** Production currently allocates its noncanonical fallback; Unicode still uses whole-string normalization |
| Ranking 12 from 2,000 candidates | Legacy cloned-comparison partial selection 226.59 µs | Borrowed partial selection 4.37 µs | Borrowed/prepared comparisons and reused scratch implemented. Old code already used partial selection; this gain is not “full sort replaced.” Microbenchmark selects 12; production UI returns eight |
| Ranking alternative | Bounded heap 2.61 µs on mixed 2,000-candidate order | Partial selection retained for small app catalogs | Heap deteriorated to 45.72 µs in worst-first ordering versus partial selection 3.56 µs. No universally fastest construct was established |
| App engine, 2,000 synthetic apps, median | Initial v2 scan 121.9 µs | Bounded index 3.3 µs | Implemented; 36.9× median in the historical same-harness run, **v2 scan versus v2 index**, not old Core versus new Core |
| Same engine, p95 / p99 | Scan 193.1 / 234.8 µs | Index 126.1 / 196.0 µs | p95 decreased about 34.7%. Short/broad queries still scan and dominate the tail |
| Same index storage | Scan: no index | 190,216 retained bytes (185.8 KiB) | Additional memory excludes prepared catalog, paths, allocator overhead and query scratch |
| App engine, 10,000 synthetic apps, p95 | Scan 933.5 µs | Budget-triggered scan fallback 1,010.6 µs | About 8.3% slower in this observation. No retained index; do not claim every dataset improved |
| Local search debounce | Old source adds 28 ms before ordinary query handling | No intentional debounce; work begins on the search worker | Implemented architectural removal. It is not proof that presented results are exactly 28 ms faster |
| Hotkey / focus / tray | Old source polls at 40 / 100 / 120 ms respectively | Windows messages; visible-only foreground subscription; blocking idle | Implemented. No comparable old process CPU capture exists, so percentage savings are unknown |
| Obsolete query work | Existing debouncing/cached results | One running plus one replaceable pending request, generation checks and cancellation checkpoints | Implemented bound; prevents an ever-growing queue. Not a latency benchmark |
| Icon work | Eager discovery lookup | Visible-result requests on a separate sleeping worker, 64 retained cache entries | Implemented. Count bound replaces the plan's proposed 8 MiB policy; it is not a strict cap on all Shell/process memory |
| Calculator pattern compilation | Isolated compile+match: 228.71 µs and 1,937 allocations; reused pattern: 19.88 ns | Current arithmetic/time grammar uses direct parsing, no regex compilation | Regex reuse is an experiment relevant to future ported grammars, not a claimed shipped regex-cache optimization |
| Minimal GUI shell experiment | Minimal GPUI probe: 64.89 MiB private, hidden | Minimal native probe: 1.24 MiB | Native shell selected and implemented. These are minimal prototypes, **not full old/new application footprints** |
| Discovery / storage | Old startup waits for indexes; usage writes occur in the action path | Background discovery shipped | Catalog cache and background atomic usage persistence are still proposed; absent persistence is not a completed persistence optimization |

Raw construct results: [summary CSV](../benchmarks/constructs/results/summary.csv). Search/algorithm context: [follow-up experiments](../CORE_SEARCH_EXPERIMENTS.md). Production engine comparison: [scan CSV](measurements/engine-scan-search.csv), [indexed CSV](measurements/engine-search.csv). The Search-inspired rarest-trigram candidate filter is implemented locally; the entire upstream Search runtime was not embedded.

## Current executable observations

Candidate and published-build identity: SHA-256 `04097A9B5F0276B8F283A6AFC21E39F69337A8EBE01DD283175E0EE35FB8E62D`, executable size **509,440 bytes (497.5 KiB)**. These observations apply to that exact build. The previous time/icon build was 507,392 bytes; click-away adds 2,048 bytes to the executable in this build configuration.

| Measurement | Earlier recorded observation | Current build observation | Meaning / limitation |
| --- | --- | --- | --- |
| Hidden private memory after an application-icon query | Previous time/icon build: 5.60 MiB | **5.46 MiB** (5,722,112 bytes) | Separate short process samples; the small difference is not an established optimization |
| Same working set | Previous time/icon build: 25.84 MiB | **23.67 MiB** (24,821,760 bytes) | Working set is OS-managed; not interchangeable with private memory |
| Hidden idle CPU | Earlier 30-second capture: no counter increase | **0.0515% of one logical CPU** over 30.33 seconds (0.015625 CPU seconds) | Short sample and coarse counter; not zero CPU and not the final ten-minute release gate |
| Resource stability in that sample | Different build/process | Private bytes unchanged; handles 350 → 352; threads 11 → 11 | Two added handles mean this is not proof of zero handle growth; long soak still required |
| Hidden calculator control pipeline, 1,000 changing queries | Initial alpha p50 / p95 / p99: 0.584 / 1.060 / 1.448 ms | **0.272 / 0.594 / 1.064 ms**; maximum 14.069 ms | Separate runs; no causal speedup assigned to click-away. Includes harness/scheduling, excludes keyboard and visible presentation |
| Pipeline process before / after | — | Private bytes 2,260,992 unchanged; handles 175 unchanged; threads six unchanged | Hidden calculator-only workload; icons have not been warmed, so do not compare its footprint to the icon-warmed sample |
| Outside activation → fully hidden, fades enabled | No v2 click-away before this change | 12 samples: median **134.9 ms**, maximum **136.4 ms** | Includes test activation overhead, 100 ms exit animation and 2 ms observation polling; not hardware-click latency |
| Same, reduced motion | — | 12 samples: median **17.3 ms**, maximum **76.0 ms** | No animation delay; test/Windows activation dominates. No universal immediate-input timing guarantee |
| Styling/reversal resource check | — | GDI objects **19 → 19**, 20 interrupted/reversed cycles | Does not replace 1,000-cycle memory/handle/GPU soak |

Current records: [idle resources](measurements/click-away-apps-idle.json), [query pipeline](measurements/click-away-query-pipeline.json), [animated dismissal](measurements/click-away-animated.json), [reduced-motion dismissal](measurements/click-away-reduced.json), [styling](measurements/click-away-styling.json). The idle sample used 294 real Start Menu entries and a `code` query before hiding. It was collected on an active desktop, including other test activity, rather than an isolated benchmark host. It does not prove all icon-cache entries were filled.

## Proposed gates that are not yet completed

| Gate from the release plan | Evidence so far | Still required |
| --- | --- | --- |
| Hidden CPU ≤0.1% of one core, ten minutes | Current short sample 0.0515%; an older pre-index build also had a ten-minute capture | Repeat ten-minute capture on the final feature-complete binary |
| Hidden ≤50 MiB / visible ≤75 MiB private | Current small real catalog is comfortably below hidden target | Full 2,000-app / 200-quicklink / 100-usage dataset, visible/peak catalog replacement and GPU measurements; two features do not yet exist |
| Engine p95 ≤5 ms / p99 ≤10 ms | Synthetic 2,000-app run: 0.126 / 0.196 ms | Representative complete reference data and several independent runs |
| Keystroke → correct presented results p95 ≤35 ms / p99 ≤70 ms | Hidden control pipeline measured | Real keyboard, paint/compositor and display timing; hidden-control timing is not a substitute |
| Hotkey → first presented/focused frame p95 ≤50 ms / p99 ≤100 ms | Functional show/hide tested | At least 200 activations across runs; the 130 ms fade-to-full-opacity duration is a different metric |
| Cached startup → hotkey-ready p95 ≤500 ms | Shell initializes before background discovery | Implement cache and measure 30 fresh launches; no startup latency claim yet |
| 1,000 cycles, retained growth ≤5 MiB, stable handles/GPU | 12 dismissal cycles per motion mode; 20 animation reversals | Full soak, icon eviction/reload, catalog replacement and GPU accounting |
| Arithmetic p95 ≤100 µs; richer local calculations ≤500 µs | Pure tests and aggregate hidden query timings | Dedicated current-provider measurements; these remain budgets |

## Click-away implementation and validation

The launcher registers an out-of-context `EVENT_SYSTEM_FOREGROUND` observer only while logically visible. The callback posts a small message to the UI thread; it never borrows mutable launcher state reentrantly. The UI validates the observer, ignores activation notifications for Core/its owned window tree, and rechecks current foreground before hiding. This also avoids treating Core's own entrance event as a click outside. Hide unregisters the observer, cancels outstanding presentation requests and uses the existing fade. There is no mouse polling or permanent hidden-window subscription, and no new dependency: one feature of the existing Windows bindings was enabled.

Passed for this build: 21 Rust tests, strict Clippy, native query/DST/stale-Enter integration, 1,000 changing hidden calculator queries, 12 external-activation cycles in each motion mode, result-control focus, preserved query on reopen, interrupted entrance/reopen, and 20 styling reversals. [Integration record](measurements/click-away-integration.json).

The foreground harness sometimes needs temporary input-queue attachment or a hit-checked click to obtain Windows foreground permission. Earlier iterations exposed a real activation-notification ordering issue, now fixed, as well as unstable test activation. A separate cross-process owned-popup fixture was excluded from the final focused test: Windows attaches the processes' input queues and the fixture did not reliably retain focus. **Owned-dialog behavior is not certified**; Core currently has no such dialog. This is not an exhaustive foreground/IME/accessibility test. External application launching, clipboard writes and browser navigation remain disabled in automated `--dry-run` checks.

## Remaining work in priority order

| Priority | Work | Benefit |
| --- | --- | --- |
| 1 | Catalog refresh/manual reload, packaged-app support and Explorer tray recovery | Correctness after app installs and desktop lifecycle changes |
| 2 | Quicklinks, favorites/recent persistence and essential settings | Completes the main missing first-release workflows |
| 3 | Atomic storage, recovery and opt-in migration | Durable settings without UI-path writes or old-data loss |
| 4 | Presented-frame latency, full dataset and long resource soak | Establishes whether the proposed performance gates actually pass |
| 5 | IME/accessibility/mixed-DPI acceptance, signing and packaging | Makes the alpha suitable for reliable everyday release |

The original 15–25 focused engineering-day estimate was a planning range, not a measured delivery time or a remaining-work forecast. No fresh schedule is inferred from benchmark nanoseconds. Further optimizations should target measured remaining costs: broad-query tails, larger catalogs and resource stability; the numbers above do not justify claiming “fastest in every situation.”
