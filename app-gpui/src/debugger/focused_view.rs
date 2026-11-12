//! The debugger view (Frames or Variables) that has the keyboard focus, so that view-local actions
//! bound to global keys (F2 Set Value, F4 Jump to Source) reach it.

/// Context key that is true while the Frames view has the focus.
pub const FRAMES_FOCUS: &str = "debugFramesFocus";
/// Context key that is true while the Variables view has the focus.
pub const VARIABLES_FOCUS: &str = "debugVariablesFocus";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugView {
    Frames,
    Variables,
}

/// What the focused view can do for the global actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugViewCommands {
    pub view: DebugView,
    pub set_value: bool,
    pub jump_to_source: bool,
}

/// The two focus context keys and their values, in the order they are published.
pub type FocusContext = [(&'static str, bool); 2];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedView {
    current: Option<DebugViewCommands>,
}

impl FocusedView {
    /// A view took the focus: the context keys to publish.
    pub fn focus(&mut self, commands: DebugViewCommands) -> FocusContext {
        self.current = Some(commands);
        [(FRAMES_FOCUS, commands.view == DebugView::Frames), (VARIABLES_FOCUS, commands.view == DebugView::Variables)]
    }

    /// `view` lost the focus: the context keys to publish, unless another view has it by now.
    pub fn blur(&mut self, view: DebugView) -> Option<FocusContext> {
        if self.current.map(|commands| commands.view) != Some(view) {
            return None;
        }
        self.current = None;
        Some([(FRAMES_FOCUS, false), (VARIABLES_FOCUS, false)])
    }

    pub fn current(&self) -> Option<DebugViewCommands> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: focusing a view publishes its key, and a stale blur changes nothing.
    #[test]
    fn publishes_the_focus_of_the_view_that_has_it() {
        let mut focused = FocusedView::default();
        let variables = DebugViewCommands { view: DebugView::Variables, set_value: true, jump_to_source: true };
        assert_eq!(focused.focus(variables), [(FRAMES_FOCUS, false), (VARIABLES_FOCUS, true)]);
        assert_eq!(focused.blur(DebugView::Frames), None);
        assert_eq!(focused.current(), Some(variables));
        assert_eq!(focused.blur(DebugView::Variables), Some([(FRAMES_FOCUS, false), (VARIABLES_FOCUS, false)]));
        assert_eq!(focused.current(), None);
    }
}
