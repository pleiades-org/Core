use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandCategory {
    Calculation,
    Application,
    File,
    BuiltIn,
    Web,
    Help,
    Note,
    Focus,
    Clipboard,
    WindowManagement,
    Snippet,
    Quicklink,
    Calendar,
    System,
    Emoji,
    Context,
    DevTools,
    Git,
    Package,
    Lookup,
    Media,
    Network,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltInAction {
    Quit,
    OpenSettings,
    OpenOnboarding,
    ReloadApplications,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandAction {
    CopyToClipboard(String),
    OpenPath(PathBuf),
    OpenUrl(String),
    RunProgram {
        program: String,
        arguments: Vec<String>,
    },
    BuiltIn(BuiltInAction),
    Feature(FeatureAction),
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteExportFormat {
    PlainText,
    Markdown,
    Html,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileOperationKind {
    CopyPath,
    CopyName,
    CopyFileReference,
    ShowInFolder,
    DeleteToRecovery,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowManagementCommand {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeftQuarter,
    TopRightQuarter,
    BottomLeftQuarter,
    BottomRightQuarter,
    LeftThird,
    CenterThird,
    RightThird,
    Maximize,
    Center,
    MoveToNextDisplay,
    MoveToPreviousDisplay,
    RestorePreviousPosition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemControlCommand {
    LockScreen,
    Sleep,
    Restart,
    Shutdown,
    EmptyTrash,
    ShowDesktop,
    HideApps,
    VolumeUp,
    VolumeDown,
    MuteVolume,
    BrightnessUp,
    BrightnessDown,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    MediaStop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureAction {
    CreateNote {
        title: String,
        body: String,
    },
    DeleteNote {
        note_path: PathBuf,
    },
    RestoreNote {
        deleted_note_path: PathBuf,
    },
    ExportNote {
        note_path: PathBuf,
        export_format: NoteExportFormat,
    },
    SaveSnippet {
        keyword: String,
        title: String,
        body: String,
    },
    SaveQuicklink {
        keyword: String,
        title: String,
        target: String,
    },
    SaveCalendarEvent {
        title: String,
        start_text: String,
        duration_minutes: u32,
        meeting_url: Option<String>,
        attendees: Vec<String>,
    },
    RunCustomCommand {
        command: String,
        working_directory: Option<PathBuf>,
    },
    StartFocusSession {
        duration_minutes: u32,
        goal: String,
        categories: Vec<String>,
    },
    PauseFocusSession,
    ResumeFocusSession,
    EndFocusSession,
    SnoozeFocusSession {
        minutes: u32,
    },
    PinClipboardItem {
        item_id: String,
    },
    CopyClipboardImage {
        image_path: PathBuf,
    },
    DeleteClipboardItem {
        item_id: String,
    },
    ClearClipboardHistory,
    PinRecentUsageItem {
        item_id: String,
    },
    ClearRecentUsage,
    FileOperation {
        operation: FileOperationKind,
        file_path: PathBuf,
    },
    WindowManagement(WindowManagementCommand),
    SystemControl(SystemControlCommand),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalculationDisplay {
    pub expression: String,
    pub result: String,
    pub kind_label: String,
    pub result_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResult {
    pub title: String,
    pub subtitle: String,
    pub copy_text: String,
    pub explanation: Option<String>,
    pub icon_path: Option<PathBuf>,
    pub calculation_display: Option<CalculationDisplay>,
    pub category: CommandCategory,
    pub action: CommandAction,
    pub confidence: u8,
}

impl CommandResult {
    pub fn calculation(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        copy_text: impl Into<String>,
        explanation: impl Into<String>,
        confidence: u8,
    ) -> Self {
        Self::calculation_with_display(
            title,
            subtitle,
            copy_text,
            explanation,
            "Calculation",
            "Result",
            confidence,
        )
    }

    pub fn calculation_with_display(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        copy_text: impl Into<String>,
        explanation: impl Into<String>,
        kind_label: impl Into<String>,
        result_label: impl Into<String>,
        confidence: u8,
    ) -> Self {
        let title = title.into();
        let subtitle = subtitle.into();
        let copy_text = copy_text.into();
        let kind_label = kind_label.into();
        let result_label = result_label.into();

        Self {
            calculation_display: Some(CalculationDisplay {
                expression: subtitle.clone(),
                result: title.clone(),
                kind_label,
                result_label,
            }),
            title,
            subtitle,
            copy_text: copy_text.clone(),
            explanation: Some(explanation.into()),
            icon_path: None,
            category: CommandCategory::Calculation,
            action: CommandAction::CopyToClipboard(copy_text),
            confidence,
        }
    }

    pub fn application(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        application_path: PathBuf,
        icon_path: Option<PathBuf>,
        confidence: u8,
    ) -> Self {
        let title = title.into();

        Self {
            copy_text: title.clone(),
            title,
            subtitle: subtitle.into(),
            explanation: None,
            icon_path,
            calculation_display: None,
            category: CommandCategory::Application,
            action: CommandAction::OpenPath(application_path),
            confidence,
        }
    }

    pub fn built_in(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        action: BuiltInAction,
        confidence: u8,
    ) -> Self {
        let title = title.into();

        Self {
            copy_text: title.clone(),
            title,
            subtitle: subtitle.into(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::BuiltIn,
            action: CommandAction::BuiltIn(action),
            confidence,
        }
    }

    pub fn file(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        file_path: PathBuf,
        confidence: u8,
    ) -> Self {
        let title = title.into();
        let subtitle = subtitle.into();

        Self {
            copy_text: file_path.display().to_string(),
            title,
            subtitle,
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::File,
            action: CommandAction::OpenPath(file_path),
            confidence,
        }
    }

    pub fn web_search(query: &str) -> Self {
        let encoded_query = query.replace(' ', "+");
        let search_url = format!("https://www.google.com/search?q={encoded_query}");

        Self {
            title: format!("Search the web for \"{query}\""),
            subtitle: search_url.clone(),
            copy_text: search_url.clone(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Web,
            action: CommandAction::OpenUrl(search_url),
            confidence: 20,
        }
    }

    pub fn open_website(url: impl Into<String>, display_label: impl Into<String>) -> Self {
        let url = url.into();
        let display_label = display_label.into();

        Self {
            title: format!("Open {display_label}"),
            subtitle: url.clone(),
            copy_text: url.clone(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Web,
            action: CommandAction::OpenUrl(url),
            confidence: 94,
        }
    }

    pub fn feature(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        category: CommandCategory,
        action: FeatureAction,
        confidence: u8,
    ) -> Self {
        let title = title.into();

        Self {
            copy_text: title.clone(),
            title,
            subtitle: subtitle.into(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category,
            action: CommandAction::Feature(action),
            confidence,
        }
    }

    pub fn copyable_feature(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        copy_text: impl Into<String>,
        category: CommandCategory,
        confidence: u8,
    ) -> Self {
        let title = title.into();
        let copy_text = copy_text.into();

        Self {
            title,
            subtitle: subtitle.into(),
            copy_text: copy_text.clone(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category,
            action: CommandAction::CopyToClipboard(copy_text),
            confidence,
        }
    }

    pub fn help(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        copy_text: impl Into<String>,
    ) -> Self {
        let copy_text = copy_text.into();

        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            copy_text: copy_text.clone(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Help,
            action: CommandAction::CopyToClipboard(copy_text),
            confidence: 10,
        }
    }

    pub fn informational(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        let title = title.into();

        Self {
            copy_text: String::new(),
            title,
            subtitle: subtitle.into(),
            explanation: None,
            icon_path: None,
            calculation_display: None,
            category: CommandCategory::Help,
            action: CommandAction::None,
            confidence: 0,
        }
    }
}
