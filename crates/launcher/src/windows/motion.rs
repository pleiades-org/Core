use std::time::{Duration, Instant};
use windows::Win32::{Foundation::*, Graphics::Gdi::UpdateWindow, UI::WindowsAndMessaging::*};

pub const TRANSITION_TIMER: usize = 40;
const FRAME_INTERVAL_MS: u32 = 15;
const SHOW_DURATION_MS: u64 = 130;
const HIDE_DURATION_MS: u64 = 100;

#[derive(Clone, Copy)]
pub enum MotionPreference {
    Enabled,
    Reduced,
    System,
}

/// A timer exists only during a transition. Reversals continue from the current opacity.
pub struct VisibilityTransition {
    alpha: u8,
    start_alpha: u8,
    target_alpha: u8,
    started: Instant,
    duration: Duration,
    running: bool,
    preference: MotionPreference,
}

impl Default for VisibilityTransition {
    fn default() -> Self {
        Self {
            alpha: 0,
            start_alpha: 0,
            target_alpha: 0,
            started: Instant::now(),
            duration: Duration::ZERO,
            running: false,
            preference: MotionPreference::Enabled,
        }
    }
}

impl VisibilityTransition {
    pub fn new(preference: MotionPreference) -> Self {
        Self {
            preference,
            ..Self::default()
        }
    }
    pub fn set_visible(&mut self, window: HWND, visible: bool) {
        self.stop_timer(window);
        self.start_alpha = self.alpha;
        self.target_alpha = if visible { 255 } else { 0 };
        if self.alpha == self.target_alpha || !self.animations_enabled() {
            self.finish(window);
            return;
        }
        self.started = Instant::now();
        self.duration = Duration::from_millis(if visible {
            SHOW_DURATION_MS
        } else {
            HIDE_DURATION_MS
        });
        if visible {
            let first_frame = Duration::from_millis(u64::from(FRAME_INTERVAL_MS));
            self.set_opacity(window, self.opacity_at(first_frame));
            unsafe {
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                let _ = UpdateWindow(window);
            }
        } else {
            self.set_opacity(window, self.alpha);
        }
        self.running =
            unsafe { SetTimer(Some(window), TRANSITION_TIMER, FRAME_INTERVAL_MS, None) } != 0;
        if !self.running {
            self.finish(window);
        }
    }

    fn animations_enabled(&self) -> bool {
        match self.preference {
            MotionPreference::Enabled => true,
            MotionPreference::Reduced => false,
            MotionPreference::System => animations_enabled(),
        }
    }

    pub fn tick(&mut self, window: HWND) {
        if !self.running {
            return;
        }
        let progress = (self.started.elapsed().as_secs_f32() / self.duration.as_secs_f32()).min(1.);
        if progress >= 1. {
            self.finish(window);
            return;
        }
        let elapsed = if self.target_alpha > 0 {
            self.started
                .elapsed()
                .max(Duration::from_millis(u64::from(FRAME_INTERVAL_MS)))
        } else {
            self.started.elapsed()
        };
        self.set_opacity(window, self.opacity_at(elapsed));
    }

    fn opacity_at(&self, elapsed: Duration) -> u8 {
        let progress = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.);
        let eased = 1. - (1. - progress).powi(3);
        (self.start_alpha as f32 + (self.target_alpha as f32 - self.start_alpha as f32) * eased)
            .round() as u8
    }

    fn finish(&mut self, window: HWND) {
        self.stop_timer(window);
        self.set_opacity(window, self.target_alpha);
        unsafe {
            let _ = ShowWindow(
                window,
                if self.target_alpha == 0 {
                    SW_HIDE
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
        }
    }

    fn stop_timer(&mut self, window: HWND) {
        if self.running {
            if let Err(error) = unsafe { KillTimer(Some(window), TRANSITION_TIMER) } {
                eprintln!("Could not stop Core's transition timer: {error}");
            }
            self.running = false;
        }
    }

    fn set_opacity(&mut self, window: HWND, alpha: u8) {
        self.alpha = alpha;
        if let Err(error) =
            unsafe { SetLayeredWindowAttributes(window, COLORREF(0), alpha, LWA_ALPHA) }
        {
            eprintln!("Could not set Core's window opacity: {error}");
        }
    }
}

fn animations_enabled() -> bool {
    let mut enabled = windows::core::BOOL(0);
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut windows::core::BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && enabled.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_show_frame_is_visible_and_the_final_frame_is_opaque() {
        let transition = VisibilityTransition {
            target_alpha: 255,
            duration: Duration::from_millis(SHOW_DURATION_MS),
            ..Default::default()
        };
        assert_eq!(transition.opacity_at(Duration::ZERO), 0);
        assert!(transition.opacity_at(Duration::from_millis(u64::from(FRAME_INTERVAL_MS))) > 0);
        assert_eq!(transition.opacity_at(transition.duration), 255);
        assert!(!VisibilityTransition::new(MotionPreference::Reduced).animations_enabled());
    }
}
