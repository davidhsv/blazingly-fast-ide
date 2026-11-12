//! Find tool window results ("Open in Find Window"), one tab per search. A new search replaces the selected
//! tab unless that tab is pinned.
//!
//! [`FindWindow`] owns the tabs and their search sessions and talks to nothing: what a transition needs done
//! comes back as [`WindowEffect`]s, and the outcomes of a tab's search steps are handed to
//! [`FindWindow::validated`], [`FindWindow::file_found`] and [`FindWindow::finished`].

use super::ipc::{FileMatches, RequestId, SearchQuery, SearchSummary};
use super::session::{FindEffect, FindSession, FindState, count_file_matches};
use super::store::StorageWrite;
use ide_core::IdeError;
use serde_json::{Value, json};

/// Most tabs kept; opening another closes the oldest.
pub const MAX_TABS: usize = 8;

/// One search and its results.
#[derive(Debug)]
pub struct FindTab {
    pub id: u32,
    pub title: String,
    pub query: SearchQuery,
    /// What Replace puts in place of each occurrence; `None` for a plain search.
    pub replacement: Option<String>,
    /// Replace keeps the case of each occurrence.
    pub preserve_case: bool,
    /// Kept when a new search runs (Pin Tab).
    pub pinned: bool,
    /// The results are those of an open editor, read live: selecting one reveals its line while the focus
    /// stays in the tree.
    pub editor_results: bool,
    session: FindSession,
}

impl FindTab {
    pub fn state(&self) -> &FindState {
        self.session.state()
    }
}

/// What the window asks of its driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEffect {
    /// Carry out a step of this tab's search.
    Search { tab: u32, effect: FindEffect },
    /// Read the editor's current results of this tab and hand them to [`FindWindow::editor_results`].
    ReadEditorResults { tab: u32 },
    /// Show and focus the Find tool window.
    ShowToolWindow,
}

/// How a search is opened in the window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    pub replacement: Option<String>,
    pub preserve_case: bool,
    /// "Open results in new tab": keep the active tab instead of replacing it.
    pub new_tab: bool,
}

/// A newly opened tab and what opening it needs done.
#[derive(Debug)]
#[must_use]
pub struct Opened {
    pub id: u32,
    pub effects: Vec<WindowEffect>,
}

fn search_effects(tab: u32, effects: Vec<FindEffect>) -> impl Iterator<Item = WindowEffect> {
    effects.into_iter().map(move |effect| WindowEffect::Search { tab, effect })
}

#[derive(Debug, Default)]
pub struct FindWindow {
    tabs: Vec<FindTab>,
    active: Option<u32>,
    /// The id given to the last opened tab.
    last_tab: u32,
}

impl FindWindow {
    pub fn tabs(&self) -> &[FindTab] {
        &self.tabs
    }

    pub fn tab(&self, id: u32) -> Option<&FindTab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// The selected tab.
    pub fn active(&self) -> Option<u32> {
        self.active
    }

    pub fn select(&mut self, id: u32) {
        if self.tab(id).is_some() {
            self.active = Some(id);
        }
    }

    fn tab_mut(&mut self, id: u32) -> Option<&mut FindTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Puts a new tab in the place of the selected one, or after the others when that one is pinned or
    /// `new_tab` is set, and selects it.
    fn add_tab(&mut self, query: SearchQuery, title: &str, options: OpenOptions, editor_results: bool) -> Opened {
        self.last_tab += 1;
        let id = self.last_tab;
        let new_tab = options.new_tab;
        let tab = FindTab {
            id,
            title: title.to_owned(),
            query,
            replacement: options.replacement,
            preserve_case: options.preserve_case,
            pinned: false,
            editor_results,
            session: FindSession::new(),
        };
        let mut effects = Vec::new();
        let selected = self.tabs.iter().position(|tab| Some(tab.id) == self.active && !tab.pinned);
        match selected.filter(|_| !new_tab) {
            Some(index) => {
                let mut replaced = std::mem::replace(&mut self.tabs[index], tab);
                effects.extend(search_effects(replaced.id, replaced.session.dispose()));
            }
            None => self.tabs.push(tab),
        }
        while self.tabs.len() > MAX_TABS {
            let mut oldest = self.tabs.remove(0);
            effects.extend(search_effects(oldest.id, oldest.session.dispose()));
        }
        self.active = Some(id);
        Opened { id, effects }
    }

    /// Opens a search in the window and starts it.
    pub fn open(&mut self, query: SearchQuery, title: &str, options: OpenOptions) -> Opened {
        let mut opened = self.add_tab(query, title, options, false);
        opened.effects.extend(self.rerun(opened.id));
        opened.effects.push(WindowEffect::ShowToolWindow);
        opened
    }

