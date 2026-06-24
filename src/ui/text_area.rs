use std::ops::Range;

use gpui::{
    actions, div, fill, hsla, point, prelude::*, px, relative, rgb, rgba, size, App, Bounds,
    ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    browse_views::{input_field_background, input_field_border},
    text_editing::{apply_text_replacement, resolve_replace_range},
};

actions!(
    text_area,
    [
        Backspace,
        Delete,
        DeletePreviousWord,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Enter,
        Paste,
        Cut,
        Copy,
    ]
);

pub fn bind_text_area_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", Delete, None),
        KeyBinding::new("ctrl-backspace", DeletePreviousWord, None),
        KeyBinding::new("left", Left, None),
        KeyBinding::new("right", Right, None),
        KeyBinding::new("up", Up, None),
        KeyBinding::new("down", Down, None),
        KeyBinding::new("shift-left", SelectLeft, None),
        KeyBinding::new("shift-right", SelectRight, None),
        KeyBinding::new("shift-up", SelectLeft, None),
        KeyBinding::new("shift-down", SelectRight, None),
        KeyBinding::new("ctrl-left", Left, None),
        KeyBinding::new("ctrl-right", Right, None),
        KeyBinding::new("ctrl-shift-left", SelectLeft, None),
        KeyBinding::new("ctrl-shift-right", SelectRight, None),
        KeyBinding::new("ctrl-a", SelectAll, None),
        KeyBinding::new("cmd-a", SelectAll, None),
        KeyBinding::new("enter", Enter, None),
        KeyBinding::new("shift-enter", Enter, None),
        KeyBinding::new("ctrl-v", Paste, None),
        KeyBinding::new("cmd-v", Paste, None),
        KeyBinding::new("ctrl-c", Copy, None),
        KeyBinding::new("cmd-c", Copy, None),
        KeyBinding::new("ctrl-x", Cut, None),
        KeyBinding::new("cmd-x", Cut, None),
        KeyBinding::new("home", Home, None),
        KeyBinding::new("end", End, None),
    ]);
}

pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    line_height: Pixels,
}

impl TextArea {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_bounds: None,
            is_selecting: false,
            line_height: px(18.),
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn selected_range(&self) -> &Range<usize> {
        &self.selected_range
    }

    pub(crate) fn marked_range(&self) -> &Option<Range<usize>> {
        &self.marked_range
    }

    pub(crate) fn edit_content_mut(&mut self) -> &mut SharedString {
        &mut self.content
    }

    pub(crate) fn edit_selected_range_mut(&mut self) -> &mut Range<usize> {
        &mut self.selected_range
    }

    pub(crate) fn edit_marked_range_mut(&mut self) -> &mut Option<Range<usize>> {
        &mut self.marked_range
    }

    pub(crate) fn apply_widget_replacement(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
    ) {
        let range = resolve_replace_range(
            range_utf16.as_ref(),
            &self.marked_range,
            &self.selected_range,
            |range_utf16| self.range_from_utf16(range_utf16),
        );
        apply_text_replacement(
            &mut self.content,
            &mut self.selected_range,
            &mut self.marked_range,
            range,
            new_text,
        );
    }

