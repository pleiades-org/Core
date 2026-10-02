# Core 2.4.3 candidate

Song playback now respects the active device chosen in Spotify, including its Web Player. Previously Core preferred the local desktop app even when another device was active, preventing users from selecting an alternative playback client. When no device is active, Core retains its local-PC and single-computer fallbacks. There is no settings or layout change.

If Spotify accepts playback without starting the selected track, Core suggests its Web Player. Explicit device targeting, selected-track confirmation, cancellation checks, and bounded exact-track retry remain in place; HTTP acceptance alone is never reported as verified playback.

## Investigation

The reported failure persisted in 2.4.2. Six live requests were accepted with HTTP 204 but never confirmed the selected track playing. Read-only requests showed that the saved connection grants both permissions, belongs to the expected Spotify account, and can read the active, unrestricted local PC playing music. Both catalog results named "22 Grand" are playable without a restriction, and the user confirms their selected song plays when started directly in Spotify. Exact payload and HTTP framing tests found no missing request body or malformed URI.

A [recent first-hand Spotify Community report](https://community.spotify.com/t5/Spotify-for-Developers/Web-API-me-player-play-returns-204-but-does-not-start-playback/td-p/7552922) describes the same desktop failure, reproduced through Spotify's API console, while Web Player playback succeeds. This points to a desktop/API interoperability problem; it is not an official Spotify diagnosis or proof of the exact cause on this PC. No speculative transport rewrite, extra authorization flow, or live playback command was added to the investigation.

## Workaround and validation

Open Spotify's Web Player yourself, play a song there, and select Web Player in Spotify's device picker. Then retry an `@song` result with this candidate. Core now keeps that active device rather than returning to the desktop app.

All 28 targeted Spotify tests pass, including an active Web Player beside an inactive matching local PC, the exact selected track sent only to the Web Player, active-phone selection, inactive local-PC fallback, and ambiguous devices. Formatting, Clippy with warnings denied, and the optimized build pass. Existing playback-notice and HTTP checks remain applicable; their production code is unchanged. Live Web Player playback still requires the user's retry; this candidate does not claim to fix Spotify's desktop playback service.

Verification does not open Core, automate onscreen tests, or send live playback commands. The installer is a local candidate and is not published through automatic updates, whose normal release packaging gates require native UI checks.