    /// Opens the results an editor found in its own text (`file`, read by the caller) as a tab with the
    /// usual lifecycle. Rerunning the tab asks for them again instead of searching.
    pub fn open_editor_results(&mut self, query: SearchQuery, title: &str, file: FileMatches) -> Opened {
        let mut opened = self.add_tab(query, title, OpenOptions::default(), true);
        self.editor_results(opened.id, file);
        opened.effects.push(WindowEffect::ShowToolWindow);
        opened
    }

    /// The current results of an editor-backed tab: they replace the tab's results at once, without the
    /// backend.
    pub fn editor_results(&mut self, id: u32, file: FileMatches) {
        let Some(tab) = self.tab_mut(id) else { return };
        let validate = tab.session.search(tab.query.clone()).into_iter().find_map(|effect| match effect {
            FindEffect::Validate { run, .. } => Some(run),
            _ => None,
        });
        let Some(run) = validate else { return };
        // An editor's results need no validation, and there is no request to cancel.
        if tab.session.validated(run, Ok(()), RequestId::next).is_empty() {
            return;
        }
        let matches = count_file_matches(&file);
        if matches > 0 {
            tab.session.file_found(run, file);
        }
        let summary = SearchSummary { files: u32::from(matches > 0), matches, ..SearchSummary::default() };
        tab.session.finished(run, Ok(Some(summary)));
    }

    /// The backend's verdict on the pattern of a tab's search.
    pub fn validated(
        &mut self,
        id: u32,
        run: u64,
        outcome: Result<(), IdeError>,
        next_request_id: impl FnOnce() -> RequestId,
    ) -> Vec<WindowEffect> {
        let Some(tab) = self.tab_mut(id) else { return Vec::new() };
        search_effects(id, tab.session.validated(run, outcome, next_request_id)).collect()
    }

    /// A file with matches arrived for a tab's search.
    pub fn file_found(&mut self, id: u32, run: u64, file: FileMatches) {
        if let Some(tab) = self.tab_mut(id) {
            tab.session.file_found(run, file);
        }
    }

    /// A tab's search ended.
    pub fn finished(&mut self, id: u32, run: u64, outcome: Result<Option<SearchSummary>, IdeError>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.session.finished(run, outcome);
        }
    }

    pub fn pin(&mut self, id: u32, pinned: bool) {
        if let Some(tab) = self.tab_mut(id) {
            tab.pinned = pinned;
        }
    }

    /// Reruns the selected tab, or the last one when none is selected.
    pub fn rerun_active(&mut self) -> Vec<WindowEffect> {
        match self.active.or_else(|| self.tabs.last().map(|tab| tab.id)) {
            Some(id) => self.rerun(id),
            None => Vec::new(),
        }
    }

    pub fn rerun(&mut self, id: u32) -> Vec<WindowEffect> {
        let Some(tab) = self.tab_mut(id) else { return Vec::new() };
        if tab.editor_results {
            return vec![WindowEffect::ReadEditorResults { tab: id }];
        }
        search_effects(id, tab.session.search(tab.query.clone())).collect()
    }

    /// Stops a tab's search, keeping what it found.
    pub fn stop(&mut self, id: u32) -> Vec<WindowEffect> {
        let Some(tab) = self.tab_mut(id) else { return Vec::new() };
        search_effects(id, tab.session.cancel()).collect()
    }

    /// Closes a tab; when it was selected, the tab that takes its place is.
    pub fn close(&mut self, id: u32) -> Vec<WindowEffect> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else { return Vec::new() };
        let mut closed = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self.tabs.get(index).or(self.tabs.last()).map(|tab| tab.id);
        }
        search_effects(id, closed.session.dispose()).collect()
    }

    /// Closes every tab.
    pub fn dispose(&mut self) -> Vec<WindowEffect> {
        self.active = None;
        let closed = std::mem::take(&mut self.tabs);
        closed.into_iter().flat_map(|mut tab| search_effects(tab.id, tab.session.dispose())).collect()
    }
}

/// Storage key of the view options.
pub const OPTIONS_KEY: &str = "openidea.find.view.v1";

/// View options of the tool window's ⋮ menu, kept between sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindViewOptions {
    /// Code preview beside the results.
    pub preview: bool,
    /// Group files by directory; off lists them flat.
    pub group_by_directory: bool,
}

impl Default for FindViewOptions {
    fn default() -> Self {
        Self { preview: true, group_by_directory: false }
    }
}

/// An entry of the tool window's ⋮ menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderMenuItem {
    Header(&'static str),
    Separator,
    /// A view option, toggled by [`FindViewOptions::toggle`] with its id.
    Check {
        id: &'static str,
        label: &'static str,
        checked: bool,
    },
}

