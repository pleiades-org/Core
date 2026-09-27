//! Control colors, owner/custom drawing and the launcher background.
use super::*;

impl View {
    pub fn color_control(&self, context: HDC, control: HWND) -> LRESULT {
        let palette = self.palette.get();
        let identifier = unsafe { GetDlgCtrlID(control) } as usize;
        let color_input = matches!(identifier, COLOR_ID | SHORTCUT_ID)
            || crate::windows::settings::quicklink_table::is_edit(identifier);
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
            painting::result_row(
                item.hDC,
                item.rcItem,
                row,
                item.itemState.0 & ODS_SELECTED.0 != 0,
                self.fonts.get(),
                self.dpi.get(),
                palette,
            );
        } else {
            painting::fill(item.hDC, &item.rcItem, palette.background);
        }
    }

    pub fn paint(&self, context: HDC) {
        let dpi = self.dpi.get();
        let width = self.width.get();
        let height = self.height.get();
        let area = painting::rectangle(0, 0, width, height);
        let palette = self.palette.get();
        painting::fill(context, &area, palette.background);
        if self.settings_open.get() {
            self.settings_page
                .get()
                .expect("open settings page")
                .paint(context, palette);
            return;
        }
        painting::search_icon(
            context,
            scale(27, dpi),
            scale(32, dpi),
            dpi,
            palette.secondary,
        );
        self.paint_labels(context);
        if self.grid() {
            self.paint_grid(context);
        }
    }

    fn paint_labels(&self, context: HDC) {
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
            None => "SEARCH",
        };
        let area = |left, top, right, bottom| {
            painting::rectangle(
                scale(left, dpi),
                scale(top, dpi),
                scale(right, dpi),
                scale(bottom, dpi),
            )
        };
        painting::text(
            context,
            section,
            area(24, 83, 400, 100),
            fonts.detail,
            self.palette.get().secondary,
        );
        if rows.is_empty() && !terminal {
            painting::text(
                context,
                "No results yet",
                area(
                    25,
                    theme::RESULTS_TOP + 10,
                    theme::WIDTH - 25,
                    theme::RESULTS_TOP + 38,
                ),
                fonts.title,
                self.palette.get().secondary,
            );
        }
    }
}
