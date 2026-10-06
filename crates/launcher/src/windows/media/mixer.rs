//! Windows' volume mixer for `@volume`: the whole PC's volume and every program that has
//! sound, read and changed on the media worker. Nothing here talks to the programs themselves.
use super::{
    app_volume::{audio_sessions, output_devices},
    window_players::executable_path,
    MixerChange,
};
use core_engine::media::{known_program, sort_mixer, MixerApp, VolumeLevel, SYSTEM_VOLUME_ID};
use std::{collections::HashMap, sync::Arc};
use windows::{
    core::{w, Result, PCWSTR},
    Win32::{
        Media::Audio::{eMultimedia, eRender, Endpoints::IAudioEndpointVolume, ISimpleAudioVolume},
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::Com::CLSCTX_ALL,
    },
};

const SYSTEM_VOLUME_NAME: &str = "System volume";
/// A program's description is shown on one line; longer ones are cut here.
const NAME_LIMIT: usize = 64;

/// One program's row and the Windows objects that change it: one for each output device the
/// program plays on.
struct Program {
    app: MixerApp,
    sessions: Vec<ISimpleAudioVolume>,
}

/// The mixer as last read. Changes go to the rows of that reading.
#[derive(Default)]
pub(super) struct Mixer {
    /// The default output device's own volume, which the keyboard's volume keys change.
    system: Option<IAudioEndpointVolume>,
    programs: Vec<Program>,
    /// Names already found, by program path, so no file is read twice for its description.
    names: HashMap<Arc<str>, Arc<str>>,
}

impl Mixer {
    /// Reads the mixer again: the PC's volume first, then programs playing now, then the rest.
    pub fn read(&mut self) -> Result<Vec<MixerApp>> {
        let devices = output_devices()?;
        self.system = None;
        self.programs.clear();
        let mut apps = Vec::new();
        // No default device, as on a PC without speakers, leaves only the programs.
        let system = unsafe { devices.GetDefaultAudioEndpoint(eRender, eMultimedia) }.and_then(
            |device| unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) },
        );
        if let Ok(system) = system {
            apps.push(MixerApp {
                id: SYSTEM_VOLUME_ID.into(),
                name: SYSTEM_VOLUME_NAME.into(),
                level: VolumeLevel::from_scalar(
                    unsafe { system.GetMasterVolumeLevelScalar()? },
                    unsafe { system.GetMute()? }.as_bool(),
                ),
                active: false,
            });
            self.system = Some(system);
        }
        let mut paths: HashMap<u32, Option<String>> = HashMap::new();
        for session in audio_sessions(&devices)? {
            let Some(path) = paths
                .entry(session.process)
                .or_insert_with(|| executable_path(session.process))
                .clone()
            else {
                continue;
            };
            // A session that ended between listing and reading is left out.
            let level = unsafe { session.volume.GetMasterVolume() }.and_then(|level| {
                Ok(VolumeLevel::from_scalar(
                    level,
                    unsafe { session.volume.GetMute()? }.as_bool(),
                ))
            });
            let Ok(level) = level else {
                continue;
            };
            self.add_session(&path, level, session.active, session.volume);
        }
        apps.extend(self.programs.iter().map(|program| program.app.clone()));
        sort_mixer(&mut apps);
        Ok(apps)
    }

    /// A program is one row however many processes and devices it plays through. Its level
    /// is that of a session playing now, when it has one.
    fn add_session(
        &mut self,
        path: &str,
        level: VolumeLevel,
        active: bool,
        volume: ISimpleAudioVolume,
    ) {
        let id: Arc<str> = path.to_lowercase().into();
        match self
            .programs
            .iter_mut()
            .find(|program| program.app.id == id)
        {
            Some(program) => {
                if active && !program.app.active {
                    program.app.level = level;
                    program.app.active = true;
                }
                program.sessions.push(volume);
            }
            None => {
                let name = self
                    .names
                    .entry(id.clone())
                    .or_insert_with(|| program_name(path))
                    .clone();
                self.programs.push(Program {
                    app: MixerApp {
                        id,
                        name,
                        level,
                        active,
                    },
                    sessions: vec![volume],
                });
            }
        }
    }

    /// Applies a change to a row of the last reading. False when that row is no longer there.
    /// Raising a volume above zero also ends a mute, as in Windows' own mixer.
    pub fn change(&self, id: &str, change: MixerChange) -> Result<bool> {
        if id == SYSTEM_VOLUME_ID {
            let Some(system) = &self.system else {
                return Ok(false);
            };
            unsafe {
                match change {
                    MixerChange::Volume(percent) => {
                        system.SetMasterVolumeLevelScalar(
                            VolumeLevel::scalar(percent),
                            std::ptr::null(),
                        )?;
                        if percent > 0 {
                            system.SetMute(false, std::ptr::null())?;
                        }
                    }
                    MixerChange::Muted(muted) => system.SetMute(muted, std::ptr::null())?,
                }
            }
            return Ok(true);
        }
        let Some(program) = self.programs.iter().find(|program| &*program.app.id == id) else {
            return Ok(false);
        };
        for session in &program.sessions {
            unsafe {
                match change {
                    MixerChange::Volume(percent) => {
                        session.SetMasterVolume(VolumeLevel::scalar(percent), std::ptr::null())?;
                        if percent > 0 {
                            session.SetMute(false, std::ptr::null())?;
                        }
                    }
                    MixerChange::Muted(muted) => session.SetMute(muted, std::ptr::null())?,
                }
            }
        }
        Ok(true)
    }
}

