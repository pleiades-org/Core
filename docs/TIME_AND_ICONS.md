# Time conversion and application icons

Implemented on 2026-09-20. Validated release SHA-256: `A76ECAC59645F8A1A880F44015AE6535C40462E82276BB4E5293C2D2424E8AD3`.

## Time expressions

Use `9pm et to uk`, `9 pm ET to UK`, `21:30 et to uk`, or an explicit `@time` / `@tz` prefix. `@calc` also accepts this conversion syntax. Add `on YYYY-MM-DD` to select a date. Without a date, Core uses today's calendar date in the **source** zone. The result shows a day change, both calendar dates, and the original time. Enter copies the destination time, zone and date.

Supported regional aliases are case-insensitive:

| Region | Aliases |
| --- | --- |
| US Eastern | ET, eastern, NYC, new_york, America/New_York |
| US Central | CT, Chicago, America/Chicago |
| US Mountain | MT, Denver, America/Denver |
| US Pacific | PT, pacific, LA, America/Los_Angeles |
| UK | UK, London, Europe/London |
| Germany | Berlin, Europe/Berlin |
| Eastern Australia | Sydney, Australia/Sydney |
| Japan | Tokyo, JST, Asia/Tokyo |
| India | India, Kolkata, Asia/Kolkata |

UTC/GMT is a fixed zero offset. EST/EDT, CST/CDT, MST/MDT and PST/PDT explicitly mean the corresponding **US** standard/daylight offsets. BST explicitly means British Summer Time (+01:00). Use regional names for seasonal rules. Ambiguous `IST` is intentionally not guessed; use `India` or another supported unambiguous name. This is a finite alias list, not arbitrary IANA-name support or unrestricted natural-language parsing. `tomorrow`, seconds and numeric offsets are not implemented.

The engine parses bounded input and dispatches through enums. A small platform adapter loads nine Windows zone identities lazily and uses date-specific Windows rules. No new dependency or online service was added. Both possible UTC offsets are round-tripped against the source local time: a spring gap rejects the query, and an autumn overlap requires an explicit seasonal abbreviation. This avoids silently selecting one occurrence of a repeated time. The Windows APIs and the limitations of implicit repeated-time conversion are documented in [Microsoft's local-time guidance](https://learn.microsoft.com/en-us/windows/win32/sysinfo/local-time).

Examples covered by deterministic tests:

| Input | Result |
| --- | --- |
| `9pm et to uk on 2026-01-15` | 2:00 am UK on Jan 16 |
| `9pm et to uk on 2026-07-15` | 2:00 am UK on Jul 16 |
| `9pm et to uk on 2026-03-10` | 1:00 am UK on Mar 11 |
| `9pm et to uk on 2026-10-28` | 1:00 am UK on Oct 29 |
| `2:30am et to uk on 2026-03-08` | Rejected: skipped local time |
| `1:30am et to uk on 2026-11-01` | Rejected: repeated local time |

Calendar validation accepts 1900–2100, including leap years. Historical completeness and future legislative changes depend on the Windows time-zone database; accepting a year does not certify historical rules for every date. Core does not implement its own historical rule database. No new time-query latency benchmark was recorded.

## Application icons

Actual application icons are extracted from Start Menu targets on a separate, lazily COM-initialized worker. Only visible results request icons; neither search nor painting performs icon I/O. The worker has one replaceable pending batch and a 64-entry LRU cache, including failed lookups. Idle waits on a condition variable. Hidden startup does not load Shell icon infrastructure. The renderer uses a generic symbol until an icon is available, or permanently if lookup fails.

The loader reads the Shell's system image-list entry and owns a copy of the base icon without shortcut-arrow overlays. The shared system list is never modified or destroyed. Each owned icon is released with `DestroyIcon`; references keep currently displayed icons alive through cache eviction. Windows recommends background execution and COM initialization for [SHGetFileInfoW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shgetfileinfow); ownership of copied icons follows [ImageList_GetIcon](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-imagelist_geticon).

The 64-entry bound applies to Core's cache. Visible rows and the completion mailbox can retain additional references, and Windows maintains its own Shell caches. Cold lookup time depends on the application and its Shell extension. A currently executing Shell call cannot be canceled; obsolete pending requests are replaced and stale batches are discarded. Shutdown does not wait indefinitely for a stalled Shell extension. Missing or generic icons supplied by Windows still use the normal fallback behavior. Mixed-DPI icon sharpness needs further acceptance testing.

The visual check also found a discovery-path issue: mixed forward/backward separators worked for filesystem enumeration but failed Shell icon lookup. Start Menu paths now use native Windows separators for both icon and launch targets.

## Verification and resources

- 21 Rust tests passed: 16 engine tests and five Windows adapter/resource tests. Coverage includes source-zone date selection, US/UK DST gaps and overlaps, mismatch weeks, previous/next day and year boundaries, half-hour offsets, scoped/implicit routing and invalid input.
- The icon resource test exercised 192 distinct cache keys, verified repeated-key handle reuse, preserved a displayed icon through eviction, and returned the process USER-handle count to baseline after releasing all icons.
- `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` and formatting checks passed.
- Native [integration checks](measurements/time-icons-integration.json) passed, including rapid edits followed immediately by Enter and invalid time queries clearing old actions. External launches and clipboard writes were disabled by `--dry-run`; this run verifies result/action routing, not those external side effects.
- [Animated](measurements/time-icons-styling.json) and [reduced-motion](measurements/time-icons-reduced-motion.json) checks passed. Both retained 19 GDI objects across the animation/geometry exercise. The reduced-motion harness was corrected to force its initial paint before sampling resources; otherwise native font initialization could be mistaken for a leak.
- Visual inspection confirmed [time conversion](measurements/time-preview.bmp) and [actual app icons](measurements/app-icons-preview.bmp), preserving OLED black, rounded corners and borderless rows.

The [resource run](measurements/time-icons-apps-idle.json) used 294 real Start Menu entries. It started hidden, showed the `code` query, allowed three seconds for icon loading, then hid the window and sampled for 30.39 seconds. Final private bytes were 5,869,568 (5.60 MiB); working set was 27,099,136 (25.84 MiB). The process CPU counter did not increase during that interval. These observations do not imply literally zero work below the counter's resolution or bound memory after every possible Shell extension. The earlier 2.39 MiB build lacked actual icons and used a different warm-up; it is context, not a controlled attribution of every byte to this change.
