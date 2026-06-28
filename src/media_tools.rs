use crate::{
    command::{CommandCategory, CommandResult, FeatureAction, SystemControlCommand},
    search_text::normalize_search_text,
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn search_media(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);

    if normalized_search_text.is_empty() {
        return media_catalog();
    }

    let mut results = now_playing_results();
    results.retain(|result| {
        normalize_search_text(&result.title).contains(&normalized_search_text)
            || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
    });
    if results.is_empty() {
        results = now_playing_results();
    }
    results
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(query);
    if matches!(
        normalized.as_str(),
        "now playing" | "now" | "spotify" | "media" | "what's playing" | "whats playing"
    ) {
        return now_playing_results();
    }

    Vec::new()
}

fn media_catalog() -> Vec<CommandResult> {
    let mut results = now_playing_results();
    results.extend(media_control_results());
    results
}

fn now_playing_results() -> Vec<CommandResult> {
    match read_now_playing() {
        Some((title, artist)) => {
            let subtitle = if artist.is_empty() {
                "Now playing".to_string()
            } else {
                format!("{artist} · Now playing")
            };
            let copy_text = format!("{artist} - {title}");
            vec![CommandResult::copyable_feature(
                title,
                subtitle,
                copy_text,
                CommandCategory::Media,
                92,
            )]
        }
        None => vec![CommandResult::informational(
            "Now playing",
            "No active media session found",
        )],
    }
}

pub fn read_now_playing() -> Option<(String, String)> {
    let script = r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    return $netTask.Result
}
[Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType=WindowsRuntime] | Out-Null
$manager = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
$session = $manager.GetCurrentSession()
if ($null -eq $session) { exit 1 }
$info = Await ($session.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
$title = $info.Title
$artist = $info.Artist
Write-Output ("$title`t$artist")
"#;

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-Command",
        script,
    ]);
    command.stdin(Stdio::null());
    configure_no_console_window(&mut command);

    let child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout?;
    let output = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .next()?;

    let mut parts = output.splitn(2, '\t');
    let title = parts.next()?.trim().to_string();
    let artist = parts.next().unwrap_or("").trim().to_string();

    if title.is_empty() {
        return None;
    }

    Some((title, artist))
}

fn media_control_results() -> Vec<CommandResult> {
    [
        (
            "Play / Pause media",
            "Toggle playback",
            SystemControlCommand::MediaPlayPause,
            84,
        ),
        (
            "Next track",
            "Skip to next song",
            SystemControlCommand::MediaNext,
            83,
        ),
        (
            "Previous track",
            "Go to previous song",
            SystemControlCommand::MediaPrevious,
            82,
        ),
        (
            "Stop media",
            "Stop playback",
            SystemControlCommand::MediaStop,
            81,
        ),
    ]
    .into_iter()
    .map(|(title, subtitle, command, confidence)| {
        CommandResult::feature(
            title,
            subtitle,
            CommandCategory::Media,
            FeatureAction::SystemControl(command),
            confidence,
        )
    })
    .collect()
}

#[cfg(target_os = "windows")]
fn configure_no_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_no_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_now_playing_triggers_media_results() {
        let results = search_inline("now playing");
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.category == CommandCategory::Media || r.category == CommandCategory::Help));
    }

    #[test]
    fn media_catalog_includes_controls() {
        let results = search_media("");
        assert!(results.len() >= 4);
    }
}

#[cfg(target_os = "windows")]
unsafe fn is_spotify_process(pid: u32) -> bool {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    };
    use windows::Win32::Foundation::CloseHandle;
    
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    if let Ok(handle) = handle {
        let mut buffer = [0u16; 512];
        let mut size = buffer.len() as u32;
        if QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buffer.as_mut_ptr()), &mut size).is_ok() {
            let path = String::from_utf16_lossy(&buffer[..size as usize]);
            let path_lower = path.to_lowercase();
            let is_spotify = path_lower.contains("spotify.exe");
            let _ = CloseHandle(handle);
            return is_spotify;
        }
        let _ = CloseHandle(handle);
    }
    false
}

#[cfg(target_os = "windows")]
fn find_spotify_audio_volume() -> Option<windows::Win32::Media::Audio::ISimpleAudioVolume> {
    unsafe {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eRender, eConsole,
            IAudioSessionManager2, IAudioSessionControl2, ISimpleAudioVolume,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };
        use windows::core::Interface;

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ).ok()?;

        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let session_manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).ok()?;
        let enumerator = session_manager.GetSessionEnumerator().ok()?;
        let count = enumerator.GetCount().ok()?;
        
        for i in 0..count {
            let session_control = enumerator.GetSession(i).ok()?;
            if let Ok(session_control2) = session_control.cast::<IAudioSessionControl2>() {
                let pid = session_control2.GetProcessId().unwrap_or(0);
                if pid != 0 && is_spotify_process(pid) {
                    if let Ok(simple_volume) = session_control2.cast::<ISimpleAudioVolume>() {
                        return Some(simple_volume);
                    }
                }
            }
        }
        None
    }
}

#[cfg(target_os = "windows")]
pub fn get_system_volume() -> Option<f32> {
    if let Some(spotify_vol) = find_spotify_audio_volume() {
        unsafe {
            if let Ok(level) = spotify_vol.GetMasterVolume() {
                return Some(level);
            }
        }
    }

    unsafe {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eRender, eConsole,
        };
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ).ok()?;

        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
        let level = volume.GetMasterVolumeLevelScalar().ok()?;
        Some(level)
    }
}

#[cfg(target_os = "windows")]
pub fn set_system_volume(level: f32) -> Option<()> {
    if let Some(spotify_vol) = find_spotify_audio_volume() {
        unsafe {
            if spotify_vol.SetMasterVolume(level, std::ptr::null()).is_ok() {
                return Some(());
            }
        }
    }

    unsafe {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eRender, eConsole,
        };
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ).ok()?;

        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
        
        volume.SetMasterVolumeLevelScalar(level, std::ptr::null_mut()).ok()?;
        Some(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_system_volume() -> Option<f32> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn set_system_volume(_level: f32) -> Option<()> {
    None
}