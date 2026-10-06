//! What the results list's own window procedures share: finding the row under the pointer,
//! selecting a row as a click would, and telling Core's window. The volume mixer's rows and a
//! playlist's buttons both use them.
use super::*;
use level_slider::contains;

/// A row's area in the list's own coordinates; None when the list has no such row.
pub(super) fn item_area(list: HWND, index: usize) -> Option<RECT> {
    let mut area = RECT::default();
    let found = unsafe {
        SendMessageW(
            list,
            LB_GETITEMRECT,
            Some(WPARAM(index)),
            Some(LPARAM(&mut area as *mut RECT as isize)),
        )
    };
    (found.0 != LB_ERR as isize).then_some(area)
}

/// The row under the pointer and its area. The list names its nearest row for a point below
/// the last one, so the area is checked too.
pub(super) fn row_at(list: HWND, point: POINT) -> Option<(usize, RECT)> {
    let packed = ((point.y as u16 as isize) << 16) | (point.x as u16 as isize);
    let hit = unsafe { SendMessageW(list, LB_ITEMFROMPOINT, None, Some(LPARAM(packed))) }.0;
    // The high word is set for a point outside the list.
    if (hit >> 16) & 0xffff != 0 {
        return None;
    }
    let index = (hit & 0xffff) as usize;
    let area = item_area(list, index)?;
    contains(&area, point).then_some((index, area))
}

/// Selects a row as a click on it would. LB_SETCURSEL tells nobody, and the footer follows
/// the selection.
pub(super) fn select(list: HWND, index: usize) {
    unsafe {
        if SendMessageW(list, LB_GETCURSEL, None, None).0 == index as isize {
            return;
        }
        SendMessageW(list, LB_SETCURSEL, Some(WPARAM(index)), None);
    }
    notify(list, LBN_SELCHANGE);
}

/// Tells Core's window, as the list's own notifications do.
pub(super) fn notify(list: HWND, notification: u32) {
    let Ok(parent) = (unsafe { GetParent(list) }) else {
        return;
    };
    unsafe {
        SendMessageW(
            parent,
            WM_COMMAND,
            Some(WPARAM(((notification as usize) << 16) | RESULTS_ID)),
            Some(LPARAM(list.0 as isize)),
        );
    }
}
