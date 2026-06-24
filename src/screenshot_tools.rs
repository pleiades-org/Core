use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    search_text::normalize_search_text,
};

pub fn search_screenshot_tools(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);

    if normalized_search_text.is_empty() {
        return screenshot_catalog();
    }

    let mut results = execute_screenshot_inline(search_text);
    results.extend(
        screenshot_catalog()
            .into_iter()
            .filter(|result| {
                normalize_search_text(&result.title).contains(&normalized_search_text)
                    || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
            }),
    );
    results
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    execute_screenshot_inline(query)
}

fn screenshot_catalog() -> Vec<CommandResult> {
    vec![
        region_snip_result(86),
        full_screen_result(85),
        ocr_clipboard_result(84),
        ocr_result_hint(83),
    ]
}

fn execute_screenshot_inline(query: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(query);

    if matches!(
        normalized.as_str(),
        "screenshot" | "capture screen" | "screen capture" | "snip"
    ) {
        return vec![region_snip_result(90)];
    }

    if normalized == "screenshot full" || normalized == "full screenshot" || normalized == "capture full screen"
    {
        return vec![full_screen_result(90)];
    }

    if normalized == "ocr" || normalized == "ocr clipboard" || normalized.starts_with("ocr ") {
        return vec![ocr_clipboard_result(90)];
    }

    if normalized.contains("capture screen") || normalized.contains("screen capture") {
        return vec![region_snip_result(88)];
    }

    Vec::new()
}

fn region_snip_result(confidence: u8) -> CommandResult {
    CommandResult {
        title: "Screenshot region".to_string(),
        subtitle: "Open Windows screen snip (Win+Shift+S)".to_string(),
        copy_text: "ms-screenclip:".to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: CommandAction::RunProgram {
            program: "explorer.exe".to_string(),
            arguments: vec!["ms-screenclip:".to_string()],
        },
        confidence,
    }
}

fn full_screen_result(confidence: u8) -> CommandResult {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
[System.Windows.Forms.Clipboard]::SetImage($bitmap)
$graphics.Dispose()
$bitmap.Dispose()
"#;

    CommandResult {
        title: "Screenshot full screen".to_string(),
        subtitle: "Capture entire screen to clipboard".to_string(),
        copy_text: "full screen screenshot".to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: hidden_powershell_action(script),
        confidence,
    }
}

fn ocr_clipboard_result(confidence: u8) -> CommandResult {
    let script = r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTaskGeneric = ([System.Windows.Forms.Form].Assembly.GetType('System.Windows.Forms.UnsafeNativeMethods')).GetMethod('GetTypeFromHandle').Invoke($null, @([System.Runtime.InteropServices.HandleRef]::new([IntPtr]::Zero, [System.Runtime.InteropServices.GCHandle]::Alloc([Windows.Storage.Streams.DataReader]).AddrOfPinnedObject())))
function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}
function Get-ClipboardImage {
    Add-Type -AssemblyName System.Windows.Forms
    if ([System.Windows.Forms.Clipboard]::ContainsImage()) {
        return [System.Windows.Forms.Clipboard]::GetImage()
    }
    return $null
}
$img = Get-ClipboardImage
if ($null -eq $img) {
    Write-Output 'No image on clipboard. Take a screenshot first.'
    exit 0
}
$tmp = Join-Path $env:TEMP ('core-ocr-' + [guid]::NewGuid().ToString() + '.png')
$img.Save($tmp, [System.Drawing.Imaging.ImageFormat]::Png)
[Windows.Storage.Streams.RandomAccessStreamReference, Windows.Storage.Streams, ContentType=WindowsRuntime] | Out-Null
[Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType=WindowsRuntime] | Out-Null
[Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType=WindowsRuntime] | Out-Null
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($tmp)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$softwareBitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$ocrEngine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
if ($null -eq $ocrEngine) { $ocrEngine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage([Windows.Globalization.Language]::new('en')) }
$result = Await ($ocrEngine.RecognizeAsync($softwareBitmap)) ([Windows.Media.Ocr.OcrResult])
$text = $result.Text
Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
if ([string]::IsNullOrWhiteSpace($text)) {
    Write-Output 'No text recognized.'
} else {
    Set-Clipboard -Value $text
    Write-Output $text
}
"#;

    CommandResult {
        title: "OCR clipboard image".to_string(),
        subtitle: "Recognize text from clipboard image and copy result".to_string(),
        copy_text: "ocr".to_string(),
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::System,
        action: hidden_powershell_action(script),
        confidence,
    }
}

fn ocr_result_hint(confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(
        "OCR from clipboard",
        "Run OCR on the current clipboard image",
        "ocr",
        CommandCategory::System,
        confidence,
    )
}

fn hidden_powershell_action(script: &str) -> CommandAction {
    CommandAction::RunProgram {
        program: "powershell.exe".to_string(),
        arguments: vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-WindowStyle".to_string(),
            "Hidden".to_string(),
            "-Command".to_string(),
            script.trim().to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_screenshot_triggers_region_snip() {
        let results = search_inline("screenshot");
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("region"));
        assert_eq!(results[0].category, CommandCategory::System);
    }

    #[test]
    fn inline_ocr_triggers_clipboard_ocr() {
        let results = search_inline("ocr");
        assert_eq!(results.len(), 1);
        assert!(results[0].title.to_lowercase().contains("ocr"));
    }

    #[test]
    fn scoped_catalog_returns_shortcuts() {
        let results = search_screenshot_tools("");
        assert!(results.len() >= 3);
    }
}