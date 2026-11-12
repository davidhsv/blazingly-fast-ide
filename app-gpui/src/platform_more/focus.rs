//! Focus routing between tool windows, popups and the editor area: which workbench part holds
//! the focus, and the rule that closing the last popup, menu or dialog never leaves the focus
//! nowhere.

/// The workbench part that holds the keyboard focus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FocusPlace {
    /// Nothing in the window has it.
    #[default]
    Nowhere,
    /// The tool window of this id.
    ToolWindow(String),
    EditorArea,
    /// Some other component (a popup's field, a dialog, the title bar).
    Other,
}

/// When to move the focus to the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorFocus {
    Now,
    /// After the next frame: a layout that was just restored shows the editor area only then,
    /// and what is hidden cannot take the focus.
    NextFrame,
}

/// How a request to focus the active editor (or the editor area when none is open) is served.
pub fn focus_editor(editor_area_hidden: bool) -> EditorFocus {
    if editor_area_hidden { EditorFocus::NextFrame } else { EditorFocus::Now }
}

/// Follows the focus and the popups of one window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusTracker {
    place: FocusPlace,
    popup_open: bool,
    /// The last popup closed and the focus has not been looked at since.
    refocus_pending: bool,
}

impl FocusTracker {
    /// `popup_open`: whether a popup is open already.
    pub fn new(popup_open: bool) -> Self {
        Self { popup_open, ..Self::default() }
    }

    pub fn place(&self) -> &FocusPlace {
        &self.place
    }

    /// The tool window that contains the focus: the `activeToolWindow` fact, and
    /// `toolWindowFocus` is whether there is one.
    pub fn tool_window(&self) -> Option<&str> {
        match &self.place {
            FocusPlace::ToolWindow(id) => Some(id),
            _ => None,
        }
    }

    /// The `editorAreaFocus` fact.
    pub fn editor_area_focus(&self) -> bool {
        self.place == FocusPlace::EditorArea
    }

    /// A component of the window took the focus.
    pub fn focus_in(&mut self, place: FocusPlace) {
        self.place = place;
    }

    /// The focus left to nowhere inside the window. A window that merely stopped being the
    /// active one keeps its focus for its return and does not report this.
    pub fn focus_lost(&mut self) {
        self.place = FocusPlace::Nowhere;
    }

    /// A popup opened, or the last one closed. After the last one closes the caller asks
    /// [`FocusTracker::refocus_due`] once the event that closed it has been handled, so that a
    /// popup which hands the focus on gets to do so first.
    pub fn popup_changed(&mut self, open: bool) {
        if open == self.popup_open {
            return;
        }
        self.popup_open = open;
        self.refocus_pending = !open;
    }

    /// Whether the editor takes the focus now ([`focus_editor`]): the last popup closed, none
    /// opened since, and the focus was left nowhere.
    pub fn refocus_due(&mut self) -> bool {
        std::mem::take(&mut self.refocus_pending) && !self.popup_open && self.place == FocusPlace::Nowhere
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // focus after popups › waits for the restored editor to become visible before focusing it
    #[test]
    fn waits_for_the_restored_editor_to_become_visible_before_focusing_it() {
        assert_eq!(focus_editor(true), EditorFocus::NextFrame);
        assert_eq!(focus_editor(false), EditorFocus::Now);
    }

    // focus after popups › returns focus to the editor when the last popup closes and focus was left nowhere
    #[test]
    fn returns_focus_to_the_editor_when_the_last_popup_closes_and_focus_was_left_nowhere() {
        let mut tracker = FocusTracker::new(false);
        tracker.focus_in(FocusPlace::EditorArea);
        tracker.popup_changed(true);
        tracker.focus_lost();
        tracker.popup_changed(false);
        assert!(tracker.refocus_due());
        // It is decided once per close.
        assert!(!tracker.refocus_due());
    }

    // focus after popups › leaves focus alone when the popup handed it to another component
    #[test]
    fn leaves_focus_alone_when_the_popup_handed_it_to_another_component() {
        let mut tracker = FocusTracker::new(false);
        tracker.popup_changed(true);
        tracker.focus_in(FocusPlace::Other);
        tracker.popup_changed(false);
        assert!(!tracker.refocus_due());
        assert_eq!(tracker.place(), &FocusPlace::Other);
    }

    // Not from a spec: the facts published about the focus, and a popup that opens again before
    // the focus was looked at cancels the return to the editor.
    #[test]
    fn publishes_where_the_focus_is_and_cancels_the_return_when_a_popup_reopens() {
        let mut tracker = FocusTracker::new(true);
        tracker.focus_in(FocusPlace::ToolWindow("terminal".to_owned()));
        assert_eq!((tracker.tool_window(), tracker.editor_area_focus()), (Some("terminal"), false));
        tracker.focus_in(FocusPlace::EditorArea);
        assert_eq!((tracker.tool_window(), tracker.editor_area_focus()), (None, true));
        tracker.focus_lost();
        assert!(!tracker.editor_area_focus());
        tracker.popup_changed(false);
        tracker.popup_changed(true);
        assert!(!tracker.refocus_due());
        // Opening is not a close: nothing is owed when the state did not change either.
        tracker.popup_changed(true);
        assert!(!tracker.refocus_due());
    }
}
