//! Tab-level editor commands that need no document: what a project change means for the open
//! editors, Open in Opposite Group, the open file paths, and switching image tabs to text tabs
//! when a text editor wrapper claims their files.

use crate::editor::tabs::{EditorInput, OpenTabOptions, SplitDir, TabsState};
use std::collections::BTreeSet;

/// The saved editor session of LightEdit windows: their tabs reopen the next time a window
/// switches to LightEdit.
pub const LIGHT_EDIT_SESSION: &str = "<lightEdit>";

/// The input of a text tab: file tabs are keyed by their path.
pub fn file_input(path: &str) -> EditorInput {
    EditorInput { kind: "file".to_owned(), key: path.to_owned(), path: Some(path.to_owned()), data: None }
}

/// The window's project as the editor sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectState {
    pub root: Option<String>,
    pub light_edit: bool,
    /// A project is being opened in the window.
    pub opening: bool,
}

/// Whether going from `before` to `after` replaces the window's documents: another root, entering
/// or leaving LightEdit, or a project that was (re)opened, even at the same root (`reopened`: the
/// project itself changed while opening). Attaching or detaching a project to the workspace keeps
/// every tab and unsaved document.
pub fn replaces_project(before: &ProjectState, after: &ProjectState, reopened: bool) -> bool {
    before.root != after.root || before.light_edit != after.light_edit || (after.opening && reopened)
}

/// The saved session the window restores after a project change.
pub fn session_key(after: &ProjectState) -> Option<&str> {
    match &after.root {
        Some(root) => Some(root.as_str()),
        None => after.light_edit.then_some(LIGHT_EDIT_SESSION),
    }
}

/// Paths of the open file tabs, all panes in visual order, each once.
pub fn open_file_paths(tabs: &TabsState) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for pane_id in tabs.pane_ids() {
        let Some(pane) = tabs.pane(pane_id) else { continue };
        for tab in &pane.tabs {
            if let Some(path) = tab.input.path.as_deref().filter(|_| tab.input.kind == "file")
                && seen.insert(path)
            {
                out.push(path.to_owned());
            }
        }
    }
    out
}

/// What Open in Opposite Group did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OppositeOpen {
    /// There was no other pane: the tab was activated and its pane split, the new pane showing a
    /// copy of it.
    Split { pane: String },
    /// A copy of the tab opened in the neighbouring pane.
    Opened { pane: String, tab: String },
}

/// IDEA's Open in Opposite Group: the tab stays and a copy opens next door, or in a new split.
pub fn open_in_opposite(tabs: &mut TabsState, pane: &str, tab: &str) -> Option<OppositeOpen> {
    let input = tabs.pane(pane)?.tab(tab)?.input.clone();
    match tabs.neighbour_pane(pane).filter(|other| *other != pane).map(str::to_owned) {
        None => {
            tabs.activate_tab(pane, tab);
            tabs.split_pane(pane, SplitDir::Row).map(|pane| OppositeOpen::Split { pane })
        }
        Some(target) => {
            let opened = tabs.open_tab(&target, input, OpenTabOptions::default())?;
            Some(OppositeOpen::Opened { pane: target, tab: opened })
        }
    }
}

