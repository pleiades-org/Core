# Core v2: AI prompts from the 27 September 2026 audit

Each block below is one self-contained prompt for a coding AI opened in this folder. Findings come from a read-only audit of all 25k lines. Top items were verified by reading the code; the Steam and `.url` launch behaviour was verified with a standalone probe. Line numbers are approximate, so every prompt also quotes the code to look for.

## How to use

1. Once, before the first prompt: this folder is not a git repository. Run `git init; git add -A; git commit -m "Baseline before audit fixes"` so every AI change can be reviewed and reverted.
2. Copy one whole prompt block into a fresh AI session.
3. Review `git diff`, then commit before the next prompt.
4. Work top to bottom inside a category. Respect **Depends on** notes.

## Index

| ID | Prompt | Time | Kind |
| --- | --- | --- | --- |
| **Quick** | *≤ 45 min each, one area, low risk* | | |
| Q1 | Calculator and calendar correctness (`e` shows 2.718…) | 30 min | Bug |
| Q2 | Number base and search normalisation correctness | 30 min | Bug |
| Q3 | Release profile: LTO (−102 KiB measured) | 10 min | Size |
| Q4 | Win32 and COM correctness hygiene | 40 min | Bug |
| Q5 | Terminal correctness | 45 min | Bug |
| Q6 | Skip redundant UI updates | 45 min | Perf |
| Q7 | Network hardening | 45 min | Security |
| **Moderate** | *1–3 h each, several functions, new tests* | | |
| M1 | **Steam and app-protocol quicklinks (your reported bug)** | 2 h | Feature/Bug |
| M2 | Converter fast path (about 45 allocations → 0 per keystroke) | 1.5 h | Perf |
| M3 | Engine ranking fixes and a truthful benchmark | 2.5 h | Perf |
| M4 | Settings page efficiency | 2 h | Perf |
| M5 | Main window repaint efficiency | 2 h | Perf |
| M6 | Terminal throughput | 3 h | Perf |
| M7 | Terminal work off the UI thread (WSL/UNC freeze) | 1.5 h | Bug/Perf |
| M8 | Favicon network behaviour | 2 h | Perf |
| M9 | Time-zone rule caching | 1.5 h | Perf |
| **Complex** | *half a day or more, threading or data layout, needs measurement* | | |
| C1 | Take the registry off the show path | ½ day | Perf |
| C2 | Split favicons from app icons and add a UI icon cache | ½ day | Perf |
| C3 | Search index scaling to 10k apps | ½–1 day | Perf |
| C4 | One catalog instead of two | ½ day | Perf/Memory |

---

## Quick

### Q1 · Calculator and calendar correctness (≈30 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies; crates/launcher is native Win32 via the windows 0.61.3 crate. Search runs on every keystroke. Line numbers below are approximate; locate code by the quoted snippets.

Task: fix these five calculator/calendar correctness bugs. Only do these items.

1. Bare constants hijack search. In crates/engine/src/calculator/evaluate.rs, CalculatorEngine::recognizes (~lines 35-52) returns true when the whole query is `e`, `pi` or `tau` (any case), because `constant(input.as_bytes()).is_some()` sets `named_start`. crates/engine/src/search/execute_search.rs (~167-169) then shows a calculator row instead of apps, so typing `e` (Edge, Excel, Explorer) shows 2.718… and Enter copies it. Fix: in `recognizes`, return false when the trimmed input consists only of ASCII letters. Tests: `e`, `E`, `pi`, `tau` are not recognized as calculations in plain search; `pi*2`, `e^2`, `2*pi`, `sqrt(81)` still are; `@calc pi` still evaluates (the @calc path falls back to `calculate()` around execute_search.rs ~177-179).

2. `25% OF 80` fails. `recognizes` accepts "of" case-insensitively (~line 67, `name.eq_ignore_ascii_case(b"of")`) but evaluation only accepts lowercase (~line 149, `b"of"`). Make evaluation case-insensitive too. Test: `25% OF 80` = 20.

3. Long calendar-looking queries show an error. crates/engine/src/calculator/calendar/parse.rs (~37-45) returns `Some(Err(CalendarError::Syntax))` for more than 8 words before the shape check (~47), so any 9+ word query starting with date/time/today/now/next/last/in shows "Try 2 days from now…" instead of apps. On overflow return None unless the first word is a date (`starts_date`). Test: `next time i open the launcher please show me my apps` is not a calendar result.

4. NaN output. crates/engine/src/conversions/format.rs `significant()` (~41-48), and `readable()` if it has the same pattern: for |x| below ~1e-297, `10_f64.powi(...)` overflows to infinity and inf/inf = NaN, so `1e-300 m to km` shows "NaN km". Return the value unchanged when the scale factor is not finite. Test it.

5. Debug-build panic. crates/engine/src/conversions/timestamp.rs (~41-42) calls `value.abs()`, which panics on i64::MIN in debug builds. Use `value.unsigned_abs()` against the threshold as u64. Test: `unix -9223372036854775808` does not panic.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Every behaviour change gets a unit test (normal case plus edge case). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### Q2 · Number base and search normalisation correctness (≈30 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies; crates/launcher is native Win32 via the windows 0.61.3 crate. Search runs on every keystroke. Line numbers below are approximate; locate code by the quoted snippets.

Task: fix these three correctness bugs. Only do these items.

1. Wrong decimal display above 2^53. crates/engine/src/conversions/number_base.rs (~112-124) formats with `grouped(number as f64, 0)`. `0xFFFFFFFFFFFFFFFF` shows the title 18,446,744,073,709,551,616 while the copied value is …615; `9007199254740993 to hex` shows 9,007,199,254,740,992 in the detail. Add an integer digit-grouping helper working on `u64::to_string()` and use it for the title and detail. Tests for both examples.

2. Misplaced commas accepted. The same file (~145-151) does `text.replace(',', "")`, so `1,00 to hex` becomes 100. This contradicts the rule documented in crates/engine/src/conversions/quantity.rs (~3-4) that a typo must never become a different amount. Reuse quantity.rs's comma-grouping validation before stripping commas. Tests: `1,00 to hex` rejected; `1,000 to hex` accepted.

