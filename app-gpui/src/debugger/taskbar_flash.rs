//! IDEA-style taskbar flash while a breakpoint holds this window in the background: the shell
//! flashes the native window (it never flashes a focused one), this side decides when to ask (on a
//! breakpoint stop while the window is unfocused) and stops the flash when the window is focused
//! again or the paused session resumes or ends.

use crate::debugger::effects::BackendCall;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskbarFlash {
    /// This window started a flash that nothing has stopped yet.
    flashing: bool,
}

impl TaskbarFlash {
    pub fn is_flashing(&self) -> bool {
        self.flashing
    }

    /// Stops the flash this window started (nothing to do when none is running). Also what the
    /// window does when it gets the focus. `desktop`: a shell is there to ask.
    pub fn stop(&mut self, desktop: bool) -> Option<BackendCall> {
        if !std::mem::take(&mut self.flashing) || !desktop {
            return None;
        }
        Some(BackendCall::WindowFlashTaskbar(false))
    }

    /// Asks the shell to flash the taskbar for a breakpoint hit in a background window. While
    /// already flashing, further hits do not ask again; a focused window never flashes.
    pub fn flash_for_breakpoint(&mut self, desktop: bool, window_focused: bool) -> Option<BackendCall> {
        if !desktop || self.flashing || window_focused {
            return None;
        }
        self.flashing = true;
        Some(BackendCall::WindowFlashTaskbar(true))
    }

    /// The shell could not flash: nothing is flashing, so the next hit asks again.
    pub fn flash_failed(&mut self) {
        self.flashing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: one flash per background stay, stopped once, and nothing without a shell.
    #[test]
    fn flashes_once_until_stopped_and_never_without_a_shell() {
        let mut flash = TaskbarFlash::default();
        assert!(flash.flash_for_breakpoint(false, false).is_none());
        assert!(flash.flash_for_breakpoint(true, true).is_none());
        assert!(matches!(flash.flash_for_breakpoint(true, false), Some(BackendCall::WindowFlashTaskbar(true))));
        assert!(flash.flash_for_breakpoint(true, false).is_none());
        assert!(matches!(flash.stop(true), Some(BackendCall::WindowFlashTaskbar(false))));
        assert!(flash.stop(true).is_none());
        assert!(flash.flash_for_breakpoint(true, false).is_some());
        flash.flash_failed();
        assert!(!flash.is_flashing() && flash.stop(true).is_none());
    }
}
