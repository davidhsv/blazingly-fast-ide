//! The pane the Project view's tree shows ("Project ▾" in the tool window header): the whole
//! project, or just the files open in editor tabs or with local changes.
//!
//! "Open Files" and "All Changed Files" list their files below the project, through their
//! folders, which open the first time they appear; a folder closed after that stays closed. The
//! pane is remembered across sessions under [`MODE_KEY`]; only the "Project" pane's expansion is.

use crate::platform::storage::UiStorage;
use crate::project_tree::ViewOptions;
use crate::project_tree_model::{ExtraRoot, Listings, TreeModel, build_model, build_paths_model};
use crate::project_view::store::{MODE_KEY, ViewMode};
use ide_core::ignored::IgnoredNames;
use std::collections::BTreeSet;

/// What the tree shows of the project, and the files the filtered panes list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreePane {
    mode: ViewMode,
    /// Files open in editor tabs, for "Open Files".
    open_files: Vec<String>,
    /// Files with local changes, for "All Changed Files".
    changed_files: Vec<String>,
    /// Folders of a filtered pane opened once already: collapsing them sticks.
    auto_expanded: BTreeSet<String>,
}

/// Whether the tree can show the pane `mode`: Unity projects and scratch files have no pane of
/// their own here, so their panes show as "Project".
fn shown(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::Unity | ViewMode::Scratches => ViewMode::Project,
        mode => mode,
    }
}

impl TreePane {
    /// The pane remembered in `storage`; "Project" without one.
    pub fn stored(storage: Option<&UiStorage>) -> Self {
        let mode = ViewMode::stored(storage.and_then(|storage| storage.get(MODE_KEY)));
        Self { mode: shown(mode), ..Self::default() }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    pub fn open_files(&self) -> &[String] {
        &self.open_files
    }

    pub fn changed_files(&self) -> &[String] {
        &self.changed_files
    }

    /// Switches to `mode`. False when it is the pane shown already.
    pub fn set_mode(&mut self, mode: ViewMode) -> bool {
        let mode = shown(mode);
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.auto_expanded.clear();
        true
    }

    /// The files open in editor tabs are now `paths`. True when the pane shown lists them.
    pub fn set_open_files(&mut self, paths: &[String]) -> bool {
        if self.open_files == paths {
            return false;
        }
        paths.clone_into(&mut self.open_files);
        self.mode == ViewMode::OpenFiles
    }

    /// The files with local changes are now `paths`. True when the pane shown lists them.
    pub fn set_changed_files(&mut self, paths: &[String]) -> bool {
        if self.changed_files == paths {
            return false;
        }
        paths.clone_into(&mut self.changed_files);
        self.mode == ViewMode::ChangedFiles
    }

    /// The files the pane lists, when it lists just some.
    fn files(&self) -> Option<&[String]> {
        match self.mode {
            ViewMode::OpenFiles => Some(&self.open_files),
            ViewMode::ChangedFiles => Some(&self.changed_files),
            _ => None,
        }
    }

    /// Whether the expansion shown is the one remembered for the project.
    pub fn keeps_stored_expansion(&self) -> bool {
        self.mode == ViewMode::Project
    }

    /// The tree of the pane over the folders loaded so far, for the project at `root` named `name`.
    pub fn model(
        &self,
        root: &str,
        name: &str,
        listings: &Listings,
        options: &ViewOptions,
        ignored: &IgnoredNames,
    ) -> TreeModel {
        match self.files() {
            Some(files) => {
                let project = ExtraRoot { path: Some(root.to_owned()), label: name.to_owned(), ..ExtraRoot::default() };
                build_paths_model(std::slice::from_ref(&project), files, options.folders_first)
            }
            None => build_model(root, name, listings, &options.tree_options(), ignored, None),
        }
    }

    /// Opens the folders of `model` that a filtered pane shows for the first time. True when
    /// `expanded` changed.
    pub fn auto_expand(&mut self, model: &TreeModel, expanded: &mut BTreeSet<String>) -> bool {
        if self.files().is_none() {
            return false;
        }
        let mut changed = false;
        for (key, node) in &model.by_key {
            if node.dir && self.auto_expanded.insert(key.clone()) {
                changed |= expanded.insert(key.clone());
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(model: &TreeModel, key: &str) -> Vec<String> {
        model.children.get(key).map(|kids| kids.iter().map(|node| node.label.clone()).collect()).unwrap_or_default()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    // Not from a spec: "Open Files" and "All Changed Files" list their own files below the project,
    // their folders open once, and a folder closed after that stays closed.
    #[test]
    fn lists_open_and_changed_files_below_the_project_and_opens_their_folders_once() {
        let (listings, options, ignored) = (Listings::new(), ViewOptions::default(), IgnoredNames::default());
        let mut pane = TreePane::default();
        assert!(!pane.set_open_files(&strings(&["/p/src/App.kt"])), "the Project pane does not list them");
        assert!(!pane.set_changed_files(&strings(&["/p/README.md"])));
        assert!(pane.set_mode(ViewMode::OpenFiles));
        assert!(!pane.keeps_stored_expansion());
        let model = pane.model("/p", "proj", &listings, &options, &ignored);
        assert_eq!(model.roots.iter().map(|node| node.label.clone()).collect::<Vec<_>>(), ["proj"]);
        assert_eq!(labels(&model, "/p"), ["src"]);
        let mut expanded = BTreeSet::new();
        assert!(pane.auto_expand(&model, &mut expanded));
        assert_eq!(expanded, BTreeSet::from(["/p".to_owned(), "/p/src".to_owned()]));
        expanded.remove("/p/src");
        assert!(!pane.auto_expand(&model, &mut expanded), "a collapsed folder stays collapsed");

        assert!(pane.set_mode(ViewMode::ChangedFiles));
        assert_eq!(labels(&pane.model("/p", "proj", &listings, &options, &ignored), "/p"), ["README.md"]);
        assert!(pane.set_changed_files(&strings(&["/p/README.md", "/p/b.txt"])));
        assert!(!pane.set_changed_files(&strings(&["/p/README.md", "/p/b.txt"])), "nothing changed");
    }

    // Not from a spec: the pane is read from the storage; panes the tree cannot show count as
    // "Project", which keeps the project's expansion.
    #[test]
    fn the_stored_pane_is_shown_and_unknown_panes_show_as_project() {
        let mut storage = UiStorage::default();
        assert_eq!(TreePane::stored(Some(&storage)).mode(), ViewMode::Project);
        storage.set(MODE_KEY, ViewMode::ChangedFiles.to_stored().value);
        assert_eq!(TreePane::stored(Some(&storage)).mode(), ViewMode::ChangedFiles);
        storage.set(MODE_KEY, ViewMode::Scratches.to_stored().value);
        let pane = TreePane::stored(Some(&storage));
        assert_eq!(pane.mode(), ViewMode::Project);
        assert!(pane.keeps_stored_expansion());
        let mut unchanged = pane.clone();
        assert!(!unchanged.set_mode(ViewMode::Unity), "Unity shows as Project, which is shown");
    }
}
