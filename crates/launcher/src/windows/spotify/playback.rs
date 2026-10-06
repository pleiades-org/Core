//! Explicit device selection and confirmation for what the person chose: a song, or a
//! playlist, album or artist played as a whole.
use super::api::{
    valid_album_uri, valid_collection_uri, valid_track_uri, ApiError, ApiResult, Client,
};
use super::encoding::url_encode;
use core_engine::search::{PlayMode, SongStatus};
use std::{sync::Arc, thread, time::Duration};
use windows::{core::HSTRING, Data::Json::JsonObject};

const DEVICES_PATH: &str = "/v1/me/player/devices";
pub(super) const PLAYBACK_PATH: &str = "/v1/me/player";
const CONFIRMATION_ATTEMPTS: usize = 5;
const CONFIRMATION_PAUSE: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct PlaybackTarget {
    pub name: Arc<str>,
}

#[derive(Clone)]
struct Device {
    id: String,
    name: Arc<str>,
    computer: bool,
    active: bool,
}

struct Reading {
    device_id: Option<String>,
    uri: Option<String>,
    linked_uri: Option<String>,
    /// The album, playlist or artist Spotify is playing from, when it plays from one.
    context_uri: Option<String>,
    /// Spotify's shuffle switch, when it reports one.
    shuffle: Option<bool>,
    /// Spotify's repeat setting: `off`, `track` or `context`.
    repeat: Option<String>,
    playing: bool,
}

/// What was sent to start playback, kept for sending again.
struct Sent<'request> {
    play: &'request str,
    body: &'request [u8],
    /// The request for shuffle or repeat, when the person asked for one.
    mode: Option<&'request str>,
}

