use super::*;

impl SetupWindow {
    pub(super) unsafe fn create_controls(&self, window: HWND) -> windows::core::Result<()> {
        let instance = GetModuleHandleW(None)?.into();
        let description = match self.options.operation {
            Operation::Install => "Install Core",
            Operation::Uninstall => "Uninstall Core",
        };
        let explanation = match self.options.operation {
            Operation::Install => "Includes a Start Menu shortcut and a Windows uninstall entry.",
            Operation::Uninstall => "Your settings, quicklinks and command history will be kept.",
        };
        for (identifier, label) in [
            (TITLE_ID, "Core"),
            (DESCRIPTION_ID, description),
            (VERSION_ID, env!("CARGO_PKG_VERSION")),
            (INSTALL_TYPE_ID, "For your Windows account"),
            (EXPLANATION_ID, explanation),
            (STATUS_ID, ""),
        ] {
            self.add_control(
                window,
                instance,
                identifier,
                "STATIC",
                label,
                WINDOW_STYLE(0),
            )?;
        }
        self.add_control(
            window,
            instance,
            PATH_ID,
            "STATIC",
            &self.paths.directory.to_string_lossy(),
            WINDOW_STYLE(0),
        )?;
        for (identifier, label) in [
            (
                PRIMARY_ID,
                if self.options.operation == Operation::Install {
                    "Install"
                } else {
                    "Uninstall"
                },
            ),
            (CANCEL_ID, "Cancel"),
            (DESKTOP_ID, "Desktop shortcut"),
        ] {
            self.add_control(
                window,
                instance,
                identifier,
                "BUTTON",
                label,
                WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            )?;
            button_hover::install(self.control(identifier))?;
            printing::install(self.control(identifier))?;
        }
        if self.options.operation == Operation::Uninstall {
            let _ = ShowWindow(self.control(DESKTOP_ID), SW_HIDE);
        }
        self.fringe
            .set(CornerFringe::create(window, instance)?)
            .map_err(|_| windows::core::Error::new(E_FAIL, "Setup corners already exist"))?;
        self.apply_fonts();
        self.layout(window)?;
        self.update(window);
        Ok(())
    }

    pub(super) unsafe fn add_control(
        &self,
        parent: HWND,
        instance: HINSTANCE,
        identifier: usize,
        class: &str,
        label: &str,
        style: WINDOW_STYLE,
    ) -> windows::core::Result<()> {
        let control = child(
            parent,
            instance,
            PCWSTR(wide(class).as_ptr()),
            PCWSTR(wide(label).as_ptr()),
            style,
            identifier,
        )?;
        self.controls.borrow_mut().insert(identifier, control);
        Ok(())
    }

    pub(super) fn apply_fonts(&self) {
        let fonts = self.look.get().fonts;
        let controls = self.controls.borrow().clone();
        for (identifier, control) in controls {
            let font = match identifier {
                TITLE_ID => fonts.answer,
                DESCRIPTION_ID => fonts.title,
                _ => fonts.detail,
            };
            unsafe {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                )
            };
        }
    }

    pub(super) unsafe fn layout(&self, window: HWND) -> windows::core::Result<()> {
        let mut client = RECT::default();
        GetClientRect(window, &mut client)?;
        let dpi = self.look.get().dpi;
        let inset = scale(INSET, dpi);
        let right = client.right - inset;
        let width = right - inset;
        for (identifier, left, top, control_width, height) in [
            (TITLE_ID, inset, scale(22, dpi), width / 2, scale(48, dpi)),
            (
                VERSION_ID,
                right - scale(90, dpi),
                scale(33, dpi),
                scale(90, dpi),
                scale(24, dpi),
            ),
            (DESCRIPTION_ID, inset, scale(82, dpi), width, scale(28, dpi)),
            (
                INSTALL_TYPE_ID,
                inset + scale(16, dpi),
                scale(139, dpi),
                width - scale(32, dpi),
                scale(24, dpi),
            ),
            (
                PATH_ID,
                inset + scale(16, dpi),
                scale(174, dpi),
                width - scale(32, dpi),
                scale(38, dpi),
            ),
            (
                DESKTOP_ID,
                inset,
                scale(238, dpi),
                width,
                scale(CONTROL_HEIGHT, dpi),
            ),
            (
                EXPLANATION_ID,
                inset,
                scale(298, dpi),
                width,
                scale(34, dpi),
            ),
            (STATUS_ID, inset, scale(338, dpi), width, scale(34, dpi)),
            (
                CANCEL_ID,
                right - scale(292, dpi),
                client.bottom - scale(64, dpi),
                scale(140, dpi),
                scale(CONTROL_HEIGHT, dpi),
            ),
            (
                PRIMARY_ID,
                right - scale(140, dpi),
                client.bottom - scale(64, dpi),
                scale(140, dpi),
                scale(CONTROL_HEIGHT, dpi),
            ),
        ] {
            SetWindowPos(
                self.control(identifier),
                None,
                left,
                top,
                control_width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        self.place_corners(window)?;
        let _ = InvalidateRect(Some(window), None, true);
        Ok(())
    }

    pub(super) unsafe fn place_corners(&self, window: HWND) -> windows::core::Result<()> {
        let Some(fringe) = self.fringe.get() else {
            return Ok(());
        };
        let mut bounds = RECT::default();
        GetWindowRect(window, &mut bounds)?;
        let edges = RECT {
            left: bounds.left - 1,
            top: bounds.top - 1,
            right: bounds.right + 1,
            bottom: bounds.bottom + 1,
        };
        let shape = ClipShape::new(
            bounds,
            edges,
            scale(
                self.preferences.corner_radius.logical(),
                self.look.get().dpi,
            ),
        );
        if self.clip_shape.replace(Some(shape)) != Some(shape) {
            window_placement::clip_to_edges(window, shape)?;
        }
        fringe.place(bounds, shape, self.look.get().palette.background)?;
        fringe.set_opacity(255);
        Ok(())
    }
}
