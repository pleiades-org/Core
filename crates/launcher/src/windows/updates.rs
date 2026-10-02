//! Signed GitHub releases, checked in the background when Core is shown.
//! Verified downloads replace the executable only on exit or an explicit restart.
mod crypto;
mod download;
pub mod handoff;
mod manifest;
mod staging;
#[cfg(test)]
mod tests;

use manifest::Manifest;
pub use manifest::Version;
use staging::Installation;
use std::{
    fmt, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const UPDATE_READY: u32 = WM_APP + 15;
pub const RELEASE_PAGE: &str = "https://github.com/pleiades-org/Core/releases/latest";
/// Followed by a version, the release notes of that version.
pub const RELEASE_NOTES_PAGE: &str = "https://github.com/pleiades-org/Core/releases/tag/v";
const MANIFEST_PATH: &str = "/pleiades-org/Core/releases/latest/download/core-update.txt";
const MAX_MANIFEST_BYTES: usize = 4 * 1024;
const MAX_EXECUTABLE_BYTES: usize = 8 * 1024 * 1024;
const CHECK_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
/// Enter on `@update` checks GitHub at once instead of waiting for the daily check, but no more
/// often than this, so repeated presses cannot flood GitHub.
const MANUAL_CHECK_COOLDOWN: Duration = Duration::from_secs(60);
const RELEASE_KEY: &[u8; 64] = include_bytes!("../../assets/release-key.bin");

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckTrigger {
    Automatic,
    Manual,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateMode {
    #[default]
    Automatic,
    NotifyOnly,
    Off,
}
impl UpdateMode {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::NotifyOnly, Self::Off];
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::NotifyOnly => "Notify",
            Self::Off => "Off",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.label().eq_ignore_ascii_case(text))
    }
}

#[derive(Clone, Debug)]
pub enum UpdateState {
    UpToDate,
    Available(Version),
    Staged(Version),
    Failed(UpdateError),
}

#[derive(Clone, Debug)]
pub enum UpdateError {
    Network(windows::core::Error),
    Crypto(windows::core::Error),
    MalformedManifest,
    BadSignature,
    HashMismatch,
    FolderNotWritable,
    UnsafeRedirect,
    UnexpectedResponse(u32),
    Io(String),
    Handoff(String),
}
impl UpdateError {
    fn io(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Self::FolderNotWritable
        } else {
            Self::Io(error.to_string())
        }
    }
}
impl fmt::Display for UpdateError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(output, "Could not contact GitHub: {error}"),
            Self::Crypto(error) => write!(output, "Windows could not verify the update: {error}"),
            Self::MalformedManifest => output.write_str("The update manifest is invalid"),
            Self::BadSignature => output.write_str("The update signature is not trusted"),
            Self::HashMismatch => {
                output.write_str("The update download failed its integrity check")
            }
            Self::FolderNotWritable => {
                output.write_str("Core's folder is not writable; open the release page")
            }
            Self::UnsafeRedirect => output.write_str("GitHub returned an unsupported redirect"),
            Self::UnexpectedResponse(status) => write!(output, "GitHub returned HTTP {status}"),
            Self::Io(error) | Self::Handoff(error) => output.write_str(error),
        }
    }
}

struct Shared {
    mode: UpdateMode,
    state: UpdateState,
    /// The newest release a verified manifest named, cached or downloaded; None before any.
    latest: Option<Version>,
    working: bool,
    shutdown: bool,
    loaded_stage: bool,
    next_check: SystemTime,
    /// When Enter on `@update` last started a check.
    last_manual_check: Option<Instant>,
}

