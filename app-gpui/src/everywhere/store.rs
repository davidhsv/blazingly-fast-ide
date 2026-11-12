//! The Search Everywhere dialog's state, and what it remembers between sessions: the preview switch, the
//! funnel's filters, the recently used actions and the last queries of each tab.
//!
//! Nothing here reads or writes storage. What is remembered is stored as JSON under the keys given here:
//! the caller passes the stored text in and saves the [`StorageWrite`] that comes back.

use super::filters::{SeFilters, sanitize_filters};
use super::model::SeTab;
use super::open::{StorageWrite, storage_write, stored_json, utf16_len};
use crate::platform::context::ContextKeys;
use crate::platform::project_roots::ProjectFileSet;
use serde::Serialize;
use serde_json::Value;

/// Key of the code preview switch (`true` or `false`).
pub const PREVIEW_KEY: &str = "openidea.search.everywherePreview.v1";
/// Key of the per-tab filters ([`SeFilters`]).
pub const FILTERS_KEY: &str = "openidea.search.everywhereFilters.v1";
/// Key of the recently used action ids, most recent first.
pub const RECENT_ACTIONS_KEY: &str = "openidea.search.recentActions.v1";
/// Key of the search history: `{ tab, query }` entries, newest first.
pub const HISTORY_KEY: &str = "openidea.search.everywhereHistory.v1";

/// Most recently used actions remembered.
pub const MAX_RECENT_ACTIONS: usize = 12;
/// Most history entries kept, over all tabs.
pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct SearchEverywhereState {
    pub open: bool,
    pub tab: SeTab,
    pub query: String,
    pub include_non_project: bool,
    /// Scope of the Classes, Files, Symbols and Text tabs when not "All Places".
    pub file_set: ProjectFileSet,
    /// Context keys captured when the dialog opened: whether an action can run is judged against the
    /// editor, not the popup.
    pub ctx: ContextKeys,
    /// Bumped on every opening, so the field selects its text again.
    pub session: u64,
    /// Code preview of the selected result under the list (remembered).
    pub preview: bool,
    /// IDEA's automatic "everywhere": while the user has not picked a scope, a query with no project
    /// results widens to non-project items; `not_found` is that query.
    pub auto_everywhere: bool,
    pub not_found: Option<String>,
    /// The funnel's per-tab filters (remembered).
    pub filters: SeFilters,
}

impl SearchEverywhereState {
    /// The closed dialog with what is remembered: the values stored under [`PREVIEW_KEY`] and
    /// [`FILTERS_KEY`].
    pub fn new(stored_preview: Option<&str>, stored_filters: Option<&str>) -> Self {
        Self {
            open: false,
            tab: SeTab::All,
            query: String::new(),
            include_non_project: false,
            file_set: ProjectFileSet::All,
            ctx: ContextKeys::new(),
            session: 0,
            preview: stored_json(stored_preview) == Some(Value::Bool(true)),
            auto_everywhere: true,
            not_found: None,
            filters: stored_json(stored_filters).map_or_else(SeFilters::default, |raw| sanitize_filters(&raw)),
        }
    }

    pub fn set_preview(&mut self, preview: bool) -> StorageWrite {
        self.preview = preview;
        storage_write(PREVIEW_KEY, &preview)
    }

    /// Changes the filters (one tab's, usually) and returns the write that remembers them.
    pub fn update_filters(&mut self, change: impl FnOnce(&mut SeFilters)) -> StorageWrite {
        change(&mut self.filters);
        storage_write(FILTERS_KEY, &self.filters)
    }

