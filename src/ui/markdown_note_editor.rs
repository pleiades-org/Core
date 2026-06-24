use crate::notes::{
    parse_markdown_inline, parse_markdown_line, MarkdownInlineStyle, MarkdownLineKind,
};
use gpui::{
    actions, div, prelude::*, px, rgb, Context, Entity, FocusHandle, Focusable, FontWeight,
    KeyBinding, MouseButton, MouseUpEvent, Render, Window,
};

use super::{
    browse_views::{
        input_field_background, input_field_border, result_row_hover_background,
    },
    text_input::TextInput,
};

const MARKDOWN_NOTE_KEY_CONTEXT: &str = "MarkdownNoteEditor";

actions!(
    markdown_note_editor,
    [CommitMarkdownLine, EditPreviousMarkdownLine, EditNextMarkdownLine]
);

pub fn bind_markdown_note_editor_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", CommitMarkdownLine, Some(MARKDOWN_NOTE_KEY_CONTEXT)),
        KeyBinding::new("shift-enter", CommitMarkdownLine, Some(MARKDOWN_NOTE_KEY_CONTEXT)),
        KeyBinding::new("up", EditPreviousMarkdownLine, Some(MARKDOWN_NOTE_KEY_CONTEXT)),
        KeyBinding::new("down", EditNextMarkdownLine, Some(MARKDOWN_NOTE_KEY_CONTEXT)),
    ]);
}

pub struct MarkdownNoteEditor {
    focus_handle: FocusHandle,
    lines: Vec<String>,
    editing_line: usize,
    line_input: Entity<TextInput>,
}

impl MarkdownNoteEditor {
    pub fn new(placeholder: impl Into<gpui::SharedString>, cx: &mut Context<Self>) -> Self {
        let line_input = cx.new(|cx| {
            TextInput::new_compact(placeholder, cx)
                .borderless()
                .with_key_context(MARKDOWN_NOTE_KEY_CONTEXT)
        });

        cx.observe(&line_input, |editor, line_input, cx| {
            if editor.editing_line < editor.lines.len() {
                editor.lines[editor.editing_line] = line_input.read(cx).content().to_string();
            }
            cx.notify();
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            lines: vec![String::new()],
            editing_line: 0,
            line_input,
        }
    }

    pub fn editor_focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.line_input.focus_handle(cx)
    }

