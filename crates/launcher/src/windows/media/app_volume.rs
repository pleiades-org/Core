//! A player's volume in Windows' volume mixer: the audio sessions its processes opened on any
//! output device. Read and set on the media worker. Nothing here talks to the player itself, so
//! a player that stopped answering cannot stall it.
use super::{window_players::executable_path, VolumeReading, VolumeWork};
use core_engine::media::{is_app_process, known_app_name, VolumeLevel};
use std::sync::Arc;
use windows::{
    core::{Interface, Result, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS},
        Media::Audio::{
            eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2,
            IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume,
            MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        Storage::Packaging::Appx::{
            GetApplicationUserModelId, APPLICATION_USER_MODEL_ID_MAX_LENGTH,
        },
        System::{
            Com::{CoCreateInstance, CLSCTX_ALL},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
};

/// What separates a Store app's identifier (`Publisher.Product_hash!App`) from a desktop app's.
const STORE_APP_SEPARATOR: char = '!';

/// One player's audio sessions on every active output device, the one playing first.
pub(super) struct AppVolume {
    app_id: Arc<str>,
    sessions: Vec<ISimpleAudioVolume>,
}

impl AppVolume {
    /// Finds the player's sessions as they are now. None are found while it plays no sound
    /// through this PC: stopped long enough for Windows to close them, or playing elsewhere.
    fn find(app_id: &Arc<str>) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let devices = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)? };
        let mut processes = PlayerProcesses::new(app_id);
        let mut found: Vec<(bool, ISimpleAudioVolume)> = Vec::new();
        for index in 0..unsafe { devices.GetCount()? } {
            // A device that cannot be read, such as one unplugged just now, hides no other.
            let sessions = unsafe { devices.Item(index) }
                .and_then(|device| device_sessions(&device, &mut processes));
            match sessions {
                Ok(sessions) => found.extend(sessions),
                Err(error) => eprintln!("Could not read an audio device's sessions: {error}"),
            }
        }
        found.sort_by_key(|(active, _)| !active);
        Ok(Self {
            app_id: app_id.clone(),
            sessions: found.into_iter().map(|(_, session)| session).collect(),
        })
    }

    fn is_for(&self, app_id: &str) -> bool {
        &*self.app_id == app_id
    }

    /// The player has no session to read or set.
    fn is_silent(&self) -> bool {
        self.sessions.is_empty()
    }

    fn level(&self) -> Result<Option<VolumeLevel>> {
        let Some(session) = self.sessions.first() else {
            return Ok(None);
        };
        let level = unsafe { session.GetMasterVolume()? };
        let muted = unsafe { session.GetMute()? }.as_bool();
        Ok(Some(VolumeLevel::from_scalar(level, muted)))
    }

    /// Sets every session, so the player sounds the same on each device it uses. Moving the
    /// slider above zero also ends a mute, as it does in Windows' own mixer.
    fn set(&self, percent: u8) -> Result<()> {
        for session in &self.sessions {
            unsafe {
                session.SetMasterVolume(VolumeLevel::scalar(percent), std::ptr::null())?;
                if percent > 0 {
                    session.SetMute(false, std::ptr::null())?;
                }
            }
        }
        Ok(())
    }
}

/// Does what the slider asked: a change, then a reading. A change that worked says nothing,
/// because the slider already shows what the person chose. `found` keeps the player's sessions
/// between the changes of one drag; a reading always looks for them again.
pub(super) fn perform(found: &mut Option<AppVolume>, work: &VolumeWork) -> Option<VolumeReading> {
    let level = match apply(found, work) {
        Ok(None) => return None,
        Ok(Some(level)) => Ok(level),
        Err(reason) => {
            // The player may be playing by the next time, or its sessions may answer again.
            *found = None;
            Err(reason)
        }
    };
    Some(VolumeReading {
        app_id: work.app_id.clone(),
        level,
    })
}

/// The level read, None after a change alone, or why the player has no volume here.
fn apply(
    found: &mut Option<AppVolume>,
    work: &VolumeWork,
) -> std::result::Result<Option<VolumeLevel>, String> {
    let refused =
        |error: windows::core::Error| format!("Windows' volume mixer did not answer: {error}");
    let silent = || {
        format!(
            "{} is not playing sound on this PC",
            known_app_name(&work.app_id)
        )
    };
    let reusable = found
        .take()
        .filter(|volume| !work.read && volume.is_for(&work.app_id));
    let volume = match reusable {
        Some(volume) => volume,
        None => AppVolume::find(&work.app_id).map_err(refused)?,
    };
    let volume = found.insert(volume);
    if volume.is_silent() {
        return Err(silent());
    }
    if let Some(percent) = work.set {
        volume.set(percent).map_err(refused)?;
    }
    if !work.read {
        return Ok(None);
    }
    volume
        .level()
        .map_err(refused)?
        .map(Some)
        .ok_or_else(silent)
}

/// The player's sessions on one device, each with whether it is playing now.
fn device_sessions(
    device: &IMMDevice,
    processes: &mut PlayerProcesses,
) -> Result<Vec<(bool, ISimpleAudioVolume)>> {
    let manager: IAudioSessionManager2 = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let sessions = unsafe { manager.GetSessionEnumerator()? };
    let mut found = Vec::new();
    for index in 0..unsafe { sessions.GetCount()? } {
        let control: IAudioSessionControl2 = unsafe { sessions.GetSession(index)? }.cast()?;
        let state = unsafe { control.GetState()? };
        // Process 0 is Windows' own sounds.
        let process = unsafe { control.GetProcessId() }.unwrap_or(0);
        if state == AudioSessionStateExpired || process == 0 || !processes.includes(process) {
            continue;
        }
        found.push((state == AudioSessionStateActive, control.cast()?));
    }
    Ok(found)
}

