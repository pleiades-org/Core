use crate::{
    command::{CommandCategory, CommandResult, FeatureAction, SystemControlCommand},
    search_text::normalize_search_text,
};
use std::{
    io,
    process::{Command, Stdio},
};

const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;
const VK_MEDIA_NEXT_TRACK: u16 = 0xB0;
const VK_MEDIA_PREV_TRACK: u16 = 0xB1;
const VK_MEDIA_STOP: u16 = 0xB2;
const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;

pub fn search_system_controls(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    if normalized_search_text.is_empty() {
        return system_control_results();
    }

    system_control_results()
        .into_iter()
        .filter(|result| {
            normalize_search_text(&result.title).contains(&normalized_search_text)
                || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
        })
        .collect()
}

pub fn execute_system_control(command: &SystemControlCommand) -> io::Result<()> {
    match command {
        SystemControlCommand::LockScreen => {
            run_hidden("rundll32.exe", &["user32.dll,LockWorkStation"])
        }
        SystemControlCommand::Sleep => {
            run_hidden("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"])
        }
        SystemControlCommand::Restart => run_hidden("shutdown.exe", &["/r", "/t", "0"]),
        SystemControlCommand::Shutdown => run_hidden("shutdown.exe", &["/s", "/t", "0"]),
        SystemControlCommand::EmptyTrash => run_hidden(
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Clear-RecycleBin -Force -ErrorAction SilentlyContinue",
            ],
        ),
        SystemControlCommand::ShowDesktop => run_hidden(
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "(New-Object -ComObject Shell.Application).ToggleDesktop()",
            ],
        ),
        SystemControlCommand::HideApps => run_hidden(
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "(New-Object -ComObject Shell.Application).MinimizeAll()",
            ],
        ),
        SystemControlCommand::VolumeUp => send_virtual_key(VK_VOLUME_UP),
        SystemControlCommand::VolumeDown => send_virtual_key(VK_VOLUME_DOWN),
        SystemControlCommand::MuteVolume => send_virtual_key(VK_VOLUME_MUTE),
        SystemControlCommand::MediaPlayPause => send_virtual_key(VK_MEDIA_PLAY_PAUSE),
        SystemControlCommand::MediaNext => send_virtual_key(VK_MEDIA_NEXT_TRACK),
        SystemControlCommand::MediaPrevious => send_virtual_key(VK_MEDIA_PREV_TRACK),
        SystemControlCommand::MediaStop => send_virtual_key(VK_MEDIA_STOP),
        SystemControlCommand::BrightnessUp => adjust_brightness(10),
        SystemControlCommand::BrightnessDown => adjust_brightness(-10),
    }
}

fn system_control_results() -> Vec<CommandResult> {
    [
        (
            "Lock screen",
            "Secure the current Windows session",
            SystemControlCommand::LockScreen,
            92,
        ),
        (
            "Sleep computer",
            "Put the device to sleep",
            SystemControlCommand::Sleep,
            90,
        ),
        (
            "Show desktop",
            "Toggle the desktop view",
            SystemControlCommand::ShowDesktop,
            88,
        ),
        (
            "Hide apps",
            "Minimize all open windows",
            SystemControlCommand::HideApps,
            86,
        ),
        (
            "Empty trash",
            "Clear the Recycle Bin",
            SystemControlCommand::EmptyTrash,
            84,
        ),
        (
            "Volume up",
            "Increase system volume",
            SystemControlCommand::VolumeUp,
            82,
        ),
        (
            "Volume down",
            "Decrease system volume",
            SystemControlCommand::VolumeDown,
            82,
        ),
        (
            "Mute volume",
            "Toggle mute",
            SystemControlCommand::MuteVolume,
            82,
        ),
        (
            "Play / Pause media",
            "Toggle playback for the active media app",
            SystemControlCommand::MediaPlayPause,
            81,
        ),
        (
            "Next track",
            "Skip to the next song or video",
            SystemControlCommand::MediaNext,
            80,
        ),
        (
            "Previous track",
            "Go back to the previous song or video",
            SystemControlCommand::MediaPrevious,
            80,
        ),
        (
            "Stop media",
            "Stop the current media session",
            SystemControlCommand::MediaStop,
            78,
        ),
        (
            "Brightness up",
            "Increase display brightness when supported",
            SystemControlCommand::BrightnessUp,
            78,
        ),
        (
            "Brightness down",
            "Decrease display brightness when supported",
            SystemControlCommand::BrightnessDown,
            78,
        ),
        (
            "Restart computer",
            "Restart immediately",
            SystemControlCommand::Restart,
            72,
        ),
        (
            "Shut down computer",
            "Shut down immediately",
            SystemControlCommand::Shutdown,
            70,
        ),
    ]
    .into_iter()
    .map(|(title, subtitle, system_command, confidence)| {
        CommandResult::feature(
            title,
            subtitle,
            CommandCategory::System,
            FeatureAction::SystemControl(system_command),
            confidence,
        )
    })
    .collect()
}

fn send_virtual_key(virtual_key: u16) -> io::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let key_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: Default::default(),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let key_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let sent = unsafe { SendInput(&[key_down, key_up], std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn adjust_brightness(delta: i32) -> io::Result<()> {
    let script = format!(
        "$monitor = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1; \
         if ($monitor) {{ \
             $target = [Math]::Max(0, [Math]::Min(100, $monitor.CurrentBrightness + ({delta}))); \
             (Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods).WmiSetBrightness(1, $target) | Out-Null \
         }}"
    );

    run_hidden(
        "powershell.exe",
        &["-NoLogo", "-NoProfile", "-Command", &script],
    )
}

fn run_hidden(program: &str, arguments: &[&str]) -> io::Result<()> {
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}


