//! Tabs in a window that edits loose files on its own (LightEdit). Such a window keeps its editor
//! tabs with the same parts as a project window (`Content::Editor` has the tab fields of
//! `Content::Project`), so the tab strip, the tab menu, Close Tab and Close Other Tabs, the Find bars
//! and the viewers work the same way. A loose file opened in it becomes a tab; closing its last tab
//! closes the window once the documents it held are settled (the last window shows the Welcome
//! screen instead); its tabs are stored as one session, which the next window that turns into
//! LightEdit opens again around its own file.

use super::app_shell;
use super::tab_views::{self, FILE_KIND};
use super::windows::{self, Target};
use super::{Content, Workbench, Workbenches, for_window};
use crate::app_models::windows::{FileOpen, open_failed_notice};
use crate::editor::session::{self, FileViewState};
use crate::editor::tabs::{EditorInput, OpenTabOptions, TabsState};
use crate::editor::view::Editor;
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use gpui_kit::{AnyWindowHandle, App, AsyncWindowContext, Context, Entity, Focusable, WeakEntity, Window};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The session key of LightEdit: one set of tabs for the windows that edit loose files.
pub(super) const LIGHT_EDIT_SESSION: &str = "<lightEdit>";

/// Whether a window other than `window` edits loose files in tabs now.
fn other_light_edit_window(window: &Window, cx: &App) -> bool {
    light_edit_window(window.window_handle(), cx).is_some()
}

/// The window, other than `except`, that edits loose files in tabs: loose files picked in a window
/// that shows nothing go there.
pub(super) fn light_edit_window(except: AnyWindowHandle, cx: &App) -> Option<AnyWindowHandle> {
    let known = cx.try_global::<Workbenches>()?;
    cx.windows().into_iter().filter(|handle| handle.window_id() != except.window_id()).find(|handle| {
        known
            .0
            .get(&handle.window_id())
            .and_then(WeakEntity::upgrade)
            .is_some_and(|workbench| matches!(workbench.read(cx).content, Content::Editor { .. }))
    })
}

/// A tab closed with unsaved changes has been saved, or let go: the window it was in closes when
/// that was its last tab.
pub(super) fn closed_tab_settled(workbench: &WeakEntity<Workbench>, cx: &mut AsyncWindowContext) {
    let _ = workbench.update_in(cx, |workbench, window, cx| workbench.close_emptied_light_edit(true, window, cx));
}

/// Opens each of `files` in a window of its own, after the current update; a window that cannot
/// open is reported in `window`.
fn open_in_own_windows(files: Vec<FileOpen>, window: &mut Window, cx: &mut Context<Workbench>) {
    if files.is_empty() {
        return;
    }
    window.defer(cx, move |window, cx| {
        for open in files {
            let Err(error) = windows::open_window(Target::File(PathBuf::from(&open.path)), cx) else { continue };
            let error = ide_core::IdeError::internal(format!("{error:#}"));
            let notice = open_failed_notice(&open.path, &error, &cx.default_global::<Localization>().0);
            if let (Some(notice), Some(workbench)) = (notice, for_window(window, cx)) {
                workbench.update(cx, |workbench, cx| workbench.notify(notice, cx));
            }
        }
    });
}

