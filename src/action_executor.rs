use crate::{
    calendar, clipboard_history,
    command::{BuiltInAction, CommandAction, CommandResult, FeatureAction, FileOperationKind},
    recent_usage,
    focus, notes, quicklinks,
    paths::deleted_files_dir,
    settings::settings_file_path,
    snippets, system_controls, window_management,
};
use gpui::{Image, ImageFormat};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("No action is available for this result.")]
    NoAction,
    #[error("Failed to launch action: {0}")]
    LaunchFailed(String),
}

pub enum ExecutedAction {
    Copied(String),
    CopiedImage(Image),
    Launched(String),
    OpenedSettings,
    ReloadApplications,
    Quit,
    Nothing,
}

pub fn execute_result_action(result: &CommandResult) -> Result<ExecutedAction, ActionError> {
    execute_result_action_with_context(result, &ActionExecutionContext::default())
}

#[derive(Default)]
pub struct ActionExecutionContext {
    pub target_window_handle: Option<isize>,
}

pub fn execute_result_action_with_context(
    result: &CommandResult,
    context: &ActionExecutionContext,
) -> Result<ExecutedAction, ActionError> {
    match &result.action {
        CommandAction::CopyToClipboard(text) => Ok(ExecutedAction::Copied(text.clone())),
        CommandAction::OpenPath(path) => {
            open_path(path)?;
            Ok(ExecutedAction::Launched(result.title.clone()))
        }
        CommandAction::OpenUrl(url) => {
            open_url(url)?;
            Ok(ExecutedAction::Launched(result.title.clone()))
        }
        CommandAction::RunProgram { program, arguments } => {
            Command::new(program)
                .args(arguments)
                .spawn()
                .map_err(|error| ActionError::LaunchFailed(error.to_string()))?;
            Ok(ExecutedAction::Launched(result.title.clone()))
        }
        CommandAction::BuiltIn(BuiltInAction::OpenSettings) => {
            let settings_path = settings_file_path();
            open_path(&settings_path)?;
            Ok(ExecutedAction::OpenedSettings)
        }
        CommandAction::BuiltIn(BuiltInAction::OpenOnboarding) => {
            Ok(ExecutedAction::OpenedSettings)
        }
        CommandAction::BuiltIn(BuiltInAction::ReloadApplications) => {
            Ok(ExecutedAction::ReloadApplications)
        }
        CommandAction::BuiltIn(BuiltInAction::Quit) => Ok(ExecutedAction::Quit),
        CommandAction::Feature(feature_action) => execute_feature_action(feature_action, context),
        CommandAction::None => Ok(ExecutedAction::Nothing),
    }
}

