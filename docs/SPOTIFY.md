# Optional Spotify song search

**Settings → Music → Spotify song search** enables catalog search and playback. It is off by default. Existing media controls and the now-playing bar work without connecting an account.

## Set up once

1. Open [Spotify's developer dashboard](https://developer.spotify.com/dashboard) using your Premium account and create a personal app. Select **Web API**.
2. In the app's settings, add this exact Redirect URI: **`http://127.0.0.1:43821/callback`**. Copy the **Client ID**; Core does not need a client secret.
3. In Core's Spotify song search settings, turn the switch on and paste the Client ID.
4. Click **Connect Spotify** and approve the connection in your browser. Core receives the result through a listener restricted to this PC. Sign-in expires after three minutes.
5. If another account will use this app, add it under the Spotify app's **Users Management**. Spotify currently permits up to five allowed accounts per development app, and the app owner must have Premium. Each account needs Premium for direct playback. See [Spotify's access limits](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) and [playback requirements](https://developer.spotify.com/documentation/web-api/reference/start-a-users-playback).

## Search and play

Type **`@song basorexia`**, or search by song and artist together. Matching songs appear in Core's existing results list with title, artist, album and artwork. Use the arrows to select a result and press **Enter** to play it from 0:00. Core stays open. The footer keeps playback status or errors visible for that result.

Core uses your **active Spotify device**, respecting the device you choose in Spotify. If none is active, it prefers this PC by name, then a single available computer. Open Spotify here so it appears in the device list. Restricted devices and devices without a usable ID are excluded; ambiguous inactive computers require you to choose a device in Spotify.

After sending the selected song to that device, Core checks that the same song is playing there before showing **Playing in Spotify**. It checks briefly after Enter, without polling while idle. If the selected song remains paused, Core retries that exact song once. An accepted request without confirmed playback shows an error instead of success.

The connection needs both playback-control and playback-state permissions. Connections made before Core 2.4.2 may need **Connect Spotify** once more to approve playback-state access; Core explains when this is required.

### Desktop playback does not start

Other developers have [reported desktop playback stopping after an accepted Web API request](https://community.spotify.com/t5/Spotify-for-Developers/Web-API-me-player-play-returns-204-but-does-not-start-playback/td-p/7552922), including reproduction through Spotify's API console. Their reported workaround is to use Spotify's Web Player. This is a community report, not a confirmed Spotify diagnosis, but it matches the observed Core failure.

1. Open [Spotify's Web Player](https://open.spotify.com/) yourself and start a song there.
2. In Spotify's device picker, select **Web Player** as the playback device.
3. Retry the `@song` result in Core 2.4.3 or later. Core keeps that active device instead of overriding it with the desktop app.

Core still requires a matching track and playing state before reporting success. This workaround has offscreen device-selection coverage; live Web Player playback requires a manual retry. Reconnecting repeatedly will not fix an already-authorized connection with this failure.

Only an explicit `@song` query is sent to Spotify. Searches wait for a short typing pause, newer queries replace queued ones, and responses for older text are discarded. General application searches remain local. Album artwork is downloaded from Spotify's image host and cached in memory.

Turning the switch off stops new Spotify requests and discards pending song work. An already-started request may finish, but its result is discarded. Your saved account connection is kept so you can enable the feature again. **Disconnect** deletes the saved credentials on this PC.

## Account storage

The public Client ID and enable switch are stored with Music settings. The refresh token is stored separately in `%APPDATA%\Pleiades\Core\v2\spotify-token.bin`, protected with Windows DPAPI for the current user. Core refreshes expiring access tokens on demand; it does not poll Spotify while idle. Credentials and authorization responses are never written to the log.

Search and playback use the official Web API and authorization with PKCE. Core does not embed a client secret or share a developer account across its users. A personal app allows you to use the feature without relying on approval for a shared public Spotify integration.

## Validation

Offscreen checks cover explicit command parsing, device selection, exact-track playback confirmation, accepted requests that never start playback, bounded retry of a paused selected song, stale query rejection, cancellation during token refresh, PKCE's published test vector, callback state and path validation, encrypted credential storage, token rotation, URI and image-host validation, rate-limit backoff, HTTP methods/headers/bodies through a local server, artwork decoding in memory, and settings compatibility. These checks do not open Core or a browser and do not change playback.

Authorization and read-only device/state requests have been checked with the user's connection. Live playback and the settings page's visual and assistive-technology acceptance still require manual verification. They have not been run automatically.