impl Workbench {
    /// What a window shows once its first loose file has opened in `editor`: a tab for it, among
    /// the tabs the last LightEdit session was left with, unless another window edits loose files
    /// now (that window holds those tabs).
    pub(super) fn light_edit_content(
        &mut self,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Content {
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        let mut live = TabsState::new();
        let pane = live.active_pane.clone();
        let input = EditorInput { kind: FILE_KIND.to_owned(), key: path.clone(), path: Some(path.clone()), data: None };
        let tab_id = live.open_tab(&pane, input, OpenTabOptions::default());
        let saved = if other_light_edit_window(window, cx) {
            None
        } else {
            cx.try_global::<UiStorage>().and_then(|storage| session::load_session(storage, LIGHT_EDIT_SESSION))
        };
        let (tabs, mut restored_views) = match saved {
            Some(saved) => {
                let restored = session::restore_tabs_state(&saved, |path| path.to_owned());
                (TabsState::merge_session_tabs(&live, restored), saved.files)
            }
            None => (live, Default::default()),
        };
        // The open file shows where it is now, not where it was left.
        restored_views.remove(&path);
        let mut editors = HashMap::new();
        let mut editor_focus = HashMap::new();
        if let Some(tab_id) = tab_id {
            let focused = tab_id.clone();
            let took_focus = cx.on_focus_in(&editor.focus_handle(cx), window, move |workbench, window, cx| {
                workbench.editor_took_focus(&focused, window, cx);
            });
            editor_focus.insert(tab_id.clone(), took_focus);
            editors.insert(tab_id, editor);
        }
        Content::Editor {
            tabs: Box::new(tabs),
            editors,
            loading: Default::default(),
            restored_views,
            reveals: HashMap::new(),
            editor_focus,
            find_bars: HashMap::new(),
            view_tabs: Default::default(),
        }
    }

    /// The window has started opening its first loose file: the files that follow wait for it.
    pub(super) fn queue_light_tabs(&mut self) {
        if matches!(self.content, Content::Opening(_)) {
            self.light_pending.get_or_insert_default();
        }
    }

    /// The first loose file of the window has opened (as a text tab, in a viewer, or not at all):
    /// the files that waited for it open as its tabs, the last one shown, or each in a window of its
    /// own when this window shows no tabs.
    pub(super) fn light_edit_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let waiting = self.light_pending.take().unwrap_or_default();
        let shown = match &self.content {
            Content::Editor { tabs, editors, .. } => tabs
                .pane(&tabs.active_pane)
                .and_then(|pane| pane.active.clone())
                .and_then(|id| editors.get(&id).cloned().map(|editor| (id, editor))),
            _ => {
                open_in_own_windows(waiting, window, cx);
                return;
            }
        };
        // A Markdown file gets its layouts, as it does in a project.
        if let Some((id, editor)) = shown {
            self.wrap_markdown(&id, &editor, window, cx);
        }
        for open in waiting {
            self.open_light_tab(PathBuf::from(open.path), open.position, window, cx);
        }
        self.tabs_changed(true, window, cx);
    }

