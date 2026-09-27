use super::{
    foreground_observer::FOREGROUND_CHANGED,
    icon_worker::ICONS_READY,
    launcher_state::{run_mode, LauncherState, Options, COMMAND_OUTPUT_TIMER},
    search_worker::WORKER_READY,
    settings::{
        page::{COLOR_ID, DONE_ID, SHORTCUT_ID},
        SETTINGS_SAVED,
    },
    tray::{Tray, TrayAction, TRAY_EVENT},
};
use super::{
    view::{View, INPUT_ID, RESULTS_ID, SETTINGS_ID},
    wide,
};
use core_engine::search::RunMode;
use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, EM_SETSEL, MEASUREITEMSTRUCT, NMCUSTOMDRAW, NMHDR, NM_CUSTOMDRAW, WM_MOUSELEAVE,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
            Threading::CreateMutexW,
        },
        UI::{
            HiDpi::*,
            Input::{Ime::*, KeyboardAndMouse::*},
            WindowsAndMessaging::*,
        },
    },
};

const CLASS_NAME: PCWSTR = w!("Pleiades.Core.V2");
const ACCEPT_RESULT: u32 = WM_APP + 2;
const SHOW_LAUNCHER: u32 = WM_APP + 4;
const SHOW_SETTINGS: u32 = WM_APP + 7;
const UPDATE_DPI: u32 = WM_APP + 9;
const TEST_QUEUE_FENCE: u32 = WM_APP + 10;

struct WindowContext {
    state: RefCell<LauncherState>,
    view: OnceCell<Rc<View>>,
    update_startup: RefCell<super::updates::handoff::Startup>,
}

pub fn run() -> windows::core::Result<()> {
    super::diagnostics::redirect_stderr_to_log();
    let update_error =
        |error: super::updates::UpdateError| windows::core::Error::new(E_FAIL, error.to_string());
    if super::updates::handoff::dispatch().map_err(update_error)? {
        return Ok(());
    }
    super::updates::handoff::wait_for_previous().map_err(update_error)?;
    let options = Options::read();
    let _instance = if options.probe || options.dry_run {
        None
    } else {
        let instance = SingleInstance::acquire()?;
        if instance.is_none() {
            return Ok(());
        }
        instance
    };
    let update_startup =
        super::updates::handoff::Startup::prepare(options.probe || options.dry_run)
            .map_err(update_error)?;
    let _apartment = UiApartment::initialize()?;
    let context = Box::new(WindowContext {
        state: RefCell::new(LauncherState::new(options)),
        view: OnceCell::new(),
        update_startup: RefCell::new(update_startup),
    });
    let shell = &context.state;
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: super::app_icon::class_icon(instance).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let width = super::view::scale(super::theme::WIDTH, GetDpiForSystem());
        let height = super::view::scale(420, GetDpiForSystem());
        let left = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
        let top = (GetSystemMetrics(SM_CYSCREEN) - height) / 3;
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_CONTROLPARENT,
            CLASS_NAME,
            w!("Core v2"),
            WS_POPUP | WS_CLIPCHILDREN,
            left,
            top,
            width,
            height,
            None,
            None,
            Some(instance),
            Some(context.as_ref() as *const WindowContext as *const _),
        )?;
        let _window_lifetime = WindowLifetime(window);
        if let Err(error) = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) {
            eprintln!("Lock/unlock notifications are unavailable: {error}");
        }
        shell
            .borrow_mut()
            .start_worker(window)
            .map_err(|error| windows::core::Error::new(E_FAIL, error.to_string()))?;
        if !options.probe {
            let shortcut = shell.borrow().settings.saved.preferences.shortcut;
            if let Err(error) = shell.borrow_mut().configure_shortcut(window, shortcut) {
                eprintln!("Core shortcut could not be registered: {error}");
                if let Some(view) = context.view.get() {
                    view.set_footer(&error);
                }
            }
            // Explorer may not be ready at sign-in. TaskbarCreated retries the icon later.
            let mut tray = Tray::new(window, shortcut);
            if let Err(error) = tray.show() {
                eprintln!("Core's tray icon is waiting for the taskbar: {error}");
            }
            shell.borrow_mut().tray = Some(tray);
            if !options.hidden || context.update_startup.borrow().pending() {
                shell.borrow_mut().set_visible(window, true);
            }
        }
        if context.update_startup.borrow().pending() {
            SetTimer(
                Some(window),
                super::updates::handoff::STARTUP_TIMER,
                50,
                None,
            );
        }
        message_loop(window, shell)?;
        shell.borrow_mut().binding.take();
    }
    Ok(())
}

