//! Untitled buffers (File › New › Untitled File): editable documents the backend keeps under an
//! `untitled:Untitled-N` key until Save As gives them a file. A tab shows one with an input of its
//! own kind, so everything that only means files (the session, Open Files, copying a path) leaves
//! it out; Save As turns every tab of it into the file's tab.

use crate::editor::tabs::{EditorInput, TabsState};
use std::path::{Path, PathBuf};

/// The editor kind of untitled buffers.
pub const UNTITLED_KIND: &str = "untitled";

/// The scheme of an untitled buffer's key.
pub const UNTITLED_SCHEME: &str = "untitled:";

/// Whether `path` is an untitled buffer's key (`untitled:Untitled-1`).
pub fn is_untitled_path(path: &str) -> bool {
    path.starts_with(UNTITLED_SCHEME)
}

/// The name an untitled buffer is shown under (`Untitled-1`); any other path unchanged.
pub fn untitled_name(path: &str) -> &str {
    path.strip_prefix(UNTITLED_SCHEME).unwrap_or(path)
}

/// What a tab, the window title and the breadcrumbs call `path`: an untitled buffer's name, or a
/// file's name.
pub fn display_name(path: &str) -> &str {
    let path = untitled_name(path);
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The tab input of the untitled buffer `key`.
pub fn untitled_input(key: &str) -> EditorInput {
    EditorInput { kind: UNTITLED_KIND.to_owned(), key: key.to_owned(), path: Some(key.to_owned()), data: None }
}

/// Whether the text editor shows tabs of `kind`: files and untitled buffers.
pub fn is_text_kind(kind: &str) -> bool {
    kind == "file" || kind == UNTITLED_KIND
}

/// Save As turned the untitled buffer `old_key` into the file `path`: every tab of it, in every
/// pane, becomes that file's tab. Returns whether a tab changed.
pub fn retarget_tabs(tabs: &mut TabsState, old_key: &str, path: &str) -> bool {
    let mut changed = false;
    for tab in tabs.panes.iter_mut().flat_map(|pane| pane.tabs.iter_mut()) {
        if tab.input.key == old_key {
            tab.input =
                EditorInput { kind: "file".to_owned(), key: path.to_owned(), path: Some(path.to_owned()), data: None };
            changed = true;
        }
    }
    changed
}

/// Where Save As opens for the untitled buffer `key`, and the name it suggests (without an
/// extension: the file type is the user's to pick). The folder is the project's root; without a
/// project, the folder of the most recently used file tab of the first pane that has one; else
/// `home`, since the system's save dialog needs a folder to open in.
pub fn save_as_start(root: Option<&Path>, tabs: &TabsState, home: &Path, key: &str) -> (PathBuf, String) {
    let name = untitled_name(key).to_owned();
    if let Some(root) = root {
        return (root.to_path_buf(), name);
    }
    let recent_file = tabs.panes.iter().find_map(|pane| {
        pane.mru.iter().find_map(|id| {
            let tab = pane.tab(id)?;
            let path = tab.input.path.as_deref().filter(|_| tab.input.kind == "file")?;
            Path::new(path).parent().filter(|dir| !dir.as_os_str().is_empty()).map(Path::to_path_buf)
        })
    });
    (recent_file.unwrap_or_else(|| home.to_path_buf()), name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tabs::OpenTabOptions;

    fn file(path: &str) -> EditorInput {
        EditorInput { kind: "file".to_owned(), key: path.to_owned(), path: Some(path.to_owned()), data: None }
    }

    #[test]
    fn an_untitled_key_is_shown_by_its_name_and_a_file_by_its_file_name() {
        assert!(is_untitled_path("untitled:Untitled-1"));
        assert!(!is_untitled_path("/p/untitled.txt"));
        assert_eq!(untitled_name("untitled:Untitled-3"), "Untitled-3");
        assert_eq!(display_name("untitled:Untitled-3"), "Untitled-3");
        assert_eq!(display_name("/p/src/main.rs"), "main.rs");
        assert_eq!(display_name("C:\\p\\main.rs"), "main.rs");
        let input = untitled_input("untitled:Untitled-9");
        assert_eq!((input.kind.as_str(), input.key.as_str()), (UNTITLED_KIND, "untitled:Untitled-9"));
        assert_eq!(input.path.as_deref(), Some("untitled:Untitled-9"));
        assert!(is_text_kind("file") && is_text_kind(UNTITLED_KIND) && !is_text_kind("image"));
    }

    #[test]
    fn save_as_turns_every_tab_of_the_buffer_into_the_files_tab() {
        let mut tabs = TabsState::new();
        let first = tabs.active_pane.clone();
        tabs.open_tab(&first, untitled_input("untitled:Untitled-1"), OpenTabOptions::default());
        tabs.open_tab(&first, file("/p/a.rs"), OpenTabOptions::default());
        let second = tabs.split_pane(&first, crate::editor::tabs::SplitDir::Row).expect("a second pane");
        tabs.open_tab(&second, untitled_input("untitled:Untitled-1"), OpenTabOptions::default());

        assert!(retarget_tabs(&mut tabs, "untitled:Untitled-1", "/p/new/hello.rs"));
        let inputs: Vec<&EditorInput> = tabs.panes.iter().flat_map(|pane| &pane.tabs).map(|tab| &tab.input).collect();
        let saved = inputs.iter().filter(|input| input.key == "/p/new/hello.rs").count();
        assert_eq!(saved, 2, "{inputs:?}");
        assert!(inputs.iter().all(|input| input.kind == "file"));
        assert!(inputs.iter().any(|input| input.key == "/p/a.rs"), "other tabs are left alone");
        assert!(!retarget_tabs(&mut tabs, "untitled:Untitled-1", "/p/other.rs"));
    }

    #[test]
    fn save_as_opens_in_the_project_else_beside_the_last_used_file_else_at_home() {
        let home = Path::new("/home/me");
        let mut tabs = TabsState::new();
        let pane = tabs.active_pane.clone();
        let key = "untitled:Untitled-2";
        assert_eq!(save_as_start(None, &tabs, home, key), (home.to_path_buf(), "Untitled-2".to_owned()));
        tabs.open_tab(&pane, file("/a/one.txt"), OpenTabOptions::default());
        tabs.open_tab(&pane, file("/b/two.txt"), OpenTabOptions::default());
        tabs.open_tab(&pane, untitled_input(key), OpenTabOptions::default());
        assert_eq!(save_as_start(None, &tabs, home, key).0, PathBuf::from("/b"), "the most recent file tab");
        let root = Path::new("/project");
        assert_eq!(save_as_start(Some(root), &tabs, home, key), (root.to_path_buf(), "Untitled-2".to_owned()));
    }
}