    /// Opens the dialog on `tab`, or while it is open switches to `tab` (the query is kept); invoking the
    /// shown tab again toggles "include non-project items", like IDEA. Without `query` (an editor
    /// selection) the field starts with the tab's last search from `stored_history` (the value under
    /// [`HISTORY_KEY`]), so each tab remembers its own text. `ctx` are the context keys at this moment.
    pub fn show(&mut self, tab: SeTab, query: Option<&str>, stored_history: Option<&str>, mut ctx: ContextKeys) {
        if self.open {
            if self.tab == tab {
                self.set_include_non_project(!self.include_non_project);
            } else {
                self.select_tab(tab);
            }
            return;
        }
        ctx.set("popupOpen", false);
        self.open = true;
        self.tab = tab;
        self.query = match query {
            Some(query) => query.to_owned(),
            None => search_history(stored_history, tab).into_iter().next().unwrap_or_default(),
        };
        self.include_non_project = false;
        self.auto_everywhere = true;
        self.not_found = None;
        self.ctx = ctx;
        self.session += 1;
    }

    /// The user's choice of scope: stops widening automatically.
    pub fn set_include_non_project(&mut self, include_non_project: bool) {
        self.include_non_project = include_non_project;
        self.auto_everywhere = false;
        self.not_found = None;
    }

    /// Switching tabs drops an automatically widened scope.
    pub fn select_tab(&mut self, tab: SeTab) {
        if self.auto_everywhere && self.not_found.is_some() {
            self.include_non_project = false;
            self.not_found = None;
        }
        self.tab = tab;
    }

    /// Nothing in the project for `query`: search everywhere instead, as IDEA does.
    pub fn widen(&mut self, query: &str) {
        if !self.auto_everywhere || self.include_non_project || query.trim().is_empty() {
            return;
        }
        self.include_non_project = true;
        self.not_found = Some(query.to_owned());
    }

    /// A new query that no longer contains the not-found one narrows back to the project.
    pub fn on_query(&mut self, query: &str) {
        let Some(not_found) = &self.not_found else { return };
        if !self.auto_everywhere || !self.include_non_project {
            return;
        }
        if utf16_len(not_found) > 1 && query.contains(not_found.as_str()) {
            return;
        }
        self.include_non_project = false;
        self.not_found = None;
    }

    /// Closes the dialog. The text is remembered for the shown tab, also when nothing was opened: the
    /// write to save, given the value now under [`HISTORY_KEY`], is returned unless the text is blank.
    pub fn hide(&mut self, stored_history: Option<&str>) -> Option<StorageWrite> {
        let write = if self.open { push_search_history(stored_history, self.tab, &self.query) } else { None };
        self.open = false;
        write
    }
}

/// The strings of a stored list; anything else in it is dropped.
fn stored_strings(stored: Option<&str>) -> Vec<String> {
    let Some(Value::Array(items)) = stored_json(stored) else { return Vec::new() };
    items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()
}

/// Ids of the recently used actions, most recent first, from the value under [`RECENT_ACTIONS_KEY`].
pub fn recent_actions(stored: Option<&str>) -> Vec<String> {
    stored_strings(stored)
}

/// Puts `id` in front of the recently used actions; returns the write that remembers the list.
pub fn push_recent_action(stored: Option<&str>, id: &str) -> StorageWrite {
    let mut recent = recent_actions(stored);
    recent.retain(|other| other != id);
    recent.insert(0, id.to_owned());
    recent.truncate(MAX_RECENT_ACTIONS);
    storage_write(RECENT_ACTIONS_KEY, &recent)
}

/// A query typed in a tab. The tab is kept as its id, so entries of tabs this version does not know
/// survive a rewrite of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    pub tab: String,
    pub query: String,
}

/// The search history from the value under [`HISTORY_KEY`], newest first. Entries without a text tab
/// and a text query are dropped.
pub fn parse_history(stored: Option<&str>) -> Vec<HistoryEntry> {
    let Some(Value::Array(items)) = stored_json(stored) else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let tab = item.get("tab")?.as_str()?;
            let query = item.get("query")?.as_str()?;
            Some(HistoryEntry { tab: tab.to_owned(), query: query.to_owned() })
        })
        .collect()
}

/// Queries typed in `tab` (opened or left there), newest first.
pub fn search_history(stored: Option<&str>, tab: SeTab) -> Vec<String> {
    parse_history(stored).into_iter().filter(|entry| entry.tab == tab.id()).map(|entry| entry.query).collect()
}

