//! Programs with sound in Windows' volume mixer, as `@volume` lists them. The launcher reads
//! and sets the levels; this module only orders the list and steps a level.
use super::VolumeLevel;
use std::sync::Arc;

/// The identifier of the row for the whole PC's volume, which is no program's.
pub const SYSTEM_VOLUME_ID: &str = "system";
/// How far Left and Right move a volume, in percent.
pub const VOLUME_STEP: u8 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixerApp {
    /// Stable while the program runs: its executable's path in lowercase, or
    /// `SYSTEM_VOLUME_ID`.
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub level: VolumeLevel,
    /// Playing sound right now.
    pub active: bool,
}

impl MixerApp {
    pub fn is_system(&self) -> bool {
        &*self.id == SYSTEM_VOLUME_ID
    }
}

/// The next level one step louder or quieter. Levels between steps go to the nearest step in
/// that direction, so 62% becomes 65% or 60%.
pub fn step_volume(percent: u8, louder: bool) -> u8 {
    let percent = percent.min(VolumeLevel::MAX_PERCENT);
    if louder {
        ((percent / VOLUME_STEP + 1) * VOLUME_STEP).min(VolumeLevel::MAX_PERCENT)
    } else {
        (percent.div_ceil(VOLUME_STEP).saturating_sub(1)) * VOLUME_STEP
    }
}

/// The whole PC's volume first, then programs playing now, then the rest, each by name.
pub fn sort_mixer(apps: &mut [MixerApp]) {
    apps.sort_by_cached_key(|app| (!app.is_system(), !app.active, app.name.to_lowercase()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, active: bool) -> MixerApp {
        MixerApp {
            id: id.into(),
            name: name.into(),
            level: VolumeLevel::new(50, false),
            active,
        }
    }

    #[test]
    fn steps_land_on_multiples_of_five_and_stop_at_the_ends() {
        for (percent, louder, quieter) in [
            (0, 5, 0),
            (3, 5, 0),
            (5, 10, 0),
            (60, 65, 55),
            (62, 65, 60),
            (96, 100, 95),
            (100, 100, 95),
            (250, 100, 95),
        ] {
            assert_eq!(step_volume(percent, true), louder, "{percent} louder");
            assert_eq!(step_volume(percent, false), quieter, "{percent} quieter");
        }
    }

    #[test]
    fn the_pc_comes_first_then_what_plays_now_then_the_rest_by_name() {
        let mut apps = vec![
            app(r"c:\apps\zeta.exe", "zeta", true),
            app(r"c:\apps\idle.exe", "Idle", false),
            app(SYSTEM_VOLUME_ID, "System volume", false),
            app(r"c:\apps\alpha.exe", "Alpha", true),
            app(r"c:\apps\beta.exe", "beta", false),
        ];
        sort_mixer(&mut apps);
        let names: Vec<&str> = apps.iter().map(|app| &*app.name).collect();
        assert_eq!(names, ["System volume", "Alpha", "zeta", "beta", "Idle"]);
        assert!(apps[0].is_system() && !apps[1].is_system());
    }
}
