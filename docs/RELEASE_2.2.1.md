# Core 2.2.1

This release makes media controls work with players that Windows itself does not see, and adds `@info`.

- **Players Windows does not see:** some players publish nothing to Windows. Pressing a media key then shows no song in Windows' own pop-up; Spotify does this when its **Show desktop overlay when using media keys** setting is off. In 2.2.0, Core then showed no media controls at all.
  - Core now reads Spotify from its window instead: the title gives artist and song, and controls go straight to Spotify. While Core is visible it follows Spotify's title changes without polling. There is no album art, progress line or seeking in this mode; turning Spotify's overlay setting on brings them back.
  - For any other player Windows cannot see, `@media` and the `play`, `pause`, `next` and `previous` keywords still offer the controls and send the keyboard's media keys.
- **`@info`** (also `@about` and `@version`) shows the installed version (Enter copies it), the latest release from the last update check, and a link to this version's release notes. Enter on the latest release behaves like `@update`. Between daily checks, the latest version comes from the cached signed manifest, so no extra network request is made.

See [media controls](https://github.com/pleiades-org/Core/blob/main/docs/MEDIA.md)
and [the update guide](https://github.com/pleiades-org/Core/blob/main/docs/UPDATES.md).

## Validation

- 325 unit tests passed (130 engine, 195 launcher), including Spotify window-title parsing, the media-key fallback rows and every `@info` update state. Eleven opt-in tests remain excluded from the default run.
- `cargo fmt --all --check` and Clippy with warnings denied passed.
- All nine background release suites passed on the packaged executable, including the signed update handoff, verification and rollback checks.
- Reading Spotify from its window was checked against the running desktop app (title only; nothing sent). Sending controls to Spotify's window, and running on Windows 10 itself, were not exercised here: this PC runs Windows 11, and its Spotify publishes a normal Windows session, which Core prefers.
