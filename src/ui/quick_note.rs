use crate::{
    notes,
    settings::LauncherSettings,
    ui::{
        markdown_note_editor::MarkdownNoteEditor,
        platform_window::{hide_platform_window, show_platform_window},
    },
};
use gpui::{
    actions, div, prelude::*, px, rgb, size, Context, Entity, FocusHandle, Focusable, KeyBinding,
    MouseButton, MouseUpEvent, Render, Window,
};
use std::time::{Duration, Instant};

const QUICK_NOTE_PLACEHOLDER: &str = "Quick markdown note...";
const QUICK_NOTE_SAVE_DEBOUNCE_MS: u64 = 2_000;

actions!(quick_note, [DismissQuickNote]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickNoteSaveState {
    Saved,
    Unsaved,
    Saving,
}

pub struct QuickNoteView {
    note_editor: Entity<MarkdownNoteEditor>,
    focus_handle: FocusHandle,
    settings: LauncherSettings,
    is_visible: bool,
    last_visibility_change_at: Instant,
    save_generation: u64,
    saved_content: String,
    save_state: QuickNoteSaveState,
}

impl QuickNoteView {
    pub fn new(settings: LauncherSettings, cx: &mut Context<Self>) -> Self {
        let initial_content = notes::load_quick_note();
        let note_editor = cx.new(|cx| MarkdownNoteEditor::new(QUICK_NOTE_PLACEHOLDER, cx));
        note_editor.update(cx, |editor, cx| {
            editor.set_content(initial_content.clone(), cx);
        });

        cx.observe(&note_editor, |view, note_editor, cx| {
            let content = note_editor.read(cx).content();
            view.mark_dirty(&content, cx);
            view.schedule_save(content, cx);
        })
        .detach();

        Self {
            note_editor,
            focus_handle: cx.focus_handle(),
            settings,
            is_visible: false,
            last_visibility_change_at: Instant::now(),
            save_generation: 0,
            saved_content: initial_content,
            save_state: QuickNoteSaveState::Saved,
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        apply_quick_note_window_geometry(window, &self.settings);
        set_platform_window_topmost(window);
        show_platform_window(window);
        self.is_visible = true;
        self.last_visibility_change_at = Instant::now();
        window.focus(&self.note_editor.read(cx).editor_focus_handle(cx), cx);
        window.activate_window();
        cx.activate(true);
        cx.notify();
    }

    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_save(cx);
        self.is_visible = false;
        self.last_visibility_change_at = Instant::now();
        hide_platform_window(window);
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_visible {
            self.hide(window, cx);
        } else {
            self.show(window, cx);
        }
    }

    pub fn update_settings(&mut self, settings: LauncherSettings) {
        self.settings = settings;
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    fn dismiss(&mut self, _: &DismissQuickNote, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    fn mark_dirty(&mut self, content: &str, cx: &mut Context<Self>) {
        let next_state = if content == self.saved_content {
            QuickNoteSaveState::Saved
        } else if self.save_state == QuickNoteSaveState::Saving {
            QuickNoteSaveState::Saving
        } else {
            QuickNoteSaveState::Unsaved
        };

        if next_state != self.save_state {
            self.save_state = next_state;
            cx.notify();
        }
    }

    fn schedule_save(&mut self, content: String, cx: &mut Context<Self>) {
        if content == self.saved_content {
            return;
        }

        self.save_state = QuickNoteSaveState::Unsaved;
        self.save_generation = self.save_generation.wrapping_add(1);
        let generation = self.save_generation;

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(QUICK_NOTE_SAVE_DEBOUNCE_MS))
                .await;

            this.update(cx, |view, cx| {
                if view.save_generation != generation {
                    return;
                }
                view.save_state = QuickNoteSaveState::Saving;
                cx.notify();

                if notes::save_quick_note(&content).is_ok() {
                    view.saved_content = content;
                    view.save_state = QuickNoteSaveState::Saved;
                } else {
                    view.save_state = QuickNoteSaveState::Unsaved;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn flush_save(&mut self, cx: &mut Context<Self>) {
        let content = self.note_editor.read(cx).content();
        if content == self.saved_content {
            return;
        }

        if notes::save_quick_note(&content).is_ok() {
            self.saved_content = content;
            self.save_state = QuickNoteSaveState::Saved;
            cx.notify();
        }
    }

    fn save_state_label(&self) -> &'static str {
        match self.save_state {
            QuickNoteSaveState::Saved => "Saved",
            QuickNoteSaveState::Unsaved => "Unsaved",
            QuickNoteSaveState::Saving => "Saving...",
        }
    }

    fn save_state_color(&self) -> u32 {
        match self.save_state {
            QuickNoteSaveState::Saved => 0x22c55e,
            QuickNoteSaveState::Unsaved => 0xf59e0b,
            QuickNoteSaveState::Saving => 0x71717a,
        }
    }
}

impl Focusable for QuickNoteView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for QuickNoteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let save_label = self.save_state_label();
        let save_color = self.save_state_color();

        div()
            .key_context("QuickNote")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::dismiss))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x050505))
            .border_1()
            .border_color(rgb(0x1a1a1a))
            .text_color(rgb(0xffffff))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(rgb(0x27272a))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xe4e4e7))
                                    .child("Quick Note"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(save_color))
                                    .child(save_label),
                            ),
                    )
                    .child(
                        div()
                            .id("quick-note-close")
                            .text_size(px(11.))
                            .text_color(rgb(0x71717a))
                            .hover(|style| style.text_color(rgb(0xffffff)).cursor_pointer())
                            .child("Esc to hide")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::dismiss_mouse)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(10.))
                    .pb(px(10.))
                    .child(self.note_editor.clone()),
            )
    }
}

impl QuickNoteView {
    fn dismiss_mouse(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }
}

pub fn bind_quick_note_keys(cx: &mut gpui::App) {
    cx.bind_keys([KeyBinding::new("escape", DismissQuickNote, None)]);
}

pub fn apply_quick_note_window_geometry(window: &mut Window, settings: &LauncherSettings) {
    let (left, top, width, height) = notes::quick_note_window_origin(settings);
    window.resize(size(px(width), px(height)));
    set_platform_window_position(window, left, top);
}

#[cfg(target_os = "windows")]
fn set_platform_window_topmost(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let _ = SetWindowPos(
                    HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void),
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_platform_window_topmost(_window: &Window) {}

#[cfg(target_os = "windows")]
fn set_platform_window_position(window: &Window, left: i32, top: i32) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER},
    };

    if let Ok(window_handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() {
            unsafe {
                let _ = SetWindowPos(
                    HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void),
                    None,
                    left,
                    top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_platform_window_position(_window: &Window, _left: i32, _top: i32) {}