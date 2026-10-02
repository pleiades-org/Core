mod events;
mod layout;
mod painting;
mod printing;
mod progress;
use events::window_proc;

use super::{
    install,
    options::{Operation, Options},
    paths::InstallPaths,
};
use crate::windows::{
    app_icon, button_hover,
    corner_fringe::CornerFringe,
    painting as core_painting,
    settings::{
        control_style::{self, Look},
        Preferences, SettingsDocument,
    },
    theme::{self, scale, Fonts, Palette},
    view::{child, control_text},
    wide,
    window_placement::{self, ClipShape},
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::BTreeMap,
    fs,
    os::windows::process::CommandExt,
    process::Command,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{CreateMutexW, CREATE_NO_WINDOW},
        },
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED},
            HiDpi::*,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::*,
        },
    },
};

const CLASS_NAME: PCWSTR = w!("Pleiades.Core.Setup");
const HEIGHT: i32 = 444;
const INSET: i32 = 24;
const CONTROL_HEIGHT: i32 = 40;
const TITLE_ID: usize = 1200;
const DESCRIPTION_ID: usize = 1201;
const VERSION_ID: usize = 1202;
const INSTALL_TYPE_ID: usize = 1203;
const PATH_ID: usize = 1204;
const EXPLANATION_ID: usize = 1205;
const STATUS_ID: usize = 1207;
const PRIMARY_ID: usize = 1210;
const CANCEL_ID: usize = 1211;
const DESKTOP_ID: usize = 1212;
const WORK_TIMER: usize = 1;
const WORK_POLL_MILLISECONDS: u32 = 50;

#[derive(Clone)]
enum Status {
    Ready,
    Working,
    Complete,
    Failed(String),
}

struct SetupWindow {
    options: Options,
    paths: InstallPaths,
    preferences: Preferences,
    look: Cell<Look>,
    controls: RefCell<BTreeMap<usize, HWND>>,
    fringe: OnceCell<CornerFringe>,
    clip_shape: Cell<Option<ClipShape>>,
    status: RefCell<Status>,
    desktop: Cell<bool>,
    worker: RefCell<Option<JoinHandle<()>>>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
    background: Cell<HBRUSH>,
    selected: Cell<HBRUSH>,
}

impl SetupWindow {
    fn new(options: Options, paths: InstallPaths) -> windows::core::Result<Self> {
        let preferences = preferences(&paths);
        let dpi = unsafe { GetDpiForSystem() };
        let palette = Palette::for_background(preferences.background);
        let desktop = paths
            .desktop_enabled()
            .map_err(|error| windows::core::Error::new(E_FAIL, error))?;
        Ok(Self {
            options,
            paths,
            preferences,
            look: Cell::new(Look {
                dpi,
                fonts: Fonts::create(dpi)?,
                palette,
            }),
            controls: RefCell::new(BTreeMap::new()),
            fringe: OnceCell::new(),
            clip_shape: Cell::new(None),
            status: RefCell::new(Status::Ready),
            desktop: Cell::new(desktop),
            worker: RefCell::new(None),
            result: Arc::new(Mutex::new(None)),
            background: Cell::new(unsafe { CreateSolidBrush(palette.background) }),
            selected: Cell::new(unsafe { CreateSolidBrush(palette.selected) }),
        })
    }

    fn control(&self, identifier: usize) -> HWND {
        self.controls
            .borrow()
            .get(&identifier)
            .copied()
            .unwrap_or_default()
    }
}

impl Drop for SetupWindow {
    fn drop(&mut self) {
        self.look.get().fonts.delete();
        unsafe {
            let _ = DeleteObject(self.background.get().into());
            let _ = DeleteObject(self.selected.get().into());
        }
    }
}

pub(super) fn run(options: Options, paths: InstallPaths) -> Result<(), String> {
    let mutex_name = if paths.isolated {
        format!(
            "Local\\Pleiades.Core.Setup.{}",
            paths.uninstall_key.rsplit('\\').nth(1).unwrap_or("test")
        )
    } else {
        "Local\\Pleiades.Core.Setup".to_owned()
    };
    let mutex = unsafe { CreateMutexW(None, false, PCWSTR(wide(&mutex_name).as_ptr())) }
        .map_err(|error| error.to_string())?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Err("Another Core installer is already open.".into());
    }
    let outcome = (|| {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let context =
            Box::new(SetupWindow::new(options, paths).map_err(|error| error.to_string())?);
        unsafe { run_window(&context) }.map_err(|error| error.to_string())
    })();
    if let Err(error) = unsafe { CloseHandle(mutex) } {
        eprintln!("Could not close the setup lock: {error}");
    }
    outcome
}

unsafe fn run_window(context: &SetupWindow) -> windows::core::Result<()> {
    let instance: HINSTANCE = GetModuleHandleW(None)?.into();
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hIcon: app_icon::class_icon(instance)?,
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return Err(windows::core::Error::from_win32());
    }
    let dpi = context.look.get().dpi;
    let width = scale(theme::WIDTH, dpi);
    let height = scale(HEIGHT, dpi);
    let window = CreateWindowExW(
        WS_EX_APPWINDOW | WS_EX_CONTROLPARENT,
        CLASS_NAME,
        w!("Core Setup"),
        WS_POPUP | WS_CLIPCHILDREN,
        (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
        (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
        width,
        height,
        None,
        None,
        Some(instance),
        Some(context as *const SetupWindow as *const _),
    )?;
    let _ = ShowWindow(
        window,
        if context.options.background {
            SW_SHOWNOACTIVATE
        } else {
            SW_SHOW
        },
    );
    if !context.options.background {
        let _ = SetForegroundWindow(window);
    }
    let _ = SetFocus(Some(context.control(PRIMARY_ID)));
    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, None, 0, 0).0;
        if result == 0 {
            return Ok(());
        }
        if result == -1 {
            let _ = DestroyWindow(window);
            return Err(windows::core::Error::from_win32());
        }
        if message.message == WM_KEYDOWN && message.wParam.0 == VK_ESCAPE.0 as usize {
            let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
        } else if message.message == WM_KEYDOWN && message.wParam.0 == VK_RETURN.0 as usize {
            let focused = GetDlgCtrlID(GetFocus()) as usize;
            if matches!(focused, PRIMARY_ID | CANCEL_ID | DESKTOP_ID) {
                PostMessageW(Some(window), WM_COMMAND, WPARAM(focused), LPARAM(0))?;
            } else {
                context.accept(window);
            }
        } else if !IsDialogMessageW(window, &message).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn preferences(paths: &InstallPaths) -> Preferences {
    let result = fs::metadata(&paths.settings).and_then(|metadata| {
        if metadata.len() > 128 * 1024 {
            return Err(std::io::Error::other("Settings file is too large"));
        }
        fs::read_to_string(&paths.settings)
    });
    match result {
        Ok(text) => match SettingsDocument::decode(&text) {
            Ok(document) => document.preferences,
            Err(error) => {
                eprintln!("Setup could not read Core's appearance: {error}");
                Preferences::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
        Err(error) => {
            eprintln!("Setup could not read Core's appearance: {error}");
            Preferences::default()
        }
    }
}
