//! Recently used apps as a grid when nothing is typed, like Start's pinned apps. The hidden
//! result list still holds the apps and the selection, so Enter, accessibility names and the
//! launch path are the same as for rows; this module only draws and hit-tests the tiles.
use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};

impl View {
    /// The results are recently used apps, drawn as tiles.
    pub fn grid(&self) -> bool {
        !self.terminal.get()
            && !self.settings_open.get()
            && self
                .rows
                .borrow()
                .first()
                .is_some_and(|row| row.kind == ResultKind::Recent)
    }

    /// Rows of tiles for `count` apps.
    pub(super) fn grid_rows(count: usize) -> i32 {
        count.div_ceil(theme::GRID_COLUMNS) as i32
    }

    /// The rows shown in `area`: all of them unless a small or heavily scaled screen cannot fit
    /// them, since tiles do not scroll. At least one row is always shown.
    pub(super) fn fit_grid(&self, count: usize, area: RECT, dpi: u32) -> i32 {
        let logical_height = (area.bottom - area.top) * 96 / dpi.max(1) as i32;
        let room = logical_height
            - self.media_bar_height()
            - theme::RESULTS_TOP
            - theme::FOOTER_HEIGHT
            - theme::GRID_GAP;
        let rows = Self::grid_rows(count).min((room / theme::GRID_TILE_HEIGHT).max(1));
        let visible = (rows as usize * theme::GRID_COLUMNS).min(count);
        self.grid_visible.set(visible);
        if self.selected() >= visible {
            unsafe {
                SendMessageW(self.results, LB_SETCURSEL, Some(WPARAM(0)), None);
            }
        }
        rows
    }

    /// Where tile `index` is drawn, in client pixels.
    fn tile(&self, index: usize) -> RECT {
        let dpi = self.dpi.get();
        let left = scale(12, dpi);
        let width = (self.width.get() - scale(12, dpi) - left) / theme::GRID_COLUMNS as i32;
        let height = scale(theme::GRID_TILE_HEIGHT, dpi);
        let column = (index % theme::GRID_COLUMNS) as i32;
        let row = (index / theme::GRID_COLUMNS) as i32;
        let top = scale(self.media_bar_height() + theme::RESULTS_TOP, dpi) + row * height;
        painting::rectangle(
            left + column * width,
            top,
            left + (column + 1) * width,
            top + height,
        )
    }

    /// Draws the tiles that intersect `update`; a hover change repaints only its two tiles.
    pub(super) fn paint_grid(&self, context: HDC, update: &RECT) {
        let selected = self.selected();
        let hovered = self.grid_hover.get();
        let fonts = self.fonts.get();
        let palette = self.palette.get();
        let visible = self.grid_visible.get();
        for (index, row) in self.rows.borrow().iter().enumerate().take(visible) {
            let tile = self.tile(index);
            if !painting::intersects(&tile, update) {
                continue;
            }
            painting::app_tile(
                context,
                tile,
                row,
                index == selected || hovered == Some(index),
                fonts,
                self.dpi.get(),
                palette,
            );
        }
    }

    /// The tile under a client point.
    pub fn grid_hit(&self, x: i32, y: i32) -> Option<usize> {
        if !self.grid() {
            return None;
        }
        let point = POINT { x, y };
        (0..self.grid_visible.get())
            .find(|&index| unsafe { PtInRect(&self.tile(index), point) }.as_bool())
    }

    /// Highlights the tile under the pointer; the highlight clears when the pointer leaves.
    pub fn hover_grid(&self, x: i32, y: i32) {
        let hovered = self.grid_hit(x, y);
        let previous = self.grid_hover.replace(hovered);
        if previous == hovered {
            return;
        }
        if hovered.is_some() {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.parent,
                ..Default::default()
            };
            if let Err(error) = unsafe { TrackMouseEvent(&mut tracking) } {
                eprintln!("Could not track the pointer over the app grid: {error}");
            }
        }
        for index in [previous, hovered].into_iter().flatten() {
            self.invalidate_tile(index);
        }
    }

    pub fn leave_grid(&self) {
        if let Some(index) = self.grid_hover.take() {
            self.invalidate_tile(index);
        }
    }

    /// Selects a tile, as clicking it does before it opens.
    pub fn select_tile(&self, index: usize) {
        let previous = self.selected();
        unsafe {
            SendMessageW(self.results, LB_SETCURSEL, Some(WPARAM(index)), None);
        }
        self.invalidate_tile(previous);
        self.invalidate_tile(index);
    }

    /// Arrow keys: left and right step through the tiles, up and down move a row. Down from
    /// a full row onto a shorter one lands on the last tile.
    pub fn move_grid_selection(&self, columns: isize, rows: isize) {
        let count = self.grid_visible.get();
        let Some(next) = grid_step(self.selected(), count, columns, rows) else {
            return;
        };
        self.select_tile(next);
    }

    pub(super) fn invalidate_tile(&self, index: usize) {
        let area = self.tile(index);
        unsafe {
            let _ = InvalidateRect(Some(self.parent), Some(&area), false);
        }
    }
}

/// The tile an arrow key moves to, or `None` when there is nothing in that direction.
fn grid_step(selected: usize, count: usize, columns: isize, rows: isize) -> Option<usize> {
    let target = selected as isize + columns + rows * theme::GRID_COLUMNS as isize;
    if (0..count as isize).contains(&target) {
        return Some(target as usize);
    }
    let last = count.checked_sub(1)?;
    let last_row = last / theme::GRID_COLUMNS;
    (rows > 0 && selected / theme::GRID_COLUMNS < last_row).then_some(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_move_through_rows_of_six() {
        // 14 apps: rows of 6, 6 and 2.
        assert_eq!(grid_step(0, 14, 1, 0), Some(1));
        assert_eq!(grid_step(0, 14, -1, 0), None);
        assert_eq!(grid_step(5, 14, 1, 0), Some(6));
        assert_eq!(grid_step(2, 14, 0, 1), Some(8));
        assert_eq!(grid_step(8, 14, 0, -1), Some(2));
        assert_eq!(grid_step(2, 14, 0, -1), None);
        // Down onto the short last row lands on its last tile.
        assert_eq!(grid_step(10, 14, 0, 1), Some(13));
        assert_eq!(grid_step(13, 14, 0, 1), None);
        assert_eq!(grid_step(0, 0, 1, 0), None);
    }

    #[test]
    fn rows_round_up() {
        assert_eq!(View::grid_rows(1), 1);
        assert_eq!(View::grid_rows(6), 1);
        assert_eq!(View::grid_rows(7), 2);
        assert_eq!(View::grid_rows(18), 3);
    }
}
