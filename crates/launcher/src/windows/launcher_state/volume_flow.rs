//! The now-playing bar's volume slider. It moves the player's volume in Windows' mixer, or
//! Spotify's own volume on its active device when that is switched on in Settings → Music →
//! Spotify song search. Nothing is read until the pointer reaches the bar's album art.
use super::LauncherState;
use crate::windows::{media::VolumeReading, view::BarVolume};
use core_engine::media::{is_spotify, VolumeLevel};

/// Whose volume the slider moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VolumeRoute {
    /// The player's level in Windows' volume mixer; works for every player, with no account.
    Mixer,
    /// Spotify's own volume, through the connected account: it follows playback to a phone
    /// or speaker, where Windows' mixer has nothing to change.
    Spotify,
}

fn volume_route(app_id: &str, spotify_volume: bool) -> VolumeRoute {
    if spotify_volume && is_spotify(app_id) {
        VolumeRoute::Spotify
    } else {
        VolumeRoute::Mixer
    }
}

impl LauncherState {
    fn volume_route(&self, app_id: &str) -> VolumeRoute {
        let spotify = self.spotify_settings();
        volume_route(app_id, spotify.enabled && spotify.volume)
    }

    /// The pointer reached the bar's album art: the slider it reveals needs the volume.
    pub fn media_volume_wanted(&mut self) {
        let Some(app_id) = self.view.as_ref().and_then(|view| view.media_bar_app()) else {
            return;
        };
        let asked = match self.volume_route(&app_id) {
            VolumeRoute::Spotify => self
                .spotify_service()
                .and_then(|service| service.read_volume()),
            VolumeRoute::Mixer => self
                .media_worker()
                .map(|service| service.read_volume(app_id.clone())),
        };
        if let Err(reason) = asked {
            self.volume_unavailable(&app_id, &reason);
        }
    }

    /// The person moved the slider: the player follows.
    pub fn media_volume_changed(&mut self) {
        let Some(view) = self.view.clone() else {
            return;
        };
        let (Some(app_id), Some(percent)) = (view.media_bar_app(), view.media_volume()) else {
            return;
        };
        if self.options.dry_run {
            view.set_footer("Verified volume change · no side effect");
            return;
        }
        let asked = match self.volume_route(&app_id) {
            VolumeRoute::Spotify => self
                .spotify_service()
                .and_then(|service| service.set_volume(percent)),
            VolumeRoute::Mixer => self
                .media_worker()
                .map(|service| service.set_volume(app_id.clone(), percent)),
        };
        if let Err(reason) = asked {
            self.volume_unavailable(&app_id, &reason);
        }
    }

    /// The media worker read a player's volume in Windows' mixer, or could not reach it.
    pub(super) fn receive_mixer_volume(&mut self, reading: VolumeReading) {
        // The setting may have changed while the worker read.
        if self.volume_route(&reading.app_id) != VolumeRoute::Mixer {
            return;
        }
        match reading.level {
            Ok(level) => self.show_volume(&reading.app_id, BarVolume::Level(level)),
            Err(reason) => self.volume_unavailable(&reading.app_id, &reason),
        }
    }

    /// Spotify answered for its active device; the answer is for the bar's player only while
    /// that is still Spotify with its own volume chosen.
    pub(super) fn receive_spotify_volume(&mut self, answer: Result<u8, String>) {
        let shown = self.view.as_ref().and_then(|view| view.media_bar_app());
        let Some(app_id) = shown.filter(|app_id| self.volume_route(app_id) == VolumeRoute::Spotify)
        else {
            return;
        };
        match answer {
            Ok(percent) => {
                self.show_volume(&app_id, BarVolume::Level(VolumeLevel::new(percent, false)))
            }
            Err(reason) => self.volume_unavailable(&app_id, &reason),
        }
    }

    fn show_volume(&self, app_id: &str, volume: BarVolume) {
        if let Some(view) = &self.view {
            view.set_media_volume(app_id, volume);
        }
    }

    /// The slider says so in the title's place; the footer says why.
    fn volume_unavailable(&self, app_id: &str, reason: &str) {
        self.show_volume(app_id, BarVolume::Unavailable);
        match &self.view {
            Some(view) if self.visible => view.set_footer(reason),
            _ => eprintln!("Media volume: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotifys_own_volume_is_used_only_for_spotify_and_only_when_chosen() {
        let store_spotify = "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify";
        for app_id in ["Spotify.exe", store_spotify] {
            assert_eq!(volume_route(app_id, true), VolumeRoute::Spotify);
            assert_eq!(volume_route(app_id, false), VolumeRoute::Mixer);
        }
        // Spotify's Web Player in a browser is the browser's sound in Windows' mixer.
        for app_id in ["Chrome", "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App"] {
            assert_eq!(volume_route(app_id, true), VolumeRoute::Mixer);
        }
    }
}