3. Greek final sigma. crates/engine/src/search/normalize.rs (~14) uses `to_lowercase()`, which applies the final-sigma rule: `ΣΊΣ` becomes `σίς` and never matches a stored `σίσυφος`. After lowercasing, map `ς` to `σ`. normalize is used for both catalog names and queries, so the index and the scan path stay consistent. Keep the fast path that returns the borrowed input unchanged for clean ASCII. Extend the existing `"editor ΣΊΣ"` test so it asserts an actual match, not just index/scan agreement.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Every behaviour change gets a unit test (normal case plus edge case). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### Q3 · Release profile (≈10 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies; crates/launcher is native Win32 via the windows 0.61.3 crate.

Task: the root Cargo.toml has no [profile.release], so release builds use no LTO and 16 codegen units. Add:

[profile.release]
lto = "fat"
codegen-units = 1

Measured on 2026-09-27: core-v2.exe 1,455,616 → 1,351,168 bytes (−7.2%), build 30 s → 36 s.

Do NOT set panic = "abort". Worker threads are joined and their panics handled: crates/launcher/src/windows/icon_worker.rs (~130, `filter(JoinHandle::is_finished)` then `join().is_err()`), search_worker.rs (~136) and activation/windows_key.rs (~125). With abort, any worker panic would close Core. Do not change CRT linkage either.

Then run `cargo build --release --locked -p core-launcher-v2 --bin core-v2` and report the new size of target\release\core-v2.exe. Mention in your report that the next release must re-run .\scripts\run-release-checks.ps1: scripts\package-release.ps1 only accepts a candidate whose SHA256 matches passing validation records. Do not edit the scripts.

Rules: Make the minimum change. Finish with `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; both must pass. Report the change and the command results.
```

### Q4 · Win32 and COM correctness hygiene (≈40 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies; crates/launcher is native Win32 via the windows 0.61.3 crate, with one UI thread plus search, icon, discovery and settings-save worker threads. Line numbers below are approximate; locate code by the quoted snippets.

Task: fix these four Win32 correctness issues. Only do these items.

1. COM teardown order. crates/launcher/src/windows/app_aliases.rs: `ShortcutReader` (~51-56) holds `link: IShellLinkW` and `file: IPersistFile`, and its `Drop` (~97-103) calls `CoUninitialize()`. Rust drops the fields only after `drop()` returns, so the interfaces are Released after the apartment is torn down. This is the discovery thread's last CoUninitialize, which breaks COM's rules. Release both interfaces first (for example ManuallyDrop fields dropped explicitly inside drop(), or Option + take()), then uninitialize.

2. The UI thread never initializes COM. No CoInitializeEx runs on the UI thread, yet it calls ShellExecuteW, ShellExecuteExW and SHGetKnownFolderPath, and Microsoft documents that ShellExecute can delegate to COM shell extensions. In crates/launcher/src/windows/shell.rs `run()`, before any window is created, call `CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` once. Hold a guard that calls CoUninitialize only after all windows and COM objects owned by the UI thread are gone. Treat S_FALSE as success and RPC_E_CHANGED_MODE as "already initialized, do not uninitialize". Context: a probe on 2026-09-27 showed .url (Steam) launches already work without COM, so this is hardening, not a fix for a known failure.

3. Undefined behaviour in WM_NOTIFY. shell.rs (~749) does `&*(long.0 as *const NMCUSTOMDRAW)` for every WM_NOTIFY, but many notifications only carry an NMHDR. Read `&*(lparam as *const NMHDR)` first and cast to NMCUSTOMDRAW only when `code == NM_CUSTOMDRAW`.

4. Pointless shortcut parsing. crates/launcher/src/windows/discover_applications.rs (~64-68) calls `reader.program_alias(&path)` for every discovered file, including .exe, .url and .appref-ms, where `IPersistFile::Load` on a ShellLink always fails after opening the file. Only call it when the extension is .lnk (case-insensitive).

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Add a unit test wherever behaviour is testable (item 4 at least). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### Q5 · Terminal correctness (≈45 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate. It has an embedded ConPTY terminal (`/ipconfig /all`, `@pwsh Get-Process`); see docs/COMMANDS.md. Line numbers below are approximate; locate code by the quoted snippets.

Task: fix these three terminal bugs. Only do these items.

1. Scrollback drift. In crates/launcher/src/windows/view/console/mod.rs (~228-230 and ~324-332) and commands/terminal/screen.rs (~304-307), `top` is an absolute line index. Once the 5,000-line scrollback is full, each new line calls pop_front and increments `dropped`, but `top` is never reduced to match. A scrolled-back view therefore drifts while output streams, and `if dropped != self.dropped.replace(dropped) { self.selection.set(None) }` clears the selection on every chunk. Compute `delta = dropped - previous`. When not following output, set `top = top.saturating_sub(delta)`. Shift the selection's anchor and head up by delta, and clear the selection only once it falls off the top. Add a unit test at the Screen/console-model level.

2. Emoji typed into finished output reach the search box broken. view/console/keyboard.rs (~81-91): after a high and low surrogate are combined, the not-running branch forwards only the low surrogate via `SendMessageW(prompt, WM_CHAR, WPARAM(unit as usize))`. Forward the whole character (both UTF-16 units, in order).

3. A command can be orphaned. commands/capture.rs (~150-183): `ResumeThread` runs, with its result ignored, before the two `thread::Builder::spawn` calls. If either spawn fails, `start` returns Err while the child is running. The job handle is then closed without JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, so the process is orphaned and Esc cannot stop it. Spawn both threads before resuming, or call TerminateJobObject on those error paths. Also treat `ResumeThread` returning u32::MAX as an error that terminates the job.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Every behaviour change that can be unit-tested gets a test. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### Q6 · Skip redundant UI updates (≈45 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate, with one UI thread. Design goals: hidden idle does no work; the hotkey-to-first-frame budget is 50 ms p95; 130 ms fade-in, 100 ms fade-out, `--reduced-motion` shows immediately. Line numbers below are approximate; locate code by the quoted snippets.