const GROUP_BY_DIRECTORY: &str = "groupByDirectory";
const PREVIEW: &str = "preview";

impl FindViewOptions {
    /// The options stored under [`OPTIONS_KEY`]; whatever is missing or malformed keeps its default.
    pub fn restored(stored: Option<&str>) -> Self {
        let value = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok()).unwrap_or(Value::Null);
        let flag = |key: &str, default: bool| value.get(key).and_then(Value::as_bool).unwrap_or(default);
        let defaults = Self::default();
        Self {
            preview: flag(PREVIEW, defaults.preview),
            group_by_directory: flag(GROUP_BY_DIRECTORY, defaults.group_by_directory),
        }
    }

    pub fn stored(self) -> StorageWrite {
        let value = json!({ "preview": self.preview, "groupByDirectory": self.group_by_directory });
        StorageWrite { key: OPTIONS_KEY, value: value.to_string() }
    }

    /// The entries of the ⋮ menu.
    pub fn header_menu(self) -> [HeaderMenuItem; 4] {
        [
            HeaderMenuItem::Header("Group By"),
            HeaderMenuItem::Check { id: GROUP_BY_DIRECTORY, label: "Directory", checked: self.group_by_directory },
            HeaderMenuItem::Separator,
            HeaderMenuItem::Check { id: PREVIEW, label: "Preview Source", checked: self.preview },
        ]
    }

    /// Toggles the option a menu entry stands for; returns what to store, `None` for an unknown id.
    pub fn toggle(&mut self, id: &str) -> Option<StorageWrite> {
        match id {
            GROUP_BY_DIRECTORY => self.group_by_directory = !self.group_by_directory,
            PREVIEW => self.preview = !self.preview,
            _ => return None,
        }
        Some(self.stored())
    }
}

#[cfg(test)]
mod tests {
    use super::super::find_tool_window::{Node, build_find_tree};
    use super::super::ipc::LineMatch;
    use super::super::session::FindStatus;
    use super::*;

    fn q(pattern: &str) -> SearchQuery {
        SearchQuery { pattern: pattern.to_owned(), max_matches: 100, ..SearchQuery::default() }
    }

    fn file(rel: &str) -> FileMatches {
        let line = LineMatch { line: 0, text: "foo".to_owned(), ranges: vec![(0, 3)], text_offset: 0 };
        FileMatches { path: format!("/p/{rel}"), rel_path: rel.to_owned(), matches: vec![line] }
    }

    fn open(window: &mut FindWindow, pattern: &str, new_tab: bool) -> u32 {
        window.open(q(pattern), pattern, OpenOptions { new_tab, ..OpenOptions::default() }).id
    }

    fn titles(window: &FindWindow) -> Vec<&str> {
        window.tabs().iter().map(|tab| tab.title.as_str()).collect()
    }

    /// The run of the validation or start a search step asks for.
    fn run_of(effects: &[WindowEffect]) -> u64 {
        let run = effects.iter().find_map(|effect| match effect {
            WindowEffect::Search {
                effect: FindEffect::Validate { run, .. } | FindEffect::Start { run, .. }, ..
            } => Some(*run),
            _ => None,
        });
        run.expect("a search step")
    }

    // src/features/search/__tests__/findWindow.test.ts › Find tool window tabs › reuses the selected tab unless
    // it is pinned
    #[test]
    fn reuses_the_selected_tab_unless_it_is_pinned() {
        let mut window = FindWindow::default();
        let a = open(&mut window, "a", false);
        let b = open(&mut window, "b", false);
        assert_eq!(titles(&window), ["b"]);
        assert_eq!(window.active(), Some(b));
        assert_ne!(b, a);

        window.pin(b, true);
        let c = open(&mut window, "c", false);
        assert_eq!(titles(&window), ["b", "c"]);
        assert_eq!(window.active(), Some(c));

        // Selecting the pinned tab again opens the next search beside it.
        window.select(b);
        open(&mut window, "d", false);
        assert_eq!(titles(&window), ["b", "c", "d"]);
    }

    // src/features/search/__tests__/findWindow.test.ts › Find tool window tabs › keeps the selected tab with
    // IDEA's Open results in new tab
    #[test]
    fn keeps_the_selected_tab_with_open_results_in_new_tab() {
        let mut window = FindWindow::default();
        open(&mut window, "a", false);
        let b = open(&mut window, "b", true);
        assert_eq!(titles(&window), ["a", "b"]);
        assert_eq!(window.active(), Some(b));
    }