/// Which processes belong to one player; each is looked up once, however many sessions and
/// devices it has.
struct PlayerProcesses<'player> {
    app_id: &'player str,
    known: Vec<(u32, bool)>,
}

impl<'player> PlayerProcesses<'player> {
    fn new(app_id: &'player str) -> Self {
        Self {
            app_id,
            known: Vec::new(),
        }
    }

    fn includes(&mut self, process: u32) -> bool {
        if let Some((_, included)) = self.known.iter().find(|(known, _)| *known == process) {
            return *included;
        }
        let included = self.belongs(process);
        self.known.push((process, included));
        included
    }

    fn belongs(&self, process: u32) -> bool {
        // With its folders: some browsers share an executable name and differ only there.
        let Some(executable) = executable_path(process) else {
            return false;
        };
        if is_app_process(self.app_id, &executable, None) {
            return true;
        }
        // Only a Store app's processes carry an identifier Windows can be asked for.
        self.app_id.contains(STORE_APP_SEPARATOR)
            && is_app_process(self.app_id, &executable, process_app_id(process).as_deref())
    }
}

/// The identifier Windows gives a Store app's process; None for desktop apps.
fn process_app_id(process: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut text = [0_u16; APPLICATION_USER_MODEL_ID_MAX_LENGTH as usize];
    let mut length = text.len() as u32;
    let status =
        unsafe { GetApplicationUserModelId(handle, &mut length, Some(PWSTR(text.as_mut_ptr()))) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if status != ERROR_SUCCESS {
        return None;
    }
    // The length counts the terminating null.
    let length = (length as usize).saturating_sub(1).min(text.len());
    Some(String::from_utf16_lossy(&text[..length]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::media::{tests::wait_for, MediaReading, MediaService};

    fn ask(
        found: &mut Option<AppVolume>,
        app_id: &Arc<str>,
        set: Option<u8>,
        read: bool,
    ) -> Option<VolumeReading> {
        perform(
            found,
            &VolumeWork {
                app_id: app_id.clone(),
                set,
                read,
            },
        )
    }

    /// What Windows reports is playing, read once through the media worker.
    fn players() -> MediaReading {
        // A null window makes the ready notifications harmless thread messages.
        let service = MediaService::start(Default::default()).unwrap();
        service.refresh(false, 0);
        wait_for(|| service.take_reading(), "A reading")
    }

    #[test]
    fn this_process_is_not_a_store_app() {
        assert_eq!(process_app_id(std::process::id()), None);
    }

    #[test]
    fn a_player_without_sound_here_is_explained_rather_than_shown_at_zero() {
        let _runtime = crate::windows::TestRuntime::enter();
        let app_id: Arc<str> = "Core.Test.NoSuchPlayer.exe".into();
        let mut found = None;
        for (set, read) in [(None, true), (Some(40), false), (Some(40), true)] {
            let reading = ask(&mut found, &app_id, set, read).expect("an explanation");
            assert_eq!(reading.app_id, app_id);
            // Without any audio device at all, Windows refuses instead.
            let reason = reading.level.unwrap_err();
            assert!(
                reason.contains("is not playing sound") || reason.contains("did not answer"),
                "{reason}"
            );
            // Nothing is kept to ask again, so a player that starts later is found.
            assert!(found.is_none());
        }
    }

    /// Prints the mixer volume of every player Windows reports. Reads only.
    /// Run by hand: `cargo test -p core-launcher-v2 volume_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the mixer volume of the person's own players"]
    fn volume_probe() {
        let _runtime = crate::windows::TestRuntime::enter();
        let reading = players();
        println!("{} player(s)", reading.sessions.len());
        for session in &reading.sessions {
            let volume = AppVolume::find(&session.app_id).expect("Windows' volume mixer");
            println!(
                "{:?}: {} session(s), {:?}",
                session.app_id,
                volume.sessions.len(),
                volume.level()
            );
        }
    }

    /// Sets each player's volume to the percentage it already shows, through the path a drag
    /// takes. Muted players are skipped, since a change would end the mute.
    /// Run by hand: `cargo test -p core-launcher-v2 volume_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore = "writes the mixer volume of the person's own players"]
    fn volume_round_trip() {
        let _runtime = crate::windows::TestRuntime::enter();
        for session in &players().sessions {
            let mut found = None;
            let level = |reading: Option<VolumeReading>| reading.expect("a reading").level;
            match level(ask(&mut found, &session.app_id, None, true)) {
                Ok(before) if !before.muted => {
                    // A change alone says nothing; the reading after it shows it was applied.
                    assert!(
                        ask(&mut found, &session.app_id, Some(before.percent), false).is_none()
                    );
                    let after = level(ask(&mut found, &session.app_id, None, true));
                    assert_eq!(after, Ok(before));
                    println!("{:?}: stayed at {}%", session.app_id, before.percent);
                }
                other => println!("{:?}: skipped, {other:?}", session.app_id),
            }
        }
    }
}
