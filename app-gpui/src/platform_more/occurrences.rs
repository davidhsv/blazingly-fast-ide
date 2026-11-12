//! Next / Previous Occurrence: result views (Find, Usages, Problems…) let the two actions
//! step through their results from anywhere, the editor included. The actions ask the focused
//! tool window's navigator, else the last active one's.

use crate::platform::i18n::{I18n, Param, format_message};
use crate::workbench::tool_windows::ToolWindowLayout;

/// Which way to step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
}

/// The occurrence navigated to: its 1-based position among `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccurrenceStep {
    pub index: usize,
    pub total: usize,
}

/// A result view that can step through its occurrences.
pub trait OccurrenceNavigator {
    /// Selects the next / previous occurrence and opens it in the editor, focusing the editor
    /// only when `focus_editor` (the action ran outside the tool window). `None` when there is
    /// none in that direction.
    fn go(&self, direction: Direction, focus_editor: bool) -> Option<OccurrenceStep>;
}

/// Handle of one [`OccurrenceNavigators::register`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigatorRegistration(u64);

#[derive(Debug, Clone)]
struct Registered<N> {
    serial: u64,
    tool_window: String,
    navigator: N,
}

/// The navigators of the tool windows; the latest registration of a tool window wins.
#[derive(Debug, Clone)]
pub struct OccurrenceNavigators<N> {
    registered: Vec<Registered<N>>,
    next_serial: u64,
}

impl<N> Default for OccurrenceNavigators<N> {
    fn default() -> Self {
        Self { registered: Vec::new(), next_serial: 0 }
    }
}

impl<N: OccurrenceNavigator> OccurrenceNavigators<N> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `navigator` for `tool_window`, in front of the ones registered before.
    pub fn register(&mut self, tool_window: &str, navigator: N) -> NavigatorRegistration {
        self.next_serial += 1;
        self.registered.push(Registered { serial: self.next_serial, tool_window: tool_window.to_owned(), navigator });
        NavigatorRegistration(self.next_serial)
    }

    /// Removes the navigator `registration` registered; an earlier one of the same tool window
    /// takes over again.
    pub fn unregister(&mut self, registration: NavigatorRegistration) {
        self.registered.retain(|registered| registered.serial != registration.0);
    }

    /// The navigator of `tool_window`: the one registered last.
    pub fn navigator(&self, tool_window: &str) -> Option<&N> {
        self.registered.iter().rev().find(|registered| registered.tool_window == tool_window).map(|r| &r.navigator)
    }

    /// Steps the results of the focused tool window, else of the last active one, the last used
    /// one or a visible one: the first of them that has a navigator, even when it has no
    /// occurrence left that way. Returns the status bar text of the step (`2 of 5`), or `None`
    /// when nothing was navigated to.
    pub fn go_to_occurrence(&self, layout: &ToolWindowLayout, direction: Direction, i18n: &I18n) -> Option<String> {
        let focused = layout.focused();
        let candidates = [focused, layout.last_active(), layout.last_used()];
        let (id, navigator) = candidates
            .into_iter()
            .flatten()
            .chain(layout.visible().ids())
            .find_map(|id| self.navigator(id).map(|navigator| (id, navigator)))?;
        let step = navigator.go(direction, focused != Some(id))?;
        let params = [("index", Param::from(step.index)), ("total", Param::from(step.total))];
        Some(format_message(&i18n.tx("{index} of {total}"), &params, i18n.locale()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::tool_windows::{Anchor, ToolWindow};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Calls = Rc<RefCell<Vec<(Direction, bool)>>>;

    struct Fixed {
        step: Option<OccurrenceStep>,
        calls: Calls,
    }

    impl OccurrenceNavigator for Fixed {
        fn go(&self, direction: Direction, focus_editor: bool) -> Option<OccurrenceStep> {
            self.calls.borrow_mut().push((direction, focus_editor));
            self.step
        }
    }

    fn navigator(step: Option<(usize, usize)>) -> (Fixed, Calls) {
        let calls = Calls::default();
        (Fixed { step: step.map(|(index, total)| OccurrenceStep { index, total }), calls: calls.clone() }, calls)
    }

    /// Project on the left and Find at the bottom, with a project's layout loaded.
    fn layout() -> ToolWindowLayout {
        let mut layout = ToolWindowLayout::new();
        for window in [
            ToolWindow::new("project", "Project", "folder", Anchor::Left),
            ToolWindow::new("find", "Find", "search", Anchor::Bottom),
        ] {
            let _ = layout.register(window);
        }
        layout.load("/work/occurrences", None);
        layout
    }

    // Next / Previous Occurrence › steps the focused tool window's results, keeping the focus there
    #[test]
    fn steps_the_focused_tool_windows_results_keeping_the_focus_there() {
        let (find, calls) = navigator(Some((2, 5)));
        let mut navigators = OccurrenceNavigators::new();
        navigators.register("find", find);
        let mut layout = layout();
        let _ = layout.show("find", false);
        layout.set_focused(Some("find"));
        let status = navigators.go_to_occurrence(&layout, Direction::Next, &I18n::default());
        assert_eq!(status.as_deref(), Some("2 of 5"));
        assert_eq!(*calls.borrow(), [(Direction::Next, false)]);
    }

    // Next / Previous Occurrence › from elsewhere (the editor) uses the last active tool window and focuses the
    // editor
    #[test]
    fn from_elsewhere_the_editor_uses_the_last_active_tool_window_and_focuses_the_editor() {
        let (find, calls) = navigator(Some((1, 1)));
        let mut navigators = OccurrenceNavigators::new();
        navigators.register("find", find);
        let mut layout = layout();
        let _ = layout.show("find", false);
        layout.set_focused(Some("project"));
        layout.set_focused(None);
        assert!(navigators.go_to_occurrence(&layout, Direction::Previous, &I18n::default()).is_some());
        assert_eq!(*calls.borrow(), [(Direction::Previous, true)]);
    }

    // Next / Previous Occurrence › does nothing without a navigator or past the last occurrence
    #[test]
    fn does_nothing_without_a_navigator_or_past_the_last_occurrence() {
        let mut navigators: OccurrenceNavigators<Fixed> = OccurrenceNavigators::new();
        let mut layout = layout();
        let _ = layout.show("project", false);
        assert_eq!(navigators.go_to_occurrence(&layout, Direction::Next, &I18n::default()), None);
        let (project, calls) = navigator(None);
        let registration = navigators.register("project", project);
        assert_eq!(navigators.go_to_occurrence(&layout, Direction::Next, &I18n::default()), None);
        assert_eq!(calls.borrow().len(), 1);
        navigators.unregister(registration);
        assert!(navigators.navigator("project").is_none());
    }

    // Not from a spec: the latest registration of a tool window answers, and the earlier one
    // takes over again once it is gone.
    #[test]
    fn the_latest_navigator_of_a_tool_window_wins() {
        let (old, old_calls) = navigator(Some((1, 2)));
        let (new, new_calls) = navigator(Some((2, 2)));
        let mut navigators = OccurrenceNavigators::new();
        navigators.register("find", old);
        let latest = navigators.register("find", new);
        let mut layout = layout();
        let _ = layout.show("find", false);
        let go = |navigators: &OccurrenceNavigators<Fixed>| {
            navigators.go_to_occurrence(&layout, Direction::Next, &I18n::default())
        };
        assert_eq!(go(&navigators).as_deref(), Some("2 of 2"));
        navigators.unregister(latest);
        assert_eq!(go(&navigators).as_deref(), Some("1 of 2"));
        assert_eq!((old_calls.borrow().len(), new_calls.borrow().len()), (1, 1));
    }
}
