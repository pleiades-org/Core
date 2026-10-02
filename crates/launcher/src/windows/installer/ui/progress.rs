use super::*;

impl SetupWindow {
    pub(super) fn update(&self, window: HWND) {
        let status = self.status.borrow().clone();
        let complete = matches!(status, Status::Complete);
        let (primary, cancel, message, enabled) = match status {
            Status::Ready => (
                if self.options.operation == Operation::Install {
                    "Install"
                } else {
                    "Uninstall"
                },
                "Cancel",
                if self.options.preview {
                    "Preview only — no files will be changed.".to_owned()
                } else {
                    String::new()
                },
                true,
            ),
            Status::Working => ("Please wait…", "Cancel", "Working…".to_owned(), false),
            Status::Complete => (
                if self.options.operation == Operation::Install {
                    "Open Core"
                } else {
                    "Done"
                },
                "Done",
                if self.options.operation == Operation::Install {
                    "Core is installed.".to_owned()
                } else {
                    "Core has been uninstalled.".to_owned()
                },
                true,
            ),
            Status::Failed(error) => ("Retry", "Cancel", error, true),
        };
        unsafe {
            for (identifier, text) in [
                (PRIMARY_ID, primary),
                (CANCEL_ID, cancel),
                (STATUS_ID, message.as_str()),
            ] {
                if let Err(error) =
                    SetWindowTextW(self.control(identifier), PCWSTR(wide(text).as_ptr()))
                {
                    eprintln!("Could not update setup text: {error}");
                }
            }
            let _ = EnableWindow(self.control(PRIMARY_ID), enabled);
            let _ = EnableWindow(self.control(CANCEL_ID), enabled);
            let _ = EnableWindow(self.control(DESKTOP_ID), enabled && !complete);
            let _ = InvalidateRect(Some(window), None, true);
        }
    }

    pub(super) fn accept(&self, window: HWND) {
        let status = self.status.borrow().clone();
        match status {
            Status::Working => {}
            Status::Complete => {
                if self.options.operation == Operation::Install && !self.paths.isolated {
                    if let Err(error) = Command::new(&self.paths.executable)
                        .creation_flags(CREATE_NO_WINDOW.0)
                        .spawn()
                    {
                        *self.status.borrow_mut() = Status::Failed(format!(
                            "Core is installed, but could not open: {error}"
                        ));
                        self.update(window);
                        return;
                    }
                }
                unsafe {
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            Status::Ready | Status::Failed(_) => self.start(window),
        }
    }

    pub(super) fn start(&self, window: HWND) {
        if self.options.preview {
            return;
        }
        if unsafe { SetTimer(Some(window), WORK_TIMER, WORK_POLL_MILLISECONDS, None) } == 0 {
            *self.status.borrow_mut() =
                Status::Failed("Could not monitor setup progress. Choose Retry.".into());
            self.update(window);
            return;
        }
        let paths = self.paths.clone();
        let desktop = self.desktop.get();
        let operation = self.options.operation;
        let result = self.result.clone();
        let worker = std::thread::Builder::new()
            .name("core-setup".into())
            .spawn(move || {
                let outcome = match operation {
                    Operation::Install => std::env::current_exe()
                        .map_err(|error| error.to_string())
                        .and_then(|source| install::install(&paths, &source, desktop)),
                    Operation::Uninstall => install::uninstall(&paths),
                };
                *result.lock().expect("setup result lock") = Some(outcome);
            });
        match worker {
            Ok(worker) => {
                *self.worker.borrow_mut() = Some(worker);
                *self.status.borrow_mut() = Status::Working;
            }
            Err(error) => {
                unsafe {
                    let _ = KillTimer(Some(window), WORK_TIMER);
                }
                *self.status.borrow_mut() =
                    Status::Failed(format!("Could not start setup: {error}"))
            }
        }
        self.update(window);
    }

    pub(super) fn finish_worker(&self, window: HWND) {
        if !self
            .worker
            .borrow()
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            return;
        }
        let worker = self
            .worker
            .borrow_mut()
            .take()
            .expect("finished setup worker");
        let outcome = if worker.join().is_err() {
            Err("Setup stopped unexpectedly. Choose Retry.".into())
        } else {
            self.result
                .lock()
                .expect("setup result lock")
                .take()
                .unwrap_or_else(|| Err("Setup returned no result. Choose Retry.".into()))
        };
        *self.status.borrow_mut() = match outcome {
            Ok(()) => Status::Complete,
            Err(error) => {
                eprintln!("Core setup operation failed: {error}");
                Status::Failed(error)
            }
        };
        unsafe {
            let _ = KillTimer(Some(window), WORK_TIMER);
        }
        self.update(window);
    }
}
