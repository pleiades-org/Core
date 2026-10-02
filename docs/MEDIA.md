# Media controls

Core controls the music and media apps Windows knows about: Spotify, Apple Music, Media Player, TIDAL, Deezer, YouTube Music, VLC, browsers and anything else that shows in Windows' volume flyout. It reads Windows' own media sessions (`Windows.Media.Control`), so it works with every app that publishes one and needs no account, network access or app-specific plug-in.

Windows sends the keyboard's media keys to whichever player last took them, which is often a paused browser tab. Core chooses for itself, and prefers music.

## Using it

| Input | What happens |
| --- | --- |
| `@media` or `@music` | The chosen player's track, with Enter to play or pause, then **Next track**, **Previous track**, then every other open player |
| `@media spotify`, `@music chrome next` | Only that player; a final `play`, `pause`, `next` (or `skip`), `previous` (or `prev`) or `toggle` picks the control. Naming a player reaches it even when it is ignored |
| `play`, `pause`, `next`, `previous`, `prev`, `now playing` on their own | That control above the usual app results. Longer text (`playlist`, `next friday`) searches as usual |

Media controls keep Core open, so you can press Enter again to skip another track. The footer confirms what happened ("Paused Spotify", "Next track · Spotify") or explains why not ("Already playing", "Nothing is playing", "Spotify is no longer open").

## Which player Core chooses

**Settings → Music → Which player Core controls**:

| Situation | Music apps first (default) | Playing media first |
| --- | --- | --- |
| Spotify paused, a browser tab paused | Spotify | Spotify |
| Spotify paused, a browser video playing | Spotify | The browser |
| Both playing | Spotify | Spotify |

Within the same standing, apps you rank under **Preferred apps** come first, then music apps, then media players, then browsers, then other apps. Windows' own choice only breaks ties. Two typed words narrow this further:

- `pause` only considers what is playing, so it stops what you hear even with Music first.
- `play` resumes a paused player only if it ranks above everything already playing: with Spotify playing, `play` says "Already playing" rather than also starting a paused tab.

For five seconds after Core sends a control, it keeps using the same player, so quick presses do not jump to another app while a track changes.

Spotify is recognised whether it is the desktop app (`Spotify.exe`) or the Microsoft Store app; the same holds for the other apps Core knows. Apps Core does not recognise still work and rank with "other apps" until you move them up.

## Players Windows does not see

Some players publish nothing to Windows: pressing a media or volume key then shows no song in Windows' own pop-up. Spotify does this when its setting **Show desktop overlay when using media keys** is off.

- **Spotify** is then read from its window instead: the title shows "Artist - Song" while it plays, and controls go straight to Spotify's window. There is no album art (a music note shows instead) and no progress line or seeking. While Core is visible it follows Spotify's title changes; it does not poll. Turning the overlay setting on gives Core art, progress and seeking again.
- **Any other player** that Windows cannot see: `@media` and the keywords still offer Play / pause, Next track and Previous track, and send the keyboard's media keys, which Windows passes to whichever player listens for them. The bar needs a player Core can read, so it does not appear.

## Searching for a song

Optional **Settings → Music → Spotify song search** connects a personal Spotify app. Type **`@song`** followed by a song or artist, select a result, and press Enter to play it on your active Spotify device while Core stays open. This requires Spotify Premium and a one-time account connection. It is disabled by default and is separate from the existing Windows media controls. See [Spotify setup and privacy](SPOTIFY.md).

## The now-playing bar

A strip above the search box shows the chosen player's album art, title, artist and app, the elapsed and total time, a progress line, and **previous**, **play / pause** and **next** buttons.

- It shows while something plays, and after you pause from Core, so you can resume from it. A player paused in its own window does not keep it.
- When Core opens, the bar shows what was playing when it was last open, and corrects itself a moment later from a fresh reading, before you type. After that it can still appear, but it only disappears the next time Core opens: it keeps showing the same player between tracks or after a pause elsewhere, so the search box never moves under your typing.
- Click the progress line to move through the track, when the player allows it. Click the art or title to bring the player forward.
- The buttons are ordinary buttons: Tab reaches them, Enter or Space presses them, and screen readers announce them ("Previous track", "Pause", "Next track") and the track.
- Players without a track length show a divider instead of a progress line. Players without art show a music note.
- Turn it off with **Settings → Music → Now playing bar**.

