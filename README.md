# Core v2

A small Windows launcher written in Rust. This is the first working alpha of the new implementation. The previous Core project remains separate.

The [second 22 September release notes](docs/RELEASE_2026_09_22_B.md) add [smart conversions](docs/SMART_CONVERSIONS.md) (currency, units, download time, percentages, tips, bases, colours, Unix time, screens, loans and BMI) and website quicklink icons. The [first 22 September release notes](docs/RELEASE_2026_09_22.md) describe the app and tray icons, taskbar command, corner rounding and edge spacing sliders, logging and reliability fixes. The [21 September release notes](docs/RELEASE_2026_09_21.md) describe the expanded calculator, power menu, configurable shortcuts, monitor/startup settings and packaged-app discovery. Earlier benchmarks remain available in the [feature/performance comparison](docs/FEATURE_AND_PERFORMANCE_DIFF.md); each measurement identifies its tested executable.

Run `target\release\core-v2.exe`. Press **Ctrl+Alt+Space** to show or hide Core. Use the arrow keys and Enter to choose a result; Escape or clicking outside hides the window. The tray menu opens or exits Core. Starting Core again activates the existing instance.

The footer shows one hint for the selected action, such as **Enter to copy** or **Enter to open**, with the local time centred in `HH:MM` format. The clock updates each minute while the search window is visible and stops while Core is hidden or Settings is open. Errors and command completion status appear in place of the action hint.

