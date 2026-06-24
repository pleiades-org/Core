use gpui::SharedString;
use std::ops::Range;

/// Resolve the byte range to replace from IME/marked text, a UTF-16 range, or the current selection.
pub fn resolve_replace_range(
    range_utf16: Option<&Range<usize>>,
    marked_range: &Option<Range<usize>>,
    selected_range: &Range<usize>,
    range_from_utf16: impl FnOnce(&Range<usize>) -> Range<usize>,
) -> Range<usize> {
    range_utf16
        .map(range_from_utf16)
        .or_else(|| marked_range.clone())
        .unwrap_or_else(|| selected_range.clone())
}

/// Apply a text replacement and collapse the selection to a caret after the inserted text.
pub fn apply_text_replacement(
    content: &mut SharedString,
    selected_range: &mut Range<usize>,
    marked_range: &mut Option<Range<usize>>,
    range: Range<usize>,
    new_text: &str,
) {
    *content = (content[0..range.start].to_owned() + new_text + &content[range.end..]).into();
    *selected_range = range.start + new_text.len()..range.start + new_text.len();
    marked_range.take();
}