The bar is hidden in Settings and while a `/` command's output is showing.

## Media shortcuts

**Settings → Music → Media shortcuts** sets keys for **Play / pause**, **Next track** and **Previous track**. Each works either **While Core is open** (default: Alt+P, Alt+→, Alt+←) or **In every app**. Media shortcuts never choose Core's own keys (Ctrl+A, Ctrl+C, arrows with Ctrl or Shift, and similar), the Open Core shortcut, or the same keys twice; Settings explains the conflict and keeps the previous shortcuts. A shortcut another app already uses in every app is refused in the same way.

### Recording a shortcut

Every shortcut box in Settings, including **Behaviour → Open Core shortcut**, records keys rather than taking typed text:

1. Click the box, or Tab to it.
2. Press the keys together. Held modifiers show as "Ctrl+Alt+…" until the key completes the shortcut; it saves at once.
3. Backspace or Delete clears a media shortcut ("Not set"). Esc closes Settings and Tab moves on, as elsewhere.

While a box records, Core releases its own shortcuts, so pressing the current shortcut records it instead of triggering it. They return when the box loses focus or Core hides. Shortcuts that another app or Windows keeps for itself (such as Win+L) never reach the box. A letter on its own, or a key Core cannot use, shows in the box and Settings explains why it is not saved.

Usable keys: letters, digits, F1–F24, arrows, Home/End, Page Up/Page Down, Insert/Delete, punctuation, number-pad keys and the keyboard's media and volume keys. F-keys and media keys work on their own; anything else needs Ctrl, Alt, Shift or Win.

## Settings file

Music settings are stored with the other preferences in `%APPDATA%\Pleiades\Core\v2\appearance.ini`, and only once they differ from the defaults:

```text
music_priority=playing-first
music_bar=false
music_shortcut=play-pause<TAB>Ctrl+Alt+P<TAB>everywhere
music_preferred=spotify<TAB>Spotify
music_ignored=chrome<TAB>Google Chrome
```

Older Core builds ignore these lines and keep working with the rest of the file.

## Cost and privacy

- Nothing media-related starts until you use a media command or open Core with the bar on. Then one worker thread reads the sessions. While Core is hidden it sleeps without event subscriptions or timers. While Core is visible it follows the players' change events, and the progress clock ticks once a second only while a track plays.
- Commands are re-checked against a fresh reading, so a press never acts on a player that closed since the results were shown. A player that does not answer within two seconds is given up on rather than stalling Core.
- Windows media session text, app names and album art stay in memory. Optional Spotify song search sends only explicit `@song` queries to Spotify and stores its connection credentials protected for your Windows user; see [Spotify setup and privacy](SPOTIFY.md).
- If Windows' media sessions are unavailable (Windows 10 before version 1809), Core still reads Spotify from its window, and otherwise sends the keyboard media keys, letting Windows choose the player.

Measured on this PC with Spotify playing (release build, `--dry-run --start-hidden`, warmed with one `@media` show and hide, then 60 seconds hidden): hidden CPU 0.0000% in both runs, and private bytes 6.2 MiB without media against 6.7 MiB with the media worker started ([without](measurements/media-idle-off.json), [with](measurements/media-idle-on.json)).

## Testing

- `cargo test --workspace` covers the ranking rules, keywords, the `@media` list, timeline arithmetic, app recognition, settings round trips and validation, the key recorder's key handling, bar geometry and album-art rounding.
- `cargo test -p core-launcher-v2 media_probe -- --ignored --nocapture` prints what Windows reports for each open player, including whether its art decodes.
- `cargo test -p core-launcher-v2 media_round_trip -- --ignored --nocapture` sends **play** to a player that is already playing (which changes nothing) through the whole worker path.
- `cargo test -p core-launcher-v2 window_player_probe -- --ignored --nocapture` prints the players found from their windows; it reads titles only.
- Dry runs (`--dry-run`) read media sessions only with `--test-media`, so integration checks do not depend on what is playing; media controls in a dry run report "Verified media control · no side effect".
