#[derive(Default)]
pub(super) struct WindowsKeyState {
    held: u8,
    replayed: u8,
    used: bool,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Decision {
    pub block: bool,
    pub activate: bool,
    pub replay_windows: u8,
}

impl WindowsKeyState {
    /// Both Windows keys share one gesture. Other key presses turn the gesture into a shortcut.
    pub(super) fn event(&mut self, key: u16, down: bool, modified: bool) -> Decision {
        let bit = match key {
            0x5B => 1,
            0x5C => 2,
            _ => 0,
        };
        if bit == 0 {
            if self.held == 0 {
                return Decision::default();
            }
            self.used = true;
            let pending = if down { self.held & !self.replayed } else { 0 };
            self.replayed |= pending;
            return Decision {
                block: pending != 0,
                replay_windows: pending,
                activate: false,
            };
        }
        if down {
            if self.held & bit != 0 {
                return Decision {
                    block: self.replayed & bit == 0,
                    ..Default::default()
                };
            }
            if self.held == 0 {
                self.used = modified;
            }
            self.held |= bit;
            if modified || self.replayed != 0 {
                self.used = true;
                self.replayed |= bit;
                return Decision::default();
            }
            return Decision {
                block: true,
                ..Default::default()
            };
        }
        if self.held & bit == 0 {
            return Decision::default();
        }
        self.held &= !bit;
        let block = self.replayed & bit == 0;
        self.replayed &= !bit;
        Decision {
            block,
            activate: self.held == 0 && !self.used,
            replay_windows: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taps_suppress_both_edges_and_activate_once_after_release() {
        let mut state = WindowsKeyState::default();
        assert!(state.event(0x5B, true, false).block);
        assert!(state.event(0x5B, true, false).block);
        assert!(state.event(0x5C, true, false).block);
        assert!(!state.event(0x5B, false, false).activate);
        assert!(state.event(0x5C, false, false).activate);
        assert!(!state.event(0x5C, false, false).activate);
    }
    #[test]
    fn a_reset_recovers_from_a_release_lost_on_the_secure_desktop() {
        let mut state = WindowsKeyState::default();
        state.event(0x5B, true, false);
        state.event(b'L' as u16, true, false);
        // Win+L locks the session; neither key-up reaches the hook.
        let mut stuck = std::mem::take(&mut state);
        assert!(!stuck.event(0x5B, true, false).block);
        state.event(0x5B, true, false);
        assert!(state.event(0x5B, false, false).activate);
    }
    #[test]
    fn combinations_preserve_key_order_and_do_not_activate() {
        let mut state = WindowsKeyState::default();
        state.event(0x5B, true, false);
        assert_eq!(state.event(b'E' as u16, true, false).replay_windows, 1);
        assert!(!state.event(b'E' as u16, false, false).block);
        assert_eq!(state.event(0x5B, false, false), Decision::default());
        assert_eq!(state.event(0x5B, true, true), Decision::default());
        assert_eq!(state.event(0x5B, false, true), Decision::default());
    }
}
