//! What Search Everywhere keeps beyond one query: the scope of its tab (project items only, or everything,
//! widened by itself when the project has nothing), the queries typed in each tab and the actions run
//! from it. The queries and actions are stored under the keys of [`crate::everywhere::store`], so the
//! next popup starts with the tab's last query and ranks recently run actions first.

use super::{SearchEverywhere, SearchEverywhereEvent};
use crate::everywhere::model::{SeItem, SeRow, SeTab};
use crate::everywhere::open::StorageWrite;
use crate::everywhere::store::{
    HISTORY_KEY, RECENT_ACTIONS_KEY, SearchEverywhereState, push_recent_action, recent_actions, search_history,
};
use crate::platform::context::ContextKeys;
use crate::platform::storage::UiStorage;
use gpui_kit::{App, Context};
use serde_json::Value;

/// The action that records a shortcut for the action given as its argument.
pub(super) const ASSIGN_SHORTCUT: &str = "settings.assignShortcut";
/// The action that opens Find in Files, given `{ "query": … }`.
pub(super) const FIND_IN_PATH: &str = "find.inPath";

/// The popup's scope and its walk through the search history.
pub(super) struct SeMemory {
    /// Only the scope fields (`include_non_project`, `auto_everywhere`, `not_found`) and, when the popup
    /// closes, the tab and the query are read: the popup keeps its own tab and query while it is open.
    pub(super) state: SearchEverywhereState,
    /// Position in the tab's history while ⌥↑ and ⌥↓ walk it; `None` while the field holds what was typed.
    pub(super) history: Option<usize>,
    /// What was typed before the walk started, given back when ⌥↓ walks past the newest entry.
    draft: String,
}

impl SeMemory {
    /// The scope of a popup opening on `tab` with `query`: project items only, widened when nothing is
    /// found.
    pub(super) fn new(tab: SeTab, query: &str) -> Self {
        let mut state = SearchEverywhereState::new(None, None);
        state.show(tab, Some(query), None, ContextKeys::new());
        Self { state, history: None, draft: String::new() }
    }
}

/// The text stored under `key`, when the app keeps a store.
fn stored(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned)
}

fn save(write: StorageWrite, cx: &mut App) {
    if cx.has_global::<UiStorage>() {
        cx.global_mut::<UiStorage>().set(&write.key, write.value);
    }
}

/// The last query typed in `tab`: what a popup opened there without a selection starts with.
pub(super) fn last_query(tab: SeTab, cx: &App) -> String {
    search_history(stored(cx, HISTORY_KEY).as_deref(), tab).into_iter().next().unwrap_or_default()
}

/// The ids of the actions run from the popup, most recent first.
pub(super) fn recent_action_ids(cx: &App) -> Vec<String> {
    recent_actions(stored(cx, RECENT_ACTIONS_KEY).as_deref())
}

/// Puts `id` in front of the actions run from the popup.
pub(super) fn remember_action(id: &str, cx: &mut App) {
    let write = push_recent_action(stored(cx, RECENT_ACTIONS_KEY).as_deref(), id);
    save(write, cx);
}

impl SearchEverywhere {
    /// Whether the tab lists what is outside the project too (on Actions: the actions that cannot run here).
    pub fn include_non_project(&self) -> bool {
        self.memory.state.include_non_project
    }

    /// The query the popup widened its scope for, because the project had nothing for it.
    pub fn not_found(&self) -> Option<&str> {
        self.memory.state.not_found.as_deref()
    }

    /// The popup's action asked for again: on the shown tab it switches the non-project items (the
    /// disabled actions on Actions) on or off; on another tab it moves there and keeps the query.
    pub fn show_again(&mut self, tab: SeTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            let include = !self.memory.state.include_non_project;
            self.set_include_non_project(include, cx);
        } else {
            self.select_tab(tab, cx);
        }
    }

    /// The user's choice of scope: the popup no longer widens it by itself.
    pub fn set_include_non_project(&mut self, include: bool, cx: &mut Context<Self>) {
        self.memory.state.set_include_non_project(include);
        self.extra.clear();
        self.reset_selection();
        self.run(cx);
    }

    /// The text of the field changed: the walk through the history ends there, and a query that no longer
    /// contains the one the scope was widened for narrows it back to the project.
    pub(super) fn note_query(&mut self, cx: &mut Context<Self>) {
        let typed = self.field.read(cx).text().to_owned();
        let widened = self.memory.state.include_non_project;
        self.memory.state.on_query(&typed);
        self.memory.state.query = typed;
        if widened != self.memory.state.include_non_project {
            self.extra.clear();
        }
    }

    /// ⌥↑ (`older`) and ⌥↓: the tab's earlier queries, newest first, in the field; ⌥↓ past the newest gives
    /// back what was typed before the walk started. The walk stops at either end.
    pub(super) fn browse_history(&mut self, older: bool, cx: &mut Context<Self>) {
        let history = search_history(stored(cx, HISTORY_KEY).as_deref(), self.tab);
        let next = if older {
            let next = self.memory.history.map_or(0, |at| at + 1);
            if next >= history.len() {
                return;
            }
            Some(next)
        } else {
            match self.memory.history {
                None => return,
                Some(0) => None,
                Some(at) => Some(at - 1),
            }
        };
        if self.memory.history.is_none() {
            self.memory.draft = self.field.read(cx).text().to_owned();
        }
        self.memory.history = next;
        let text = next.map_or_else(|| self.memory.draft.clone(), |at| history[at].clone());
        self.field.update(cx, |field, cx| field.set_text(&text, cx));
        self.requery(cx);
    }

    /// The popup goes away: the query is remembered for the shown tab, whether or not anything was opened.
    pub(super) fn remember_query(&mut self, cx: &mut App) {
        self.memory.state.tab = self.tab;
        self.memory.state.query = self.field.read(cx).text().to_owned();
        if let Some(write) = self.memory.state.hide(stored(cx, HISTORY_KEY).as_deref()) {
            save(write, cx);
        }
    }

    /// Nothing in the project for the query: once its answer is final the popup searches everywhere
    /// instead, unless the user chose the scope.
    pub(super) fn widen_when_nothing_found(&mut self, cx: &mut Context<Self>) {
        let nothing = self.rows.is_empty()
            && !self.controller.is_loading()
            && !self.memory.state.include_non_project
            && !self.query_text().trim().is_empty()
            && self.listing(cx).is_none()
            && self.settled(cx);
        if !nothing {
            return;
        }
        let searched = self.search_query().to_owned();
        self.memory.state.widen(&searched);
        if self.memory.state.include_non_project {
            self.extra.clear();
            // After this refresh: the rows being built are those of the narrower search.
            let popup = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = popup.update(cx, |popup, cx| popup.run(cx));
            });
        }
    }

    /// ⌥↵: on an action, records a shortcut for it; the popup closes. Other rows have no such menu here.
    pub(super) fn assign_shortcut(&mut self, cx: &mut Context<Self>) {
        let Some(SeRow::Item { item: SeItem::Action(action), .. }) = self.active.and_then(|index| self.rows.get(index))
        else {
            return;
        };
        let arg = Value::String(action.id.clone());
        cx.emit(SearchEverywhereEvent::RunWith { id: ASSIGN_SHORTCUT.to_owned(), arg });
    }

    /// ⌘↵ on Text: the query goes to Find in Files, which lists every match.
    pub(super) fn find_in_files(&mut self, cx: &mut Context<Self>) {
        let arg = serde_json::json!({ "query": self.query_text() });
        cx.emit(SearchEverywhereEvent::RunWith { id: FIND_IN_PATH.to_owned(), arg });
    }
}
