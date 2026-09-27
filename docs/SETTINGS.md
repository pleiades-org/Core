# Appearance and position settings

This is the original settings milestone record. For the current seven positions, top-right settings icon, configurable shortcut, display selection and startup setting, see the [21 September release](RELEASE_2026_09_21.md). Measurements below describe the original build.

Added 20 September 2026. Open the page from the **Settings** button in Core's footer, **Ctrl+,**, or the tray's **Settings** item. The existing search layout, borderless styling, icons and fades are retained.

Enter a six-digit hexadecimal background color, with or without `#`, or choose **OLED black**, **Charcoal**, or **Light**. Text and selection colors adapt for readability. Changes preview immediately; **Save** commits them and returns to search. **Cancel** or **Escape** restores the saved appearance. Clicking outside also dismisses the page and discards an unsaved preview. Opening Settings again while it is already open preserves the draft. Tab/Shift+Tab navigate controls; Enter saves from the color field or activates the focused button.

| Position | Placement in the monitor's usable area | Square corners |
| --- | --- | --- |
| Center | Horizontally and vertically centered | None |
| Top | Top edge, horizontally centered | Top left and top right |
| Bottom | Bottom edge, horizontally centered | Bottom left and bottom right |
| Left | Left edge, vertically centered | Top left and bottom left |
| Right | Right edge, vertically centered | Top right and bottom right |

The usable area excludes the taskbar. Core chooses the foreground window's monitor when opened, and retains that monitor while queries resize the result list. Bottom/right anchors remain fixed as the window changes size. Display/work-area changes refresh the placement, and deferred DPI updates avoid reentrant layout updates. On unusually small work areas, outer bounds are clamped; the fixed-width native control layout is not a fully responsive mobile UI.

Preferences are stored separately from old Core data at `%APPDATA%\Pleiades\Core\v2\appearance.ini`. The file is versioned and bounded to 1 KiB when read. A short-lived worker writes a same-folder temporary file, flushes it and replaces the destination through Windows; no settings I/O runs during application matching or painting. A failed replacement leaves the previous file intact and reports an error. Unsupported/corrupt settings are preserved, with defaults used and saving disabled for that session. An accepted save finishes before normal app shutdown.

Settings controls are constructed only when first opened. There is no settings polling, persistent save worker or new dependency. Brushes and window regions are replaced with their previous native resources released. The settings schema contains only background and position; configurable hotkeys, startup and quicklinks remain future work.

## Validation

The settings build is 603,136 bytes (589 KiB). The final settings validation record identifies SHA-256 `6F6B1007BC3E867FCA3C92938494A8CEAC8E89BD73171A858D7F5F60CE1D870C`.

- 25 Rust tests cover the engine, color/config validation, contrast, negative-coordinate monitor geometry, attached corners and result-height anchoring. Strict Clippy and formatting checks pass.
- Native settings tests cover all five placements and corner regions, color pixels, light/dark rendering, Tab navigation, Cancel, invalid input, preview preservation, restart persistence, denied-save recovery, save-before-exit and preservation of unsupported settings files.
- Thirty successive appearance previews remain within the test's two-object GDI growth allowance. Existing search/time/Enter integration, animation reversal and click-away regression checks also passed during this change; their records identify the specific tested binaries.
- Screenshots of Core's own window were inspected in OLED black and light themes. Tests use isolated files under `target/settings-tests`; real preferences are not changed.

Native test commands are queued, with a dry-run-only acknowledgement after queued UI work completes. This prevents tests from observing a partially updated layout or sending a command reentrantly during another update. These tests do not certify physical mixed-DPI monitor switching or screen-reader behavior. Current settings-build CPU/RAM and full release-gate benchmarks have not been repeated; the earlier milestone's resource figures should not be attributed to this executable.

Records: [settings](measurements/settings-checks.json), [search integration](measurements/settings-integration.json), [styling](measurements/settings-styling.json), [dismissal](measurements/settings-dismissal.json).

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
.\scripts\test-settings.ps1
```

`--dry-run` uses in-memory defaults unless `--settings-file PATH` is supplied explicitly. This keeps automated launcher tests independent of personal appearance preferences. The settings test supplies its own isolated path and suppresses external launch/clipboard/browser actions.
