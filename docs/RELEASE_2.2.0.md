# Core 2.2.0

This release adds media controls that prefer your music, a now-playing bar, and smoother window corners.

- `@media` and `@music` control Spotify, Apple Music, Media Player, browsers and any other player Windows knows: play or pause, next, previous, and every open player in one list. `@media spotify next` names a player.
- `play`, `pause`, `next`, `previous`, `prev` and `now playing` typed on their own offer that control above the usual app results. Media controls keep Core open for another press.
- Core chooses the player itself instead of following Windows, which often picks a paused browser tab. **Music apps first** (default) or **Playing media first**, with preferred and ignored apps. `pause` always stops what you hear; `play` never also starts a paused tab while music plays.
- A now-playing bar above the search box shows album art, title, artist, elapsed time and progress, with previous, play/pause and next buttons. Click the progress line to seek, or the art to bring the player forward. It shows while something plays, or after you pause from Core, and never moves the search box while you type.
- Settings > Music sets the priority, preferred and ignored apps, the bar, and media shortcuts that work while Core is open (Alt+P, Alt+Right, Alt+Left by default) or in every app.
- Every shortcut box in Settings, including Open Core shortcut, now records keys: click it and press the shortcut. Core's own shortcuts pause while a box records, so the current shortcut can be recorded again.
- Rounded window corners are anti-aliased against the desktop and fade with the window.

See [media controls](https://github.com/pleiades-org/Core/blob/main/docs/MEDIA.md) for the rules, settings and file format,
and [the update guide](https://github.com/pleiades-org/Core/blob/main/docs/UPDATES.md) for updates and recovery.

## Cost

Nothing media-related starts until media is used or Core opens with the bar on. While Core is hidden the media worker keeps no event subscriptions or timers. Measured with Spotify playing (release build, 60 s hidden after one `@media` show and hide): 0.0000% CPU with and without media; private bytes 6.2 MiB without and 6.7 MiB with the media worker started. Visible with the bar's once-a-second progress clock: 0.10% of one core over 30 s.

## Validation

- 320 unit tests passed (127 engine, 193 launcher); ten opt-in tests remain excluded from the default run, including two that read or command the person's own players.
- `cargo fmt --all --check` and Clippy with warnings denied passed.
- All nine background release suites passed on the packaged executable, including the signed update handoff, verification and rollback checks.
- Against real players on the release PC: Spotify (desktop) was recognised as a music app with art, timeline and seek support, and a browser tab session was read alongside it. A full command round trip sent play to the already-playing Spotify, which changes nothing, and Spotify accepted it.
- An isolated-settings script checked recording, clearing and refusing shortcuts, scopes, priority, the bar switch, the app list, media shortcuts in use, and persistence across restart. It passed in repeated runs, but some runs failed intermittently, so it is not part of the release suites yet.
- Not exercised: commands that change playback (to avoid interrupting the music playing during testing), recording shortcuts with held modifiers (posted keys cannot hold them), interactive pointer and keyboard checks, screen readers, and mixed-DPI displays.
