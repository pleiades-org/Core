use super::*;
use windows::Win32::System::SystemInformation::GetLocalTime;

pub const CLOCK_ID: usize = 110;
pub const CLOCK_TIMER: usize = 42;
const MILLISECONDS_PER_SECOND: u32 = 1_000;
const SECONDS_PER_MINUTE: u32 = 60;

impl View {
    /// The clock runs only while the search footer is visible.
    pub fn set_clock_active(&self, active: bool) {
        let active = active && !self.settings_open();
        if active {
            self.clock_active.set(true);
            self.refresh_clock();
        } else if self.clock_active.replace(false) {
            if let Err(error) = unsafe { KillTimer(Some(self.parent), CLOCK_TIMER) } {
                eprintln!("Could not stop the footer clock: {error}");
            }
        }
    }

    pub fn refresh_clock(&self) {
        if !self.clock_active.get() {
            return;
        }
        let local = unsafe { GetLocalTime() };
        let text = wide(&format!("{:02}:{:02}", local.wHour, local.wMinute));
        if let Err(error) = unsafe { SetWindowTextW(self.clock, PCWSTR(text.as_ptr())) } {
            eprintln!("Could not update the footer clock: {error}");
        }
        // Align with the next minute rather than polling or repainting every second.
        let delay = next_minute_delay(local.wSecond, local.wMilliseconds);
        if unsafe { SetTimer(Some(self.parent), CLOCK_TIMER, delay, None) } == 0 {
            eprintln!(
                "Could not schedule the footer clock: {}",
                windows::core::Error::from_win32()
            );
        }
    }
}

fn next_minute_delay(seconds: u16, milliseconds: u16) -> u32 {
    ((SECONDS_PER_MINUTE - u32::from(seconds)) * MILLISECONDS_PER_SECOND)
        .saturating_sub(u32::from(milliseconds))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_schedules_the_next_minute_including_rollover() {
        assert_eq!(next_minute_delay(0, 0), 60_000);
        assert_eq!(next_minute_delay(23, 450), 36_550);
        assert_eq!(next_minute_delay(59, 999), 1);
    }
}