Task: remove these redundant UI updates. Visible behaviour must not change. Only do these items.

1. Footer text. crates/launcher/src/windows/view.rs `set_footer` (~564-569) calls SetWindowTextW on every keystroke, arrow press and command-output chunk, even when the text is unchanged. Each call repaints the footer and fires EVENT_OBJECT_NAMECHANGE, so screen readers may re-announce it. Store the last text in View and skip the call when it is equal. Callers include launcher_state/footer.rs (~23) and launcher_state/command_flow.rs (~171-173, "Esc to stop" on every chunk).

2. Console scroll bar. view/console/mod.rs `update_scrollbar` (~348-361) calls `SetScrollInfo(..., true)` plus two line_count scans on every output chunk. Cache the last (count, page, pos) and skip when it is unchanged.

3. Console drag-select. view/console/mod.rs `extend_selection` (~414-433) invalidates the whole view on every WM_MOUSEMOVE. Invalidate only when the selection head actually changes.

4. Work while hidden. shell.rs (~659-673): WM_SETTINGCHANGE and WM_DISPLAYCHANGE run `position_on_monitor` (full relayout, SetWindowRgn and RedrawWindow) even while Core is hidden. Skip the relayout when hidden but keep the tray theme refresh. Confirm the next show still repositions: set_visible(true) → position_on_monitor, around launcher_state.rs ~425-431 and view.rs ~495-512.

5. First fade frame is invisible. motion.rs (~282-289) sets alpha 0, then ShowWindow; the first visible alpha only arrives with the first WM_TIMER at least ~16 ms later. Apply the first eased step immediately (the alpha for one frame interval) and call UpdateWindow right after ShowWindow so the content paints synchronously. `--reduced-motion` and `--system-motion` must behave as before.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Add unit tests where logic is testable (the change-detection caches). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. If pwsh is available, also run `.\scripts\run-release-checks.ps1` (background mode) after `cargo build --release --locked -p core-launcher-v2 --bin core-v2`; otherwise say you did not. Report each change with file:line, the tests you added, and the command results.
```

### Q7 · Network hardening (≈45 min)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate. The network is used only for website-quicklink favicons (Google s2 service, falling back to the site's /favicon.ico, cached 30 days) and the daily ECB exchange-rate file. Private hosts (localhost, IPs, intranet names) are never sent to Google. `--dry-run` never contacts websites. Line numbers below are approximate; locate code by the quoted snippets.

Task: harden these network paths. Only do these items.

1. No overall time limit. crates/launcher/src/windows/http.rs (~9-10, 56-62, 111-138) sets only per-step WinHTTP timeouts, so a server sending 1 byte every 4.9 s keeps the single icon thread busy indefinitely. Add a total deadline parameter to `get` (10 s for favicons; keep exchange rates working with a generous deadline such as 30 s) and check it in `read_body`. Query WINHTTP_QUERY_CONTENT_LENGTH and reject a declared length above max_bytes before reading.

2. Redirects bypass the private-host rule. http.rs (~71-91) uses WinHTTP's default redirect policy (follows up to 10 redirects to any host, blocking only HTTPS→HTTP), so a public site's /favicon.ico can redirect Core to a LAN address. Set WINHTTP_OPTION_REDIRECT_POLICY to WINHTTP_OPTION_REDIRECT_POLICY_NEVER. Handle at most one redirect manually, after re-validating the Location target with the same private-host rules favicon.rs applies, or reject redirects to private hosts.

3. Non-atomic favicon cache. favicon.rs (~94-96, 115) writes cache files with `fs::write(&path, cached)` in place. A truncated or corrupt non-empty file then counts as fresh for 30 days, and `load` returns `decode(&bytes)` = None without fetching again. Write to a temp file and rename, as exchange_rates.rs does (~195-197). If a cached file fails to decode, delete it and fall through to fetching.

4. Disk access on the UI thread at every show. exchange_rates.rs `refresh()` (~79-83) calls `cache_age(&path)` (fs::metadata) on the UI thread while holding the mutex. Core is the only writer, so store the last-known download/load time in `Shared` (set when the cache is loaded or a download is saved) and decide staleness without touching the disk.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Add unit tests for the pure logic (deadline handling, redirect-target validation, cache decode fallback). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

---

## Moderate

### M1 · Steam and app-protocol quicklinks: user-reported bug (≈2 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies; crates/launcher is native Win32 via the windows 0.61.3 crate. Line numbers below are approximate; locate code by the quoted snippets.

User report: "For booting Steam games, the shortcut doesn't work, but routing directly to the exe works."

Verified diagnosis (2026-09-27):
- Steam shortcuts are .url files whose target is a steam:// URI. Example: %APPDATA%\Microsoft\Windows\Start Menu\Programs\Steam\Balatro.url contains `URL=steam://rungameid/2379780`.
- crates/engine/src/quicklinks.rs `validate_target` (~43-101) accepts only http/https links, bare domains and absolute drive/UNC paths. Any other link containing ':' fails with "Use an HTTP/HTTPS website or an absolute file/folder path." So a Quicklink to steam://rungameid/2379780 cannot be saved, while the game's .exe path is accepted. This was deliberate: docs/RELEASE_2026_09_21.md line 32 says custom URI schemes are not accepted, and the function comment says "never interpret shell commands". It should now change.
- Launching works once a URI is accepted: a probe that reproduced Core's exact call (ShellExecuteW "open" with an owner window, no COM, window hidden straight after) launched a .url file and its protocol target successfully. Steam games found by normal search (the Start Menu .url files) and Quicklinks pointing at a .url file path already work. `@run steam://rungameid/2379780` also reaches ShellExecute via crates/launcher/src/windows/commands/launch.rs `open_run_target`.

Task:
1. Extend `validate_target` to accept app-protocol URIs `scheme:rest`:
   - scheme matches RFC 3986 (a letter, then letters, digits, '+', '-' or '.') and is at least 2 characters, so a drive letter like `c:relative` is never a scheme;
   - rest is non-empty;
   - no whitespace, control characters, '"', '<', '>' or '\\';
   - total length ≤ MAX_LINK_LENGTH.
   Lowercase only the scheme; keep the rest byte-for-byte. The http/https/bare-domain handling and its error messages must not change.