| Input | Action |
| --- | --- |
| Nothing typed | Recently used apps as a grid, like Start: up to 18, six across. Apps opened from Core come first, then apps Windows has seen you start (read locally from Windows' own usage record). Arrow keys move, Enter or a click opens |
| `code`, `vsc`, `studio code`, `xbox` | Search Start Menu and registered packaged apps by name, word prefix, initials and substring |
| `cmd`, `wt`, `taskmgr`, `regedit` | Windows aliases: the program a Start Menu shortcut starts (`cmd` finds Command Prompt) and Store apps' command names (`wt` finds Windows Terminal) |
| `@app calculator` | Search applications explicitly |
| `> docs` or `@quicklink docs` | Search saved quicklinks; names also appear in normal search |
| `2 + 3 * 4` or `@calc 25% of 80` | Calculate locally; Enter copies the result |
| `sqrt(81)`, `round(2.6)`, `pi*2` | Math functions and constants |
| `time` or `@calc time` | Current local time, like `now`, in the calculator answer display; Enter copies `YYYY-MM-DD HH:MM` |
| `date` or `@calc date` | Today's date, like `today`, in the calculator answer display; Enter copies `YYYY-MM-DD` |
| `2 days from now`, `next Friday`, `3 hours ago` | Local calendar dates and elapsed time |
| `2028-01-31 + 1 month`, `2026-12-25 - 2026-09-21` | Date arithmetic and signed differences in days |
| `10 kg to lb`, `4.7GB to MiB`, `30 mpg to l/100km` | Unit conversions across 15 kinds, including data sizes and rates; Enter copies the numeric answer |
| `100 usd to eur`, `£50 in dollars`, `100 usd` | Currency conversion with European Central Bank reference rates (bare amounts convert to your regional currency) |
| `10 GB at 100 Mbps`, `5 km in 25 min` | Download time, speed needed, data used, running pace |
| `20% off 80`, `tip 15% on 80 split 4`, `255 to hex`, `#ff8800`, `unix now` | Percentages, tips, number bases, colours, Unix time and more; see [smart conversions](docs/SMART_CONVERSIONS.md) |
| `9pm et to uk` or `@time 21:00 ET to UK` | Convert time zones using today's date in the source zone; Enter copies the dated result |
| `9pm et to uk on 2026-03-10` | Convert for a particular date, including daylight-saving differences |
| `@web Rust & Windows` | Open an encoded Google query in the default browser |
| `@` or `@cal` | Complete a supported command |
| `@power`, `power off`, `restart`, `sleep` | Choose a power action, then explicitly confirm |
| `/ipconfig /all`, `@pwsh Get-Process` | Run a command in your shell, in Core's own terminal: colours, prompts, REPLs and full-screen programs work, and keys typed into it reach the command. Ctrl+Enter opens a terminal window, Ctrl+Shift+Enter runs as administrator, and ↑ recalls recent commands. See [running commands](docs/COMMANDS.md) |
| `@run notepad`, `shell:startup`, `ms-settings:display`, `%appdata%` | Open anything Windows Run (Win+R) accepts, optionally as administrator |
| `taskbar`, `tb`, `@taskbar` | Show the Windows taskbar on Core's display (useful with the Windows-key-only shortcut and an auto-hidden taskbar) |

In **Settings → Quicklinks**, enter a **Link** and **Name**. Completing a row appends another blank row. Scroll to add more; the × button removes a row. Completed valid changes save automatically, including renames and removals. Websites support HTTP/HTTPS (a bare domain gets `https://`); files and folders use absolute Windows paths. Names must be unique. Up to 1,000 entries are supported, with only four rows of native controls kept on screen. Partial or invalid rows show an explanation and are not saved. `>` lists quicklinks; Enter opens the selected target through Windows. File and folder quicklinks show their Windows icons. Website quicklinks show the site's icon. The first time a link is displayed, Core asks Google's favicon service (`www.google.com/s2/favicons`, 64 px), which also finds icons declared only in a page's HTML, and falls back to the site's own `/favicon.ico`. Google therefore sees the domain names of your website quicklinks; `localhost`, IP addresses and intranet names (no dot, or ending `.local`, `.lan`, `.internal`, `.home.arpa`) are only ever requested directly. No cookies or credentials are sent. Icons are cached in `%APPDATA%\Pleiades\Core\v2\favicons` for 30 days; links with no icon anywhere keep the ↗ symbol and are asked again after a day, and unreachable sites are retried while shown after 30 seconds, then 5 minutes, then hourly. While Google is unreachable, sites are asked directly for a few minutes. Each icon appears as soon as it downloads. `--dry-run` never contacts websites.

Arithmetic supports parentheses, unary signs, `+ - * / ^`, scientific notation and postfix `%`. `%` divides by 100: `20 + 10%` means `20.1`. `25% of 80` means `20`. Calculations use floating-point arithmetic. Invalid or unfinished input cannot execute an old result.

BODMAS is already applied: `2+3*4` gives `14`, and `(2+3)*4` gives `20`. Division/multiplication and addition/subtraction each share a precedence level and evaluate left to right; powers evaluate right to left. The [calculator evaluation comparison](docs/CALCULATOR_EVALUATION.md) explains the rules and measures postfix alternatives against the current parser.

Time conversion supports regional ET, CT, MT, PT, UK/London, Berlin, Sydney, Tokyo and India, plus UTC/GMT and explicit seasonal abbreviations. Regional names follow Windows daylight-saving rules; `EST` is a fixed offset while `ET` changes with the date. Skipped or repeated local times produce an explanation instead of an arbitrary conversion. Dates use `on YYYY-MM-DD` (1900–2100); see the [alias list and date semantics](docs/TIME_AND_ICONS.md).

Open **Settings** using the top-right gear, **Ctrl+,**, or the tray menu. Choose a category in the left sidebar to see its controls on the right. Appearance includes any `#RRGGBB` background and **Center, Top, Bottom, Left, Right, Bottom left or Bottom right**. **Corner rounding** (0–32 px; 0 is square) and **Screen edge spacing** (0–200 px) sliders shape and inset the window. Spacing is measured from the physical screen edge, so a value larger than the taskbar keeps Core clear of it, while smaller values may overlap it; at 0 Core uses the automatic taskbar-aware placement. Spaced windows round every corner. Behaviour includes a configurable shortcut with a **Use Windows key** switch, **Display** and **Commands run in** dropdowns, a **Start with Windows** switch and **Clear command history**. On/off settings are switches, choices are dropdowns, and actions are filled buttons. Valid changes save automatically and survive restarting Core. Color and shortcut typing use a 400 ms delay; buttons save immediately. **Done**, Escape, dismissal and normal exit flush valid pending edits. Invalid text is explained and never saved; write failures show **Retry save**. Appearance previews immediately. Edge positions respect the taskbar and square every corner touching an edge.

The bottom-right power icon opens a recessed inline menu. All four power icons highlight on hover. Sleep, Restart and Power off also appear through `@power`. Each opens a confirmation with **Cancel** selected; select the second row to proceed. Escape closes the popup before hiding Core. Position choices use small screen boxes showing Core's anchor. On constrained displays, the result viewport shrinks to keep the footer and power controls visible; arrow keys still reach all results. See [current behavior and validation](docs/RELEASE_2026_09_21.md).

## Build and verify

Requires Windows, the Rust MSVC toolchain and Visual Studio C++ build tools. `rust-toolchain.toml` pins Rust 1.95.0. Cargo downloads dependencies on the first build. The normal launcher uses the existing `windows` bindings; the engine has no external dependencies. GPUI is an optional measurement feature and is absent from the normal binary.

```powershell
cargo build --release --locked -p core-launcher-v2 --bin core-v2
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
.\scripts\run-release-checks.ps1              # background: you can keep working
.\scripts\run-release-checks.ps1 -Interactive # adds click-away dismissal and pointer hover
```

The native integration scripts need PowerShell 7 (`pwsh`) and an ordinary Windows session with Explorer running. By default they run in **background mode**: Core starts with `--dry-run --test-background`, never takes the foreground and ignores click-away, so the scripts do not pull you out of the app you are using. Windows may still appear briefly on screen. Checks that need the real foreground, mouse pointer or keyboard run only with `-Interactive`: click-away dismissal (`test-dismissal.ps1`), pointer hover and visible-surface captures (`test-controls.ps1`), single-instance activation, and, with `-PhysicalKeyboard`, injected keystrokes for shortcuts and quicklinks. Start those when you can leave the computer alone. `package-release.ps1` requires the interactive records unless `-SkipInteractive` is given, which is noted in `release-package.json`. `--dry-run` renders and validates results but suppresses clipboard writes, app launches, browser opens, power actions, startup writes and global shortcuts. `--test-shortcut` enables only shortcut registration/interception in a dry-run instance. The registry integration test requires HKCU write access and an explicit `cargo test --workspace --locked -- --include-ignored`; it uses a temporary key outside autorun locations.

The discovery integration case requires at least one supported Start Menu entry. `scripts/test-single-instance.ps1 -Interactive` checks second-launch activation and visible Escape behavior; close a running Core v2 before that test.

Diagnostics from release builds go to `%APPDATA%\Pleiades\Core\v2\core.log` (rotated to `core.log.old` above 1 MiB) unless stderr is already redirected. The tray icon switches between white and black glyphs to match the taskbar theme and reappears if Explorer restarts. Icons live in `crates/launcher/assets`; regenerate them from `assets/source` with `python scripts/generate-icons.py` (requires Pillow). `build.rs` embeds them without a resource compiler.

Start hidden with `core-v2.exe --start-hidden`, or enable **Start with Windows** to register this executable for the current user's sign-in. Keep the executable in a stable location before enabling it.

By default, Core uses a rounded, borderless OLED palette: pure-black background and a subtle `#121212` selected-result surface. Short 130 ms entrance and 100 ms exit fades are enabled for Core independently of Windows' animation setting. Use `--reduced-motion` for immediate show/hide, or `--system-motion` to follow Windows. The animation timer stops after each transition. Escape hides the palette, and double-clicking a result accepts it. Calculated answers have larger typography and a contextual subtitle.

## Implementation

- `crates/engine`: borrowed command parsing into an enum, arithmetic evaluation, time-expression parsing, prepared application catalog and deterministic ranking.
- `crates/launcher`: native Win32 controls, Start Menu discovery, hotkey/tray, Windows actions and a blocking message loop.
- One search worker retains its scratch storage. One pending request replaces older pending work. Generation checks reject stale results and stale Enter actions; canceled scans stop at bounded checkpoints.
- App names and initials are prepared once. Catalogs of at least 128 entries can use a flat rarest-trigram index with a separate initials lookup. An index budget of 512 KiB and a bounded build protect memory; short queries and catalogs outside that budget use complete scanning. Only eight selected rows reach the UI.
- Discovery runs once in the background, with depth and entry limits. Application matching does no disk access. Time conversions read Windows time-zone rules on the search worker; they never make network requests. Searching never uses the network. Two background features may, only while Core is shown: website-quicklink icons and the daily ECB exchange-rate file (cached in `%APPDATA%\Pleiades\Core\v2\exchange-rates.xml`, refreshed when older than 12 hours).
- A separate sleeping worker loads actual icons only for visible application results. A 64-entry LRU cache retains icons and failed lookups; evictions release native handles. Missing icons use the small generic symbol. No icon extraction runs in input handling, painting or application matching. Hidden idle has no application timer or periodic polling.

## Current limits

This is a usable alpha release, not the entire original plan. Favorites/recent persistence, custom app aliases, catalog refresh notifications and typo correction remain pending. Quicklinks now support saved websites, files and folders. Discovery covers supported Start Menu files and registered packaged apps; restart Core after installing applications.

Rows have application icons and folder descriptions; fuller duplicate-name disambiguation remains pending. Windows-key-only mode preserves tested combinations on the normal desktop; shortcut replay into elevated applications can be restricted by Windows. Secure-desktop, physical mixed-DPI, IME-candidate and screen-reader acceptance testing remains manual. Explorer-restart tray recovery remains hardening work. The portable build has no installer, signing or old-data migration. Line search is excluded.

See the [current feature/performance comparison](docs/FEATURE_AND_PERFORMANCE_DIFF.md), [initial implementation measurements](docs/IMPLEMENTATION_STATUS.md) and the [full plan](CORE_V2_PLAN.md). The comparison distinguishes implemented features, experiments and uncompleted release gates, including current click-away measurements.