/// Puts `query` in front of the history as typed in `tab`; `None` when it is blank and nothing changes.
pub fn push_search_history(stored: Option<&str>, tab: SeTab, query: &str) -> Option<StorageWrite> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let mut history = parse_history(stored);
    history.retain(|entry| entry.tab != tab.id() || entry.query != query);
    history.insert(0, HistoryEntry { tab: tab.id().to_owned(), query: query.to_owned() });
    history.truncate(MAX_HISTORY);
    Some(storage_write(HISTORY_KEY, &history))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// The dialog with the storage its writes are saved to.
    struct Harness {
        state: SearchEverywhereState,
        storage: BTreeMap<String, String>,
    }

    impl Harness {
        fn new() -> Self {
            Self { state: SearchEverywhereState::new(None, None), storage: BTreeMap::new() }
        }

        fn stored(&self, key: &str) -> Option<&str> {
            self.storage.get(key).map(String::as_str)
        }

        fn save(&mut self, write: StorageWrite) {
            self.storage.insert(write.key, write.value);
        }

        fn show(&mut self, tab: SeTab, query: Option<&str>) {
            let history = self.storage.get(HISTORY_KEY).map(String::as_str);
            self.state.show(tab, query, history, ContextKeys::new());
        }

        fn hide(&mut self) {
            let history = self.storage.get(HISTORY_KEY).map(String::as_str);
            if let Some(write) = self.state.hide(history) {
                self.save(write);
            }
        }
    }

    #[test]
    fn each_tab_starts_with_its_own_last_query_closing_remembers_the_text() {
        let mut dialog = Harness::new();
        dialog.show(SeTab::Files, None);
        dialog.state.query = "Order".to_owned();
        dialog.hide();
        dialog.show(SeTab::Classes, None);
        assert_eq!(dialog.state.query, "");
        dialog.state.query = "Pizza".to_owned();
        dialog.state.select_tab(SeTab::Symbols);
        dialog.hide();
        dialog.show(SeTab::Files, None);
        assert_eq!(dialog.state.query, "Order");
        dialog.hide();
        dialog.show(SeTab::Symbols, Some("sel"));
        assert_eq!(dialog.state.query, "sel");
        dialog.hide();
        dialog.show(SeTab::Classes, None);
        assert_eq!(dialog.state.query, "");
        dialog.hide();
    }

    // Not from a spec: opening captures the context with the popup closed and counts the session; invoking
    // the shown tab again toggles the scope, another tab switches to it.
    #[test]
    fn showing_while_open_toggles_the_scope_or_switches_the_tab() {
        let mut dialog = Harness::new();
        let mut ctx = ContextKeys::new();
        ctx.set("popupOpen", true);
        ctx.set("editorFocus", true);
        dialog.state.show(SeTab::Files, None, None, ctx);
        assert!(dialog.state.open);
        assert_eq!(dialog.state.session, 1);
        assert!(!dialog.state.ctx.is_truthy("popupOpen"));
        assert!(dialog.state.ctx.is_truthy("editorFocus"));
        dialog.state.query = "kept".to_owned();
        dialog.show(SeTab::Files, None);
        assert!(dialog.state.include_non_project);
        assert!(!dialog.state.auto_everywhere);
        dialog.show(SeTab::Text, Some("ignored"));
        assert_eq!((dialog.state.tab, dialog.state.query.as_str(), dialog.state.session), (SeTab::Text, "kept", 1));
    }

    // Not from a spec: a query with no project results widens the scope until the query changes or the tab
    // is switched; a scope the user picked is left alone.
    #[test]
    fn widens_automatically_and_narrows_back() {
        let mut dialog = Harness::new();
        dialog.show(SeTab::Classes, None);
        dialog.state.widen("  ");
        assert!(!dialog.state.include_non_project);
        dialog.state.widen("Strin");
        assert!(dialog.state.include_non_project);
        assert_eq!(dialog.state.not_found.as_deref(), Some("Strin"));
        dialog.state.on_query("String");
        assert!(dialog.state.include_non_project);
        dialog.state.on_query("Str");
        assert!(!dialog.state.include_non_project);
        assert_eq!(dialog.state.not_found, None);
        dialog.state.widen("x");
        dialog.state.on_query("xy");
        assert!(!dialog.state.include_non_project);
        dialog.state.widen("Strin");
        dialog.state.select_tab(SeTab::Symbols);
        assert!(!dialog.state.include_non_project);
        dialog.state.set_include_non_project(true);
        dialog.state.widen("Other");
        dialog.state.on_query("zzz");
        dialog.state.select_tab(SeTab::Files);
        assert!(dialog.state.include_non_project);
        assert_eq!(dialog.state.not_found, None);
    }

    // Not from a spec: the preview switch and the filters are remembered under their keys.
    #[test]
    fn preview_and_filters_are_remembered() {
        let mut dialog = Harness::new();
        assert!(!dialog.state.preview);
        let write = dialog.state.set_preview(true);
        dialog.save(write);
        let write = dialog.state.update_filters(|filters| filters.text.mask = "*.ts".to_owned());
        dialog.save(write);
        assert_eq!(dialog.stored(PREVIEW_KEY), Some("true"));
        let reopened = SearchEverywhereState::new(dialog.stored(PREVIEW_KEY), dialog.stored(FILTERS_KEY));
        assert!(reopened.preview);
        assert_eq!(reopened.filters, dialog.state.filters);
        assert_eq!(reopened.filters.text.mask, "*.ts");
        let corrupt = SearchEverywhereState::new(Some("1"), Some("{oops"));
        assert!(!corrupt.preview);
        assert_eq!(corrupt.filters, SeFilters::default());
    }

    // Not from a spec: recent actions are unique, most recent first and capped; malformed entries are
    // dropped.
    #[test]
    fn recent_actions_are_unique_and_capped() {
        let mut stored: Option<String> = Some(r#"["a", 3, "b"]"#.to_owned());
        assert_eq!(recent_actions(stored.as_deref()), ["a", "b"]);
        stored = Some(push_recent_action(stored.as_deref(), "b").value);
        assert_eq!(recent_actions(stored.as_deref()), ["b", "a"]);
        for n in 0..MAX_RECENT_ACTIONS + 3 {
            let write = push_recent_action(stored.as_deref(), &format!("action.{n}"));
            assert_eq!(write.key, RECENT_ACTIONS_KEY);
            stored = Some(write.value);
        }
        let recent = recent_actions(stored.as_deref());
        assert_eq!(recent.len(), MAX_RECENT_ACTIONS);
        assert_eq!(recent[0], format!("action.{}", MAX_RECENT_ACTIONS + 2));
        assert!(recent_actions(Some("{}")).is_empty());
    }

    // Not from a spec: the history keeps its stored shape, entries of unknown tabs and the newest entry
    // first; blank text is not remembered.
    #[test]
    fn history_keeps_its_shape_and_unknown_tabs() {
        let entries = json!([{ "tab": "endpoints", "query": "/users" }, { "tab": "files", "query": 3 }, "x"]);
        let stored = entries.to_string();
        assert_eq!(push_search_history(Some(&stored), SeTab::Files, "  "), None);
        let write = push_search_history(Some(&stored), SeTab::Files, " Order ").unwrap();
        assert_eq!(write.key, HISTORY_KEY);
        let saved: Value = serde_json::from_str(&write.value).unwrap();
        let expected = json!([{ "tab": "files", "query": "Order" }, { "tab": "endpoints", "query": "/users" }]);
        assert_eq!(saved, expected);
        let again = push_search_history(Some(&write.value), SeTab::Files, "Order").unwrap();
        assert_eq!(again.value, write.value);
        assert_eq!(search_history(Some(&write.value), SeTab::Files), ["Order"]);
        assert!(search_history(Some(&write.value), SeTab::Classes).is_empty());
    }
}