/// What the person chose, which the confirmation afterwards looks for.
#[derive(Clone, Copy)]
enum Asked<'uri> {
    Song(&'uri str),
    Collection(&'uri str, PlayMode),
}

impl Reading {
    /// A song is confirmed by the song itself; a playlist, album or artist by being what
    /// Spotify plays from, whichever of its songs came first.
    fn selected(&self, asked: Asked, device: &Device) -> bool {
        self.device_id.as_deref() == Some(&device.id)
            && match asked {
                Asked::Song(uri) => {
                    self.uri.as_deref() == Some(uri) || self.linked_uri.as_deref() == Some(uri)
                }
                Asked::Collection(uri, _) => self.context_uri.as_deref() == Some(uri),
            }
    }

    /// Whether the shuffle or repeat that was asked for is in effect; true when none was.
    fn has_mode(&self, asked: Asked) -> bool {
        match asked {
            Asked::Collection(_, PlayMode::Shuffled) => self.shuffle == Some(true),
            Asked::Collection(_, PlayMode::Looped) => self.repeat.as_deref() == Some("context"),
            Asked::Song(_) | Asked::Collection(_, PlayMode::AsItIs) => true,
        }
    }
}

/// `album`: the album the song is on, inside which it is started; see `play_request`.
pub fn play_song(
    client: &mut Client,
    uri: &str,
    album: Option<&str>,
    computer_name: &str,
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<PlaybackTarget> {
    if !valid_track_uri(uri) {
        return Err(ApiError::invalid());
    }
    let body = play_request(uri, album);
    start(
        client,
        Asked::Song(uri),
        &body,
        computer_name,
        wait,
        cancelled,
    )
}

pub fn play_collection(
    client: &mut Client,
    uri: &str,
    mode: PlayMode,
    computer_name: &str,
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<PlaybackTarget> {
    if !valid_collection_uri(uri) {
        return Err(ApiError::invalid());
    }
    let body = collection_request(uri);
    start(
        client,
        Asked::Collection(uri, mode),
        &body,
        computer_name,
        wait,
        cancelled,
    )
}

/// The request that turns shuffle or repeat on for `device`; None for anything played as it
/// is. It has no body: the setting is in its address.
fn mode_request(asked: Asked, device: &Device) -> Option<String> {
    let setting = match asked {
        Asked::Collection(_, PlayMode::Shuffled) => "shuffle?state=true",
        Asked::Collection(_, PlayMode::Looped) => "repeat?state=context",
        Asked::Song(_) | Asked::Collection(_, PlayMode::AsItIs) => return None,
    };
    Some(format!(
        "/v1/me/player/{setting}&device_id={}",
        url_encode(&device.id)
    ))
}

/// Chooses the device, sends `body` to it and confirms that what was asked for plays there,
/// with the shuffle or repeat the person asked for.
fn start(
    client: &mut Client,
    asked: Asked,
    body: &str,
    computer_name: &str,
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<PlaybackTarget> {
    check_cancelled(cancelled)?;
    let devices = client
        .authenticated("GET", DEVICES_PATH, &[])
        .map_err(device_access_error)?;
    let device = choose_device(&parse_devices(&devices.body)?, computer_name)?;
    let path = format!("/v1/me/player/play?device_id={}", url_encode(&device.id));
    let mode = mode_request(asked, &device);
    // Shuffle has to be on before the playlist starts for its first song to be a random one
    // too. Spotify only takes that from a device already in use, so a refusal here is not
    // an error: the confirmation below sees to the setting once the playlist plays.
    if let (Some(mode), Asked::Collection(_, PlayMode::Shuffled)) = (&mode, asked) {
        check_cancelled(cancelled)?;
        let _ = client.authenticated_if_current("PUT", mode, &[], cancelled);
    }
    check_cancelled(cancelled)?;
    client.authenticated_if_current("PUT", &path, body.as_bytes(), cancelled)?;
    let sent = Sent {
        play: &path,
        body: body.as_bytes(),
        mode: mode.as_deref(),
    };
    confirm_playback(client, asked, &device, &sent, wait, cancelled)?;
    Ok(PlaybackTarget { name: device.name })
}

/// What Spotify is asked to play, from the song's start.
///
/// The song is started inside its album, at that song. Asked for a song on its own, Spotify's
/// desktop app accepts the request, stops what was playing and starts nothing, while its Web
/// Player and other devices play it; a song inside its album starts on all of them. Checked
/// against the desktop app on 5 October 2026. Afterwards the album plays on, as it does when
/// the song is started from its album in Spotify.
///
/// A song without a usable album address is asked for on its own, which is all that is left.
fn play_request(uri: &str, album: Option<&str>) -> String {
    match album.filter(|album| valid_album_uri(album)) {
        Some(album) => format!(
            "{{\"context_uri\":\"{album}\",\"offset\":{{\"uri\":\"{uri}\"}},\"position_ms\":0}}"
        ),
        None => format!("{{\"uris\":[\"{uri}\"],\"position_ms\":0}}"),
    }
}

/// A playlist, album or artist is asked for as the place to play from, which every Spotify
/// device accepts. It starts at its first song, or wherever Spotify's shuffle puts it.
fn collection_request(uri: &str) -> String {
    format!("{{\"context_uri\":\"{uri}\"}}")
}

pub fn wait(duration: Duration) {
    thread::sleep(duration);
}

fn device_access_error(error: ApiError) -> ApiError {
    match error.status {
        SongStatus::ConnectRequired | SongStatus::AccessDenied => ApiError {
            status: SongStatus::ConnectRequired,
            message: "Reconnect Spotify to allow playback-state access.",
        },
        _ => error,
    }
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> ApiResult<()> {
    if cancelled() {
        return Err(ApiError::cancelled());
    }
    Ok(())
}

fn choose_device(devices: &[Device], computer_name: &str) -> ApiResult<Device> {
    // Respect the device chosen in Spotify, including the Web Player desktop workaround.
    if let Some(device) = devices.iter().find(|device| device.active) {
        return Ok(device.clone());
    }
    if let Some(device) = devices.iter().find(|device| {
        device.computer
            && !computer_name.is_empty()
            && device.name.eq_ignore_ascii_case(computer_name)
    }) {
        return Ok(device.clone());
    }
    let mut computers = devices.iter().filter(|device| device.computer);
    if let (Some(device), None) = (computers.next(), computers.next()) {
        return Ok(device.clone());
    }
    Err(ApiError {
        status: SongStatus::NetworkError,
        message: "Open Spotify on this PC, or choose a device in Spotify.",
    })
}

fn parse_devices(bytes: &[u8]) -> ApiResult<Vec<Device>> {
    let root = super::api::parse_json(bytes)?;
    let items = root
        .GetNamedArray(&HSTRING::from("devices"))
        .map_err(|_| ApiError::invalid())?;
    let mut devices = Vec::new();
    for index in 0..items.Size().map_err(|_| ApiError::invalid())?.min(64) {
        let item = items.GetObjectAt(index).map_err(|_| ApiError::invalid())?;
        let id = optional_string(&item, "id");
        let restricted = item
            .GetNamedBoolean(&HSTRING::from("is_restricted"))
            .map_err(|_| ApiError::invalid())?;
        let Some(id) = id.filter(|id| valid_device_id(id) && !restricted) else {
            continue;
        };
        devices.push(Device {
            id,
            name: super::api::bounded(super::api::string(&item, "name")?),
            computer: super::api::string(&item, "type")?.eq_ignore_ascii_case("computer"),
            active: item
                .GetNamedBoolean(&HSTRING::from("is_active"))
                .map_err(|_| ApiError::invalid())?,
        });
    }
    Ok(devices)
}

fn valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn optional_string(object: &JsonObject, name: &str) -> Option<String> {
    object
        .GetNamedString(&HSTRING::from(name))
        .ok()
        .map(|text| text.to_string())
}

fn read_playback(client: &mut Client) -> ApiResult<Option<Reading>> {
    let response = client
        .authenticated("GET", PLAYBACK_PATH, &[])
        .map_err(device_access_error)?;
    if response.status == 204 {
        return Ok(None);
    }
    let root = super::api::parse_json(&response.body)?;
    let device = root
        .GetNamedObject(&HSTRING::from("device"))
        .map_err(|_| ApiError::invalid())?;
    let item = root.GetNamedObject(&HSTRING::from("item")).ok();
    let linked = item
        .as_ref()
        .and_then(|item| item.GetNamedObject(&HSTRING::from("linked_from")).ok());
    Ok(Some(Reading {
        context_uri: root
            .GetNamedObject(&HSTRING::from("context"))
            .ok()
            .and_then(|context| optional_string(&context, "uri")),
        device_id: optional_string(&device, "id"),
        uri: item.as_ref().and_then(|item| optional_string(item, "uri")),
        linked_uri: linked
            .as_ref()
            .and_then(|linked| optional_string(linked, "uri")),
        shuffle: root.GetNamedBoolean(&HSTRING::from("shuffle_state")).ok(),
        repeat: optional_string(&root, "repeat_state"),
        playing: root
            .GetNamedBoolean(&HSTRING::from("is_playing"))
            .map_err(|_| ApiError::invalid())?,
    }))
}

const MODE_NOT_SET: &str = "It plays, but Spotify did not turn its shuffle or repeat on.";

/// Reads what Spotify plays until it is what was asked for, playing, and with the shuffle or
/// repeat that was asked for.
///
/// Spotify answers a request for shuffle or repeat with success whatever its app then does.
/// On the desktop app on 6 October 2026, asking for shuffle before the playlist started and
/// again straight after it left shuffle off, though both requests succeeded; asking only
/// before left it on. So no answer is taken on trust: the setting is asked for only when
/// Spotify's own state lacks it once the playlist plays, and again until Spotify reports it.
fn confirm_playback(
    client: &mut Client,
    asked: Asked,
    device: &Device,
    sent: &Sent,
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<()> {
    let mut resumed = false;
    let mut plays = false;
    for attempt in 0..CONFIRMATION_ATTEMPTS {
        if attempt != 0 {
            wait(CONFIRMATION_PAUSE);
        }
        check_cancelled(cancelled)?;
        let reading = read_playback(client)?;
        let selected = reading
            .as_ref()
            .is_some_and(|reading| reading.selected(asked, device));
        let playing = reading.as_ref().is_some_and(|reading| reading.playing);
        let set = reading
            .as_ref()
            .is_some_and(|reading| reading.has_mode(asked));
        match sent.mode {
            Some(_) => eprintln!(
                "Spotify playback confirmation: selected={selected}, playing={playing}, shuffle or repeat set={set}"
            ),
            None => {
                eprintln!("Spotify playback confirmation: selected={selected}, playing={playing}")
            }
        }
        plays |= selected && playing;
        if selected && playing && set {
            return Ok(());
        }
        let more = attempt + 1 < CONFIRMATION_ATTEMPTS;
        if let (true, true, Some(mode)) = (selected && playing, more, sent.mode) {
            check_cancelled(cancelled)?;
            client
                .authenticated_if_current("PUT", mode, &[], cancelled)
                .map_err(|error| ApiError {
                    status: error.status,
                    message: MODE_NOT_SET,
                })?;
        }
        // The explicit URI keeps this retry safe if another controller changes tracks.
        if selected && !playing && !resumed && attempt != 0 && more {
            check_cancelled(cancelled)?;
            client.authenticated_if_current("PUT", sent.play, sent.body, cancelled)?;
            resumed = true;
        }
    }
    Err(ApiError {
        status: SongStatus::NetworkError,
        message: match asked {
            Asked::Song(_) => "Spotify did not start the song. Try its Web Player.",
            Asked::Collection(..) if plays => MODE_NOT_SET,
            Asked::Collection(..) => "Spotify did not start the playlist. Try again in a moment.",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str, computer: bool, active: bool) -> Device {
        Device {
            id: id.into(),
            name: name.into(),
            computer,
            active,
        }
    }

    #[test]
    fn a_song_is_started_inside_its_album_and_alone_only_without_one() {
        let uri = "spotify:track:0123456789abcdefghijkl";
        let album = "spotify:album:abcdefghijkl0123456789";
        assert_eq!(
            play_request(uri, Some(album)),
            r#"{"context_uri":"spotify:album:abcdefghijkl0123456789","offset":{"uri":"spotify:track:0123456789abcdefghijkl"},"position_ms":0}"#
        );
        let alone = r#"{"uris":["spotify:track:0123456789abcdefghijkl"],"position_ms":0}"#;
        assert_eq!(play_request(uri, None), alone);
        // Anything that is not an album's address is left out rather than sent.
        for invalid in [
            "",
            "spotify:playlist:abcdefghijkl0123456789",
            "spotify:album:short",
            "spotify:album:abcdefghijkl01234567\"}",
        ] {
            assert_eq!(play_request(uri, Some(invalid)), alone, "{invalid}");
        }
    }

    #[test]
    fn a_playlist_is_asked_for_as_the_place_to_play_from() {
        assert_eq!(
            collection_request("spotify:playlist:abcdefghijkl0123456789"),
            r#"{"context_uri":"spotify:playlist:abcdefghijkl0123456789"}"#
        );
    }

    #[test]
    fn shuffle_and_repeat_are_asked_of_the_chosen_device_and_only_for_a_playlist() {
        let device = device("pc_device", "This PC", true, false);
        let playlist = "spotify:playlist:abcdefghijkl0123456789";
        assert_eq!(
            mode_request(Asked::Collection(playlist, PlayMode::Shuffled), &device).as_deref(),
            Some("/v1/me/player/shuffle?state=true&device_id=pc_device")
        );
        assert_eq!(
            mode_request(Asked::Collection(playlist, PlayMode::Looped), &device).as_deref(),
            Some("/v1/me/player/repeat?state=context&device_id=pc_device")
        );
        assert!(mode_request(Asked::Collection(playlist, PlayMode::AsItIs), &device).is_none());
        assert!(
            mode_request(Asked::Song("spotify:track:0123456789abcdefghijkl"), &device).is_none()
        );
    }

    #[test]
    fn the_active_device_is_respected_even_when_the_local_pc_is_available() {
        let devices = [
            device("phone", "Phone", false, true),
            device("pc", "This PC", true, false),
        ];
        assert_eq!(choose_device(&devices, "THIS PC").unwrap().id, "phone");
        assert_eq!(choose_device(&devices, "Unknown").unwrap().id, "phone");
        let devices = [
            device("pc", "This PC", true, false),
            device("web", "Web Player (Chrome)", true, true),
        ];
        assert_eq!(choose_device(&devices, "THIS PC").unwrap().id, "web");
    }

    #[test]
    fn an_inactive_local_pc_is_used_when_no_device_is_active() {
        let devices = [
            device("other", "Other PC", true, false),
            device("pc", "This PC", true, false),
        ];
        assert_eq!(choose_device(&devices, "THIS PC").unwrap().id, "pc");
        assert_eq!(choose_device(&devices[1..], "Renamed PC").unwrap().id, "pc");
    }

    #[test]
    fn missing_or_ambiguous_inactive_devices_require_a_spotify_device_choice() {
        assert!(choose_device(&[], "This PC").is_err());
        assert!(choose_device(
            &[
                device("one", "One", true, false),
                device("two", "Two", true, false)
            ],
            ""
        )
        .is_err());
    }

    #[test]
    fn a_different_device_or_track_is_not_a_playback_confirmation() {
        let device = device("pc", "This PC", true, false);
        let mut reading = Reading {
            device_id: Some("phone".into()),
            uri: Some("selected".into()),
            linked_uri: None,
            context_uri: None,
            shuffle: Some(false),
            repeat: Some("off".into()),
            playing: true,
        };
        assert!(!reading.selected(Asked::Song("selected"), &device));
        reading.device_id = Some("pc".into());
        assert!(reading.selected(Asked::Song("selected"), &device));
        assert!(!reading.selected(Asked::Song("different"), &device));
        reading.linked_uri = Some("original".into());
        assert!(reading.selected(Asked::Song("original"), &device));
        // A playlist is what Spotify plays from, not the song that happens to play.
        let playlist = |uri| Asked::Collection(uri, PlayMode::AsItIs);
        assert!(!reading.selected(playlist("selected"), &device));
        reading.context_uri = Some("playlist".into());
        assert!(reading.selected(playlist("playlist"), &device));
        assert!(reading.selected(Asked::Collection("playlist", PlayMode::Shuffled), &device));
        assert!(!reading.selected(playlist("another"), &device));
        reading.device_id = Some("phone".into());
        assert!(!reading.selected(playlist("playlist"), &device));

        // Shuffle and repeat count only when Spotify itself reports them.
        let shuffled = Asked::Collection("playlist", PlayMode::Shuffled);
        let looped = Asked::Collection("playlist", PlayMode::Looped);
        assert!(reading.has_mode(playlist("playlist")) && reading.has_mode(Asked::Song("song")));
        assert!(!reading.has_mode(shuffled) && !reading.has_mode(looped));
        reading.shuffle = Some(true);
        assert!(reading.has_mode(shuffled) && !reading.has_mode(looped));
        // Repeating one song is not repeating the playlist.
        reading.repeat = Some("track".into());
        assert!(!reading.has_mode(looped));
        reading.repeat = Some("context".into());
        assert!(reading.has_mode(looped));
        (reading.shuffle, reading.repeat) = (None, None);
        assert!(!reading.has_mode(shuffled) && !reading.has_mode(looped));
        for invalid in ["", "bad&id=other", "bad\r\nheader"] {
            assert!(!valid_device_id(invalid));
        }
    }
}
