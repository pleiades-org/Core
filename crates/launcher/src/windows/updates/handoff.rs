//! A known-good helper outlives the launcher and rolls back unsuccessful startup.
use super::{
    crypto,
    staging::{self, Installation},
    UpdateError,
};
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::*, Security::Cryptography::*, System::Threading::*, UI::WindowsAndMessaging::*,
    },
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT_MS: u32 = 2_000;
const EXIT_TIMEOUT_MS: u32 = 30_000;
pub const STARTUP_TIMER: usize = 45;

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn native(error: windows::core::Error) -> UpdateError {
    UpdateError::Handoff(error.to_string())
}

fn event(name: &str, create: bool) -> Result<Handle, UpdateError> {
    let wide = super::super::wide(name);
    let handle = unsafe {
        if create {
            CreateEventW(None, true, false, PCWSTR(wide.as_ptr()))
        } else {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                false,
                PCWSTR(wide.as_ptr()),
            )
        }
    }
    .map_err(native)?;
    Ok(Handle(handle))
}

fn token() -> Result<String, UpdateError> {
    let mut random = [0; 16];
    unsafe { BCryptGenRandom(None, &mut random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(native)?;
    let random: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("Local\\Pleiades.Core.Update.{random}"))
}

fn valid_token(text: &str) -> bool {
    text.strip_prefix("Local\\Pleiades.Core.Update.")
        .is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn argument(name: &str) -> Option<String> {
    let mut arguments = std::env::args();
    arguments.find(|argument| argument == name)?;
    arguments.next()
}

pub(super) fn prepare_helper(installation: &Installation) -> Result<(), UpdateError> {
    let bytes = staging::read_bounded(&installation.executable, super::MAX_EXECUTABLE_BYTES)?;
    staging::atomic_write(&installation.helper, &bytes)
}

fn start_guard(
    installation: &Installation,
    mode: &str,
    process: u32,
    name: &str,
) -> Result<(), UpdateError> {
    let armed = event(&format!("{name}.armed"), true)?;
    let mut helper = Command::new(&installation.helper)
        .args([mode, &process.to_string(), name])
        .args(relaunch_arguments())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(UpdateError::io)?;
    if unsafe { WaitForSingleObject(armed.0, HANDSHAKE_TIMEOUT_MS) } != WAIT_OBJECT_0 {
        stop_child(&mut helper)?;
        return Err(UpdateError::Handoff(
            "Update helper did not start; Core remains open".into(),
        ));
    }
    Ok(())
}

pub(super) fn restart(installation: &Installation) -> Result<(), UpdateError> {
    start_guard(
        installation,
        "--update-guard",
        std::process::id(),
        &token()?,
    )
}

/// Runs before COM, the window, or the singleton. Helper processes never own the singleton.
pub fn dispatch() -> Result<bool, UpdateError> {
    let arguments: Vec<_> = std::env::args().collect();
    let Some(mode) = arguments
        .get(1)
        .filter(|mode| matches!(mode.as_str(), "--update-guard" | "--update-monitor"))
    else {
        return Ok(false);
    };
    if arguments.len() < 4 || !valid_token(&arguments[3]) {
        return Err(UpdateError::MalformedManifest);
    }
    let process = arguments[2]
        .parse::<u32>()
        .map_err(|_| UpdateError::MalformedManifest)?;
    let current = std::env::current_exe().map_err(UpdateError::io)?;
    let stem = current
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_suffix(".updater"))
        .ok_or(UpdateError::MalformedManifest)?;
    let installation = Installation::new(current.with_file_name(format!("{stem}.exe")));
    let target = Handle(
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, process) }
            .map_err(native)?,
    );
    let ready = event(&arguments[3], true)?;
    let armed = event(&format!("{}.armed", arguments[3]), false)?;
    unsafe { SetEvent(armed.0) }.map_err(native)?;
    if mode == "--update-monitor" {
        monitor(&installation, target, ready)?;
    } else if unsafe { WaitForSingleObject(target.0, EXIT_TIMEOUT_MS) } == WAIT_OBJECT_0 {
        launch_and_monitor(&installation, process, &arguments[3], ready)?;
    } else {
        return Err(UpdateError::Handoff(
            "Old Core did not exit; update restart was cancelled".into(),
        ));
    }
    Ok(true)
}

