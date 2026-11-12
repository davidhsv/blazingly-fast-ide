//! Documents of custom editor kinds (notebooks and other editors that own their document): one
//! per input key, however many tabs and splits show it. The editor keeps such a document alive
//! while any tab shows its key and closes it after the last one goes.

use crate::editor::tabs::{EditorInput, TabsState};
use std::collections::BTreeSet;

/// The editor kinds whose inputs carry a document lifecycle (save, discard, prepare/cancel
/// close, release).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentKinds(BTreeSet<String>);

impl DocumentKinds {
    pub fn new<S: Into<String>>(kinds: impl IntoIterator<Item = S>) -> Self {
        Self(kinds.into_iter().map(Into::into).collect())
    }

    pub fn register(&mut self, kind: &str) {
        self.0.insert(kind.to_owned());
    }

    pub fn unregister(&mut self, kind: &str) {
        self.0.remove(kind);
    }

    /// Whether `input` is a document of a custom editor kind.
    pub fn has_lifecycle(&self, input: &EditorInput) -> bool {
        self.0.contains(&input.kind)
    }
}

/// Open custom documents, one per input key, in visual tab order (the first tab's input).
pub fn custom_documents(tabs: &TabsState, kinds: &DocumentKinds) -> Vec<EditorInput> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for pane_id in tabs.pane_ids() {
        let Some(pane) = tabs.pane(pane_id) else { continue };
        for tab in &pane.tabs {
            if kinds.has_lifecycle(&tab.input) && seen.insert(tab.input.key.as_str()) {
                out.push(tab.input.clone());
            }
        }
    }
    out
}

/// Custom documents open in `before` that no tab of `after` shows any more.
pub fn closed_documents(before: &TabsState, after: &TabsState, kinds: &DocumentKinds) -> Vec<EditorInput> {
    let open = after.open_keys();
    custom_documents(before, kinds).into_iter().filter(|input| !open.contains(input.key.as_str())).collect()
}

/// Inputs of every kind that `before` showed and no tab of `after` shows: their editor kind's
/// close notification runs once, when the last split showing them goes (not when the project is
/// replaced, whose decision was made already).
pub fn closed_inputs(before: &TabsState, after: &TabsState) -> Vec<EditorInput> {
    let open = after.open_keys();
    let mut seen = BTreeSet::new();
    before
        .panes
        .iter()
        .flat_map(|pane| &pane.tabs)
        .filter(|tab| !open.contains(tab.input.key.as_str()) && seen.insert(tab.input.key.as_str()))
        .map(|tab| tab.input.clone())
        .collect()
}

/// Whether `input` is a custom document with unsaved changes; `None` when it is no custom
/// document. `dirty` answers for the document; a check that fails must answer `true`, so
/// nothing is dropped on its word.
pub fn input_dirty(input: &EditorInput, kinds: &DocumentKinds, dirty: &dyn Fn(&EditorInput) -> bool) -> Option<bool> {
    kinds.has_lifecycle(input).then(|| dirty(input))
}

/// The failures of several document operations that each ran to the end: reported as one, under
/// the first message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFailures(pub Vec<String>);

impl DocumentFailures {
    /// The failures of `results`; `Ok` when none failed.
    pub fn collect(results: impl IntoIterator<Item = Result<(), String>>) -> Result<(), DocumentFailures> {
        let errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
        if errors.is_empty() { Ok(()) } else { Err(DocumentFailures(errors)) }
    }

    /// The message shown for all of them: the first one's.
    pub fn message(&self) -> &str {
        self.0.first().map_or("", String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tabs::{OpenTabOptions, SplitDir};

    fn input(kind: &str, key: &str) -> EditorInput {
        EditorInput { kind: kind.to_owned(), key: key.to_owned(), path: None, data: None }
    }

    #[test]
    fn lists_each_custom_document_once_across_splits_and_finds_the_closed_ones() {
        let kinds = DocumentKinds::new(["notebook"]);
        let mut tabs = TabsState::new();
        let pane = tabs.active_pane.clone();
        tabs.open_tab(&pane, input("notebook", "a"), OpenTabOptions::default());
        tabs.open_tab(&pane, input("file", "f"), OpenTabOptions::default());
        tabs.open_tab(&pane, input("notebook", "b"), OpenTabOptions::default());
        let split = tabs.split_pane(&pane, SplitDir::Row);
        assert!(split.is_some());
        let keys: Vec<String> = custom_documents(&tabs, &kinds).into_iter().map(|input| input.key).collect();
        assert_eq!(keys, ["a", "b"]);
        let mut after = tabs.clone();
        after.close_key("a");
        let closed: Vec<String> = closed_documents(&tabs, &after, &kinds).into_iter().map(|input| input.key).collect();
        assert_eq!(closed, ["a"]);
        assert_eq!(DocumentFailures::collect([Ok(()), Err("x".into()), Err("y".into())]).unwrap_err().message(), "x");
        assert_eq!(input_dirty(&input("file", "f"), &kinds, &|_| true), None);
    }

    #[test]
    fn notifies_once_when_the_last_split_showing_an_input_closes() {
        let mut tabs = TabsState::new();
        let pane = tabs.active_pane.clone();
        let probe = input("close-probe", "close-probe:1");
        tabs.open_tab(&pane, probe.clone(), OpenTabOptions::default());
        tabs.split_pane(&pane, SplitDir::Row);
        assert_eq!(tabs.panes.len(), 2);
        let mut one_split = tabs.clone();
        let first = one_split.panes[0].clone();
        one_split.close_tab(&first.id, &first.tabs[0].id);
        assert!(closed_inputs(&tabs, &one_split).is_empty());
        let mut none = one_split.clone();
        none.close_key(&probe.key);
        assert_eq!(closed_inputs(&one_split, &none), [probe]);
    }
}
