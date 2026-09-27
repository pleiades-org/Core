use core_engine::search::PowerAction;
use windows::Win32::{
    Foundation::*,
    Security::*,
    System::{Power::SetSuspendState, Shutdown::*, Threading::*},
};

/// Called only after a separately selected confirmation. Never force applications to close.
pub fn execute(action: PowerAction) -> Result<(), String> {
    let _privilege = ShutdownPrivilege::enable().map_err(|error| {
        format!(
            "Could not obtain permission to {}: {error}",
            action.label().to_lowercase()
        )
    })?;
    let result = unsafe {
        match action {
            PowerAction::ShutDown => ExitWindowsEx(EWX_POWEROFF, SHUTDOWN_REASON(0)),
            PowerAction::Restart => ExitWindowsEx(EWX_REBOOT, SHUTDOWN_REASON(0)),
            PowerAction::Sleep => {
                if SetSuspendState(false, false, false) {
                    Ok(())
                } else {
                    Err(windows::core::Error::from_win32())
                }
            }
        }
    };
    result.map_err(|error| {
        format!(
            "Windows could not {}: {error}",
            action.label().to_lowercase()
        )
    })
}

struct ShutdownPrivilege {
    token: HANDLE,
    previous: TOKEN_PRIVILEGES,
}
impl ShutdownPrivilege {
    fn enable() -> windows::core::Result<Self> {
        let mut token = HANDLE::default();
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )?;
        }
        let mut privilege = Self {
            token,
            previous: TOKEN_PRIVILEGES::default(),
        };
        let mut identifier = LUID::default();
        unsafe {
            LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut identifier)?;
            let requested = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: identifier,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let mut length = 0;
            SetLastError(ERROR_SUCCESS);
            AdjustTokenPrivileges(
                token,
                false,
                Some(&requested),
                std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                Some(&mut privilege.previous),
                Some(&mut length),
            )?;
            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err(windows::core::Error::from_win32());
            }
        }
        Ok(privilege)
    }
}
impl Drop for ShutdownPrivilege {
    fn drop(&mut self) {
        unsafe {
            if self.previous.PrivilegeCount != 0 {
                if let Err(error) =
                    AdjustTokenPrivileges(self.token, false, Some(&self.previous), 0, None, None)
                {
                    eprintln!("Could not restore power privileges: {error}");
                }
            }
            let _ = CloseHandle(self.token);
        }
    }
}