impl Shared {
    /// Starts an eligible check; true fetches GitHub, false only loads cached update files.
    fn begin_check(
        &mut self,
        trigger: CheckTrigger,
        wall_clock_now: SystemTime,
        monotonic_now: Instant,
    ) -> Option<bool> {
        if self.mode == UpdateMode::Off
            || self.shutdown
            || self.working
            || matches!(self.state, UpdateState::Staged(_))
        {
            return None;
        }
        let download_due = match trigger {
            CheckTrigger::Automatic => wall_clock_now >= self.next_check,
            CheckTrigger::Manual
                if !manual_check_allowed(self.last_manual_check, monotonic_now) =>
            {
                return None;
            }
            CheckTrigger::Manual => true,
        };
        if !download_due && self.loaded_stage {
            return None;
        }
        if trigger == CheckTrigger::Manual {
            self.last_manual_check = Some(monotonic_now);
        }
        self.working = true;
        self.loaded_stage = true;
        if download_due {
            self.next_check = wall_clock_now + RETRY_AFTER;
        }
        Some(download_due)
    }
}

pub struct UpdateService {
    enabled: bool,
    installation: Option<Installation>,
    cache: Option<PathBuf>,
    shared: Arc<Mutex<Shared>>,
}

impl UpdateService {
    pub fn new(enabled: bool, mode: UpdateMode) -> Self {
        let cache = if enabled {
            super::local_cache::path("update-check.txt")
        } else {
            None
        };
        let next_check = cache
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| text.trim().parse::<u64>().ok())
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
            .filter(|time| *time <= SystemTime::now() + CHECK_AFTER)
            .unwrap_or(UNIX_EPOCH);
        Self {
            enabled,
            installation: std::env::current_exe().ok().map(Installation::new),
            cache,
            shared: Arc::new(Mutex::new(Shared {
                mode,
                state: UpdateState::UpToDate,
                latest: None,
                working: false,
                shutdown: false,
                loaded_stage: false,
                next_check,
                last_manual_check: None,
            })),
        }
    }

    pub fn set_mode(&self, mode: UpdateMode) {
        let mut shared = self.shared.lock().expect("update lock");
        if shared.mode != mode {
            shared.mode = mode;
            shared.loaded_stage = false;
            // A staged download is never installed after selecting Notify or Off.
            if let UpdateState::Staged(version) = shared.state {
                shared.state = UpdateState::Available(version);
            }
        }
    }

    pub fn state(&self) -> UpdateState {
        self.shared.lock().expect("update lock").state.clone()
    }
    pub fn working(&self) -> bool {
        self.shared.lock().expect("update lock").working
    }
    /// The latest release the last check saw, for `@info`.
    pub fn latest(&self) -> Option<Version> {
        self.shared.lock().expect("update lock").latest
    }

    /// The automatic check when Core is shown: GitHub is contacted at most once a day, or an
    /// hour after a failure; between those, the cached signed manifest is used. Returns at once.
    pub fn refresh(&self, window: HWND) {
        self.start_check(window, CheckTrigger::Automatic);
    }

    /// Enter on `@update`: contact GitHub now rather than at the next daily check, at most once
    /// a minute. Returns at once; `UPDATE_READY` follows.
    pub fn check_now(&self, window: HWND) {
        self.start_check(window, CheckTrigger::Manual);
    }

    fn start_check(&self, window: HWND, trigger: CheckTrigger) {
        if !self.enabled {
            return;
        }
        let Some(installation) = self.installation.clone() else {
            return;
        };
        let download_due = {
            let mut shared = self.shared.lock().expect("update lock");
            let Some(download_due) = shared.begin_check(trigger, SystemTime::now(), Instant::now())
            else {
                return;
            };
            if download_due {
                // Closing Core during a request must not bypass the retry deadline.
                save_check_time(self.cache.as_ref(), shared.next_check);
            }
            download_due
        };
        let shared = self.shared.clone();
        let cache = self.cache.clone();
        let address = window.0 as usize;
        if let Err(error) = std::thread::Builder::new()
            .name("core-updates".into())
            .spawn(move || {
                let manifest_cache = cache
                    .as_ref()
                    .map(|path| path.with_file_name("update-manifest.txt"));
                let outcome = check_release(
                    &installation,
                    &shared,
                    download_due,
                    manifest_cache.as_deref(),
                );
                let mut state = shared.lock().expect("update lock");
                state.working = false;
                if state.shutdown || state.mode == UpdateMode::Off {
                    return;
                }
                if download_due {
                    state.next_check = SystemTime::now()
                        + if outcome.is_ok() {
                            CHECK_AFTER
                        } else {
                            RETRY_AFTER
                        };
                }
                state.state = match outcome.unwrap_or_else(UpdateState::Failed) {
                    UpdateState::Staged(version) if state.mode != UpdateMode::Automatic => {
                        UpdateState::Available(version)
                    }
                    outcome => outcome,
                };
                save_check_time(cache.as_ref(), state.next_check);
                if let Err(error) = unsafe {
                    PostMessageW(
                        Some(HWND(address as *mut _)),
                        UPDATE_READY,
                        WPARAM(0),
                        LPARAM(0),
                    )
                } {
                    eprintln!("Could not deliver update status: {error}");
                }
            })
        {
            let mut shared = self.shared.lock().expect("update lock");
            shared.working = false;
            shared.state = UpdateState::Failed(UpdateError::io(error));
        }
    }

    /// Ends background work and applies only a staged, still-verified automatic update.
    pub fn apply_staged(&self) -> Result<(), UpdateError> {
        let mut shared = self.shared.lock().expect("update lock");
        shared.shutdown = true;
        if !self.enabled
            || shared.mode != UpdateMode::Automatic
            || !matches!(shared.state, UpdateState::Staged(_))
        {
            return Ok(());
        }
        let installation = self
            .installation
            .as_ref()
            .ok_or(UpdateError::FolderNotWritable)?;
        handoff::prepare_helper(installation)?;
        installation.apply()?;
        shared.state = UpdateState::UpToDate;
        Ok(())
    }

    pub fn restart(&self) -> Result<(), UpdateError> {
        let shared = self.shared.lock().expect("update lock");
        if !self.enabled
            || shared.mode != UpdateMode::Automatic
            || !matches!(shared.state, UpdateState::Staged(_))
        {
            return Err(UpdateError::Handoff("No verified update is ready".into()));
        }
        drop(shared);
        let installation = self
            .installation
            .as_ref()
            .ok_or(UpdateError::FolderNotWritable)?;
        let result = self.apply_staged().and_then(|_| {
            if let Err(error) = handoff::restart(installation) {
                installation.rollback().map_err(|rollback| UpdateError::Handoff(
                    format!("Restart failed: {error}; restoring the previous version also failed: {rollback}")))?;
                return Err(error);
            }
            Ok(())
        });
        if let Err(error) = result {
            let mut shared = self.shared.lock().expect("update lock");
            shared.shutdown = false;
            shared.state = UpdateState::Failed(error.clone());
            shared.next_check = SystemTime::now() + RETRY_AFTER;
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for UpdateService {
    fn drop(&mut self) {
        self.shared.lock().expect("update lock").shutdown = true;
    }
}

fn check_release(
    installation: &Installation,
    shared: &Mutex<Shared>,
    download_due: bool,
    cache: Option<&std::path::Path>,
) -> Result<UpdateState, UpdateError> {
    if installation.staged.is_file() && installation.manifest.is_file() {
        match installation.verified_stage() {
            Ok(manifest) if manifest.version > env!("CARGO_PKG_VERSION").parse()? => {
                let mut state = shared.lock().expect("update lock");
                remember_latest(&mut state, manifest.version);
                if state.mode != UpdateMode::Automatic || state.shutdown {
                    return Ok(UpdateState::Available(manifest.version));
                }
                drop(state);
                if !installation.writable()? {
                    return Ok(UpdateState::Available(manifest.version));
                }
                validate_startup(installation)?;
                return Ok(UpdateState::Staged(manifest.version));
            }
            Err(error) if !download_due => return Err(error),
            _ => {}
        }
    }
    // Preferences may have changed while this worker was queued or checking a stage.
    {
        let state = shared.lock().expect("update lock");
        if state.shutdown || state.mode == UpdateMode::Off {
            return Ok(UpdateState::UpToDate);
        }
    }
    let Some(manifest) = release_manifest(download_due, cache)? else {
        return Ok(UpdateState::UpToDate);
    };
    remember_latest(&mut shared.lock().expect("update lock"), manifest.version);
    if manifest.version <= env!("CARGO_PKG_VERSION").parse()? {
        return Ok(UpdateState::UpToDate);
    }
    // Cached notifications restore the footer, not a failed download before its retry time.
    if !download_due {
        return Ok(UpdateState::Available(manifest.version));
    }
    {
        let state = shared.lock().expect("update lock");
        if state.shutdown || state.mode != UpdateMode::Automatic {
            return Ok(UpdateState::Available(manifest.version));
        }
    }
    if !installation.writable()? {
        return Ok(UpdateState::Available(manifest.version));
    }
    let bytes = download::get(&manifest.path, MAX_EXECUTABLE_BYTES)?;
    let state = shared.lock().expect("update lock");
    if state.shutdown || state.mode != UpdateMode::Automatic {
        return Ok(UpdateState::Available(manifest.version));
    }
    match installation.stage(&manifest, &bytes) {
        Ok(()) => {
            drop(state);
            validate_startup(installation)?;
            Ok(UpdateState::Staged(manifest.version))
        }
        Err(UpdateError::FolderNotWritable) => Ok(UpdateState::Available(manifest.version)),
        Err(error) => Err(error),
    }
}

fn manual_check_allowed(last_manual_check: Option<Instant>, current_time: Instant) -> bool {
    last_manual_check.is_none_or(|last_check| {
        current_time.saturating_duration_since(last_check) >= MANUAL_CHECK_COOLDOWN
    })
}

/// A staged download and the release manifest can name different versions; keep the newer.
fn remember_latest(shared: &mut Shared, version: Version) {
    if shared.latest.is_none_or(|latest| version > latest) {
        shared.latest = Some(version);
    }
}

fn validate_startup(installation: &Installation) -> Result<(), UpdateError> {
    if let Err(error) = handoff::preflight(installation) {
        if let Err(cleanup) = staging::remove_if_present(&installation.staged) {
            eprintln!("Could not remove failed update: {cleanup}");
        }
        return Err(error);
    }
    Ok(())
}

fn release_manifest(
    download_due: bool,
    cache: Option<&std::path::Path>,
) -> Result<Option<Manifest>, UpdateError> {
    if !download_due {
        return cache
            .filter(|path| path.is_file())
            .map(|path| Manifest::verify(&staging::read_bounded(path, MAX_MANIFEST_BYTES)?))
            .transpose();
    }
    let manifest = Manifest::verify(&download::get(MANIFEST_PATH, MAX_MANIFEST_BYTES)?)?;
    if let Some(path) = cache {
        let result = path
            .parent()
            .map_or(Ok(()), |parent| {
                fs::create_dir_all(parent).map_err(UpdateError::io)
            })
            .and_then(|_| staging::atomic_write(path, manifest.text.as_bytes()));
        if let Err(error) = result {
            eprintln!("Could not cache update manifest: {error}");
        }
    }
    Ok(Some(manifest))
}

fn save_check_time(path: Option<&PathBuf>, next: SystemTime) {
    let Some(path) = path else { return };
    let Some(parent) = path.parent() else { return };
    let seconds = next
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let result = fs::create_dir_all(parent)
        .map_err(UpdateError::io)
        .and_then(|_| staging::atomic_write(path, seconds.to_string().as_bytes()));
    if let Err(error) = result {
        eprintln!("Could not save update check time: {error}");
    }
}