fn launch_and_monitor(
    installation: &Installation,
    previous: u32,
    name: &str,
    ready: Handle,
) -> Result<(), UpdateError> {
    let verified = staging::read_bounded(&installation.pending, super::MAX_MANIFEST_BYTES)
        .and_then(|bytes| super::Manifest::verify(&bytes))
        .and_then(|manifest| {
            staging::check_hash(
                &staging::read_bounded(&installation.executable, super::MAX_EXECUTABLE_BYTES)?,
                &manifest,
            )
        });
    if let Err(error) = verified {
        eprintln!("Update changed before restart: {error}");
        return restore_and_launch(installation);
    }
    let mut command = Command::new(&installation.executable);
    command.args([
        "--after-update",
        &previous.to_string(),
        "--update-event",
        name,
    ]);
    command
        .args(relaunch_arguments())
        .creation_flags(CREATE_NO_WINDOW.0);
    match command.spawn() {
        Ok(child) => {
            let handle = Handle(
                unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, child.id()) }
                    .map_err(native)?,
            );
            monitor(installation, handle, ready)
        }
        Err(error) => {
            eprintln!("Updated Core could not launch: {error}");
            restore_and_launch(installation)
        }
    }
}

fn monitor(installation: &Installation, process: Handle, ready: Handle) -> Result<(), UpdateError> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if unsafe { WaitForSingleObject(ready.0, 20) } == WAIT_OBJECT_0
            && unsafe { WaitForSingleObject(process.0, 0) } == WAIT_TIMEOUT
        {
            return installation.confirm();
        }
        if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            break;
        }
    }
    if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_TIMEOUT {
        unsafe { TerminateProcess(process.0, 1) }.map_err(native)?;
        if unsafe { WaitForSingleObject(process.0, HANDSHAKE_TIMEOUT_MS) } != WAIT_OBJECT_0 {
            return Err(UpdateError::Handoff(
                "Unresponsive update could not be stopped; previous version retained".into(),
            ));
        }
    }
    restore_and_launch(installation)
}

fn restore_and_launch(installation: &Installation) -> Result<(), UpdateError> {
    installation.rollback()?;
    Command::new(&installation.executable)
        .arg("--update-rollback")
        .args(
            relaunch_arguments()
                .into_iter()
                .filter(|argument| argument != "--test-update-no-ready"),
        )
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(UpdateError::io)?;
    Ok(())
}

fn stop_child(child: &mut Child) -> Result<(), UpdateError> {
    if child.try_wait().map_err(UpdateError::io)?.is_none() {
        child.kill().map_err(UpdateError::io)?;
        child.wait().map_err(UpdateError::io)?;
    }
    Ok(())
}

/// Wait for the real previous process, not its non-owned mutex, before singleton acquisition.
pub fn wait_for_previous() -> Result<(), UpdateError> {
    let Some(process) = argument("--after-update") else {
        return Ok(());
    };
    let process = process
        .parse::<u32>()
        .map_err(|_| UpdateError::MalformedManifest)?;
    match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process) } {
        Ok(handle) => {
            let process = Handle(handle);
            if unsafe { WaitForSingleObject(process.0, EXIT_TIMEOUT_MS) } != WAIT_OBJECT_0 {
                return Err(UpdateError::Handoff(
                    "Previous Core process did not exit".into(),
                ));
            }
            Ok(())
        }
        Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => Ok(()),
        Err(error) => Err(native(error)),
    }
}

