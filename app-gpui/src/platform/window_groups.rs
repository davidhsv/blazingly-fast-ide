//! Project windows grouped as the tabs of one window: their order and the tab to switch to.

/// A project window of the group the current window belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTab {
    /// The window's identifier.
    pub label: String,
    pub name: String,
    /// The project open in the window, if any.
    pub root: Option<String>,
    /// Whether this is the window showing the tabs.
    pub current: bool,
}

/// The labels of `tabs` after dragging the tab `source` onto the tab `target`: the dragged tab
/// takes the target's place. An unknown source or target leaves the order alone.
pub fn reordered_tabs(tabs: &[WindowTab], source: &str, target: &str) -> Vec<String> {
    let mut labels: Vec<String> = tabs.iter().map(|tab| tab.label.clone()).collect();
    let position = |label: &str| labels.iter().position(|listed| listed == label);
    if let (Some(from), Some(to)) = (position(source), position(target))
        && from != to
    {
        let moved = labels.remove(from);
        labels.insert(to, moved);
    }
    labels
}

/// Whether the window shares its group with other windows (the `windowGrouped` context key).
pub fn is_window_grouped(tabs: &[WindowTab]) -> bool {
    tabs.len() > 1
}

/// The label of the tab to select when cycling forward or backward from the current one,
/// wrapping around. `None` when there is no other tab.
pub fn cycled_tab(tabs: &[WindowTab], forward: bool) -> Option<&str> {
    let count = tabs.len();
    if count < 2 {
        return None;
    }
    let next = match (tabs.iter().position(|tab| tab.current), forward) {
        (Some(current), true) => (current + 1) % count,
        (Some(current), false) => (current + count - 1) % count,
        // No tab is marked current: cycling starts from before the first tab.
        (None, true) => 0,
        (None, false) => count - 2,
    };
    tabs.get(next).map(|tab| tab.label.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Vec<WindowTab> {
        ["a", "b", "c"]
            .into_iter()
            .map(|label| WindowTab {
                label: label.to_owned(),
                name: label.to_owned(),
                current: label == "b",
                root: Some(format!("/{label}")),
            })
            .collect()
    }

    fn labels(tabs: &[WindowTab]) -> Vec<&str> {
        tabs.iter().map(|tab| tab.label.as_str()).collect()
    }

    // src/platform/windowGroups.test.ts › moves a tab across its neighbours without changing the source or active window
    #[test]
    fn moves_a_tab_across_its_neighbours_without_changing_the_source_or_active_window() {
        let tabs = tabs();
        assert_eq!(reordered_tabs(&tabs, "a", "c"), ["b", "c", "a"]);
        assert_eq!(reordered_tabs(&tabs, "c", "a"), ["c", "a", "b"]);
        assert_eq!(labels(&tabs), ["a", "b", "c"]);
        assert_eq!(tabs.iter().find(|tab| tab.current).map(|tab| tab.label.as_str()), Some("b"));
    }

    // src/platform/windowGroups.test.ts › ignores external drags and missing targets
    #[test]
    fn ignores_external_drags_and_missing_targets() {
        let tabs = tabs();
        assert_eq!(reordered_tabs(&tabs, "outside", "b"), ["a", "b", "c"]);
        assert_eq!(reordered_tabs(&tabs, "a", "outside"), ["a", "b", "c"]);
    }

    /// Not from a spec: cycling wraps around, and needs a second tab.
    #[test]
    fn cycles_through_the_tabs_of_the_group() {
        let mut tabs = tabs();
        assert!(is_window_grouped(&tabs));
        assert_eq!(cycled_tab(&tabs, true), Some("c"));
        assert_eq!(cycled_tab(&tabs, false), Some("a"));
        tabs[1].current = false;
        tabs[2].current = true;
        assert_eq!(cycled_tab(&tabs, true), Some("a"));
        tabs[2].current = false;
        assert_eq!(cycled_tab(&tabs, true), Some("a"));
        assert_eq!(cycled_tab(&tabs, false), Some("b"));
        assert_eq!(cycled_tab(&tabs[..1], true), None);
        assert!(!is_window_grouped(&tabs[..1]));
    }
}