fn execute_feature_action(
    feature_action: &FeatureAction,
    context: &ActionExecutionContext,
) -> Result<ExecutedAction, ActionError> {
    match feature_action {
        FeatureAction::CreateNote { title, body } => {
            let note_path = notes::create_markdown_note(title, body).map_err(action_io_error)?;
            open_path(&note_path)?;
            Ok(ExecutedAction::Launched(note_path.display().to_string()))
        }
        FeatureAction::DeleteNote { note_path } => {
            notes::delete_note_to_recovery(note_path).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::RestoreNote { deleted_note_path } => {
            let restored_note_path =
                notes::restore_deleted_note(deleted_note_path).map_err(action_io_error)?;
            open_path(&restored_note_path)?;
            Ok(ExecutedAction::Launched(
                restored_note_path.display().to_string(),
            ))
        }
        FeatureAction::ExportNote {
            note_path,
            export_format,
        } => {
            let export_path =
                notes::export_note(note_path, export_format.clone()).map_err(action_io_error)?;
            open_path(&export_path)?;
            Ok(ExecutedAction::Launched(export_path.display().to_string()))
        }
        FeatureAction::SaveSnippet {
            keyword,
            title,
            body,
        } => {
            snippets::save_snippet(keyword.clone(), title.clone(), body.clone())
                .map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::SaveQuicklink {
            keyword,
            title,
            target,
        } => {
            quicklinks::save_quicklink(keyword.clone(), title.clone(), target.clone())
                .map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::SaveCalendarEvent {
            title,
            start_text,
            duration_minutes,
            meeting_url,
            attendees,
        } => {
            calendar::save_calendar_event(
                title.clone(),
                start_text.clone(),
                *duration_minutes,
                meeting_url.clone(),
                attendees.clone(),
            )
            .map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::RunCustomCommand {
            command,
            working_directory,
        } => {
            run_custom_command(command, working_directory.as_deref())?;
            Ok(ExecutedAction::Launched(command.clone()))
        }
        FeatureAction::StartFocusSession {
            duration_minutes,
            goal,
            categories,
        } => {
            focus::start_focus_session(*duration_minutes, goal.clone(), categories.clone())
                .map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::PauseFocusSession => {
            focus::pause_focus_session().map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::ResumeFocusSession => {
            focus::resume_focus_session().map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::EndFocusSession => {
            focus::end_focus_session().map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::SnoozeFocusSession { minutes } => {
            focus::snooze_focus_session(*minutes).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::PinClipboardItem { item_id } => {
            clipboard_history::pin_clipboard_item(item_id).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::CopyClipboardImage { image_path } => Ok(ExecutedAction::CopiedImage(
            load_clipboard_image(image_path)?,
        )),
        FeatureAction::DeleteClipboardItem { item_id } => {
            clipboard_history::delete_clipboard_item(item_id).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::ClearClipboardHistory => {
            clipboard_history::clear_clipboard_history().map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::PinRecentUsageItem { item_id } => {
            recent_usage::pin_usage_item(item_id).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::ClearRecentUsage => {
            recent_usage::clear_recent_usage().map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::FileOperation {
            operation,
            file_path,
        } => execute_file_operation(operation, file_path),
        FeatureAction::WindowManagement(window_command) => {
            window_management::execute_window_command(window_command, context.target_window_handle)
                .map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
        FeatureAction::SystemControl(system_command) => {
            system_controls::execute_system_control(system_command).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
    }
}

fn execute_file_operation(
    operation: &FileOperationKind,
    file_path: &Path,
) -> Result<ExecutedAction, ActionError> {
    match operation {
        FileOperationKind::CopyPath | FileOperationKind::CopyFileReference => {
            Ok(ExecutedAction::Copied(file_path.display().to_string()))
        }
        FileOperationKind::CopyName => Ok(ExecutedAction::Copied(
            file_path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or("")
                .to_string(),
        )),
        FileOperationKind::ShowInFolder => {
            show_file_in_folder(file_path)?;
            Ok(ExecutedAction::Launched(file_path.display().to_string()))
        }
        FileOperationKind::DeleteToRecovery => {
            move_file_to_recovery(file_path).map_err(action_io_error)?;
            Ok(ExecutedAction::Nothing)
        }
    }
}

fn open_path(path: &Path) -> Result<(), ActionError> {
    Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn()
        .map_err(|error| ActionError::LaunchFailed(error.to_string()))?;

    Ok(())
}

fn open_url(url: &str) -> Result<(), ActionError> {
    Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map_err(|error| ActionError::LaunchFailed(error.to_string()))?;

    Ok(())
}

fn run_custom_command(
    command_text: &str,
    working_directory: Option<&Path>,
) -> Result<(), ActionError> {
    let mut command = if cfg!(target_os = "windows") {
        let mut windows_command = Command::new("cmd");
        windows_command.args(["/C", command_text]);
        windows_command
    } else {
        let mut shell_command = Command::new("sh");
        shell_command.args(["-lc", command_text]);
        shell_command
    };

    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }

    command
        .spawn()
        .map_err(|error| ActionError::LaunchFailed(error.to_string()))?;

    Ok(())
}

fn show_file_in_folder(path: &Path) -> Result<(), ActionError> {
    Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(|error| ActionError::LaunchFailed(error.to_string()))?;

    Ok(())
}

fn move_file_to_recovery(path: &Path) -> io::Result<PathBuf> {
    let recovery_directory = deleted_files_dir();
    fs::create_dir_all(&recovery_directory)?;

    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or("deleted-file");
    let recovery_path = unique_recovery_path(recovery_directory.join(file_name));
    fs::rename(path, &recovery_path)?;
    Ok(recovery_path)
}

fn unique_recovery_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }

    let parent_directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(deleted_files_dir);
    let file_stem = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .unwrap_or("deleted-file");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for copy_index in 2..=999 {
        let file_name = match extension {
            Some(extension) => format!("{file_stem}-{copy_index}.{extension}"),
            None => format!("{file_stem}-{copy_index}"),
        };
        let candidate_path = parent_directory.join(file_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
    }

    path
}

fn action_io_error(error: io::Error) -> ActionError {
    ActionError::LaunchFailed(error.to_string())
}

fn load_clipboard_image(image_path: &Path) -> Result<Image, ActionError> {
    let image_bytes = fs::read(image_path).map_err(action_io_error)?;
    let image_format = image_format_from_path(image_path);
    Ok(Image::from_bytes(image_format, image_bytes))
}

fn image_format_from_path(image_path: &Path) -> ImageFormat {
    match image_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => ImageFormat::Jpeg,
        Some("webp") => ImageFormat::Webp,
        Some("gif") => ImageFormat::Gif,
        Some("svg") => ImageFormat::Svg,
        Some("bmp") => ImageFormat::Bmp,
        Some("tif" | "tiff") => ImageFormat::Tiff,
        Some("ico") => ImageFormat::Ico,
        Some("pnm" | "pbm" | "ppm" | "pgm") => ImageFormat::Pnm,
        _ => ImageFormat::Png,
    }
}
