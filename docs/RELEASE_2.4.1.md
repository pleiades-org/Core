# Core 2.4.1 candidate

Selecting a Spotify song now explicitly requests playback from 0:00. The previous request supplied only the track URI and left the starting position unspecified.

Playback status and failures stay visible for the selected song when Core refreshes its footer. Changing the query or selected result shows the appropriate message; a completion for an older song cannot replace a newer song's notice. Successful API responses say playback was requested, rather than claiming Core verified audible playback. The log records playback HTTP status or a fixed transport error without credentials, request bodies, or search text.

## Validation

The reported failure was an existing song stopping without the selected result starting. Read-only checks confirmed that the saved connection grants playback permission and Spotify reports the selected catalog track as playable. Enter dispatch reaches the selected-track API; it does not issue a pause or toggle command.

The overwritten playback notice is a confirmed defect. An unspecified start position is a possible contributor to the playback failure; the 0:00 request has not been verified against live Spotify playback. This candidate requires a manual retry before the reported playback issue can be considered resolved.

Offscreen checks cover the exact selected URI and zero starting position, preserved playback errors, immediate queue failures, and rejected stale completions. Existing Spotify checks, formatting, and Clippy with warnings denied pass. The previous mocks checked HTTP acceptance but did not verify playback in a real Spotify client or retention of status across footer refreshes; the new tests cover the latter gap.

No Core windows, browser sign-ins, or live playback commands were started during verification. The candidate installer is built locally; it is not published through automatic updates, whose normal packaging gates require native UI checks.
