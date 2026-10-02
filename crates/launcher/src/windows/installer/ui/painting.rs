use super::*;

impl SetupWindow {
    pub(super) unsafe fn paint(&self, window: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let context = BeginPaint(window, &mut paint);
        self.paint_to(window, context);
        let _ = EndPaint(window, &paint);
    }

    pub(super) unsafe fn paint_to(&self, window: HWND, context: HDC) {
        let look = self.look.get();
        let mut area = RECT::default();
        if GetClientRect(window, &mut area).is_ok() {
            core_painting::fill(context, &area, look.palette.background);
            let inset = scale(INSET, look.dpi);
            core_painting::rounded(
                context,
                &core_painting::rectangle(
                    inset,
                    scale(123, look.dpi),
                    area.right - inset,
                    scale(220, look.dpi),
                ),
                scale(9, look.dpi),
                look.palette.selected,
            );
        }
    }

    pub(super) unsafe fn draw_button(&self, item: &DRAWITEMSTRUCT) {
        let look = self.look.get();
        let highlighted = item.itemState.0 & (ODS_FOCUS.0 | ODS_SELECTED.0) != 0
            || button_hover::is_hovered(item.hwndItem);
        let label = control_text(item.hwndItem);
        if item.CtlID as usize == DESKTOP_ID {
            control_style::draw_toggle(
                item.hDC,
                item.rcItem,
                &label,
                self.desktop.get(),
                highlighted,
                look,
            );
        } else {
            control_style::draw_action(item.hDC, item.rcItem, &label, None, highlighted, look);
        }
    }
}
