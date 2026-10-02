# Core 2.4.0 candidate

Optional Spotify catalog search and playback are available under **Settings → Music → Spotify song search**. The feature is off by default and uses your personal Spotify app Client ID and a one-time Premium account connection.

- Type **`@song`** followed by a song or artist. Core shows matching songs with artwork, artist and album in its existing results list.
- Press **Enter** on a result to play that exact track on your active Spotify device. Core stays open.
- Search runs in a separate worker, waits for typing to settle, and discards stale responses. Normal application search remains local.
- The account connection uses PKCE and a loopback callback. Refresh credentials are protected with Windows DPAPI; no client secret is needed.
- Music settings offer an enable switch, Connect, Disconnect and a link to create your personal Spotify app. Existing Windows media controls remain available without account setup.

See [setup instructions](https://github.com/pleiades-org/Core/blob/main/docs/SPOTIFY.md).

## Validation and release status

The candidate passes 166 selected offscreen unit and local-server checks, formatting and Clippy with warnings denied, and an optimized build. This is targeted validation, not the full native test suite. No onscreen tests, browser sign-ins or live playback commands have been run. Spotify authorization, actual playback, and visual/keyboard/screen-reader acceptance remain manual checks.

The normal release packager requires passing native integration records for this exact executable. Those checks open Windows UI, so they have not been run for this candidate. This is a local candidate and source update; the published 2.3.0 release remains available.
