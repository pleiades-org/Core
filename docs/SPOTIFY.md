# Optional Spotify song search

**Settings → Music → Spotify song search** enables catalog search and playback. It is off by default. Existing media controls and the now-playing bar work without connecting an account.

## Set up once

1. Open [Spotify's developer dashboard](https://developer.spotify.com/dashboard) using your Premium account and create a personal app. Select **Web API**.
2. In the app's settings, add this exact Redirect URI: **`http://127.0.0.1:43821/callback`**. Copy the **Client ID**; Core does not need a client secret.
3. In Core's Spotify song search settings, turn the switch on and paste the Client ID.
4. Click **Connect Spotify** and approve the connection in your browser. Core receives the result through a listener restricted to this PC. Sign-in expires after three minutes.
5. If another account will use this app, add it under the Spotify app's **Users Management**. Spotify currently permits up to five allowed accounts per development app, and the app owner must have Premium. Each account needs Premium for direct playback. See [Spotify's access limits](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) and [playback requirements](https://developer.spotify.com/documentation/web-api/reference/start-a-users-playback).

## Search and play

Type **`@song basorexia`**, or search by song and artist together. Matching songs appear in Core's existing results list with title, artist, album and artwork. Use the arrows to select a result and press **Enter** to play it. Core stays open.

Playback goes to your **active Spotify device**, which can be your PC, phone or another Spotify Connect device. To play on this PC, open Spotify here and play a track once before using Core's song results. Core displays an actionable error if Spotify has no available active device, refuses access, or reaches its request limit.

Only an explicit `@song` query is sent to Spotify. Searches wait for a short typing pause, newer queries replace queued ones, and responses for older text are discarded. General application searches remain local. Album artwork is downloaded from Spotify's image host and cached in memory.

Turning the switch off stops new Spotify requests and discards pending song work. An already-started request may finish, but its result is discarded. Your saved account connection is kept so you can enable the feature again. **Disconnect** deletes the saved credentials on this PC.

## Account storage

The public Client ID and enable switch are stored with Music settings. The refresh token is stored separately in `%APPDATA%\Pleiades\Core\v2\spotify-token.bin`, protected with Windows DPAPI for the current user. Core refreshes expiring access tokens on demand; it does not poll Spotify while idle. Credentials and authorization responses are never written to the log.

Search and playback use the official Web API and authorization with PKCE. Core does not embed a client secret or share a developer account across its users. A personal app allows you to use the feature without relying on approval for a shared public Spotify integration.

## Validation

Offscreen checks cover explicit command parsing, selected-track playback, stale query rejection, configuration cancellation, PKCE's published test vector, callback state and path validation, encrypted credential storage, token rotation, URI and image-host validation, rate-limit backoff, HTTP methods/headers/bodies through a local server, artwork decoding in memory, and settings compatibility. These checks do not open Core or a browser and do not change playback.

Live Spotify authorization and playback, and the new settings page's visual and assistive-technology acceptance, require manual verification. They have not been run automatically.