2. Reject a named const denylist of schemes that run script, embed content or have a history of abuse: javascript, vbscript, data, file, about, blob, ms-msdt, search-ms, search, ms-officecmd, ms-word, ms-excel, ms-powerpoint, mk, its, ms-its, hcp, jar. Give a clear error ("That link type is not allowed"). Quicklinks are typed by the local user, so this is defence in depth against pasted links.
3. Launcher side (crates/launcher/src/windows/execute_action.rs, NativeAction::OpenQuicklink → open_application → ShellExecuteW): before launching a non-http URI, check that `HKEY_CLASSES_ROOT\<scheme>` exists and has a `URL Protocol` value. If not, return the error "No app is registered for <scheme>: links" instead of the generic shell error. Exempt `shell:` and `ms-settings:`, which Windows handles without a URL Protocol entry. Keep registry access in the launcher; the engine stays pure. Confirm `executable_directory` returns None for URIs.
4. Icons: make sure non-http quicklinks never go through the favicon/website path (check how crates/launcher/src/windows/icon_worker.rs and favicon.rs classify a quicklink as IconSource::Website vs a file). No network request may ever be made for a steam: link. Keeping the ↗ symbol for protocol links is fine.
5. Docs: update the Quicklinks paragraph in README.md (line ~37, "Websites support HTTP/HTTPS …") and any Settings hint or error text that says only HTTP/HTTPS is supported. Do not edit historical release notes.
6. Tests in quicklinks.rs.
   - Accept: steam://rungameid/2379780, STEAM://rungameid/1 (scheme lowercased), com.epicgames.launcher://apps/fn?action=launch, spotify:track:abc, mailto:a@b.c, ms-settings:display, shell:startup.
   - Reject: javascript:alert(1), JavaScript:alert(1), vbscript:x, data:text/html,x, file:///C:/x, search-ms:query=x, ms-msdt:/id, steam://run game (space), c:relative (drive letter), 1abc:x (must start with a letter), steam: (empty rest).
   - Every existing case in `targets_accept_websites_and_paths_without_shell_interpretation` must still behave the same, including `relative.txt/../bad:thing` and `\\.\device`.
   Add a launcher unit test for the scheme-registration check on a scheme that certainly doesn't exist (e.g. `core-no-such-scheme-12345:`).

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Do not launch any real Steam game while testing. Report each change with file:line, the tests you added, and the command results.
```

### M2 · Converter fast path (≈1.5 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies. Search runs on every keystroke: before app ranking, execute_search.rs `try_calculation` (~210-254) runs calendar, time-zone, then all 11 smart converters (crates/engine/src/conversions/mod.rs ~55-71), then the calculator. Line numbers below are approximate; locate code by the quoted snippets.

Problem: a plain app query like `code` costs about 45 heap allocations per keystroke inside the converters (about 23 lowercased/replaced copies of the query, 7 word splits, 3 full tokenize passes, and a Vec built and sorted in currency) before app search starts. That is likely more than the indexed ranking itself (~1.5–2 µs at 2,000 apps).

Task (only these items):
1. Digit pre-filter in `convert()`. Compute once whether the input has an ASCII digit. Without a digit only these converters can match: timestamp (keywords unix/epoch/timestamp/posix, e.g. `unix now`), colour (`#…`, `rgb`, `hsl`) and number_base (Roman numerals, e.g. `roman xlii`). bmi, finance, tip, percentage, display, transfer, currency and units all need a parsed number. Exception: units returns an error hint for input starting with '+', '-' or '.', so treat those leading bytes as "might be numeric" to keep behaviour identical. Before relying on this, read each converter and confirm it cannot match digit-free input; note in your report anything that contradicts it.
2. Tokenize once. finance.rs (~14), tip.rs (~13) and percentage.rs (~12) each call phrase::tokenize (phrase.rs ~19-46: to_lowercase + 4 `replace` calls + one String per word). Tokenize once in `convert()` and pass `&[Token]`. Only run the percent replacements when the text contains "per", and only strip '?' when one is present. Prefer tokens that borrow &str from one lowercased buffer.
3. currency/catalog.rs `attached_symbols()` (~264-281) walks ~180 names, collects ~26 symbols into a Vec and sorts it on every call; it is reached from currency/mod.rs `parse_attached` (~84-89) for every single-word query. Build it once with std::sync::LazyLock (std only, no new dependency) or a hand-sorted const. Only fall back to Unicode to_lowercase for non-ASCII tokens (`Zł` and `Kč` need it).
4. quantity.rs (~10-15) clones the text when there is no comma, and (~21) calls to_ascii_lowercase just to check a k/m/b suffix. Parse the text directly and compare the suffix case-insensitively.

