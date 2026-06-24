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

fn read_now_playing() -> Option<(String, String)> {
    let script = r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTaskGeneric = ([System.Windows.Forms.Form].Assembly.GetType('System.Windows.Forms.UnsafeNativeMethods')).GetMethod('GetTypeFromHandle').Invoke($null, @([System.Runtime.InteropServices.HandleRef]::new([IntPtr]::Zero, [System.Runtime.InteropServices.GCHandle]::Alloc([Windows.Storage.Streams.DataReader]).AddrOfPinnedObject())))
function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}
[Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType=WindowsRuntime] | Out-Null
$manager = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync().GetAwaiter().GetResult()
$session = $manager.GetCurrentSession()
if ($null -eq $session) { exit 1 }
$info = Await ($session.TryGetMediaPropertiesAsync()) ([Windows.Media.MediaProperties.MusicDisplayProperties])
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