/// What a program is called in the mixer: the name Core knows it by, else the description
/// in its file, else its file name.
fn program_name(path: &str) -> Arc<str> {
    if let Some(known) = known_program(path) {
        return known.name.into();
    }
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    file_description(path)
        .filter(|description| !description.trim().is_empty())
        .map_or_else(|| stem.into(), |description| bounded(&description))
}

fn bounded(text: &str) -> Arc<str> {
    let text = text.trim();
    match text.char_indices().nth(NAME_LIMIT) {
        Some((end, _)) => format!("{}…", &text[..end]).into(),
        None => text.into(),
    }
}

/// The `FileDescription` in a program's version information, in its first listed language.
fn file_description(path: &str) -> Option<String> {
    let wide_path = crate::windows::wide(path);
    let file = PCWSTR(wide_path.as_ptr());
    let size = unsafe { GetFileVersionInfoSizeW(file, None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0_u8; size as usize];
    unsafe { GetFileVersionInfoW(file, None, size, block.as_mut_ptr().cast()) }.ok()?;
    let query = |name: PCWSTR| -> Option<&[u16]> {
        let mut value = std::ptr::null_mut();
        let mut length = 0_u32;
        let found = unsafe { VerQueryValueW(block.as_ptr().cast(), name, &mut value, &mut length) };
        (found.as_bool() && !value.is_null() && length > 0)
            // The value points into `block`, which outlives the slice.
            .then(|| unsafe { std::slice::from_raw_parts(value.cast::<u16>(), length as usize) })
    };
    // Pairs of language and code page; a string table is named after one pair.
    let translation = query(w!("\\VarFileInfo\\Translation")).filter(|pairs| pairs.len() >= 2)?;
    let name = crate::windows::wide(&format!(
        "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
        translation[0], translation[1]
    ));
    let description = query(PCWSTR(name.as_ptr()))?;
    let text = String::from_utf16_lossy(description);
    Some(text.trim_end_matches('\0').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_named_as_core_knows_them_or_by_their_own_description() {
        // Helium keeps the executable name chrome.exe; its folder says which browser it is.
        assert_eq!(
            &*program_name(r"C:\Users\me\AppData\Local\imput\Helium\Application\chrome.exe"),
            "Helium"
        );
        assert_eq!(
            &*program_name(r"C:\Users\me\AppData\Roaming\Spotify\Spotify.exe"),
            "Spotify"
        );
        // A file that is not there has no description: its name without the ending is used.
        assert_eq!(&*program_name(r"C:\Missing\Some Game.exe"), "Some Game");
        assert_eq!(&*program_name("plain"), "plain");
    }

    #[test]
    fn a_windows_program_is_named_by_the_description_in_its_file() {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let notepad = format!(r"{system}\System32\notepad.exe");
        let description = file_description(&notepad).expect("notepad's description");
        assert!(!description.trim().is_empty(), "{description:?}");
        assert!(!description.contains('\0'));
        assert_eq!(&*program_name(&notepad), description.trim());
        assert!(file_description(r"C:\Missing\nothing.exe").is_none());
    }

    #[test]
    fn long_descriptions_are_cut_at_a_character_boundary() {
        let long = "é".repeat(NAME_LIMIT + 5);
        let cut = bounded(&long);
        assert_eq!(cut.chars().count(), NAME_LIMIT + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(&*bounded("  Rocket League  "), "Rocket League");
    }

    /// Prints the mixer as `@volume` would list it. Reads only.
    /// Run by hand: `cargo test -p core-launcher-v2 mixer_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the person's own volume mixer"]
    fn mixer_probe() {
        let _runtime = crate::windows::TestRuntime::enter();
        let mut mixer = Mixer::default();
        for app in mixer.read().expect("Windows' volume mixer") {
            println!(
                "{:?} {}% muted {} playing {} ({})",
                app.name, app.level.percent, app.level.muted, app.active, app.id
            );
        }
    }

    /// Sets every row to the level and mute it already has, through the path a change takes.
    /// Run by hand: `cargo test -p core-launcher-v2 mixer_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore = "writes the person's own volume mixer"]
    fn mixer_round_trip() {
        let _runtime = crate::windows::TestRuntime::enter();
        let mut mixer = Mixer::default();
        let before = mixer.read().expect("Windows' volume mixer");
        for app in &before {
            // Setting a volume above zero would end a mute, so a muted row keeps to its mute.
            let change = if app.level.muted {
                MixerChange::Muted(true)
            } else {
                MixerChange::Volume(app.level.percent)
            };
            assert_eq!(
                mixer.change(&app.id, change).ok(),
                Some(true),
                "{}",
                app.name
            );
        }
        assert_eq!(
            mixer
                .change("no such program", MixerChange::Muted(true))
                .ok(),
            Some(false)
        );
        assert_eq!(mixer.read().expect("Windows' volume mixer"), before);
    }
}
