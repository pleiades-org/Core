# Core 2.4.2 candidate

Selecting an `@song` result now targets a specific Spotify device and confirms that the selected track is playing on it. Core prefers the available local PC, even when inactive, then the active device, then a single available computer. Restricted devices and unusable device IDs are excluded.

Playback starts from 0:00. Core briefly checks the selected device and track after the request; a track Spotify relinks is recognized through its original URI. If the selected track remains paused, Core retries that exact track once, with a subsequent state check. It never resumes a different track as a recovery step. The footer reports confirmed playback or a failure, rather than treating HTTP acceptance as proof that music started.

The Spotify connection now requests playback-state permission alongside playback control. Existing connections without it are prompted to reconnect. Playback cancellation is checked after token refresh and before sending or retrying a command. There is no idle polling and no change to the existing Windows media controls or settings layout.

## Evidence and validation

The reported playback failure persisted in 2.4.1: Spotify accepted several playback requests with HTTP 204, while the selected song did not start. The original requests omitted a device ID. After the user approved read permission, read-only Spotify requests showed this PC available but inactive, with no current playback state. These observations establish missing device targeting and playback confirmation; they do not establish that this candidate has restored audible playback.

All 33 targeted offscreen tests pass: 25 Spotify checks, three playback-notice checks, and five HTTP checks. They cover explicit device targeting, exact song and position payloads, inactive local-PC preference, active-device fallback, ambiguous or restricted devices, missing read permission, track relinking, successful HTTP responses without playback, bounded retry of the paused selected song, cancellation during token refresh, stale completion handling, and HTTP request framing through a loopback server. Formatting, Clippy with warnings denied, and the optimized build pass.

The user completed Spotify authorization in their browser. Verification did not open Core, automate onscreen tests, or send live playback commands. The candidate installer is built locally for a manual playback retry. It is not published through automatic updates; the normal release packaging gates require native UI checks.
