//! Explicit device selection and confirmation for a user-selected song.
use super::api::{valid_track_uri, ApiError, ApiResult, Client};
use super::encoding::url_encode;
use core_engine::search::SongStatus;
use std::{sync::Arc, thread, time::Duration};
use windows::{core::HSTRING, Data::Json::JsonObject};

const DEVICES_PATH: &str = "/v1/me/player/devices";
const PLAYBACK_PATH: &str = "/v1/me/player";
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
    playing: bool,
}

impl Reading {
    fn selected(&self, uri: &str, device: &Device) -> bool {
        self.device_id.as_deref() == Some(&device.id)
            && (self.uri.as_deref() == Some(uri) || self.linked_uri.as_deref() == Some(uri))
    }
}

pub fn play_song(
    client: &mut Client,
    uri: &str,
    computer_name: &str,
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<PlaybackTarget> {
    if !valid_track_uri(uri) {
        return Err(ApiError::invalid());
    }
    check_cancelled(cancelled)?;
    let devices = client
        .authenticated("GET", DEVICES_PATH, &[])
        .map_err(device_access_error)?;
    let device = choose_device(&parse_devices(&devices.body)?, computer_name)?;
    let path = format!("/v1/me/player/play?device_id={}", url_encode(&device.id));
    let body = format!("{{\"uris\":[\"{uri}\"],\"position_ms\":0}}");
    check_cancelled(cancelled)?;
    client.authenticated_if_current("PUT", &path, body.as_bytes(), cancelled)?;
    confirm_playback(
        client,
        uri,
        &device,
        &path,
        body.as_bytes(),
        wait,
        cancelled,
    )?;
    Ok(PlaybackTarget { name: device.name })
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
        device_id: optional_string(&device, "id"),
        uri: item.as_ref().and_then(|item| optional_string(item, "uri")),
        linked_uri: linked
            .as_ref()
            .and_then(|linked| optional_string(linked, "uri")),
        playing: root
            .GetNamedBoolean(&HSTRING::from("is_playing"))
            .map_err(|_| ApiError::invalid())?,
    }))
}

fn confirm_playback(
    client: &mut Client,
    uri: &str,
    device: &Device,
    path: &str,
    body: &[u8],
    wait: impl Fn(Duration),
    cancelled: &impl Fn() -> bool,
) -> ApiResult<()> {
    let mut resumed = false;
    for attempt in 0..CONFIRMATION_ATTEMPTS {
        if attempt != 0 {
            wait(CONFIRMATION_PAUSE);
        }
        check_cancelled(cancelled)?;
        let reading = read_playback(client)?;
        let selected = reading
            .as_ref()
            .is_some_and(|reading| reading.selected(uri, device));
        let playing = reading.as_ref().is_some_and(|reading| reading.playing);
        eprintln!("Spotify playback confirmation: selected={selected}, playing={playing}");
        if selected && playing {
            return Ok(());
        }
        // The explicit URI keeps this retry safe if another controller changes tracks.
        if selected && !playing && !resumed && attempt != 0 && attempt + 1 < CONFIRMATION_ATTEMPTS {
            check_cancelled(cancelled)?;
            client.authenticated_if_current("PUT", path, body, cancelled)?;
            resumed = true;
        }
    }
    Err(ApiError {
        status: SongStatus::NetworkError,
        message: "Spotify did not start the song. Try its Web Player.",
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
            playing: true,
        };
        assert!(!reading.selected("selected", &device));
        reading.device_id = Some("pc".into());
        assert!(reading.selected("selected", &device));
        assert!(!reading.selected("different", &device));
        reading.linked_uri = Some("original".into());
        assert!(reading.selected("original", &device));
        for invalid in ["", "bad&id=other", "bad\r\nheader"] {
            assert!(!valid_device_id(invalid));
        }
    }
}
