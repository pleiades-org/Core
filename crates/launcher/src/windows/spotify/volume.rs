//! Spotify's own volume on the device it is playing on, for the now-playing bar's slider. It
//! follows playback to a phone or speaker, where Windows' mixer has nothing to change.
use super::{
    api::{parse_json, ApiError, ApiResult, Client},
    playback::PLAYBACK_PATH,
};
use core_engine::{media::VolumeLevel, search::SongStatus};
use windows::core::HSTRING;

const VOLUME_PATH: &str = "/v1/me/player/volume";

/// The active device's volume, from 0 to 100.
pub fn read_volume(client: &mut Client) -> ApiResult<u8> {
    let response = client.authenticated("GET", PLAYBACK_PATH, &[])?;
    // Spotify answers without content when no device is active.
    if response.status == 204 {
        return Err(unavailable(
            "No Spotify device is active. Start playback in Spotify, then try again.",
        ));
    }
    parse_volume(&response.body)
}

/// Sets the active device's volume. The request has no body: the level is in its address.
pub fn set_volume(
    client: &mut Client,
    percent: u8,
    cancelled: &impl Fn() -> bool,
) -> ApiResult<()> {
    let path = format!(
        "{VOLUME_PATH}?volume_percent={}",
        percent.min(VolumeLevel::MAX_PERCENT)
    );
    client
        .authenticated_if_current("PUT", &path, &[], cancelled)
        .map(drop)
}

fn parse_volume(bytes: &[u8]) -> ApiResult<u8> {
    let device = parse_json(bytes)?
        .GetNamedObject(&HSTRING::from("device"))
        .map_err(|_| ApiError::invalid())?;
    // Devices with a fixed volume say so, or report no level at all.
    let adjustable = device
        .GetNamedBoolean(&HSTRING::from("supports_volume"))
        .unwrap_or(true);
    let percent = device
        .GetNamedNumber(&HSTRING::from("volume_percent"))
        .ok()
        .filter(|percent| adjustable && percent.is_finite());
    match percent {
        Some(percent) => Ok(percent
            .clamp(0., f64::from(VolumeLevel::MAX_PERCENT))
            .round() as u8),
        None => Err(unavailable(
            "This Spotify device does not let apps change its volume.",
        )),
    }
}

fn unavailable(message: &'static str) -> ApiError {
    ApiError {
        status: SongStatus::NetworkError,
        message,
    }
}