unsafe fn message_loop(window: HWND, shell: &RefCell<LauncherState>) -> windows::core::Result<()> {
    let mut message = MSG::default();
    loop {
        let status = GetMessageW(&mut message, None, 0, 0).0;
        if status == 0 {
            break;
        }
        if status == -1 {
            return Err(windows::core::Error::from_win32());
        }
        if message.message == WM_KEYDOWN && !is_composing(message.hwnd) {
            let view = shell.borrow().view.clone();
            let in_console = view
                .as_ref()
                .is_some_and(|view| view.is_output(message.hwnd));
            // A running command gets Tab (completion) and, in a full-screen program, Esc.
            let console_owns = match VIRTUAL_KEY(message.wParam.0 as u16) {
                VK_TAB => view.as_ref().is_some_and(|view| view.console_running()),
                VK_ESCAPE => view
                    .as_ref()
                    .is_some_and(|view| view.console_takes_escape()),
                _ => false,
            };
            if in_console && console_owns {
                // Shift+Tab becomes its own sequence; Tab and Esc are typed as characters.
                let key = VIRTUAL_KEY(message.wParam.0 as u16);
                if !view.as_ref().is_some_and(|view| view.console_key(key)) {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                continue;
            }
            if message.wParam.0 == VK_TAB.0 as usize {
                if shell.borrow().view.as_ref().is_some_and(|view| {
                    view.advance_quicklink_tab(
                        GetDlgCtrlID(message.hwnd) as usize,
                        GetKeyState(VK_SHIFT.0 as i32) < 0,
                    )
                }) {
                    continue;
                }
                // Native dialog routing changes default-button styles, breaking owner drawing.
                let next = GetNextDlgTabItem(
                    window,
                    Some(message.hwnd),
                    GetKeyState(VK_SHIFT.0 as i32) < 0,
                );
                match next {
                    Ok(control) => {
                        let _ = SetFocus(Some(control));
                    }
                    Err(error) => eprintln!("Could not move keyboard focus: {error}"),
                }
                continue;
            }
            let in_settings = shell
                .borrow()
                .view
                .as_ref()
                .is_some_and(|view| view.settings_open());
            if message.wParam.0 == VK_OEM_COMMA.0 as usize && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                post(window, SHOW_SETTINGS, WPARAM(0), LPARAM(0));
                continue;
            }
            // An open dropdown list takes Enter (choose) and Esc (close) before Settings does.
            let dropdown_open = in_settings
                && super::settings::page::is_dropdown(GetDlgCtrlID(message.hwnd) as usize)
                && SendMessageW(message.hwnd, CB_GETDROPPEDSTATE, None, None).0 != 0;
            if dropdown_open {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
                continue;
            }
            if in_settings {
                match VIRTUAL_KEY(message.wParam.0 as u16) {
                    VK_ESCAPE => shell.borrow_mut().dismiss_settings(window),
                    VK_RETURN => {
                        let identifier = GetDlgCtrlID(message.hwnd) as usize;
                        shell.borrow_mut().settings_command(
                            window,
                            if matches!(identifier, COLOR_ID | SHORTCUT_ID)
                                || super::settings::quicklink_table::is_edit(identifier)
                            {
                                DONE_ID
                            } else {
                                identifier
                            },
                            BN_CLICKED,
                        );
                    }
                    VK_A if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                        SendMessageW(message.hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                    }
                    _ => {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                continue;
            }
            match VIRTUAL_KEY(message.wParam.0 as u16) {
                VK_ESCAPE => {
                    if let Some(view) = &shell.borrow().view {
                        if view.close_power_menu() {
                            let _ = SetFocus(Some(view.input));
                            continue;
                        }
                    }
                    // Esc stops a running command first; the next Esc hides Core.
                    if shell.borrow_mut().stop_command() {
                        continue;
                    }
                    shell.borrow_mut().set_visible(window, false);
                    continue;
                }
                // Keys in the output go to the running command. Keys it handles itself (arrows,
                // copy and paste) must not also be typed as characters.
                key if in_console => {
                    if !view.as_ref().is_some_and(|view| view.console_key(key)) {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    continue;
                }
                VK_RETURN => {
                    let identifier = GetDlgCtrlID(message.hwnd) as usize;
                    if identifier == super::power_menu::POWER_ID
                        || super::power_menu::action(identifier).is_some()
                    {
                        post(
                            window,
                            WM_COMMAND,
                            WPARAM(identifier),
                            LPARAM(message.hwnd.0 as isize),
                        );
                        continue;
                    }
                    let message = if GetDlgCtrlID(message.hwnd) as usize == SETTINGS_ID {
                        SHOW_SETTINGS
                    } else {
                        // Ctrl+Enter opens a terminal; Ctrl+Shift+Enter runs as administrator.
                        // Typed keys only arrive while Core is in front; otherwise the key
                        // state belongs to whatever the person is doing elsewhere.
                        let typed = GetForegroundWindow() == window;
                        shell.borrow_mut().set_accept_mode(run_mode(
                            typed && GetKeyState(VK_CONTROL.0 as i32) < 0,
                            typed && GetKeyState(VK_SHIFT.0 as i32) < 0,
                        ));
                        ACCEPT_RESULT
                    };
                    post(window, message, WPARAM(0), LPARAM(0));
                    continue;
                }
                VK_A if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                    if let Some(view) = &shell.borrow().view {
                        SendMessageW(view.input, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                    }
                    continue;
                }
                // Backspace in an empty command prompt removes the hidden `/`.
                VK_BACK if shell.borrow_mut().leave_command_mode(message.hwnd) => continue,
                VK_UP | VK_DOWN => {
                    let up = message.wParam.0 == VK_UP.0 as usize;
                    let Some(view) = shell.borrow().view.clone() else {
                        continue;
                    };
                    if view.power_menu_open() {
                        view.move_power_selection(up);
                        continue;
                    }
                    // A shell query recalls history instead of moving through rows.
                    if shell.borrow_mut().recall_history(up) {
                        continue;
                    }
                    let step = if up { -1 } else { 1 };
                    if view.grid() {
                        view.move_grid_selection(0, step);
                    } else {
                        view.move_selection(step);
                    }
                    shell.borrow().refresh_footer();
                    continue;
                }
                // With nothing typed there is no caret to move, so left and right move
                // through the recently used apps.
                VK_LEFT | VK_RIGHT
                    if shell
                        .borrow()
                        .view
                        .as_ref()
                        .is_some_and(|view| view.grid() && view.is_input(message.hwnd)) =>
                {
                    if let Some(view) = &shell.borrow().view {
                        view.move_grid_selection(
                            if message.wParam.0 == VK_LEFT.0 as usize {
                                -1
                            } else {
                                1
                            },
                            0,
                        );
                    }
                    continue;
                }
                _ => {}
            }
        }
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    Ok(())
}

/// A full message queue drops one keystroke's action; it must not end the message loop.
unsafe fn post(window: HWND, message: u32, word: WPARAM, long: LPARAM) {
    if let Err(error) = PostMessageW(Some(window), message, word, long) {
        eprintln!("Could not queue Core message {message:#X}: {error}");
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(long.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    // Lifecycle messages can be reentrant (DestroyWindow sends them synchronously).
    match message {
        WM_CLOSE => {
            let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const WindowContext;
            if let Some(context) = pointer.as_ref() {
                if let Ok(mut state) = context.state.try_borrow_mut() {
                    if !state.close_after_save {
                        state.flush_settings(window);
                    }
                    if state.settings.is_saving() {
                        state.close_after_save = true;
                        return LRESULT(0);
                    }
                    if !state.finish_update_exit() {
                        return LRESULT(0);
                    }
                    state.worker.take();
                    state.icon_worker.take();
                    state.foreground_observer.take();
                    state.tray.take();
                    // Stops a running command; programs it already left running are untouched.
                    state.stop_command();
                    if let Some(view) = &state.view {
                        view.set_clock_active(false);
                    }
                }
            }
            let _ = DestroyWindow(window);
            return LRESULT(0);
        }
        WM_DESTROY => {
            let _ = WTSUnRegisterSessionNotification(window);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            return DefWindowProcW(window, message, word, long);
        }
        _ => {}
    }
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const WindowContext;
    let Some(context) = pointer.as_ref() else {
        return DefWindowProcW(window, message, word, long);
    };
    let cell = &context.state;
    // SetWindowPos can synchronously deliver DPI changes while a layout borrows state.
    if message == WM_DPICHANGED {
        let _ = PostMessageW(Some(window), UPDATE_DPI, WPARAM(word.0 & 0xffff), LPARAM(0));
        return LRESULT(0);
    }
    // Owner-draw callbacks may run during a state update; rendering must not borrow that state.
    if message == WM_MEASUREITEM && word.0 == RESULTS_ID {
        (*(long.0 as *mut MEASUREITEMSTRUCT)).itemHeight =
            super::theme::scale(super::theme::ROW_HEIGHT, GetDpiForWindow(window)) as u32;
        return LRESULT(1);
    }
    if let Some(view) = context.view.get() {
        if let Some(result) = paint_message(window, message, word, long, view) {
            return result;
        }
    }
    // TrackPopupMenu dispatches messages itself. Keep the application state available to that loop.
    if message == TRAY_EVENT && long.0 as u32 == WM_RBUTTONUP {
        let activate = cell
            .try_borrow()
            .map_or(true, |state| !state.options.background_for_test);
        match Tray::menu(window, activate) {
            Ok(TrayAction::Open) => {
                let _ = PostMessageW(Some(window), SHOW_LAUNCHER, WPARAM(0), LPARAM(0));
            }
            Ok(TrayAction::Exit) => {
                let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            Ok(TrayAction::Settings) => {
                let _ = PostMessageW(Some(window), SHOW_SETTINGS, WPARAM(0), LPARAM(0));
            }
            Err(error) => eprintln!("Could not open Core tray menu: {error}"),
            _ => {}
        }
        return LRESULT(0);
    }
    // Win32 may reenter the parent while controls are being updated. Never alias mutable Rust references.
    let Ok(mut shell) = cell.try_borrow_mut() else {
        return DefWindowProcW(window, message, word, long);
    };
    let result = match message {
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION if word.0 != 0 => {
            shell.restart_for_update = false;
            shell.flush_settings(window);
            // Windows may end the process before an asynchronous preference save completes.
            // Keep the stage for a later exit rather than install against a pending Off/Notify.
            if !shell.settings.is_saving() {
                shell.finish_update_exit();
            }
            LRESULT(0)
        }
        WM_TIMER if word.0 == super::updates::handoff::STARTUP_TIMER => {
            context.update_startup.borrow_mut().confirm_visible(window);
            LRESULT(0)
        }
        super::updates::UPDATE_READY => {
            shell.refresh_footer();
            LRESULT(0)
        }
        TEST_QUEUE_FENCE if shell.options.dry_run => {
            // Test commands are posted like user input. A posted acknowledgement avoids
            // inspecting a half-finished layout through reentrant SendMessage callbacks.
            if let Err(error) = SetPropW(
                window,
                w!("Core.Test.Fence"),
                Some(HANDLE(word.0 as *mut _)),
            ) {
                eprintln!("Could not acknowledge the test queue: {error}");
            }
            LRESULT(0)
        }
        WM_CREATE => {
            let preferences = shell.settings.saved.preferences;
            match GetModuleHandleW(None)
                .and_then(|instance| View::create(window, instance.into(), preferences))
            {
                Ok(view) => {
                    let view = Rc::new(view);
                    if context.view.set(view.clone()).is_err() {
                        return LRESULT(-1);
                    }
                    shell.view = Some(view);
                    shell.render_results();
                }
                Err(error) => {
                    eprintln!("Could not create Core controls: {error}");
                    return LRESULT(-1);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND if word.0 & 0xffff == INPUT_ID && (word.0 >> 16) as u32 == EN_CHANGE => {
            shell.query_edited();
            LRESULT(0)
        }
        WM_COMMAND
            if word.0 & 0xffff == super::power_menu::POWER_ID
                && (word.0 >> 16) as u32 == BN_CLICKED =>
        {
            if let Some(view) = &shell.view {
                if let Err(error) = view.toggle_power_menu() {
                    view.set_footer(&format!("Could not open power options: {error}"));
                }
            }
            LRESULT(0)
        }
        WM_COMMAND
            if super::power_menu::action(word.0 & 0xffff).is_some()
                && (word.0 >> 16) as u32 == BN_CLICKED =>
        {
            shell.choose_power(
                super::power_menu::action(word.0 & 0xffff).expect("power action checked"),
            );
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(view) = &shell.view {
                view.close_power_menu();
            }
            LRESULT(0)
        }
        // A click on a recently used app opens it, as in Start.
        WM_LBUTTONUP => {
            let (x, y) = client_point(long);
            if let Some(index) = shell.view.as_ref().and_then(|view| view.grid_hit(x, y)) {
                if let Some(view) = &shell.view {
                    view.select_tile(index);
                }
                shell.set_accept_mode(RunMode::Capture);
                shell.accept();
            }
            LRESULT(0)
        }
        WM_VSCROLL
            if long.0 != 0
                && GetDlgCtrlID(HWND(long.0 as *mut _)) as usize
                    == super::settings::quicklink_table::SCROLL_ID =>
        {
            if let Some(view) = &shell.view {
                view.scroll_quicklinks((word.0 & 0xffff) as u16, None);
            }
            LRESULT(0)
        }
        WM_HSCROLL
            if long.0 != 0 && shell.view.as_ref().is_some_and(|view| view.settings_open()) =>
        {
            let identifier = GetDlgCtrlID(HWND(long.0 as *mut _)) as usize;
            shell.settings_command(window, identifier, (word.0 & 0xffff) as u32);
            LRESULT(0)
        }
        WM_MOUSEWHEEL if shell.view.as_ref().is_some_and(|view| view.settings_open()) => {
            if let Some(view) = &shell.view {
                view.scroll_quicklinks(0, Some((word.0 >> 16) as i16));
            }
            LRESULT(0)
        }
        WM_COMMAND if word.0 & 0xffff == INPUT_ID && (word.0 >> 16) as u32 == EN_SETFOCUS => {
            if let Some(view) = &shell.view {
                view.close_power_menu();
            }
            LRESULT(0)
        }
        WM_COMMAND if word.0 & 0xffff == SETTINGS_ID && (word.0 >> 16) as u32 == BN_CLICKED => {
            shell.open_settings(window);
            LRESULT(0)
        }
        WM_COMMAND if shell.view.as_ref().is_some_and(|view| view.settings_open()) => {
            shell.settings_command(window, word.0 & 0xffff, (word.0 >> 16) as u32);
            LRESULT(0)
        }
        WM_COMMAND if word.0 & 0xffff == RESULTS_ID && (word.0 >> 16) as u32 == LBN_DBLCLK => {
            shell.accept();
            LRESULT(0)
        }
        WM_COMMAND if word.0 & 0xffff == RESULTS_ID && (word.0 >> 16) as u32 == LBN_SELCHANGE => {
            shell.refresh_footer();
            LRESULT(0)
        }
        WM_TIMER if word.0 == super::view::CLOCK_TIMER => {
            if let Some(view) = &shell.view {
                view.refresh_clock();
            }
            LRESULT(0)
        }
        WM_TIMECHANGE => {
            if let Some(view) = &shell.view {
                view.refresh_clock();
            }
            LRESULT(0)
        }
        WM_TIMER if word.0 == super::motion::TRANSITION_TIMER => {
            shell.transition.tick(window);
            LRESULT(0)
        }
        WM_TIMER if word.0 == super::settings::AUTO_SAVE_TIMER => {
            // Quicklink typing leaves the full check to this pause, so the draft is read again.
            shell.flush_settings(window);
            LRESULT(0)
        }
        WM_TIMER if word.0 == COMMAND_OUTPUT_TIMER => {
            shell.command_output_timer();
            LRESULT(0)
        }
        WM_HOTKEY => {
            // Open but behind another app (Windows refused it the foreground): come forward
            // rather than hide, so one press always shows Core. Background tests never have
            // the foreground, so there the shortcut simply toggles.
            let in_front =
                shell.options.background_for_test || owns_window(window, GetForegroundWindow());
            let show = !(shell.visible && in_front);
            shell.set_visible(window, show);
            LRESULT(0)
        }
        SHOW_LAUNCHER => {
            shell.set_visible(window, true);
            LRESULT(0)
        }
        SHOW_SETTINGS => {
            shell.open_settings(window);
            LRESULT(0)
        }
        SETTINGS_SAVED => {
            shell.receive_settings(window);
            if shell.close_after_save && !shell.settings.is_saving() {
                let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        FOREGROUND_CHANGED => {
            let activated_window = HWND(long.0 as *mut _);
            let foreground = GetForegroundWindow();
            let current_observer = shell
                .foreground_observer
                .as_ref()
                .is_some_and(|observer| observer.matches(word.0));
            // An activation notification for Core must not dismiss its own entrance.
            // Recheck foreground too: a queued outside event may predate a rapid reopen.
            if shell.visible
                && current_observer
                && !activated_window.0.is_null()
                && !owns_window(window, activated_window)
                && !owns_window(window, foreground)
            {
                shell.set_visible(window, false);
            }
            LRESULT(0)
        }
        super::activation::SHORTCUT_ERROR => {
            if let Some(view) = &shell.view {
                view.set_footer("Windows blocked a shortcut replay. Use a normal Core shortcut with elevated apps.");
            }
            LRESULT(0)
        }
        UPDATE_DPI => {
            if let Some(view) = &mut shell.view {
                if let Err(error) = view.set_dpi((word.0 & 0xffff) as u32) {
                    view.set_footer(&format!("Could not scale Core: {error}"));
                }
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE if matches!(word.0 as u32, WTS_SESSION_LOCK | WTS_SESSION_UNLOCK) => {
            if let Some(binding) = &shell.binding {
                binding.reset_windows_key();
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            if message == WM_SETTINGCHANGE && is_theme_change(long) {
                if let Some(tray) = &mut shell.tray {
                    if let Err(error) = tray.refresh_theme() {
                        eprintln!("Could not update Core's tray icon theme: {error}");
                    }
                }
            }
            if let Some(view) = shell.view.as_ref().filter(|_| shell.visible) {
                if let Err(error) = view.position_on_monitor(window) {
                    view.set_footer(&format!("Could not update screen position: {error}"));
                }
            }
            LRESULT(0)
        }
        WORKER_READY => {
            shell.receive();
            LRESULT(0)
        }
        super::commands::COMMAND_OUTPUT => {
            shell.receive_command_output();
            LRESULT(0)
        }
        super::exchange_rates::RATES_READY => {
            shell.receive_exchange_rates();
            LRESULT(0)
        }
        ICONS_READY => {
            shell.receive_icons();
            LRESULT(0)
        }
        ACCEPT_RESULT => {
            shell.accept();
            LRESULT(0)
        }
        TRAY_EVENT => {
            if long.0 as u32 == WM_LBUTTONUP {
                shell.set_visible(window, true);
            }
            LRESULT(0)
        }
        taskbar_created if taskbar_created == super::tray::taskbar_created_message() => {
            if let Some(tray) = &mut shell.tray {
                if let Err(error) = tray.show() {
                    eprintln!("Could not restore Core's tray icon: {error}");
                }
            }
            LRESULT(0)
        }
        _ => {
            // Default painting can synchronously ask us for background and control colors.
            drop(shell);
            return DefWindowProcW(window, message, word, long);
        }
    };
    let action = shell.pending_action.take();
    drop(shell);
    // Shell APIs may run nested message loops. Never hold our state borrow across them.
    if let Some(action) = action {
        let outcome = action.execute(window);
        if IsWindow(Some(window)).as_bool() {
            let mut shell = cell.borrow_mut();
            match outcome {
                Ok(()) => shell.set_visible(window, false),
                Err(error) => {
                    if let Some(view) = &shell.view {
                        view.set_footer(&error);
                    }
                }
            }
        }
        // An app launch is recorded before it starts but written only now.
        cell.borrow_mut().save_launches();
    }
    result
}

unsafe fn paint_message(
    window: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
    view: &View,
) -> Option<LRESULT> {
    match message {
        WM_DRAWITEM => {
            view.draw_item(&*(long.0 as *const DRAWITEMSTRUCT));
            Some(LRESULT(1))
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORSTATIC => {
            Some(view.color_control(HDC(word.0 as *mut _), HWND(long.0 as *mut _)))
        }
        WM_NOTIFY => {
            let header = (long.0 as *const NMHDR).as_ref()?;
            if header.code == NM_CUSTOMDRAW {
                view.custom_draw(&*(long.0 as *const NMCUSTOMDRAW))
            } else {
                None
            }
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let context = BeginPaint(window, &mut paint);
            view.paint(context, &paint.rcPaint);
            let _ = EndPaint(window, &paint);
            Some(LRESULT(0))
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_MOUSEMOVE => {
            let (x, y) = client_point(long);
            view.hover_grid(x, y);
            None
        }
        WM_MOUSELEAVE => {
            view.leave_grid();
            None
        }
        WM_PRINTCLIENT => {
            view.paint(HDC(word.0 as *mut _), &view.client_area());
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// The client coordinates in a mouse message; negative on multi-monitor setups.
fn client_point(long: LPARAM) -> (i32, i32) {
    (
        i32::from((long.0 & 0xffff) as u16 as i16),
        i32::from(((long.0 >> 16) & 0xffff) as u16 as i16),
    )
}

pub fn show_fatal_error(message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("Core could not start"),
            MB_OK | MB_ICONERROR,
        );
    }
}

unsafe fn owns_window(launcher: HWND, candidate: HWND) -> bool {
    candidate == launcher
        || (!candidate.0.is_null() && GetAncestor(candidate, GA_ROOTOWNER) == launcher)
}

struct UiApartment(bool);
impl UiApartment {
    fn initialize() -> windows::core::Result<Self> {
        let status =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if status == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        status.ok()?;
        Ok(Self(true))
    }
}
impl Drop for UiApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct WindowLifetime(HWND);
impl Drop for WindowLifetime {
    fn drop(&mut self) {
        // Window callbacks still borrow the boxed state here. Destroy synchronously before
        // that state and its COM apartment are dropped; WM_CLOSE can defer for a settings save.
        unsafe {
            if IsWindow(Some(self.0)).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}

/// Windows announces light/dark changes with this setting name.
unsafe fn is_theme_change(long: LPARAM) -> bool {
    long.0 != 0
        && PCWSTR(long.0 as *const u16)
            .to_string()
            .is_ok_and(|setting| setting == "ImmersiveColorSet")
}

unsafe fn is_composing(window: HWND) -> bool {
    let context = ImmGetContext(window);
    if context.0.is_null() {
        return false;
    }
    let composing = ImmGetCompositionStringW(context, GCS_COMPSTR, None, 0) > 0;
    let _ = ImmReleaseContext(window, context);
    composing
}

struct SingleInstance(HANDLE);
impl SingleInstance {
    fn acquire() -> windows::core::Result<Option<Self>> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\Pleiades.Core.V2"))?;
            let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
            let guard = Self(handle);
            if !already_running {
                return Ok(Some(guard));
            }
            // Failing to activate the existing window must not show a startup error.
            if let Ok(window) = FindWindowW(CLASS_NAME, None) {
                post(window, SHOW_LAUNCHER, WPARAM(0), LPARAM(0));
            }
            Ok(None)
        }
    }
}
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::COINIT_MULTITHREADED;

    #[test]
    fn ui_apartment_balances_success_and_leaves_a_different_mode_owned() {
        std::thread::spawn(|| {
            let outer = UiApartment::initialize().unwrap();
            let nested = UiApartment::initialize().unwrap();
            assert!(outer.0 && nested.0);
            drop(nested);
            assert_eq!(
                unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) },
                RPC_E_CHANGED_MODE
            );
            drop(outer);
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .unwrap();
            let borrowed = UiApartment::initialize().unwrap();
            assert!(!borrowed.0);
            drop(borrowed);
            assert_eq!(
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) },
                RPC_E_CHANGED_MODE
            );
            unsafe { CoUninitialize() };
            assert!(UiApartment::initialize().unwrap().0);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn window_lifetime_destroys_a_window_that_defers_close() {
        unsafe extern "system" fn defer_close(
            window: HWND,
            message: u32,
            word: WPARAM,
            long: LPARAM,
        ) -> LRESULT {
            if message == WM_CLOSE {
                return LRESULT(0);
            }
            DefWindowProcW(window, message, word, long)
        }
        let _serial = super::super::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let instance = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(defer_close),
            hInstance: instance,
            lpszClassName: w!("Core.Test.DeferredClose"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        unsafe {
            SendMessageW(window, WM_CLOSE, None, None);
        }
        assert!(unsafe { IsWindow(Some(window)) }.as_bool());
        drop(WindowLifetime(window));
        assert!(!unsafe { IsWindow(Some(window)) }.as_bool());
        unsafe { UnregisterClassW(class.lpszClassName, Some(instance)) }.unwrap();
    }
}
