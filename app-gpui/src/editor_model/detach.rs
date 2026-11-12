//! Open in New Window (detach a tab): the tab's custom-document guard runs first, then its text
//! is saved, then the new window opens with the file, and only then does the tab go. A failure
//! at any step keeps the tab.
//!
//! The removal is a [`Removal`](super::removal::Removal) of [`Detach::scope`] with `with_before_remove`; its
//! [`RemovalStep::BeforeRemove`](super::removal::RemovalStep::BeforeRemove) is carried out by
//! [`Detach::before_remove`] and the steps that follow it.

use super::documents::DocumentKinds;
use super::removal::CloseScope;
use crate::editor::tabs::TabsState;
use crate::platform::i18n::I18n;
use crate::platform::paths::is_same_or_inside;
use crate::platform::virtual_docs::basename;

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub error: bool,
    pub title: String,
    pub body: Option<String>,
    /// Replaces an earlier notification with the same key.
    pub key: Option<String>,
}

/// The window a tab is detached from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DetachWindow<'a> {
    /// The window's project.
    pub root: Option<&'a str>,
    /// The projects of the window's workspace; empty: the root alone.
    pub workspace: &'a [String],
    /// A remote-development window, which cannot open other windows.
    pub remote: bool,
}

/// What the caller does next while detaching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachStep {
    /// Save the tab's text document, then report [`Detach::text_saved`].
    SaveText { pane: String, tab: String },
    /// Open `path` in a new window (of the project `root`, or a LightEdit one), then report
    /// [`Detach::window_opened`].
    OpenWindow { root: Option<String>, path: String },
    /// Report `ok` to the removal's `before_removed` and show `notice`.
    Done { ok: bool, notice: Option<Notice> },
}

/// One Open in New Window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detach {
    pane: String,
    tab: String,
    path: String,
    text: bool,
    /// The project the new window opens, when the file belongs to the workspace.
    pub window_root: Option<String>,
}

impl Detach {
    /// Detaches `tab` of `pane`: a file tab or a custom document with a path. The new window
    /// opens the project when the file belongs to the workspace. In a remote window the warning
    /// to show is returned instead; `Err(None)` when the tab cannot be detached.
    pub fn new(
        tabs: &TabsState,
        pane: &str,
        tab: &str,
        kinds: &DocumentKinds,
        window: DetachWindow,
        i18n: &I18n,
    ) -> Result<Self, Option<Notice>> {
        let found = tabs.pane(pane).and_then(|at| at.tab(tab)).ok_or(None)?;
        let path = found.input.path.clone().ok_or(None)?;
        let text = found.input.kind == "file";
        if !text && !kinds.has_lifecycle(&found.input) {
            return Err(None);
        }
        if window.remote {
            let title = i18n.t("app.window.desktopOnly", &[]);
            return Err(Some(Notice { error: false, title, body: None, key: None }));
        }
        let window_root = window
            .root
            .filter(|root| {
                let mut projects = window.workspace.iter().map(String::as_str).peekable();
                if projects.peek().is_none() {
                    is_same_or_inside(&path, root)
                } else {
                    projects.any(|project| is_same_or_inside(&path, project))
                }
            })
            .map(str::to_owned);
        Ok(Self { pane: pane.to_owned(), tab: tab.to_owned(), path, text, window_root })
    }

    /// The tab the removal closes.
    pub fn scope(&self) -> CloseScope {
        CloseScope::Tab { pane: self.pane.clone(), tab: self.tab.clone() }
    }

    /// The guard passed: saves the text first, then opens the window. A tab that went meanwhile
    /// is not detached.
    pub fn before_remove(&self, tabs: &TabsState) -> DetachStep {
        if tabs.pane(&self.pane).and_then(|pane| pane.tab(&self.tab)).is_none() {
            return DetachStep::Done { ok: false, notice: None };
        }
        if self.text {
            return DetachStep::SaveText { pane: self.pane.clone(), tab: self.tab.clone() };
        }
        self.open_window()
    }