/// Catch loader/COM/control-creation failures before a downloaded executable is staged for exit.
pub(super) fn preflight(installation: &Installation) -> Result<(), UpdateError> {
    let name = token()?;
    let ready = event(&name, true)?;
    let mut child = Command::new(&installation.staged)
        .args(["--dry-run", "--probe-hidden", "--update-probe", &name])
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(UpdateError::io)?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(UpdateError::io)? {
            if status.success() && unsafe { WaitForSingleObject(ready.0, 0) } == WAIT_OBJECT_0 {
                return Ok(());
            }
            return Err(UpdateError::Handoff(
                "Downloaded Core failed its isolated startup check".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop_child(&mut child)?;
    Err(UpdateError::Handoff(
        "Downloaded Core did not complete its startup check within five seconds".into(),
    ))
}

pub struct Startup {
    ready: Option<Handle>,
    probe: bool,
}
impl Startup {
    /// After singleton acquisition, arm a helper for an update installed on ordinary exit.
    pub fn prepare(disabled: bool) -> Result<Self, UpdateError> {
        if let Some(name) = argument("--update-probe") {
            if !valid_token(&name)
                || !std::env::args().any(|argument| argument == "--dry-run")
                || !std::env::args().any(|argument| argument == "--probe-hidden")
            {
                return Err(UpdateError::MalformedManifest);
            }
            return Ok(Self {
                ready: Some(event(&name, false)?),
                probe: true,
            });
        }
        // Isolated handoff tests exercise readiness without enabling downloads or real actions.
        let test_handoff = std::env::args().any(|argument| argument == "--dry-run")
            && std::env::args().any(|argument| argument == "--test-update-handoff");
        if disabled && !test_handoff {
            return Ok(Self {
                ready: None,
                probe: false,
            });
        }
        if let Some(name) = argument("--update-event") {
            if !valid_token(&name) {
                return Err(UpdateError::MalformedManifest);
            }
            return Ok(Self {
                ready: Some(event(&name, false)?),
                probe: false,
            });
        }
        let installation = Installation::new(std::env::current_exe().map_err(UpdateError::io)?);
        if !installation.pending.is_file() {
            return Ok(Self {
                ready: None,
                probe: false,
            });
        }
        // The helper was copied from the known-good executable before its rename.
        let previous = staging::read_bounded(&installation.previous, super::MAX_EXECUTABLE_BYTES)?;
        let helper = staging::read_bounded(&installation.helper, super::MAX_EXECUTABLE_BYTES)?;
        if crypto::sha256(&previous)? != crypto::sha256(&helper)? {
            return Err(UpdateError::HashMismatch);
        }
        let name = token()?;
        start_guard(&installation, "--update-monitor", std::process::id(), &name)?;
        let manifest = super::Manifest::verify(&staging::read_bounded(
            &installation.pending,
            super::MAX_MANIFEST_BYTES,
        )?)?;
        staging::check_hash(
            &staging::read_bounded(&installation.executable, super::MAX_EXECUTABLE_BYTES)?,
            &manifest,
        )?;
        Ok(Self {
            ready: Some(event(&name, false)?),
            probe: false,
        })
    }

    pub fn pending(&self) -> bool {
        self.ready.is_some()
    }

    /// Called by the UI timer after controls, hotkey and tray setup; confirms the visible window.
    pub fn confirm_visible(&mut self, window: HWND) {
        let Some(ready) = &self.ready else { return };
        if std::env::args().any(|argument| argument == "--dry-run")
            && std::env::args().any(|argument| argument == "--test-update-no-ready")
        {
            return;
        }
        let mut opacity = 0;
        if !self.probe
            && (!unsafe { IsWindowVisible(window) }.as_bool()
                || unsafe { GetLayeredWindowAttributes(window, None, Some(&mut opacity), None) }
                    .is_err()
                || opacity != 255)
        {
            return;
        }
        if let Err(error) = unsafe { SetEvent(ready.0) } {
            eprintln!("Could not confirm updated Core: {error}");
            return;
        }
        self.ready = None;
        unsafe {
            let _ = KillTimer(Some(window), STARTUP_TIMER);
        }
        if self.probe {
            if let Err(error) =
                unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            {
                eprintln!("Could not close update startup probe: {error}");
            }
        }
    }
}

/// Preserve explicit settings/motion choices. Test flags propagate only through isolated runs.
fn relaunch_arguments() -> Vec<String> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let dry_run = arguments.iter().any(|argument| argument == "--dry-run");
    let mut forwarded = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--settings-file" | "--exchange-rates-file" => {
                if let Some(path) = arguments.next() {
                    forwarded.extend([argument.clone(), path.clone()]);
                }
            }
            "--reduced-motion" | "--system-motion" => forwarded.push(argument.clone()),
            "--dry-run"
            | "--test-background"
            | "--test-stay-open"
            | "--test-update-handoff"
            | "--test-update-no-ready"
                if dry_run =>
            {
                forwarded.push(argument.clone())
            }
            _ => {}
        }
    }
    forwarded
}