    // src/features/search/__tests__/findWindow.test.ts › editor-backed Find results › uses the shared session and
    // recomputes live results on rerun without IPC
    #[test]
    fn uses_the_shared_session_and_recomputes_live_results_on_rerun_without_ipc() {
        let mut current = file("unsaved.ts");
        current.matches[0].text = "foo foo".to_owned();
        current.matches[0].ranges.push((4, 7));
        let mut window = FindWindow::default();
        let opened = window.open_editor_results(q("foo"), "Occurrences of 'foo'", current.clone());
        assert_eq!(opened.effects, [WindowEffect::ShowToolWindow]);
        let id = opened.id;
        let state = window.tabs()[0].state();
        assert_eq!(state.status, FindStatus::Done);
        assert_eq!(*state.files, [current]);
        assert_eq!(state.match_count, 2);
        let tree = build_find_tree(&state.files, false);
        assert!(matches!(tree[0], Node::File { count: 2, .. }));

        assert_eq!(window.rerun(id), [WindowEffect::ReadEditorResults { tab: id }]);
        window.editor_results(id, FileMatches { matches: Vec::new(), ..file("unsaved.ts") });
        let state = window.tabs()[0].state();
        assert!(state.files.is_empty());
        assert_eq!(state.summary.as_ref().map(|summary| summary.matches), Some(0));
    }

    // Not from a spec: a tab's search steps go through the window, a replaced or closed tab's request is
    // cancelled, and closing the selected tab selects the one that takes its place.
    #[test]
    fn drives_a_tabs_search_and_cancels_it_when_the_tab_goes() {
        let mut window = FindWindow::default();
        let opened = window.open(q("foo"), "foo", OpenOptions { new_tab: true, ..OpenOptions::default() });
        let first = opened.id;
        assert_eq!(opened.effects.last(), Some(&WindowEffect::ShowToolWindow));
        let run = run_of(&opened.effects);
        let started = window.validated(first, run, Ok(()), || RequestId(41));
        assert_eq!(run_of(&started), run);
        window.file_found(first, run, file("a.ts"));
        assert_eq!(window.tab(first).map(|tab| tab.state().match_count), Some(1));
        let cancel = |request: u32| WindowEffect::Search { tab: first, effect: FindEffect::Cancel(RequestId(request)) };
        assert_eq!(window.stop(first), [cancel(41)]);
        assert_eq!(window.tab(first).map(|tab| tab.state().status), Some(FindStatus::Done));

        let rerun = window.rerun_active();
        let started = window.validated(first, run_of(&rerun), Ok(()), || RequestId(42));
        window.finished(first, run_of(&started), Ok(None));
        let second = open(&mut window, "bar", true);
        let rerun = window.rerun(first);
        let _ = window.validated(first, run_of(&rerun), Ok(()), || RequestId(43));
        assert!(window.close(second).is_empty());
        assert_eq!(window.active(), Some(first));
        assert_eq!(window.close(first), [cancel(43)]);
        assert_eq!((window.active(), window.tabs().len()), (None, 0));
        assert!(window.rerun_active().is_empty());
    }

    // Not from a spec: the window keeps at most MAX_TABS tabs, closing the oldest, and disposing closes all.
    #[test]
    fn keeps_a_bounded_number_of_tabs() {
        let mut window = FindWindow::default();
        let ids: Vec<u32> = (0..MAX_TABS + 2).map(|n| open(&mut window, &n.to_string(), true)).collect();
        assert_eq!(window.tabs().len(), MAX_TABS);
        assert_eq!(window.tabs()[0].id, ids[2]);
        assert_eq!(window.active(), ids.last().copied());
        assert!(window.tab(ids[0]).is_none());
        assert!(window.dispose().is_empty());
        assert_eq!((window.active(), window.tabs().len()), (None, 0));
    }

    // Not from a spec: view options keep their defaults for whatever is not stored, and toggling one returns
    // the value to store.
    #[test]
    fn view_options_are_stored_and_toggled_from_the_menu() {
        assert_eq!(FindViewOptions::restored(None), FindViewOptions { preview: true, group_by_directory: false });
        let stored = FindViewOptions::restored(Some(r#"{"groupByDirectory":true,"preview":"yes"}"#));
        assert_eq!(stored, FindViewOptions { preview: true, group_by_directory: true });
        let mut options = FindViewOptions::default();
        let write = options.toggle("preview").expect("a known option");
        assert_eq!(write.key, OPTIONS_KEY);
        assert_eq!(FindViewOptions::restored(Some(&write.value)), options);
        assert!(!options.preview);
        assert_eq!(options.toggle("unknown"), None);
        assert_eq!(
            options.header_menu()[3],
            HeaderMenuItem::Check { id: "preview", label: "Preview Source", checked: false }
        );
    }
}
