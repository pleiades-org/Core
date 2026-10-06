//! Control colors, owner/custom drawing and the launcher background.
use super::*;

impl View {
    pub fn color_control(&self, context: HDC, control: HWND) -> LRESULT {
        let palette = self.palette.get();
        let identifier = unsafe { GetDlgCtrlID(control) } as usize;
        let color_input = crate::windows::settings::page::is_text_field(identifier)
            || crate::windows::settings::page::is_table_edit(identifier);
        unsafe {
            SetBkColor(
                context,
                if color_input {
                    palette.selected
                } else {
                    palette.background
                },
            );
            SetTextColor(
                context,
                if control == self.footer || control == self.clock || identifier == STATUS_ID {
                    palette.secondary
                } else {
                    palette.text
                },
            );
        }
        LRESULT(
            if color_input {
                self.selected_brush.get()
            } else {
                self.background.get()
            }
            .0 as isize,
        )
    }

    /// Only settings sliders use custom draw; everything else keeps default painting.
    pub fn custom_draw(&self, draw: &NMCUSTOMDRAW) -> Option<LRESULT> {
        if draw.hdr.code != NM_CUSTOMDRAW || !self.settings_open.get() {
            return None;
        }
        self.settings_page
            .get()?
            .custom_draw(draw, self.palette.get())
    }

    pub fn draw_item(&self, item: &DRAWITEMSTRUCT) {
        let palette = self.palette.get();
        if let Some(page) = self.settings_page.get() {
            if page.draw_button(item, palette) {
                return;
            }
        }
        if self.draw_media_item(item) {
            return;
        }
        if self
            .power_menu
            .get()
            .is_some_and(|menu| menu.draw(item, self.dpi.get(), self.fonts.get(), palette))
        {
            return;
        }
        if matches!(item.CtlID as usize, POWER_ID | SETTINGS_ID) {
            crate::windows::power_menu::icon_button(
                item,
                if item.CtlID as usize == SETTINGS_ID {
                    "\u{e713}"
                } else {
                    "\u{e7e8}"
                },
                self.dpi.get(),
                self.fonts.get(),
                palette,
            );
            return;
        }
        if item.CtlID as usize != RESULTS_ID {
            return;
        }
        if let Some(row) = self.rows.borrow().get(item.itemID as usize) {
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
            if let Some(level) = row.volume {
                self.draw_mixer_row(item.hDC, item.rcItem, row, level, selected);
                return;
            }
            if row.options && selected {
                self.draw_collection_row(item.hDC, item.rcItem, row);
                return;
            }
            painting::result_row(
                item.hDC,
                item.rcItem,
                row,
                selected,
                self.fonts.get(),
                self.dpi.get(),
                palette,
            );
        } else {
            painting::fill(item.hDC, &item.rcItem, palette.background);
        }
    }

    /// Draws what intersects `update`, the area Windows asked to repaint; the rest is skipped.
    pub fn paint(&self, context: HDC, update: &RECT) {
        let dpi = self.dpi.get();
        let area = self.client_area();
        let palette = self.palette.get();
        painting::fill(context, &area, palette.background);
        if self.settings_open.get() {
            self.settings_page
                .get()
                .expect("open settings page")
                .paint(context, palette);
            return;
        }
        let bar = self.media_bar_height();
        let icon = painting::rectangle(
            scale(27, dpi),
            scale(bar + 32, dpi),
            scale(48, dpi),
            scale(bar + 53, dpi),
        );
        if painting::intersects(&icon, update) {
            painting::search_icon(
                context,
                scale(27, dpi),
                scale(bar + 32, dpi),
                dpi,
                palette.secondary,
            );
        }
        self.paint_labels(context, update);
        if self.grid() {
            self.paint_grid(context, update);
        }
    }

    /// The whole window, for WM_PRINTCLIENT captures.
    pub fn client_area(&self) -> RECT {
        painting::rectangle(0, 0, self.width.get(), self.height.get())
    }

    fn paint_labels(&self, context: HDC, update: &RECT) {
        let dpi = self.dpi.get();
        let fonts = self.fonts.get();
        let rows = self.rows.borrow();
        let terminal = self.terminal.get();
        let section = match rows.first().map(|row| row.kind) {
            _ if terminal => "TERMINAL",
            Some(ResultKind::Application) => "APPLICATIONS",
            Some(ResultKind::Quicklink) => "QUICKLINKS",
            Some(ResultKind::Calculator) => "CALCULATOR",
            Some(ResultKind::Time) => "TIME CONVERSION",
            Some(ResultKind::Date) => "DATE & TIME",
            Some(ResultKind::Conversion) => "CONVERSION",
            Some(ResultKind::Power) => "POWER",
            Some(ResultKind::Web) => "WEB SEARCH",
            Some(ResultKind::Command) => "COMMANDS",
            Some(ResultKind::System) => "SYSTEM",
            Some(ResultKind::Terminal) => "TERMINAL",
            Some(ResultKind::Recent) => "RECENT",
            Some(ResultKind::Media) => "MEDIA",
            Some(ResultKind::Volume) => "VOLUME",
            None => "SEARCH",
        };
        let bar = self.media_bar_height();
        let area = |left, top, right, bottom| {
            painting::rectangle(
                scale(left, dpi),
                scale(top, dpi),
                scale(right, dpi),
                scale(bottom, dpi),
            )
        };
        let label = area(24, bar + SECTION_LABEL_TOP, 400, bar + 100);
        if painting::intersects(&label, update) {
            painting::text(
                context,
                section,
                label,
                fonts.detail,
                self.palette.get().secondary,
            );
        }
        let empty = area(
            25,
            bar + theme::RESULTS_TOP + 10,
            theme::WIDTH - 25,
            bar + theme::RESULTS_TOP + 38,
        );
        if rows.is_empty() && !terminal && painting::intersects(&empty, update) {
            painting::text(
                context,
                "No results yet",
                empty,
                fonts.title,
                self.palette.get().secondary,
            );
        }
    }
}
