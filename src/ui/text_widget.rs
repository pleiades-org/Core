use std::ops::Range;

use super::text_area::TextArea;
use super::text_input::TextInput;

/// Shared editing surface for single-line [`TextInput`] and multi-line [`TextArea`].
pub trait TextEditState {
    fn content(&self) -> &str;
    fn selected_range(&self) -> &Range<usize>;
    fn marked_range(&self) -> &Option<Range<usize>>;
    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize>;

    fn apply_replacement(&mut self, range_utf16: Option<Range<usize>>, new_text: &str);
}

impl TextEditState for TextInput {
    fn content(&self) -> &str {
        TextInput::content(self)
    }

    fn selected_range(&self) -> &Range<usize> {
        TextInput::selected_range(self)
    }

    fn marked_range(&self) -> &Option<Range<usize>> {
        TextInput::marked_range(self)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        TextInput::range_from_utf16(self, range_utf16)
    }

    fn apply_replacement(&mut self, range_utf16: Option<Range<usize>>, new_text: &str) {
        TextInput::apply_widget_replacement(self, range_utf16, new_text);
    }
}

impl TextEditState for TextArea {
    fn content(&self) -> &str {
        TextArea::content(self)
    }

    fn selected_range(&self) -> &Range<usize> {
        TextArea::selected_range(self)
    }

    fn marked_range(&self) -> &Option<Range<usize>> {
        TextArea::marked_range(self)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        TextArea::range_from_utf16(self, range_utf16)
    }

    fn apply_replacement(&mut self, range_utf16: Option<Range<usize>>, new_text: &str) {
        TextArea::apply_widget_replacement(self, range_utf16, new_text);
    }
}