    pub fn content(&self) -> String {
        let mut lines = self.lines.clone();
        if lines.len() > 1 && lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    pub fn set_content(&mut self, content: impl Into<String>, cx: &mut Context<Self>) {
        let content = content.into();
        self.lines = if content.is_empty() {
            vec![String::new()]
        } else {
            content.split('\n').map(str::to_string).collect()
        };

        if self.lines.last().is_none_or(|line| !line.is_empty()) {
            self.lines.push(String::new());
        }

        self.editing_line = self.lines.len().saturating_sub(1);
        self.sync_line_input(cx);
        cx.notify();
    }

    fn sync_editing_line_from_input(&mut self, cx: &Context<Self>) {
        if self.editing_line < self.lines.len() {
            self.lines[self.editing_line] = self.line_input.read(cx).content().to_string();
        }
    }

    fn sync_line_input(&mut self, cx: &mut Context<Self>) {
        let line_text = self
            .lines
            .get(self.editing_line)
            .cloned()
            .unwrap_or_default();
        self.line_input.update(cx, |input, cx| {
            input.set_content(line_text, cx);
        });
    }

    fn set_editing_line(&mut self, line_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if line_index >= self.lines.len() {
            return;
        }

        self.sync_editing_line_from_input(cx);
        self.editing_line = line_index;
        self.sync_line_input(cx);
        window.focus(&self.line_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn commit_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_editing_line_from_input(cx);

        let next_line_index = self.editing_line + 1;
        if next_line_index == self.lines.len() {
            self.lines.push(String::new());
        }

        self.editing_line = next_line_index;
        self.sync_line_input(cx);
        window.focus(&self.line_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn edit_relative_line(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_editing_line_from_input(cx);
        let next_line_index = (self.editing_line as isize + direction).max(0) as usize;
        if next_line_index >= self.lines.len() {
            return;
        }

        self.set_editing_line(next_line_index, window, cx);
    }

    fn commit_line_action(
        &mut self,
        _: &CommitMarkdownLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_line(window, cx);
    }

    fn edit_previous_line(
        &mut self,
        _: &EditPreviousMarkdownLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_relative_line(-1, window, cx);
    }

    fn edit_next_line(
        &mut self,
        _: &EditNextMarkdownLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_relative_line(1, window, cx);
    }

    fn edit_line_mouse(
        &mut self,
        line_index: usize,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_editing_line(line_index, window, cx);
    }
}

impl Focusable for MarkdownNoteEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MarkdownNoteEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing_line = self.editing_line;
        let lines = self.lines.clone();
        let is_focused = self.line_input.focus_handle(cx).is_focused(window);

        div()
            .key_context(MARKDOWN_NOTE_KEY_CONTEXT)
            .track_focus(&self.line_input.focus_handle(cx))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .gap(px(2.))
            .on_action(cx.listener(Self::commit_line_action))
            .on_action(cx.listener(Self::edit_previous_line))
            .on_action(cx.listener(Self::edit_next_line))
            .children(lines.into_iter().enumerate().map(|(line_index, line)| {
                if line_index == editing_line {
                    div()
                        .id(("markdown-note-edit-line", line_index))
                        .w_full()
                        .px(px(4.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(input_field_background(is_focused))
                        .border_1()
                        .border_color(input_field_border(true))
                        .child(self.line_input.clone())
                        .into_any_element()
                } else {
                    div()
                        .id(("markdown-note-preview-line", line_index))
                        .w_full()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded_md()
                        .hover(|style| {
                            style
                                .bg(result_row_hover_background())
                                .cursor_pointer()
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |editor, event, window, cx| {
                                editor.edit_line_mouse(line_index, event, window, cx);
                            }),
                        )
                        .child(render_markdown_line_preview(&line))
                        .into_any_element()
                }
            }))
    }
}

fn render_markdown_line_preview(line: &str) -> gpui::AnyElement {
    let (kind, text) = parse_markdown_line(line);

    match kind {
        MarkdownLineKind::Empty => div().h(px(18.)).into_any_element(),
        MarkdownLineKind::Heading1 => div()
            .text_size(px(20.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(0xffffff))
            .child(render_inline_markdown(&text))
            .into_any_element(),
        MarkdownLineKind::Heading2 => div()
            .text_size(px(17.))
            .line_height(px(24.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(0xf4f4f5))
            .child(render_inline_markdown(&text))
            .into_any_element(),
        MarkdownLineKind::Heading3 => div()
            .text_size(px(15.))
            .line_height(px(22.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(0xe4e4e7))
            .child(render_inline_markdown(&text))
            .into_any_element(),
        MarkdownLineKind::Bullet => div()
            .flex()
            .items_start()
            .gap(px(8.))
            .text_size(px(13.))
            .line_height(px(20.))
            .text_color(rgb(0xe4e4e7))
            .child(
                div()
                    .text_color(rgb(0x71717a))
                    .child("•"),
            )
            .child(render_inline_markdown(&text))
            .into_any_element(),
        MarkdownLineKind::Checkbox { checked } => {
            let text_node = if checked {
                div()
                    .text_color(rgb(0xa1a1aa))
                    .child(render_inline_markdown(&text))
            } else {
                div().child(render_inline_markdown(&text))
            };

            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(rgb(0xe4e4e7))
                .child(
                    div()
                        .text_color(if checked {
                            rgb(0x22c55e)
                        } else {
                            rgb(0x71717a)
                        })
                        .child(if checked { "☑" } else { "☐" }),
                )
                .child(text_node)
                .into_any_element()
        }
        MarkdownLineKind::Paragraph => div()
            .text_size(px(13.))
            .line_height(px(20.))
            .text_color(rgb(0xe4e4e7))
            .child(render_inline_markdown(&text))
            .into_any_element(),
    }
}

fn render_inline_markdown(text: &str) -> gpui::Div {
    let segments = parse_markdown_inline(text);

    div().flex().flex_wrap().children(
        segments.into_iter().map(|segment| {
            let mut node = div().child(segment.text);

            match segment.style {
                MarkdownInlineStyle::Plain => node = node.text_color(rgb(0xe4e4e7)),
                MarkdownInlineStyle::Bold => {
                    node = node
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff));
                }
                MarkdownInlineStyle::Italic => node = node.text_color(rgb(0xd4d4d8)),
                MarkdownInlineStyle::Code => {
                    node = node
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(rgb(0x1a1a1a))
                        .font_family("Consolas")
                        .text_size(px(12.))
                        .text_color(rgb(0xf4f4f5));
                }
            }

            node
        }),
    )
}