    pub fn set_content(&mut self, new_content: impl Into<SharedString>, cx: &mut Context<Self>) {
        let content: SharedString = new_content.into();
        self.content = content.clone();
        let len = content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.set_content("", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, false, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let line_start = self.current_line_range().0;
        self.move_to(line_start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let line_end = self.current_line_range().1;
        self.move_to(line_end, cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous_offset = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == previous_offset {
                window.play_system_bell();
                return;
            }
            self.select_to(previous_offset, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next_offset = self.next_boundary(self.selected_range.end);
            if self.cursor_offset() == next_offset {
                window.play_system_bell();
                return;
            }
            self.select_to(next_offset, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_previous_word(
        &mut self,
        _: &DeletePreviousWord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            let previous_word_offset = self.previous_word_boundary(self.cursor_offset());
            if self.cursor_offset() == previous_word_offset {
                window.play_system_bell();
                return;
            }
            self.select_to(previous_word_offset, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        let index = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        self.selected_range = if anchor <= offset {
            anchor..offset
        } else {
            offset..anchor
        };
        self.selection_reversed = anchor > offset;
        self.marked_range = None;
        cx.notify();
    }

    fn move_vertically(&mut self, direction: isize, extend_selection: bool, cx: &mut Context<Self>) {
        let (line_start, line_end) = self.current_line_range();
        let column = self.cursor_offset().saturating_sub(line_start);
        let line_index = self.line_index_for_offset(line_start);
        let target_line_index = (line_index as isize + direction).max(0) as usize;
        let line_starts = self.line_starts();
        let Some(target_line_start) = line_starts.get(target_line_index).copied() else {
            return;
        };
        let target_line_end = line_starts
            .get(target_line_index + 1)
            .copied()
            .unwrap_or(self.content.len())
            .saturating_sub(if target_line_index + 1 < line_starts.len() {
                1
            } else {
                0
            });
        let target_offset = (target_line_start + column).min(target_line_end);

        if extend_selection {
            self.select_to(target_offset, cx);
        } else {
            self.move_to(target_offset, cx);
        }

        let _ = line_end;
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .take_while(|index| *index < offset)
            .last()
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index > offset)
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let left = self.content[..offset].trim_end();
        if left.is_empty() {
            return 0;
        }
        if let Some(space_index) = left.rfind(char::is_whitespace) {
            space_index
        } else {
            0
        }
    }

    fn line_starts(&self) -> Vec<usize> {
        let mut starts = vec![0];
        for (index, character) in self.content.char_indices() {
            if character == '\n' {
                starts.push(index + 1);
            }
        }
        starts
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        self.line_starts()
            .iter()
            .rposition(|line_start| *line_start <= offset)
            .unwrap_or(0)
    }

    fn current_line_range(&self) -> (usize, usize) {
        let line_starts = self.line_starts();
        let line_index = self.line_index_for_offset(self.cursor_offset());
        let line_start = line_starts[line_index];
        let line_end = line_starts
            .get(line_index + 1)
            .copied()
            .map(|next_line_start| next_line_start.saturating_sub(1))
            .unwrap_or(self.content.len());
        (line_start, line_end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        let local = bounds.localize(&position).unwrap_or(point(px(0.), px(0.)));
        let line_height: f32 = self.line_height.into();
        let y: f32 = local.y.max(px(0.)).into();
        let line_index = (y / line_height).floor() as usize;
        let line_starts = self.line_starts();
        let line_start = line_starts
            .get(line_index)
            .copied()
            .unwrap_or_else(|| *line_starts.last().unwrap_or(&0));
        let line_end = line_starts
            .get(line_index + 1)
            .copied()
            .map(|next_line_start| next_line_start.saturating_sub(1))
            .unwrap_or(self.content.len());
        let line_text = &self.content[line_start..line_end];
        let relative_index = approximate_index_for_x(line_text, local.x.max(px(0.)));
        (line_start + relative_index).min(self.content.len())
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.content
            .char_indices()
            .nth(range.start)
            .map(|(start, _)| start)
            .unwrap_or(0)
            ..self
                .content
                .char_indices()
                .nth(range.end)
                .map(|(end, _)| end)
                .unwrap_or(self.content.len())
    }

    pub(crate) fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        let start = self
            .content
            .char_indices()
            .map(|(index, _)| index)
            .nth(range_utf16.start)
            .unwrap_or(self.content.len());
        let end = self
            .content
            .char_indices()
            .map(|(index, _)| index)
            .nth(range_utf16.end)
            .unwrap_or(self.content.len());
        start..end
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset].chars().count()
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = resolve_replace_range(
            range_utf16.as_ref(),
            &self.marked_range,
            &self.selected_range,
            |range_utf16| self.range_from_utf16(range_utf16),
        );
        apply_text_replacement(
            &mut self.content,
            &mut self.selected_range,
            &mut self.marked_range,
            range,
            new_text,
        );
        cx.notify();
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        TextArea::replace_text_in_range(self, range_utf16, new_text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();

        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }

        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let line_starts = self.line_starts();
        let line_index = self.line_index_for_offset(range.start);
        let line_start = line_starts[line_index];
        let line_text = self.line_range_text(line_index);
        let start_x = approximate_x_for_index(line_text, range.start.saturating_sub(line_start));
        let end_x = approximate_x_for_index(line_text, range.end.saturating_sub(line_start));
        let top = bounds.top() + self.line_height * line_index as f32;
        Some(Bounds::from_corners(
            point(bounds.left() + start_x, top),
            point(bounds.left() + end_x, top + self.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }
}

impl TextArea {
    fn line_range_text(&self, line_index: usize) -> &str {
        let line_starts = self.line_starts();
        let line_start = line_starts[line_index];
        let line_end = line_starts
            .get(line_index + 1)
            .copied()
            .map(|next_line_start| next_line_start.saturating_sub(1))
            .unwrap_or(self.content.len());
        &self.content[line_start..line_end]
    }
}

struct TextAreaElement {
    area: Entity<TextArea>,
}

struct PrepaintState {
    lines: Vec<ShapedLineState>,
    selection: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

struct ShapedLineState {
    line: ShapedLine,
    top: Pixels,
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let area = self.area.read(cx);
        let display_text = if area.content.is_empty() {
            area.placeholder.clone()
        } else {
            area.content.clone()
        };
        let text_color = if area.content.is_empty() {
            hsla(0., 0., 0.85, 1.0)
        } else {
            window.text_style().color
        };
        let line_height = area.line_height;
        let mut lines = Vec::new();
        let mut selection = Vec::new();
        let mut cursor = None;

        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        for (line_index, line_text) in display_text.split('\n').enumerate() {
            let run = TextRun {
                len: line_text.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(line_text.into(), font_size, &[run], None);
            let top = bounds.top() + line_height * line_index as f32;
            lines.push(ShapedLineState { line: shaped, top });
        }

        if !area.selected_range.is_empty() && !area.content.is_empty() {
            for line_index in 0..lines.len() {
                let line_start = area
                    .line_starts()
                    .get(line_index)
                    .copied()
                    .unwrap_or(0);
                let line_end = area
                    .line_starts()
                    .get(line_index + 1)
                    .copied()
                    .map(|next| next.saturating_sub(1))
                    .unwrap_or(area.content.len());
                let selection_start = area.selected_range.start.max(line_start).min(line_end);
                let selection_end = area.selected_range.end.max(line_start).min(line_end);
                if selection_start < selection_end {
                    let line = &lines[line_index].line;
                    selection.push(fill(
                        Bounds::from_corners(
                            point(
                                bounds.left() + line.x_for_index(selection_start - line_start),
                                lines[line_index].top,
                            ),
                            point(
                                bounds.left() + line.x_for_index(selection_end - line_start),
                                lines[line_index].top + line_height,
                            ),
                        ),
                        rgba(0x2563eb45),
                    ));
                }
            }
        } else if area.focus_handle.is_focused(window) {
            let cursor_offset = area.cursor_offset();
            let line_index = area.line_index_for_offset(cursor_offset);
            if let Some(line_state) = lines.get(line_index) {
                let line_start = area.line_starts()[line_index];
                let x = line_state
                    .line
                    .x_for_index(cursor_offset.saturating_sub(line_start));
                cursor = Some(fill(
                    Bounds::new(
                        point(bounds.left() + x, line_state.top),
                        size(px(2.), line_height),
                    ),
                    gpui::blue(),
                ));
            }
        }

        PrepaintState {
            lines,
            selection,
            cursor,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.area.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let line_height = self.area.read(cx).line_height;
        for line_state in prepaint.lines.drain(..) {
            line_state
                .line
                .paint(
                    point(bounds.left(), line_state.top),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .unwrap();
        }

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.area.update(cx, |area, _cx| {
            area.last_bounds = Some(bounds);
        });
    }
}

fn approximate_x_for_index(text: &str, index: usize) -> Pixels {
    px(7.2 * index.min(text.len()) as f32)
}

fn approximate_index_for_x(text: &str, x: Pixels) -> usize {
    ((x / px(7.2)).floor() as usize).min(text.len())
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(window);
        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .key_context("TextArea")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_previous_word))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .text_size(px(13.))
            .text_color(rgb(0xffffff))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded_md()
                    .bg(input_field_background(is_focused))
                    .border_1()
                    .border_color(input_field_border(is_focused))
                    .child(TextAreaElement {
                        area: cx.entity(),
                    }),
            )
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}