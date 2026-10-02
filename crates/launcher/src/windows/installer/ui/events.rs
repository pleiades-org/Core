use super::*;

pub(super) unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    word: WPARAM,
    long: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(long.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const SetupWindow;
    let Some(context) = pointer.as_ref() else {
        return DefWindowProcW(window, message, word, long);
    };
    match message {
        WM_CREATE => match context.create_controls(window) {
            Ok(()) => LRESULT(0),
            Err(error) => {
                eprintln!("Could not create setup controls: {error}");
                LRESULT(-1)
            }
        },
        WM_PAINT => {
            context.paint(window);
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            context.paint_to(window, HDC(word.0 as *mut _));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DRAWITEM => {
            context.draw_button(&*(long.0 as *const DRAWITEMSTRUCT));
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC => {
            let look = context.look.get();
            let identifier = GetDlgCtrlID(HWND(long.0 as *mut _)) as usize;
            let panel = matches!(identifier, INSTALL_TYPE_ID | PATH_ID);
            let color = if panel {
                look.palette.selected
            } else {
                look.palette.background
            };
            SetTextColor(
                HDC(word.0 as *mut _),
                if identifier == TITLE_ID {
                    look.palette.text
                } else {
                    look.palette.secondary
                },
            );
            SetBkColor(HDC(word.0 as *mut _), color);
            LRESULT(if panel {
                context.selected.get().0 as isize
            } else {
                context.background.get().0 as isize
            })
        }
        WM_COMMAND if (word.0 >> 16) as u32 == BN_CLICKED => {
            match word.0 & 0xffff {
                PRIMARY_ID => context.accept(window),
                CANCEL_ID => {
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                DESKTOP_ID
                    if !matches!(*context.status.borrow(), Status::Working | Status::Complete) =>
                {
                    context.desktop.set(!context.desktop.get());
                    let _ = InvalidateRect(Some(context.control(DESKTOP_ID)), None, true);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_NEXTDLGCTL => {
            let control = if long.0 != 0 {
                Ok(HWND(word.0 as *mut _))
            } else {
                GetNextDlgTabItem(window, Some(GetFocus()), word.0 != 0)
            };
            match control {
                Ok(control)
                    if context
                        .controls
                        .borrow()
                        .values()
                        .any(|candidate| *candidate == control) =>
                {
                    let _ = SetFocus(Some(control));
                }
                Ok(_) => {}
                Err(error) => eprintln!("Could not move setup keyboard focus: {error}"),
            }
            LRESULT(0)
        }
        WM_TIMER if word.0 == WORK_TIMER => {
            context.finish_worker(window);
            LRESULT(0)
        }
        WM_CLOSE => {
            if !matches!(*context.status.borrow(), Status::Working) {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DefWindowProcW(window, message, word, long)
        }
        WM_WINDOWPOSCHANGED => {
            if let Err(error) = context.place_corners(window) {
                eprintln!("Could not place setup corners: {error}");
            }
            DefWindowProcW(window, message, word, long)
        }
        WM_DPICHANGED => {
            let dpi = word.0 as u32 & 0xffff;
            match Fonts::create(dpi) {
                Ok(fonts) => {
                    let old = context.look.replace(Look {
                        dpi,
                        fonts,
                        palette: context.look.get().palette,
                    });
                    context.apply_fonts();
                    old.fonts.delete();
                }
                Err(error) => eprintln!("Could not resize setup fonts: {error}"),
            }
            let bounds = &*(long.0 as *const RECT);
            let _ = SetWindowPos(
                window,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if let Err(error) = context.layout(window) {
                eprintln!("Could not resize setup: {error}");
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, word, long),
    }
}