Acceptance: every existing conversion, calculator and search test passes unchanged. Add tests showing `code`, `vsc` and `visual studio code` reach app search, and `unix now`, `#ff8800`, `rgb(1,2,3)`, `roman xlii`, `.5 kg to lb` and `-` behave exactly as before. If crates/engine/examples/measure_search.rs can time `SearchEngine::search("code")`, report before/after numbers; otherwise add a row that does.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M3 · Engine ranking fixes and a truthful benchmark (≈2.5 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies. Search runs on every keystroke on one worker thread that reuses `SearchScratch`. Only 8 results (VISIBLE_RESULT_LIMIT) or 18 recent tiles (RECENT_APPLICATION_LIMIT) reach the UI. Read CORE_SEARCH_EXPERIMENTS.md before changing ranking. Line numbers below are approximate; locate code by the quoted snippets.

Step 1: make the benchmark see real costs. crates/engine/examples/measure_search.rs currently has:
- no aliases;
- short IDs like `app:{index}`, whereas production IDs are `app:` plus 4 hex chars per UTF-16 unit of the .lnk path, 250–400 bytes (crates/launcher/src/windows/discover_applications.rs `identity()` ~191-198);
- no recent list and no quicklinks;
- converter parsing timed together with ranking.
Add: an alias per app; realistic long IDs; ~48 recent IDs; a typing sequence (v, vi, vis, visu, visual); a quicklinks combined-catalog case; and a column timing `catalog.search` alone. Record "before" numbers.

Step 2: fix (only these items).
a. crates/engine/src/search/recent_applications.rs (~15-35) builds a HashMap of at most 48 recent IDs, then iterates the whole catalog hashing every long ID. This runs on every launcher show and every cleared query. Build an ID → ordinal lookup once in `ApplicationCatalog::new` (applications/rank_applications.rs ~66) and iterate the recent list instead. Keep the ordering and the existing de-duplication semantics: a catalog can list the same ID twice (see the comment ~line 40), so the lookup must return the first occurrence.
b. rank_applications.rs (~260-264) alias merging does `scratch.candidates.iter_mut().find(|candidate| candidate.ordinal == *ordinal)` for each alias hit, which is quadratic on short queries. Add an ordinal → candidate-slot map to SearchScratch, filled in `push_candidate` and reset only for the slots that were used. Binary search won't work: the indexed path appends initials matches out of order.
c. normalize.rs (~14-22): queries containing uppercase allocate twice. Use a reusable buffer (CORE_SEARCH_EXPERIMENTS.md measured ~27 ns vs ~101 ns). Keep the zero-copy path for clean ASCII.
d. execute_search.rs: `parse_time` runs twice for complete time queries (~232 and ~365). Pass the parsed result through.
e. Empty-scope results rescan the whole catalog every time: execute_search.rs ~165-166 (fallback when no recent app resolves), ~191-196 (`>`), and `@app` with an empty payload. Cache the empty-query top 8 per catalog, invalidated by catalog identity (Arc::ptr_eq is already used this way elsewhere).

Step 3: re-run the benchmark and report a before/after table.

Acceptance: all existing tests pass, including the indexed-vs-scan equivalence tests. Add tests for (a) duplicate IDs and ordering, (b) alias + name matching the same app, and (e) cache invalidation when the catalog changes.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M4 · Settings page efficiency (≈2 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate, with one UI thread. Settings is a native-control page (crates/launcher/src/windows/settings/), created lazily once and then shown or hidden. Appearance changes preview immediately; valid edits auto-save with a 400 ms debounce. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: none. Do this before M5 if both are planned (both touch view.rs).

Task (only these items; visible behaviour must stay identical):
1. Preview does everything for any change. view.rs `apply_preferences` (~422-475) runs on every corner-radius slider drag step and every valid colour keystroke (settings_flow.rs ~80-109, settings/page.rs ~516-523). Each call does all of this:
   - creates 2 brushes and calls SetWindowTheme 3 times;
   - calls screen_area / EnumDisplayMonitors;
   - runs a full layout, including the settings page's control placement: WM_SETFONT plus SetWindowPos for ~45-60 controls;
   - ends with RedrawWindow(RDW_ALLCHILDREN | RDW_ERASE).
   Diff old vs new preferences and update only what changed:
   - background → brushes and palette, plus SetWindowTheme only if dark/light flips;
   - display → screen area;
   - position or spacing → layout;
   - radius only → clip_to_edges plus invalidate.
   Send WM_SETFONT only when fonts were recreated (DPI change) (page.rs ~734-863, WM_SETFONT ~844-849). Drop RDW_ERASE and RDW_ALLCHILDREN unless the palette changed.
2. Quicklink typing. Every EN_CHANGE (settings/quicklink_table.rs ~134-152, settings_flow.rs ~86-104, view.rs ~477-491) does all of this:
   - re-validates all rows (`page.draft()`, up to 1,000 rows with a lowercased name set);
   - compares them with the saved list;
   - calls RedrawWindow(parent, RDW_INVALIDATE | RDW_ALLCHILDREN);
   - sets the status text.
   Validate only the edited row immediately and leave the full draft() to the existing 400 ms debounce. Invalidate only the table area, and only when the row count or remove-button visibility changes.
3. Opening Settings lays out twice: view/settings_bridge.rs (~27-28) does `invalidate_layout(); layout()`, then set_visible(true) → position_on_monitor lays out again (launcher_state.rs ~425-431, view.rs ~495-502). Keep one.
4. Startup applies preferences twice: View::create (view.rs ~236-237) applies the defaults, then WM_CREATE (shell.rs ~474) applies the saved ones. That means 2 brush sets, 2 SetWindowTheme calls, 2 RedrawWindow calls and at least 4 layouts before the first show. Pass the saved preferences into View::create.

Acceptance: identical visuals and behaviour. Add unit tests for the preference-diff logic. If pwsh is available, build with `cargo build --release --locked -p core-launcher-v2 --bin core-v2` and run `.\scripts\run-release-checks.ps1` (background mode); otherwise say you did not.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M5 · Main window repaint efficiency (≈2 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate, with one UI thread. Painting is double-buffered; fonts and brushes are cached per DPI and palette; WS_CLIPCHILDREN is set and WM_ERASEBKGND is suppressed. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: M4 if planned (both touch view.rs).

Task (only these items; visible behaviour must stay identical):
1. Layout cost on keystrokes that change the result count, in view.rs layout (~307-411) and window_placement.rs (~117):
   - the parent SetWindowPos has no SWP_NOCOPYBITS;
   - SetWindowRgn(..., true) runs every time;
   - 7 separate child SetWindowPos calls (the input box and settings button never move) should become one BeginDeferWindowPos/DeferWindowPos/EndDeferWindowPos batch;
   - power_menu.rs (~138-150) repositions 4 buttons with HWND_TOP on every layout, even while the menu is closed and hidden;
   - the whole parent is invalidated even when the layout is unchanged.
   Fix each: skip SetWindowRgn when size, radius and attached corners are unchanged; skip the power-menu layout while it is closed; invalidate only what changed.
2. view.rs `set_rows` (~528-542) resets the list even when results are identical (e.g. after an exchange-rate refresh). Skip it when ids, titles and details all match the current rows.
3. WM_PAINT ignores the update rectangle: shell.rs (~750-756) → view/paint.rs (~94-119) and view/app_grid.rs (~229-246) draw all 18 tiles (DrawTextW with word-break plus scaled DrawIconEx each) when a hover change invalidated only 2. Pass `rcPaint` into View::paint and skip anything that doesn't intersect it.
4. Missing icons are re-requested on every render (launcher_state.rs ~388, view.rs ~571-589, icon_worker.rs ~40-43). A failed icon stays `icon: None`, so every render asks again; the worker wakes, posts ICONS_READY and the whole list is invalidated with nothing changed, and the failure is logged every 30 s per app. Track "missing until" per identifier on the UI side and skip the invalidation when no row changed.

Acceptance: identical visuals. Add unit tests for the pure parts (row equality, rectangle intersection). If pwsh is available, build with `cargo build --release --locked -p core-launcher-v2 --bin core-v2` and run `.\scripts\run-release-checks.ps1` (background mode); otherwise say you did not.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M6 · Terminal throughput (≈3 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher embeds a ConPTY terminal. Already good, so keep it:
- one COMMAND_OUTPUT message at a time (a `notified` flag);
- a VT parser that allocates nothing per byte;
- scrollback in a VecDeque capped at 5,000 lines;
- 16-byte Copy cells;
- one ExtTextOutW per same-style run.
Line numbers below are approximate; locate code by the quoted snippets.
Depends on: Q5 (touches the same console files).

Task (only these items):
1. Back buffer. view/console/paint.rs (~55-62) calls CreateCompatibleDC and CreateCompatibleBitmap (~2 MB at 800×600×32bpp) on every repaint, then deletes them. Keep them and recreate only on WM_SIZE or a DPI change. Reuse the per-run scratch Vec<u16>/Vec<i32> (~104-126). Stop creating and deleting a brush for each underlined run (~185-193).
2. Scrolling. commands/terminal/screen.rs (~275-308; also scroll_down, insert_lines, delete_lines around ~287-294 and ~408-434) does `lines.remove(top)` plus `insert(bottom, vec![blank; columns])` per scrolled line. push_scrollback then does truncate + shrink_to_fit, and pop_front frees the oldest line: about 3 allocator calls per line. Use `self.lines[top..=bottom].rotate_left(count)` (rotate_right for the down direction) and fill the freed rows. When scrollback is full, reuse the Vec returned by pop_front as the new row.
3. Output rate. commands/capture.rs (~136-137) creates the output pipe with the default size (`pipe(0)`, ~4 KB). `read_output` (~317-345) re-posts COMMAND_OUTPUT whenever the UI has taken the previous batch, so each small read costs a full parse + scroll bar + layout check + InvalidateRect. Use a 128 KiB pipe. Enforce at least ~16 ms between UI notifications (for example a one-shot SetTimer on the UI side, or a timestamp on the reader side), keeping the one-pending-message flag. Esc and painting must stay responsive during `dir /s C:\`.
4. Backpressure. capture.rs (~342-344 and ~221): `state.output.extend_from_slice` grows without limit while the UI hasn't taken it, and `take_update` uses mem::take, which throws the capacity away. Cap it at ~4 MiB, with the reader waiting on the existing condvar until take_update signals. Double-buffer with a swap so capacity is kept.
5. Accessibility text. view/console/mod.rs (~599-613, ~246-260) and screen.rs (~141-147, ~535-542): WM_GETTEXTLENGTH and WM_GETTEXT each build the whole 5,000-line transcript (about 4 copies each). Cache it with a generation counter bumped in `feed`.

Acceptance: all existing terminal tests pass. Add a unit test proving rotate-based scrolling gives the same screen as before for full-screen and partial scroll regions, with count > 1. Add an #[ignore] test or example that feeds 1,000,000 lines into Screen and report before/after timing.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M7 · Terminal work off the UI thread (≈1.5 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher embeds a ConPTY terminal that remembers the working directory reported by the shell (docs/COMMANDS.md). Line numbers below are approximate; locate code by the quoted snippets.

Task (only these items):
1. UI freeze on WSL and network paths. crates/launcher/src/windows/launcher_state/command_flow.rs calls `self.working_directory.is_dir()` (~108) and `update.directory.filter(|d| d.is_dir())` (~159) on the UI thread. After `@wsl cd ~` the directory is `\\wsl.localhost\<distro>\…`, and touching it after WSL has idled out boots the distro, which takes seconds. After `cd \\nas\share` it is an SMB path, where an offline share blocks until the SMB timeout. Core is frozen throughout. Fix:
   - validate the reported directory in commands/capture.rs `wait_for_exit`, which already runs off the UI thread and reads the report;
   - at start, skip the pre-check for UNC paths;
   - fall back to the home directory if CreateProcessW fails with ERROR_DIRECTORY.
2. Repeated shell resolution. Each Enter runs the following on the UI thread (commands/shells.rs ~112-143, commands/environment.rs ~27-61):
   - re-reads and fully parses Windows Terminal's settings.json (up to 3 paths);
   - probes executable candidates;
   - calls CreateEnvironmentBlock;
   - in `names_variable`, allocates a String via from_utf16_lossy for each environment entry.
   Cache the resolved shell keyed on the settings file's modified time, and compare variable names in UTF-16 without allocating.
3. Docs mismatch. If stable Windows Terminal's default profile is one Core can't drive, `find_map` falls through to Windows Terminal Preview's default, whereas docs/COMMANDS.md says Windows PowerShell is used. Make the code match the docs, and state the behaviour you chose in your report.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Add unit tests for the cache invalidation and the fallback. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M8 · Favicon network behaviour (≈2 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher fetches website-quicklink favicons: Google s2 first, then the site's /favicon.ico. Private hosts are never sent to Google, "no icon" answers are remembered for a day, and `--dry-run` never contacts websites. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: Q7 (touches http.rs and favicon.rs).

Task (only these items):
1. Session per request. crates/launcher/src/windows/http.rs (~49-69) calls WinHttpOpen (automatic proxy) and WinHttpConnect, then closes both, for every request. Every favicon pays a fresh TCP+TLS handshake and proxy detection is thrown away. Keep one process-wide session in a std::sync::OnceLock (synchronous WinHTTP handles are thread-safe). Optionally reuse the connection to www.google.com.
2. Retries without backoff. icon_worker.rs (~19-21, 195-220) and favicon.rs (~97-104): when no source can be reached nothing is saved, and the failure is remembered in memory for only 30 s, inside the 64-entry icon LRU where it can be evicted sooner. So an unreachable host (e.g. http://nas away from home) costs 3–6 s of the only icon thread every 30 s, forever.
   - Keep per-host exponential backoff (30 s → 5 min → 1 h) in a small map outside the LRU.
   - Remember "Google unreachable" for a few minutes and go straight to /favicon.ico meanwhile.
3. Caches roam. favicon.rs (~121-123) and exchange_rates.rs (~40-42) keep caches in Roaming %APPDATA%. With folder redirection every read and metadata check goes over the network. Move the caches to %LOCALAPPDATA%\Pleiades\Core\v2\ and leave settings in Roaming. On first run, move any existing cache files across and ignore failures. Update README.md where it names these cache paths (~37 and ~77). This changes a documented path, so put it in its own commit and mention it in your report.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Add unit tests for the backoff schedule and the cache-path migration. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### M9 · Time-zone rule caching (≈1.5 h)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). Time conversion (`9pm et to uk`, `@time 21:00 ET to UK on 2026-03-10`) reads Windows time-zone rules on the search worker and never uses the network. The zone list is loaded lazily once. Line numbers below are approximate; locate code by the quoted snippets.

Task (only these items):
1. Measure first. Time one complete conversion (e.g. `9pm et to uk`) through crates/launcher/src/windows/time_converter.rs in a test and report it. If it is under 50 µs, do only step 3 and report.
2. Cache the rules. Each complete query (~97-110, 134-175, 205-213) calls GetTimeZoneInformationForYear once and SystemTimeToTzSpecificLocalTimeEx / TzSpecificLocalTimeToSystemTimeEx up to 4 times; each call reads the year's Dynamic DST data from the registry. Cache TIME_ZONE_INFORMATION per (zone key, year) and use the cached rules. Conversions that cross 31 Dec / 1 Jan must use the right year's rules.
3. Failed load retried per keystroke. In `zone()` (~121-124), if load_zones() fails, `zones` stays None and the full walk through ~140 zones is retried on every time-query keystroke. Remember the failure and retry with a backoff (e.g. after 60 s).

Acceptance: all existing time-conversion tests pass, including daylight-saving, skipped/repeated local times and `on YYYY-MM-DD`. Add tests for the year-boundary case and the failure memo.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

---

## Complex

### C1 · Take the registry off the show path (½ day)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher is native Win32 via the windows 0.61.3 crate, with one UI thread. Budgets: hotkey-to-first-frame 50 ms p95; hidden idle does no work (no timers, no polling). With nothing typed, Core shows recent apps: Core's own launch list (%APPDATA%\Pleiades\Core\v2\recent-applications.txt, max 50) first, then apps from Windows' UserAssist registry record. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: Q7 item 4 if done (exchange-rate metadata on show).

Problem (two independent audits found this):
- Every show runs `refresh_recent_applications()` (launcher_state.rs ~450-452) BEFORE `self.transition.set_visible` (~454), on the UI thread. It enumerates both UserAssist keys (584 values on the dev machine; the code allows 10,000), ROT13-decodes, lowercases, sorts and resolves every value even though only 48 are used. It also rebuilds `KnownFolders` (SHGetKnownFolderPath per folder GUID) each time. Files: user_assist.rs (~40-134, ~194-251), recent_applications.rs (~72-78).
- `remember_launch` (launcher_state.rs ~587-592) repeats the whole read before the app is launched, although only Core's own list changed. recent_list.rs (~95-111) saves recent-applications.txt synchronously (create dir, write temp, rename) before ShellExecute runs.
- view.rs `position_on_monitor` (~495-512) always invalidates the layout on show.

Task:
1. Measure first. Time refresh_recent_applications and report the number (a test, or a temporary timing log you remove afterwards).
2. Cache the Windows list. Re-read it only when a RegNotifyChangeKeyValue notification (both UserAssist keys, REG_NOTIFY_CHANGE_LAST_SET, asynchronous with an event handle) has been signalled; check it on show with WaitForSingleObject(event, 0) and re-arm after reading. No polling and no timers. An acceptable alternative is to do the read on the search worker when the query is empty, showing the previous list meanwhile.
3. Stop resolving after 48 results (CANDIDATE_LIMIT). Keep KnownFolders in LauncherState.
4. `remember_launch`: re-merge Core's own list with the cached Windows list, with no registry read. Save the file after ShellExecute returns, or on a background writer like the settings store's.
5. `position_on_monitor`: invalidate the layout only when the ScreenArea actually changed. Keep the RedrawWindow.

Acceptance: the recent grid's order and contents are identical. All tests pass. Report before/after show-path timing. If pwsh is available, build with `cargo build --release --locked -p core-launcher-v2 --bin core-v2` and run `.\scripts\run-release-checks.ps1` (background mode); otherwise say you did not.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Every handle you open (registry keys, events) must be closed on every path. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### C2 · Split favicons from app icons and add a UI icon cache (½ day)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/launcher loads icons on a sleeping worker, only for visible results:
- COM is initialized lazily;
- a 64-entry LRU releases native handles on eviction;
- icons are reference-counted and destroyed on the last drop;
- publish() coalesces ICONS_READY messages;
- `--dry-run` never contacts websites.
Line numbers below are approximate; locate code by the quoted snippets.
Depends on: Q7 and M8 (http.rs/favicon.rs changes), M5 (missing-icon tracking).

Problem:
1. crates/launcher/src/windows/icon_worker.rs `run` (~138-178): one `core-icons` thread serves both Shell icons and website favicons. `current()` is only checked between requests, so one in-flight favicon::load → http::get (worst case ~28 s across Google and /favicon.ico) blocks every later app-icon batch; the next keystroke's icons stay blank.
2. Within a Shell batch, `loaded` is only published after the whole batch (~158-165). Cached icons therefore wait behind slow extractions (5–50 ms each), and on a cold first show the 18-tile grid gets no icons until all 18 are extracted.
3. view.rs `set_rows` (~528-542) only reuses icons from the immediately previous rows, so a result that reappears while typing flashes the placeholder until the worker round-trips. `set_icons` (~571-589) invalidates the whole list or grid for every batch.

Task:
1. Move website fetches to their own worker (1–2 threads) that publishes through the same publish/ICONS_READY path. The Shell-icon thread must never touch the network. Keep generation checks and the dry-run rule.
2. In the Shell worker, publish cache hits first, then each icon as it is extracted.
3. Add a bounded HashMap<Arc<str>, Arc<ApplicationIcon>> in View (e.g. 128 entries, least-recently-used eviction), filled by set_icons and consulted by set_rows and queue_icons (launcher_state.rs ~373-405). Evicting from it must release icons through the existing refcount.
4. Invalidate only the rows or tiles whose icon changed.

Acceptance: no GDI/USER handle growth. crates/launcher/src/windows/mod.rs has GUI_RESOURCE_TEST_LOCK for handle-count tests; add one that loads, evicts and re-requests icons and checks the counts. Add an #[ignore] test showing app icons still arrive while a website request is stalled (e.g. an unroutable host such as 10.255.255.1).

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies. Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### C3 · Search index scaling to 10k apps (½–1 day)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies. App search uses a flat rarest-trigram index (crates/engine/src/applications/candidate_index.rs) with a 512 KiB budget and a separate initials lookup, plus a full scan for short queries or when the index is unavailable. Ranking uses select_nth_unstable + a sort of the top k. Read CORE_SEARCH_EXPERIMENTS.md and LINE_SEARCH_DESIGN.md first. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: M3 (benchmark must be realistic first).

Task:
1. The index switches off too early. candidate_index.rs (~1-3, 24-25, 50-53): at ~15–20 postings × 4 bytes per app plus 8 bytes of `initials_order` (rank_applications.rs ~28, Box<[usize]>), the budget is exceeded at ~5–6k apps. `build` then returns None and every query scans, although discovery allows 10,000 apps (MAX_APPLICATIONS). Fix:
   - store postings as u16 when N ≤ 65,535;
   - drop `length` from GramRange (derive it from the next range's start; 12 → 8 bytes);
   - make initials_order u32.
   If still over budget, drop the most common grams rather than the whole index.
2. 1–2 character queries always scan the whole catalog: candidate_index.rs (~67-87) returns None without a trigram, which leads to collect_scanned (rank_applications.rs ~187-202). `applications` is sorted by normalized name, so prefix matches form a contiguous range. Find it with two partition_point calls, plus the alias range; if together they give at least `limit` distinct apps, skip the scan. This is exact because match class is the primary sort key and everything outside those ranges ranks WordPrefix or lower. Still rank the whole range, so pins and launch counts are respected. This differs from the alphabetical early exit that CORE_SEARCH_EXPERIMENTS.md rejected; explain why in a comment.
3. match_class (rank_applications.rs ~290-295) re-splits the name into words for every term, candidate and keystroke. Precompute u16 word-start offsets once at catalog build.
4. Candidate ordering (~42-48, 174-178) compares four fields over 16 bytes. Pack it into one u64 key (class, unpinned, 1000 − launches, u32 ordinal).

Acceptance: the existing `indexed_results_equal_the_scan…` equivalence test passes with limits 1 and 8. Add equivalence tests for 1- and 2-character queries, and a 10,000-app catalog test asserting the index is built (index bytes > 0). Report before/after numbers from crates/engine/examples/measure_search.rs at 400, 2,000 and 10,000 apps.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```

### C4 · One catalog instead of two (½ day)

```text
Project: C:\Users\Robert\code\Pleiades\Core, "Core v2", a Windows app launcher in Rust 1.95 (pinned in rust-toolchain.toml). crates/engine is pure logic with zero dependencies. Quicklinks (saved websites/files/folders) are ranked together with apps in normal search; `@app` excludes them and `>` lists only them. Line numbers below are approximate; locate code by the quoted snippets.
Depends on: C3 if planned (same ranking code).

Problem: crates/launcher/src/windows/search_worker.rs (~172-191) and crates/engine/src/quicklinks.rs `catalogs()` (~103-117). Once any quicklink exists, every app is cloned and ApplicationCatalog::new runs again: all names re-normalized and re-sorted, trigram index rebuilt. This happens on the search thread before the first query after discovery or a quicklink edit. Two indexes (up to 512 KiB each) and two sets of prepared strings then stay in memory.

Task:
1. Search the apps catalog and the quicklinks catalog separately and merge the two top-8 lists with exactly the same comparator the combined catalog uses today (class, unpinned, launches, normalized name, ID). The result must be identical to searching the combined catalog. Write an equivalence test over randomized inputs (a simple deterministic LCG; no new dependencies).
2. Remove the combined catalog and its rebuild. Keep `@app` excluding quicklinks and `>` listing only quicklinks: the existing test `quicklinks_share_ranking_but_explicit_app_scope_excludes_them` must pass unchanged.
3. Report memory saved (index bytes and prepared-string bytes for a 2,000-app + 50-quicklink setup) and first-query latency after a quicklink edit, before and after.

Do NOT change the app ID format (`app:` + 4 hex chars per UTF-16 unit, discover_applications.rs `identity()` ~191-198) even though it is long. IDs are persisted in recent-applications.txt and changing them needs a migration the owner hasn't approved. Mention it as a follow-up in your report.

Rules: Read every file before changing it. Make the minimum change; match the surrounding style, naming and comment density; do not refactor unrelated code or fix other issues you notice (list them in your report instead). No new dependencies (crates/engine must stay dependency-free). Finish with `cargo fmt --all`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets -- -D warnings`; all must pass. Report each change with file:line, the tests you added, and the command results.
```