/// A text editor wrapper registered: every image tab whose file it claims (`wraps`) becomes a
/// text tab, unless its pane already shows that file as text. Returns the upgraded tabs that are
/// active in their pane, whose editors the caller loads.
pub fn upgrade_wrapped_image_tabs(tabs: &mut TabsState, wraps: impl Fn(&str) -> bool) -> Vec<String> {
    let mut active = Vec::new();
    for pane in &mut tabs.panes {
        let shown: BTreeSet<String> = pane.tabs.iter().map(|tab| tab.input.key.clone()).collect();
        for tab in &mut pane.tabs {
            let Some(path) = tab.input.path.clone().filter(|path| tab.input.kind == "image" && wraps(path)) else {
                continue;
            };
            let text = file_input(&path);
            if shown.contains(&text.key) {
                continue;
            }
            tab.input = text;
            if pane.active.as_ref() == Some(&tab.id) {
                active.push(tab.id.clone());
            }
        }
    }
    active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_model::guard::OpenOptions;
    use crate::editor_model::removal::CloseScope;
    use crate::editor_model::removal::tests::Ide;

    fn background() -> OpenOptions {
        OpenOptions { focus: false, ..OpenOptions::default() }
    }

    fn paths(tabs: &TabsState, pane: &str) -> Vec<String> {
        let Some(pane) = tabs.pane(pane) else { return Vec::new() };
        pane.tabs.iter().filter_map(|tab| tab.input.path.clone()).collect()
    }

    #[test]
    fn lists_open_file_paths_across_panes() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        ide.open_input(file_input("/p/b.ts"), OpenOptions { side: true, ..background() }).expect("opens");
        ide.open_input(file_input("/p/a.ts"), OpenOptions { side: true, ..background() }).expect("opens");
        assert_eq!(open_file_paths(&ide.tabs), ["/p/a.ts", "/p/b.ts"]);
    }

    #[test]
    fn opens_a_copy_of_a_tab_in_the_opposite_group_splitting_when_there_is_none() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        ide.open_input(file_input("/p/b.ts"), background()).expect("opens");
        let first = ide.active_pane();
        let tab_a = ide.tabs.pane(&first).expect("a pane").tabs[0].id.clone();
        let opened = open_in_opposite(&mut ide.tabs, &first, &tab_a);
        let second = ide.active_pane();
        assert_ne!(second, first);
        assert_eq!(opened, Some(OppositeOpen::Split { pane: second.clone() }));
        assert_eq!(paths(&ide.tabs, &second), ["/p/a.ts"]);
        let tab_b = ide.tabs.pane(&first).expect("a pane").tabs[1].id.clone();
        open_in_opposite(&mut ide.tabs, &first, &tab_b);
        assert_eq!(paths(&ide.tabs, &second), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(paths(&ide.tabs, &first), ["/p/a.ts", "/p/b.ts"]);
    }

    #[test]
    fn closes_read_only_tabs_and_keeps_a_preview_tab_open_from_the_tab_menu() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        ide.open_input(file_input("/p/b.ts"), background()).expect("opens");
        ide.open_input(file_input("/p/c.ts"), OpenOptions { preview: true, ..background() }).expect("opens");
        let pane = ide.active_pane();
        let third = ide.tabs.pane(&pane).expect("a pane").tabs[2].clone();
        assert!(third.preview);
        // Keep Tab Open.
        assert!(ide.tabs.promote_tab(&pane, &third.id));
        assert!(!ide.tabs.pane(&pane).expect("a pane").tabs[2].preview);
        let read_only = BTreeSet::from(["/p/b.ts".to_owned()]);
        assert!(ide.close(CloseScope::ReadOnly { pane: pane.clone(), read_only }));
        assert_eq!(paths(&ide.tabs, &pane), ["/p/a.ts", "/p/c.ts"]);
    }

    #[test]
    fn keeps_editor_tabs_and_unsaved_documents_through_attach_and_detach() {
        let alone = ProjectState { root: Some("/attach-a".into()), light_edit: false, opening: false };
        // The workspace gains and loses a project; the window's root stays.
        assert!(!replaces_project(&alone, &alone, true));
        let other = ProjectState { root: Some("/attach-b".into()), ..alone.clone() };
        assert!(replaces_project(&alone, &other, false));
        let reopening = ProjectState { opening: true, ..alone.clone() };
        assert!(replaces_project(&alone, &reopening, true));
        let light = ProjectState { root: None, light_edit: true, opening: false };
        assert_eq!(session_key(&light), Some(LIGHT_EDIT_SESSION));
        assert_eq!(session_key(&alone), Some("/attach-a"));
    }

    #[test]
    fn closes_a_tab_by_its_input_key() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        ide.open_input(file_input("/p/b.ts"), background()).expect("opens");
        assert!(ide.close(CloseScope::Key(file_input("/p/b.ts").key)));
        assert_eq!(open_file_paths(&ide.tabs), ["/p/a.ts"]);
    }

    #[test]
    fn switches_an_open_image_tab_to_a_text_tab_when_a_matching_text_editor_wrapper_registers() {
        let mut ide = Ide::new();
        let image = EditorInput {
            kind: "image".into(),
            key: "image:/p/arch.diagram.svg".into(),
            path: Some("/p/arch.diagram.svg".into()),
            data: None,
        };
        ide.open_input(image, background()).expect("opens");
        assert!(open_file_paths(&ide.tabs).is_empty());
        let load = upgrade_wrapped_image_tabs(&mut ide.tabs, |path| path.ends_with(".diagram.svg"));
        assert_eq!(open_file_paths(&ide.tabs), ["/p/arch.diagram.svg"]);
        let pane = ide.active_pane();
        let active = ide.tabs.pane(&pane).and_then(|pane| pane.active.clone());
        assert_eq!(load, active.into_iter().collect::<Vec<_>>());
    }
}