    fn open_window(&self) -> DetachStep {
        DetachStep::OpenWindow { root: self.window_root.clone(), path: self.path.clone() }
    }

    /// The text was saved, or saving it failed: then the tab stays and no window opens.
    pub fn text_saved(&self, result: Result<(), String>, i18n: &I18n) -> DetachStep {
        match result {
            Ok(()) => self.open_window(),
            Err(error) => DetachStep::Done {
                ok: false,
                notice: Some(Notice {
                    error: true,
                    title: i18n.t("app.unsaved.saveFailed", &[]),
                    body: Some(error),
                    key: None,
                }),
            },
        }
    }

    /// The new window opened, or opening it failed: then the tab stays.
    pub fn window_opened(&self, result: Result<(), String>, i18n: &I18n) -> DetachStep {
        match result {
            Ok(()) => DetachStep::Done { ok: true, notice: None },
            Err(error) => DetachStep::Done {
                ok: false,
                notice: Some(Notice {
                    error: true,
                    title: i18n.t("app.window.openFailed", &[("name", basename(&self.path).into_owned().into())]),
                    body: Some(error),
                    key: Some(format!("app.open:{}", self.path)),
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_model::controller::file_input;
    use crate::editor_model::guard::OpenOptions;
    use crate::editor_model::removal::tests::{Ide, input};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn background() -> OpenOptions {
        OpenOptions { focus: false, ..OpenOptions::default() }
    }

    fn current(ide: &Ide) -> (String, String) {
        let pane = ide.active_pane();
        let tab = ide.tabs.pane(&pane).and_then(|pane| pane.active.clone()).expect("an active tab");
        (pane, tab)
    }

    fn has_tab(ide: &Ide, id: &str) -> bool {
        ide.tabs.panes.iter().any(|pane| pane.tab(id).is_some())
    }

    fn detach_of(ide: &Ide, pane: &str, tab: &str) -> Detach {
        Detach::new(&ide.tabs, pane, tab, &ide.kinds, DetachWindow::default(), &ide.i18n).expect("detachable")
    }

    /// Runs the detach's last step with the given answers; returns the notices shown.
    fn run(
        ide: &mut Ide,
        detach: Detach,
        saved: Result<(), String>,
        opened: Result<(), String>,
    ) -> (bool, Rc<RefCell<Vec<Notice>>>) {
        let notices = Rc::new(RefCell::new(Vec::new()));
        let shown = Rc::clone(&notices);
        let scope = detach.scope();
        ide.before_remove = Some(Box::new(move |ide: &mut Ide| {
            let mut step = detach.before_remove(&ide.tabs);
            loop {
                step = match step {
                    DetachStep::SaveText { tab, .. } => {
                        assert!(has_tab(ide, &tab));
                        ide.events.push("saveText");
                        detach.text_saved(saved.clone(), &ide.i18n)
                    }
                    DetachStep::OpenWindow { .. } => {
                        assert!(has_tab(ide, &detach.tab));
                        ide.events.push("open");
                        detach.window_opened(opened.clone(), &ide.i18n)
                    }
                    DetachStep::Done { ok, notice } => {
                        shown.borrow_mut().extend(notice);
                        return ok;
                    }
                };
            }
        }));
        let removal = ide.plan(scope).with_before_remove();
        let removed = ide.queue(removal).unwrap_or(false);
        (removed, notices)
    }

    #[test]
    fn waits_for_text_save_before_opening_and_closing() {
        for autosave in [false, true] {
            let mut ide = Ide::new();
            ide.autosave = autosave;
            ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
            let (pane, id) = current(&ide);
            let detach = detach_of(&ide, &pane, &id);
            let (removed, _) = run(&mut ide, detach, Ok(()), Ok(()));
            assert!(removed, "autosave={autosave}");
            assert!(!has_tab(&ide, &id), "autosave={autosave}");
            assert_eq!(ide.events, ["saveText", "open"], "autosave={autosave}");
            assert_eq!(ide.asked, 0, "autosave={autosave}");
        }
    }

    #[test]
    fn keeps_the_tab_and_opens_no_window_when_text_saving_fails() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        let (pane, id) = current(&ide);
        let detach = detach_of(&ide, &pane, &id);
        let (removed, notices) = run(&mut ide, detach, Err("save failed".into()), Ok(()));
        assert!(!removed);
        assert!(has_tab(&ide, &id));
        assert!(!ide.events.contains(&"open"));
        let notices = notices.borrow();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].error);
        assert_eq!(notices[0].body.as_deref(), Some("save failed"));
    }

    #[test]
    fn keeps_a_pinned_text_tab_if_window_creation_fails() {
        let mut ide = Ide::new();
        ide.open_input(file_input("/p/a.ts"), background()).expect("opens");
        let (pane, id) = current(&ide);
        ide.tabs.toggle_pin(&pane, &id);
        let detach = detach_of(&ide, &pane, &id);
        let (removed, notices) = run(&mut ide, detach, Ok(()), Err("window failed".into()));
        assert!(!removed);
        assert!(ide.tabs.pane(&pane).and_then(|at| at.tab(&id)).is_some_and(|tab| tab.pinned));
        assert_eq!(notices.borrow()[0].key.as_deref(), Some("app.open:/p/a.ts"));
    }

    #[test]
    fn runs_the_custom_document_guard_once() {
        for decision in ["save", "discard", "cancel", "save failure", "window failure"] {
            let mut ide = Ide::new();
            ide.open("a").expect("opens");
            let (pane, id) = current(&ide);
            if decision == "save failure" {
                ide.doc("a").save_error = Some("save failed".into());
            }
            ide.answers.push_back(match decision {
                "save failure" | "window failure" => "save",
                other => other,
            });
            let detach = detach_of(&ide, &pane, &id);
            assert_eq!(detach.window_root, None);
            let opened = if decision == "window failure" { Err("window failed".to_owned()) } else { Ok(()) };
            let (removed, _) = run(&mut ide, detach, Ok(()), opened);
            let success = decision == "save" || decision == "discard";
            assert_eq!(removed, success, "{decision}");
            assert_eq!(has_tab(&ide, &id), !success, "{decision}");
            assert_eq!(ide.asked, 1, "{decision}");
            let expected: &[&str] = match decision {
                "save" => &["prepare", "ask", "save", "open", "close"],
                "discard" => &["prepare", "ask", "discard", "open", "close"],
                "cancel" => &["prepare", "ask", "cancelClose"],
                "save failure" => &["prepare", "ask", "save", "cancelClose"],
                _ => &["prepare", "ask", "save", "open", "cancelClose"],
            };
            assert_eq!(ide.events, expected, "{decision}");
            assert_eq!(input("a").path.as_deref(), Some("/p/a.ipynb"));
        }
    }

    #[test]
    fn routes_by_workspace_containment() {
        let cases: [(&str, &[&str], Option<&str>); 3] = [
            ("C:\\project\\a.ts", &["C:/project"], Some("C:/project")),
            ("C:\\other\\a.ts", &["C:/project", "C:/other"], Some("C:/project")),
            ("C:\\project-other\\a.ts", &["C:/project"], None),
        ];
        for (file, projects, expected) in cases {
            let mut ide = Ide::new();
            ide.open_input(file_input(file), background()).expect("opens");
            let (pane, id) = current(&ide);
            let workspace: Vec<String> = projects.iter().map(|project| (*project).to_owned()).collect();
            let window = DetachWindow { root: Some("C:/project"), workspace: &workspace, remote: false };
            let detach = Detach::new(&ide.tabs, &pane, &id, &ide.kinds, window, &ide.i18n).expect("detachable");
            assert_eq!(detach.window_root.as_deref(), expected, "{file}");
            let (removed, _) = run(&mut ide, detach, Ok(()), Ok(()));
            assert!(removed, "{file}");
        }
    }
}