    /// Opens the loose file `path` as a tab of this window, or brings its tab to the front, with the
    /// caret at `position` (0-based line and UTF-16 column) when one is given. Returns false when
    /// the window does not edit loose files in tabs.
    pub(super) fn open_light_tab(
        &mut self,
        path: PathBuf,
        position: Option<(u32, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(self.content, Content::Editor { .. }) {
            return false;
        }
        match position {
            Some((line, col)) => self.open_project_file_at(path, line, col, window, cx),
            None => self.open_project_file(path, window, cx),
        }
        true
    }

    /// Loose files the routing left to this window: a window that edits loose files in tabs (or is
    /// turning into one) takes them; otherwise the files it does not show already come back, each to
    /// open in a window of its own.
    pub(super) fn take_loose_files(
        &mut self,
        opens: Vec<FileOpen>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<FileOpen> {
        if matches!(self.content, Content::Editor { .. }) {
            for open in opens {
                self.open_light_tab(PathBuf::from(open.path), open.position, window, cx);
            }
            return Vec::new();
        }
        let rest: Vec<FileOpen> =
            opens.into_iter().filter(|open| !self.light_edit_shows(Path::new(&open.path))).collect();
        // The file being opened is the first tab already; the others wait for it.
        if matches!(self.content, Content::Opening(_)) && self.light_pending.is_some() {
            self.light_pending.get_or_insert_default().extend(rest);
            return Vec::new();
        }
        rest
    }

    /// A loose file picked for a tab of this window could not be opened: the window says so. A tab
    /// of the restored session whose file is gone goes away quietly instead.
    pub(super) fn report_light_tab_failure(&mut self, file: &str, message: &str, cx: &mut Context<Self>) {
        let Content::Editor { restored_views, .. } = &self.content else { return };
        if restored_views.contains_key(file) {
            return;
        }
        let error = ide_core::IdeError::internal(message.to_owned());
        if let Some(notice) = open_failed_notice(file, &error, &cx.default_global::<Localization>().0) {
            self.notify(notice, cx);
        }
    }

    /// Whether the window shows `file` on its own: as one of its tabs, in its viewer, or opening it.
    pub(super) fn light_edit_shows(&self, file: &Path) -> bool {
        match &self.content {
            Content::Editor { tabs, .. } => tabs.find_tab(&file.to_string_lossy(), None).is_some(),
            Content::Viewer(view) => view.input.path.as_deref().is_some_and(|path| Path::new(path) == file),
            Content::Opening(path) => path == file,
            _ => false,
        }
    }

    /// The file of the active tab of a window that edits loose files in tabs.
    pub(super) fn light_edit_active_path(&self) -> Option<String> {
        let Content::Editor { tabs, .. } = &self.content else { return None };
        tabs.pane(&tabs.active_pane)?.active_tab()?.input.path.clone()
    }

    /// The title bar's title of a window that edits loose files in tabs: the active tab's file name,
    /// known before its document has opened.
    pub(super) fn light_edit_title(&self) -> Option<String> {
        let path = self.light_edit_active_path()?;
        Some(path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned())
    }

    /// Stores the tabs of a window that edits loose files, as the LightEdit session: where each
    /// file's editor is, and for a file not opened since the session came back, what was saved.
    pub(super) fn save_light_session(&self, cx: &mut App) {
        let Content::Editor { tabs, editors, restored_views, .. } = &self.content else { return };
        let mut files = restored_views.clone();
        for editor in editors.values() {
            let editor = editor.read(cx);
            let view =
                FileViewState { anchor: editor.anchor(), head: editor.caret(), top_line: Some(editor.top_line(cx)) };
            files.insert(editor.path().to_string_lossy().into_owned(), view);
        }
        let saved = session::serialize_session(tabs, &files, tab_views::restorable);
        if cx.has_global::<UiStorage>() {
            session::store_session(cx.global_mut::<UiStorage>(), LIGHT_EDIT_SESSION, saved.as_ref());
        }
    }

    /// Once the last tab of a window that edits loose files has closed and the documents it held
    /// are settled (`settled`: saved, or let go), the window closes through its close guard; the last
    /// window shows the Welcome screen instead. Until then the window shows its empty pane.
    pub(super) fn close_emptied_light_edit(&self, settled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let emptied = matches!(&self.content, Content::Editor { tabs, .. } if tabs.open_keys().is_empty());
        if !emptied {
            return;
        }
        // The window goes before its delayed session write would run: no tab is left to reopen.
        self.save_light_session(cx);
        if settled {
            // The close reads this window's workbench: it runs after the current update.
            window.defer(cx, |window, cx| {
                app_shell::close_light_edit_tab(window, cx);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::platform::actions::execute_action_or_throw;
    use crate::workbench::empty_editor;
    use crate::workbench::tests::start;
    use gpui_kit::TestAppContext;
    use std::time::Duration;

    fn run_in(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
        window
            .update(cx, |_, window, cx| execute_action_or_throw(id, None, window, cx))
            .expect("the window is open")
            .unwrap_or_else(|error| panic!("{id} runs: {error}"));
        cx.run_until_parked();
    }

    fn labels(cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| windows::windows(cx).into_iter().map(|(label, _)| label).collect())
    }

    fn workbench_in(cx: &mut TestAppContext, window: AnyWindowHandle) -> Entity<Workbench> {
        window.update(cx, |_, window, cx| for_window(window, cx)).expect("the window is open").expect("a workbench")
    }

    fn tabs_of(cx: &mut TestAppContext, workbench: &Entity<Workbench>) -> Vec<String> {
        workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
    }

    fn editing(cx: &mut TestAppContext, workbench: &Entity<Workbench>) -> Option<PathBuf> {
        workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
    }

    fn wait(cx: &mut TestAppContext, what: &str, done: &dyn Fn(&mut TestAppContext) -> bool) {
        for _ in 0..3000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what} did not happen within thirty seconds");
    }

    /// Waits until `window` offers its editor tab actions: the editor has the keyboard and a tab is
    /// open.
    fn wait_for_tab(cx: &mut TestAppContext, window: AnyWindowHandle, workbench: &Entity<Workbench>) {
        wait(cx, "a tab to act on", &|cx| {
            window
                .update(cx, |_, window, cx| {
                    if let Some(editor) = workbench.read(cx).editor().cloned() {
                        window.activate_window();
                        window.focus(&editor.focus_handle(cx), cx);
                    }
                    let context = crate::platform::context::window_context(window, cx);
                    context.is_truthy("editorTabsOpen") && !context.is_truthy("toolWindowFocus")
                })
                .expect("the window is open")
        });
    }

    /// Two trusted folders, canonical: the first with `a.txt` and `c.txt`, the second with `b.md`.
    fn folders(cx: &mut TestAppContext) -> (Vec<tempfile::TempDir>, Vec<PathBuf>) {
        let folders: Vec<tempfile::TempDir> =
            (0..2).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        std::fs::write(roots[0].join("a.txt"), "a\n").expect("the file is written");
        std::fs::write(roots[0].join("c.txt"), "c\n").expect("the file is written");
        std::fs::write(roots[1].join("b.md"), "# b\n").expect("the file is written");
        for root in &roots {
            cx.update(|cx| cx.global::<Backend>().host.trust.decide(root, true, false)).expect("trusted");
        }
        (folders, roots)
    }

    /// Opens `first` in a window of its own, then `then` as its tabs through Open File…; waits until
    /// the last one is shown.
    fn light_window(cx: &mut TestAppContext, first: &Path, then: Vec<PathBuf>) -> (AnyWindowHandle, Entity<Workbench>) {
        let window =
            cx.update(|cx| windows::open_window(Target::File(first.to_path_buf()), cx)).expect("a window opens");
        let workbench = workbench_in(cx, window);
        wait(cx, "the first file in its editor", &|cx| editing(cx, &workbench).as_deref() == Some(first));
        let Some(last) = then.last().cloned() else { return (window, workbench) };
        run_in(cx, window, "file.openFile");
        assert!(cx.did_prompt_for_paths(), "Open File asks for files");
        cx.simulate_path_prompt_response(move |_| Some(then));
        wait(cx, "the last file shown", &|cx| editing(cx, &workbench) == Some(last.clone()));
        (window, workbench)
    }

    fn close_all(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let host = cx.global::<Backend>().host.clone();
            for (label, _) in windows::windows(cx) {
                host.close_project(&label);
            }
        });
    }

    // src/features/app/__tests__/lightEditClose.test.ts › LightEdit window › closes when its last editor tab closes
    /// Closing one of two tabs leaves the window with the other; closing that one too closes the
    /// window.
    #[gpui_kit::test]
    async fn closes_when_its_last_editor_tab_closes(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let _main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        let (loose, workbench) = light_window(cx, &roots[0].join("a.txt"), vec![roots[1].join("b.md")]);
        assert_eq!(tabs_of(cx, &workbench), ["a.txt", "b.md"]);
        assert_eq!(labels(cx), ["main", "project-1"]);

        wait_for_tab(cx, loose, &workbench);
        run_in(cx, loose, "editor.closeTab");
        wait(cx, "a.txt shown again", &|cx| editing(cx, &workbench) == Some(roots[0].join("a.txt")));
        assert_eq!(tabs_of(cx, &workbench), ["a.txt"]);
        assert_eq!(labels(cx), ["main", "project-1"], "the window stays while it has a tab");

        wait_for_tab(cx, loose, &workbench);
        run_in(cx, loose, "editor.closeTab");
        wait(cx, "the window to close", &|cx| labels(cx) == ["main"]);
        close_all(cx);
    }

    /// Close Other Tabs keeps the active tab only, and the window stays.
    #[gpui_kit::test]
    async fn close_other_tabs_keeps_the_active_tab(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let (loose, workbench) =
            light_window(cx, &roots[0].join("a.txt"), vec![roots[0].join("c.txt"), roots[1].join("b.md")]);
        assert_eq!(tabs_of(cx, &workbench), ["a.txt", "c.txt", "b.md"]);
        wait_for_tab(cx, loose, &workbench);
        run_in(cx, loose, "editor.closeOtherTabs");
        wait(cx, "the other tabs to close", &|cx| tabs_of(cx, &workbench) == ["b.md"]);
        assert_eq!(editing(cx, &workbench), Some(roots[1].join("b.md")));
        assert_eq!(labels(cx), ["main"]);
        close_all(cx);
    }

    /// The window's title and the status bar follow the active tab.
    #[gpui_kit::test]
    async fn the_title_and_the_status_bar_follow_the_active_tab(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let (_loose, workbench) = light_window(cx, &roots[0].join("a.txt"), vec![roots[1].join("b.md")]);
        assert_eq!(workbench.read_with(cx, |w, cx| w.title(cx)), "b.md");
        assert_eq!(workbench.read_with(cx, |w, _| w.light_edit_title()).as_deref(), Some("b.md"));
        let b = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("b.md has its editor");
        b.update(cx, |editor, cx| editor.select(2, 2, cx));
        cx.run_until_parked();
        assert_eq!(workbench.read_with(cx, |w, cx| w.status_items(cx))[0], "1:3");

        let window = cx.update(|cx| windows::windows(cx)[0].1);
        window
            .update(cx, |_, window, cx| {
                workbench.update(cx, |w, cx| {
                    w.with_tabs(window, cx, |tabs, pane| tabs.cycle_tab(-1, Some(pane)));
                });
            })
            .expect("the window is open");
        wait(cx, "a.txt shown", &|cx| editing(cx, &workbench) == Some(roots[0].join("a.txt")));
        let a = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("a.txt has its editor");
        assert!(
            a.read_with(cx, |editor, _| editor.gutter_marks_enabled()),
            "the first loose file's gutter has its marks"
        );
        assert!(b.read_with(cx, |editor, _| editor.gutter_marks_enabled()));
        assert_eq!(workbench.read_with(cx, |w, cx| w.title(cx)), "a.txt");
        assert_eq!(workbench.read_with(cx, |w, _| w.light_edit_title()).as_deref(), Some("a.txt"));
        assert_eq!(workbench.read_with(cx, |w, cx| w.status_items(cx))[0], "1:1");
        close_all(cx);
    }

    /// A loose file picked in a window that shows nothing goes to the window that edits loose files,
    /// as a tab; the asking window stays empty.
    #[gpui_kit::test]
    async fn leaves_files_to_the_existing_light_edit_window(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let (_loose, workbench) = light_window(cx, &roots[0].join("a.txt"), Vec::new());
        let empty = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        run_in(cx, empty, "file.openFile");
        let picked = roots[1].join("b.md");
        cx.simulate_path_prompt_response({
            let picked = picked.clone();
            move |_| Some(vec![picked])
        });
        wait(cx, "b.md as a tab of main", &|cx| editing(cx, &workbench) == Some(picked.clone()));
        assert_eq!(tabs_of(cx, &workbench), ["a.txt", "b.md"]);
        let asking = workbench_in(cx, empty);
        assert!(asking.read_with(cx, |w, _| w.is_empty()), "the asking window stays empty");
        assert_eq!(labels(cx), ["main", "project-1"]);
        close_all(cx);
    }

    /// The tabs a window that edits loose files was left with open again in the next window that
    /// turns into one, around that window's own file, which is the one shown.
    #[gpui_kit::test]
    async fn the_session_restores_its_tabs(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let (loose, workbench) = light_window(cx, &roots[0].join("a.txt"), vec![roots[1].join("b.md")]);
        workbench.update(cx, |w, cx| w.save_session(cx));
        drop(workbench);
        cx.update(|cx| cx.global::<Backend>().host.close_project("main"));
        loose.update(cx, |_, window, _| window.remove_window()).expect("the window is open");
        cx.run_until_parked();
        assert!(labels(cx).is_empty(), "the window is gone");

        let c = roots[0].join("c.txt");
        let next = cx.update(|cx| windows::open_window(Target::File(c.clone()), cx)).expect("a window opens");
        let next = workbench_in(cx, next);
        wait(cx, "c.txt in its editor", &|cx| editing(cx, &next) == Some(c.clone()));
        assert_eq!(tabs_of(cx, &next), ["a.txt", "b.md", "c.txt"]);
        close_all(cx);
    }

    /// With autosave on, a last tab closed with changes is saved, then the window closes.
    #[gpui_kit::test]
    async fn closing_the_last_tab_saves_its_changes_before_the_window_closes(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let _main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        let file = roots[0].join("a.txt");
        let (loose, workbench) = light_window(cx, &file, Vec::new());
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("a.txt has its editor");
        let inserted = editor.update(cx, |editor, cx| editor.insert(Some(0..0), "x", cx));
        assert!(inserted, "the text is typed");
        wait(cx, "the change to count as unsaved", &|cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
        wait_for_tab(cx, loose, &workbench);
        run_in(cx, loose, "editor.closeTab");
        wait(cx, "the window to close", &|cx| labels(cx) == ["main"]);
        assert_eq!(std::fs::read_to_string(&file).expect("the file is read"), "xa\n");
        close_all(cx);
    }

    /// A window whose last tab was closed leaves no tabs to reopen: the next window that turns into
    /// LightEdit shows its own file only.
    #[gpui_kit::test]
    async fn closing_the_last_tab_leaves_no_tabs_to_reopen(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let (_folders, roots) = folders(cx);
        let _main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        let (loose, workbench) = light_window(cx, &roots[0].join("a.txt"), Vec::new());
        workbench.update(cx, |w, cx| w.save_session(cx));
        wait_for_tab(cx, loose, &workbench);
        run_in(cx, loose, "editor.closeTab");
        wait(cx, "the window to close", &|cx| labels(cx) == ["main"]);
        drop(workbench);

        let c = roots[0].join("c.txt");
        let next = cx.update(|cx| windows::open_window(Target::File(c.clone()), cx)).expect("a window opens");
        let next = workbench_in(cx, next);
        wait(cx, "c.txt in its editor", &|cx| editing(cx, &next) == Some(c.clone()));
        assert_eq!(tabs_of(cx, &next), ["c.txt"]);
        close_all(cx);
    }

    /// An empty pane of a window that edits loose files offers Open File and says that files can be
    /// dropped; Open File runs its action.
    #[gpui_kit::test]
    async fn an_empty_light_edit_pane_offers_open_file_and_dropping(cx: &mut TestAppContext) {
        let _home = start(cx);
        let rows = cx.update(|cx| empty_editor::light_edit_hint_rows(cx));
        let shown: Vec<(&str, Option<&str>)> = rows.iter().map(|row| (row.label.as_str(), row.runs)).collect();
        assert_eq!(shown, [("Open File", Some("file.openFile")), ("Drop files here to open them", None)]);
    }
}
