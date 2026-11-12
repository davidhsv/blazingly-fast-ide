//! Content tabs of the Commit and Git tool windows. With the non-modal commit interface the Commit
//! window has Commit | Shelf | Stash; with the modal one (no Commit window) the Git window takes
//! them as Local Changes | Shelf | Stash, before Log | Console | Worktrees. "Combine stashes and
//! shelves" makes Shelf and Stash one tab.

use super::commit_store::CommitTab;
use super::log_store::{LogTab, LogTabKind, LogTabLocation, MAIN_LOG};
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::base_name;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentTab {
    pub id: String,
    /// English source text, or a title already in the UI language (log tabs).
    pub label: String,
    pub closable: bool,
    /// Count shown after the label (shelves / stashes).
    pub count: Option<usize>,
}

impl ContentTab {
    fn new(id: &str, label: &str) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), closable: false, count: None }
    }

    fn counted(id: &str, label: &str, count: usize) -> Self {
        Self { count: (count > 0).then_some(count), ..Self::new(id, label) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StashTabsInput<'a> {
    pub combine: bool,
    pub shelf_count: usize,
    pub stash_count: usize,
    /// The selected tab: an empty Shelf tab stays while it is shown.
    pub tab: &'a str,
}

/// The tab a Shelf / Stash request lands on (the combined one when they are combined).
pub fn stash_tab_of(tab: CommitTab, combine: bool) -> CommitTab {
    if combine && tab == CommitTab::Shelf { CommitTab::Stashes } else { tab }
}

/// Shelf and Stash tabs (Shelf only when there are shelves), or the combined one.
pub fn stash_tabs(input: StashTabsInput) -> Vec<ContentTab> {
    let stashes = CommitTab::Stashes.id();
    if input.combine {
        return vec![ContentTab::counted(stashes, "Stashes and Shelves", input.stash_count + input.shelf_count)];
    }
    let shelf = CommitTab::Shelf.id();
    let mut tabs = Vec::new();
    if input.shelf_count > 0 || input.tab == shelf {
        tabs.push(ContentTab::counted(shelf, "Shelf", input.shelf_count));
    }
    tabs.push(ContentTab::counted(stashes, "Stash", input.stash_count));
    tabs
}

/// Title of a log tab of its own.
pub fn log_tab_label(tab: &LogTab, i18n: &mut I18n) -> String {
    if tab.kind == LogTabKind::History {
        i18n.tx_built(&["History: ", ""], &[Param::from(base_name(&tab.target))])
    } else {
        i18n.tx_built(&["Log: ", ""], &[Param::from(tab.target.as_str())])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GitTabsInput<'a> {
    pub stash: StashTabsInput<'a>,
    /// Modal commit interface: the Git window also shows the local changes, shelves and stashes.
    pub modal: bool,
    pub log_tabs: &'a [LogTab],
}

pub fn git_tabs(input: GitTabsInput, i18n: &mut I18n) -> Vec<ContentTab> {
    let mut tabs = Vec::new();
    if input.modal {
        tabs.push(ContentTab::new(CommitTab::Changes.id(), "Local Changes"));
        tabs.extend(stash_tabs(input.stash));
    }
    tabs.push(ContentTab::new(MAIN_LOG, "Log"));
    for tab in input.log_tabs.iter().filter(|tab| tab.location != LogTabLocation::Editor) {
        tabs.push(ContentTab { closable: true, ..ContentTab::new(&tab.id, &log_tab_label(tab, i18n)) });
    }
    tabs.push(ContentTab::new("console", "Console"));
    tabs.push(ContentTab::new("worktrees", "Worktrees"));
    tabs
}

/// The tab shown for `tab`: a tab no longer listed (the commit interface changed) falls back to the
/// Log.
pub fn shown_tab<'a>(tab: &'a str, tabs: &[ContentTab], combine: bool) -> &'a str {
    let id = if combine && tab == CommitTab::Shelf.id() { CommitTab::Stashes.id() } else { tab };
    if tabs.iter().any(|known| known.id == id) { id } else { MAIN_LOG }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: StashTabsInput = StashTabsInput { combine: false, shelf_count: 0, stash_count: 0, tab: "log" };

    fn tabs(modal: bool, stash: StashTabsInput, log_tabs: &[LogTab]) -> Vec<ContentTab> {
        git_tabs(GitTabsInput { stash, modal, log_tabs }, &mut I18n::default())
    }

    fn ids(tabs: &[ContentTab]) -> Vec<&str> {
        tabs.iter().map(|tab| tab.id.as_str()).collect()
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Git and Commit tool window tabs › puts Local Changes, Shelf and Stash into the Git window only with the modal commit interface
    #[test]
    fn puts_local_changes_shelf_and_stash_into_the_git_window_only_with_the_modal_commit_interface() {
        assert_eq!(ids(&tabs(false, BASE, &[])), ["log", "console", "worktrees"]);
        assert_eq!(ids(&tabs(true, BASE, &[])), ["changes", "stashes", "log", "console", "worktrees"]);
        let shelves = StashTabsInput { shelf_count: 2, ..BASE };
        assert_eq!(ids(&tabs(true, shelves, &[])), ["changes", "shelf", "stashes", "log", "console", "worktrees"]);
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Git and Commit tool window tabs › combines stashes and shelves into one tab
    #[test]
    fn combines_stashes_and_shelves_into_one_tab() {
        let combined = StashTabsInput { combine: true, shelf_count: 2, stash_count: 1, ..BASE };
        let expected = ContentTab {
            id: "stashes".to_owned(),
            label: "Stashes and Shelves".to_owned(),
            closable: false,
            count: Some(3),
        };
        assert_eq!(stash_tabs(combined), [expected]);
        assert_eq!(stash_tab_of(CommitTab::Shelf, true), CommitTab::Stashes);
        assert_eq!(stash_tab_of(CommitTab::Shelf, false), CommitTab::Shelf);
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Git and Commit tool window tabs › lists log tabs as closable after the main Log, and falls back to the Log for a missing tab
    #[test]
    fn lists_log_tabs_as_closable_after_the_main_log_and_falls_back_to_the_log_for_a_missing_tab() {
        let log_tabs = [LogTab {
            id: "history:1".to_owned(),
            kind: LogTabKind::History,
            target: "/p/src/a.ts".to_owned(),
            repo: None,
            repositories: None,
            location: LogTabLocation::ToolWindow,
            read_only: false,
        }];
        let tabs = tabs(false, BASE, &log_tabs);
        let history =
            ContentTab { id: "history:1".to_owned(), label: "History: a.ts".to_owned(), closable: true, count: None };
        assert_eq!(tabs.iter().find(|tab| tab.id == "history:1"), Some(&history));
        assert_eq!(shown_tab("changes", &tabs, false), "log");
        assert_eq!(shown_tab("history:1", &tabs, false), "history:1");
    }
}
