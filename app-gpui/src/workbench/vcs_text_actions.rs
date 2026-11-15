//! Git actions on the text of the active editor, and the external Git log:
//!
//! - Accept Yours / Theirs / Both (Conflict at Caret) replace the conflict block holding the caret by
//!   one side, the other or both, as an edit of the editor (unsaved, undoable); with the caret in no
//!   block they say so. During a rebase the user's commit is git's `>>>>>>>` section, so the sides
//!   swap.
//! - Next / Previous Conflict Marker move the caret to the next or previous block, wrapping round.
//! - Next / Previous Change move the caret between the editor's changed ranges against HEAD (unsaved
//!   edits included), wrapping round; while a diff viewer or the merge editor has the keyboard, they
//!   move between its differences instead.
//! - Show History for Selection opens the History for Selection tab of the selected lines.
//! - Open External Git Log… asks for folders and opens the log of their repositories in a tab.
//!
//! Backend work runs off the window's thread; the change ranges are read afresh for each move, as
//! Rollback Lines reads them, so they always match the text the caret is in.

use super::Workbench;
use super::branches::{This, call, finished, host_call, notify};
use super::services_window::on_workbench;
use super::tab_views::TabView;
use crate::backend::Backend;
use crate::editor::buffer::{Assoc, map_offset};
use crate::editor::tabs::EditorInput;
use crate::editor::view::Editor;
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::storage::UiStorage;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::vcs::conflicts::Resolution;
use crate::vcs::external_log_view::{
    ExternalLogEvent, ExternalLogView, GIT_LOG_KIND, external_log_input, external_log_roots,
};
use crate::vcs::selection_history_view::{
    SELECTION_HISTORY_KIND, SelectionHistoryData, SelectionHistoryEvent, SelectionHistoryView, selected_lines,
    selection_history_data, selection_history_input,
};
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, load_diff_options};
use crate::vcs_diff::doc::{Direction, Doc};
use crate::vcs_diff::gutter::HunkState;
use crate::vcs_more::caret_conflicts::{conflict_target, resolve_at_caret};
use crate::vcs_ops::operations::{InProgress, in_progress};
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{
    App, AppContext, AsyncWindowContext, Context, Entity, Focusable, PathPromptOptions, WeakEntity, Window,
};
use ide_core::IdeError;
use ide_core::ipc_path::to_ipc;
use ide_host::ops::vcs as git;
use ide_vcs::{Commit, LineHunk, LogQuery};
use std::path::PathBuf;

const ACCEPT_YOURS: &str = "vcs.acceptYours";
const ACCEPT_THEIRS: &str = "vcs.acceptTheirs";
const ACCEPT_BOTH: &str = "vcs.acceptBoth";
const NEXT_CONFLICT: &str = "vcs.nextConflict";
const PREV_CONFLICT: &str = "vcs.prevConflict";
const NEXT_CHANGE: &str = "vcs.nextChange";
const PREV_CHANGE: &str = "vcs.prevChange";
const SHOW_HISTORY_FOR_SELECTION: &str = "vcs.showHistoryForSelection";
const OPEN_EXTERNAL_LOG: &str = "vcs.log.openExternal";

/// The context key set while a diff viewer or the merge editor has the keyboard.
const DIFF_EDITOR_FOCUS: &str = "diffEditorFocus";
/// How many commits of each repository an external log reads.
const EXTERNAL_LOG_LIMIT: u32 = 300;

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for (id, how) in
        [(ACCEPT_YOURS, Resolution::Ours), (ACCEPT_THEIRS, Resolution::Theirs), (ACCEPT_BOTH, Resolution::Both)]
    {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| workbench.accept_at_caret(how, cx))
        });
    }
    for (id, direction) in [(NEXT_CONFLICT, Direction::Next), (PREV_CONFLICT, Direction::Previous)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| workbench.go_to_conflict(direction, window, cx))
        });
    }
    let changes = [
        (NEXT_CHANGE, Direction::Next, "diff.nextDifference"),
        (PREV_CHANGE, Direction::Previous, "diff.prevDifference"),
    ];
    for (id, direction, difference) in changes {
        registry.handle(id, move |call, window, cx| {
            // The focused diff viewer (or merge editor) moves between its own differences. Its action
            // runs here, outside the workbench's update, since it updates the workbench itself.
            if call.ctx.is_truthy(DIFF_EDITOR_FOCUS) && execute_action(difference, None, window, cx) {
                return Ok(());
            }
            on_workbench(window, cx, move |workbench, window, cx| workbench.go_to_change(direction, window, cx))
        });
    }
    registry.handle(SHOW_HISTORY_FOR_SELECTION, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_history_for_selection(window, cx))
    });
    registry.handle(OPEN_EXTERNAL_LOG, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.open_external_log(window, cx))
    });
}

/// Moves the caret of `editor` to the next or previous of `hunks` (its changed ranges against HEAD),
/// centres it and gives the editor the keyboard. Without changes nothing moves.
fn move_to_change(
    editor: &Entity<Editor>,
    hunks: &[LineHunk],
    direction: Direction,
    window: &mut Window,
    cx: &mut App,
) {
    let moved = editor.update(cx, |editor, cx| {
        let doc = Doc::new(&editor.text());
        let mut state = HunkState::default();
        state.set_hunks(hunks, &doc);
        let (offset, _) = state.goto_change(&doc, editor.caret() as usize, direction)?;
        let offset = offset as u32;
        editor.select(offset, offset, cx);
        editor.scroll_caret_to_center(cx);
        Some(offset)
    });
    if moved.is_some() {
        window.focus(&editor.focus_handle(cx), cx);
    }
}

/// Builds the view of a History for Selection tab; the request reading its history is cancelled when
/// the tab goes before the answer.
pub(super) fn build_selection_history_tab(
    input: &EditorInput,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Option<TabView> {
    let data = selection_history_data(input)?;
    let options = load_diff_options(cx.default_global::<UiStorage>().get(DIFF_OPTIONS_KEY));
    let view = cx.new(|cx| SelectionHistoryView::new(data, options, cx));
    let events = cx.subscribe_in(&view, window, |workbench, view, event: &SelectionHistoryEvent, _, cx| {
        workbench.selection_history_event(view, event, cx);
    });
    // Detached: the tab's own subscriptions go with the tab, before the view is released.
    cx.observe_release(&view, |_, view, cx| {
        if let (Some(request), Some(backend)) = (view.pending_request(), cx.try_global::<Backend>()) {
            backend.host.cancel_request(request);
        }
    })
    .detach();
    view.update(cx, |view, cx| view.load(cx));
    // The diff draws as the editor of this window does.
    let viewer = view.read(cx).diff_viewer().clone();
    cx.defer_in(window, move |workbench, _, cx| {
        let label = workbench.label.clone();
        viewer.update(cx, |viewer, cx| viewer.follow_editor_settings(&label, cx));
    });
    Some(TabView::new(SELECTION_HISTORY_KIND, input.clone(), &view, cx).with_subscription(events))
}

/// Builds the view of an external Git log tab.
pub(super) fn build_external_log_tab(
    input: &EditorInput,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Option<TabView> {
    let roots = external_log_roots(input)?;
    let view = cx.new(|cx| ExternalLogView::new(roots, cx));
    let events = cx.subscribe_in(&view, window, |workbench, view, event: &ExternalLogEvent, _, cx| match event {
        ExternalLogEvent::Load(roots) => workbench.load_external_log(view.downgrade(), roots.clone(), cx),
    });
    view.update(cx, |view, cx| view.load(cx));
    Some(TabView::new(GIT_LOG_KIND, input.clone(), &view, cx).with_subscription(events))
}

/// Opens the log of the repositories holding `paths`: each is opened for reading first, and one that
/// cannot be (a folder outside any repository) is told and opens nothing.
async fn open_external_logs(this: This, cx: &mut AsyncWindowContext, paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let mut roots = Vec::new();
    for path in paths {
        let opened =
            call(&this, cx, move |host, label| async move { git::vcs_open_external_log(&host, &label, path).await })
                .await?;
        match opened {
            Ok(root) => roots.push(to_ipc(&root.0)),
            Err(error) => {
                let mut notice = vcs_notice(NotificationLevel::Error, "Could not open Git log");
                notice.body = Some(error);
                return notify(&this, cx, notice);
            }
        }
    }
    this.update_in(cx, |workbench, window, cx| workbench.open_view_tab(external_log_input(&roots), window, cx))
}

impl Workbench {
    /// The History for Selection tab being shown, when the active tab is one.
    pub fn selection_history_view(&self) -> Option<Entity<SelectionHistoryView>> {
        self.active_tab_view().and_then(TabView::downcast::<SelectionHistoryView>)
    }

    /// The external Git log tab being shown, when the active tab is one.
    pub fn external_log_view(&self) -> Option<Entity<ExternalLogView>> {
        self.active_tab_view().and_then(TabView::downcast::<ExternalLogView>)
    }

    /// Whether the current repository is stopped in a rebase.
    fn rebase_in_progress(&self) -> bool {
        let operation =
            self.vcs_state().and_then(|state| state.branch.as_ref()).and_then(|branch| branch.operation.as_deref());
        in_progress(operation) == Some(InProgress::Rebase)
    }

    /// Accept Yours / Theirs / Both: the conflict block at the active editor's caret becomes `how`;
    /// every caret and selection moves with the edit.
    fn accept_at_caret(&mut self, how: Resolution, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let reversed = self.rebase_in_progress();
        let found = editor.update(cx, |editor, cx| {
            let Some(change) = resolve_at_caret(&editor.text(), editor.caret(), how, reversed) else { return false };
            let changes = std::slice::from_ref(&change);
            let selections = editor
                .selections()
                .iter()
                .map(|&(anchor, head)| {
                    (map_offset(anchor, changes, Assoc::Before), map_offset(head, changes, Assoc::Before))
                })
                .collect();
            let main = editor.main_index();
            if !editor.apply_edit(changes, selections, main, cx) {
                tracing::info!("the conflict at the caret of {} was not replaced", editor.path().display());
            }
            true
        });
        if !found {
            self.notify(vcs_notice(NotificationLevel::Info, "The caret is not inside a conflict"), cx);
        }
    }

    /// Next / Previous Conflict Marker in the active editor; a text without conflicts keeps its caret.
    fn go_to_conflict(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let moved = editor.update(cx, |editor, cx| {
            let target = conflict_target(&editor.text(), editor.caret(), direction)?;
            editor.select(target, target, cx);
            Some(target)
        });
        if moved.is_some() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Next / Previous Change in the active editor: its changed ranges are read for the text it holds
    /// now (unsaved edits included); a file without changes, or not under version control, keeps its
    /// caret.
    fn go_to_change(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (path, version) = (editor.read(cx).path().to_path_buf(), editor.read(cx).version());
        let hunks = host_call(self.label.clone(), cx, move |host, label| async move {
            git::vcs_line_hunks(&host, &label, path).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let hunks = hunks.await;
            finished(this.update_in(cx, |workbench, window, cx| {
                let hunks = match hunks {
                    Ok(hunks) => hunks,
                    Err(error) => return tracing::debug!("no change markers to move between: {error}"),
                };
                // Ranges of an older text would put the caret on the wrong lines: they are read again.
                if editor.read(cx).version() != version {
                    return workbench.go_to_change(direction, window, cx);
                }
                move_to_change(&editor, &hunks, direction, window, cx);
            }));
        })
        .detach();
    }

    /// Show History for Selection: the History for Selection tab of the active editor's selected
    /// lines, with its text as it is now.
    fn show_history_for_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor().cloned().filter(|editor| !is_virtual_uri(&editor.read(cx).path().to_string_lossy()));
        let Some(editor) = editor else {
            let notice = vcs_notice(NotificationLevel::Info, "Open a file to show the history of its selection");
            return self.notify(notice, cx);
        };
        let data = {
            let editor = editor.read(cx);
            let (text, selection) = (editor.text(), editor.selection());
            let lines = selected_lines(&text, selection.start, selection.end);
            SelectionHistoryData { path: editor.path().to_string_lossy().into_owned(), lines, text }
        };
        self.open_view_tab(selection_history_input(&data), window, cx);
        // The tab of these lines was open already: it shows their history for the text as it is now.
        let open = self.selection_history_view().filter(|view| view.read(cx).data() != &data);
        if let Some(view) = open {
            if let (Some(request), Some(backend)) = (view.read(cx).pending_request(), cx.try_global::<Backend>()) {
                backend.host.cancel_request(request);
            }
            view.update(cx, |view, cx| view.reload(data, cx));
        }
    }

    fn selection_history_event(
        &mut self,
        view: &Entity<SelectionHistoryView>,
        event: &SelectionHistoryEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SelectionHistoryEvent::Load(data) => self.load_selection_history(view.downgrade(), (**data).clone(), cx),
        }
    }

    /// Reads the history of the selected lines (`vcs_block_history`, cancellable) and hands it to
    /// the view.
    fn load_selection_history(
        &mut self,
        view: WeakEntity<SelectionHistoryView>,
        data: SelectionHistoryData,
        cx: &mut Context<Self>,
    ) {
        let request = Backend::next_request_id();
        let SelectionHistoryData { path, lines, text } = data;
        let work = host_call(self.label.clone(), cx, move |host, label| async move {
            git::vcs_block_history(&host, &label, request, PathBuf::from(path), Some(text), lines).await
        });
        if view.update(cx, |view, cx| view.started(request, cx)).is_err() {
            return;
        }
        cx.spawn(async move |_, cx| {
            let history = work.await;
            // A tab closed meanwhile shows nothing; its request was cancelled.
            let _ = view.update(cx, |view, cx| view.set_history(request, history, cx));
        })
        .detach();
    }

    /// Reads the commits of the external repositories `roots` and hands them to the view.
    fn load_external_log(&mut self, view: WeakEntity<ExternalLogView>, roots: Vec<String>, cx: &mut Context<Self>) {
        let work = host_call(self.label.clone(), cx, move |host, label| async move {
            let mut commits: Vec<(String, Commit)> = Vec::new();
            for root in roots {
                let query = LogQuery { limit: EXTERNAL_LOG_LIMIT, ..LogQuery::default() };
                let page = git::vcs_log(&host, &label, Some(PathBuf::from(&root)), query).await?;
                commits.extend(page.into_iter().map(|commit| (root.clone(), commit)));
            }
            Ok::<_, IdeError>(commits)
        });
        cx.spawn(async move |_, cx| {
            let commits = work.await;
            // A tab closed meanwhile shows nothing.
            let _ = view.update(cx, |view, cx| view.set_commits(commits, cx));
        })
        .detach();
    }

    /// Open External Git Log…: asks for one or more folders, then opens the log of their
    /// repositories; a cancelled question opens nothing.
    fn open_external_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked =
            cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let paths = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) | Err(_) => Vec::new(),
                Ok(Err(error)) => {
                    tracing::warn!("cannot ask for the folders of an external Git log: {error:#}");
                    Vec::new()
                }
            };
            if !paths.is_empty() {
                finished(open_external_logs(this, cx, paths).await);
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{
        context_is_on, git_in, git_project, init_repo, notice_titled, notification_titles, run_action, start,
        wait_for_notice, wait_long,
    };
    use super::*;
    use crate::vcs_diff::diff_input::{DiffData, DiffSide};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use ide_workspace::settings::SettingsScope;
    use std::path::Path;

    /// `c.txt` before both sides: three lines that each side changes, far enough apart that the merge
    /// keeps three conflicts.
    const BASE: &str = "A\n1\n2\n3\n4\n5\nB\n6\n7\n8\n9\n10\nC\n";

    fn side(name: &str) -> String {
        format!("A {name}\n1\n2\n3\n4\n5\nB {name}\n6\n7\n8\n9\n10\nC {name}\n")
    }

    /// Runs git in `root` as [`git_in`] does, for a command that must stop (on a conflict).
    fn git_fails(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(!output.status.success(), "git {args:?} stops on the conflicts");
    }

    /// `main` and `feature` both change the three lines of `c.txt`; `main` is checked out.
    fn diverged(root: &Path) {
        init_repo(root);
        std::fs::write(root.join("c.txt"), BASE).expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "base"]);
        git_in(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("c.txt"), side("feature")).expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "feature side"]);
        git_in(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("c.txt"), side("main")).expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "main side"]);
    }

    /// `feature` merged into `main`, stopped on three conflicts in `c.txt`.
    fn merged_with_conflicts(root: &Path) {
        diverged(root);
        git_fails(root, &["merge", "feature"]);
    }

    /// Opens `name` of the project in an editor, once it holds the file's text.
    fn open_in_editor(
        workbench: &Entity<Workbench>,
        root: &Path,
        name: &str,
        cx: &mut VisualTestContext,
    ) -> Entity<Editor> {
        let path = root.join(name);
        let opened = path.clone();
        workbench.update_in(cx, move |w, window, cx| w.open_project_file(opened, window, cx));
        wait_long(cx, "the file opens", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
        });
        workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
    }

    /// Puts the caret at `at` and gives the editor the keyboard.
    fn caret_at(workbench: &Entity<Workbench>, editor: &Entity<Editor>, at: u32, cx: &mut VisualTestContext) {
        workbench.update_in(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| editor.set_selections(vec![(at, at)], 0, cx));
            window.focus(&editor.focus_handle(cx), cx);
        });
        wait_long(cx, "the editor has the keyboard", |cx| context_is_on(cx, "editorTextFocus"));
    }

    /// The offset of the first `needle` in the editor's (ASCII) text.
    fn offset_of(editor: &Entity<Editor>, needle: &str, cx: &mut VisualTestContext) -> u32 {
        editor.read_with(cx, |editor, _| editor.text().find(needle).expect("the text holds the needle") as u32)
    }

    fn caret(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> u32 {
        editor.read_with(cx, |editor, _| editor.caret())
    }

    fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
        editor.read_with(cx, |editor, _| editor.text())
    }

    /// Lets a backend call that should change nothing come back.
    fn let_settle(cx: &mut VisualTestContext) {
        for _ in 0..30 {
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        cx.run_until_parked();
    }

    fn enabled(id: &str, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| execute_action(id, None, window, cx))
    }

    /// In a file stopped on three conflicts, Accept Yours, Accept Theirs and Accept Both, each with
    /// the caret in a different conflict, replace that conflict's block by our side, their side and
    /// both, and leave the other blocks; the edits are the editor's, not saved.
    #[gpui_kit::test]
    async fn accept_yours_theirs_and_both_at_the_caret_replace_that_conflicts_block_and_leave_the_others(
        cx: &mut TestAppContext,
    ) {
        let (workbench, cx, _folders, root) = git_project(cx, merged_with_conflicts);
        // The edits stay unsaved for the test to tell them from the file.
        cx.update(|_, cx| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
                .expect("the setting is stored");
        });
        let editor = open_in_editor(&workbench, &root, "c.txt", cx);
        assert_eq!(text(&editor, cx).matches("<<<<<<< HEAD").count(), 3, "{}", text(&editor, cx));

        let at = offset_of(&editor, "A main", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_YOURS, None);
        let after_yours = text(&editor, cx);
        assert!(after_yours.starts_with("A main\n1\n"), "{after_yours}");
        assert_eq!(after_yours.matches("<<<<<<< HEAD").count(), 2, "the other conflicts stay: {after_yours}");
        assert!(after_yours.contains("<<<<<<< HEAD\nB main\n=======\nB feature\n>>>>>>> feature\n"));

        let at = offset_of(&editor, "B main", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_THEIRS, None);
        let at = offset_of(&editor, "C feature", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_BOTH, None);
        assert_eq!(text(&editor, cx), "A main\n1\n2\n3\n4\n5\nB feature\n6\n7\n8\n9\n10\nC main\nC feature\n");
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()), "the resolution is an unsaved edit");
        let on_disk = std::fs::read_to_string(root.join("c.txt")).expect("the file is read");
        assert_eq!(on_disk.matches("<<<<<<< HEAD").count(), 3, "the file keeps its conflicts until saved");
        assert!(!notification_titles(&workbench, cx).contains(&"The caret is not inside a conflict".to_owned()));
    }

    /// With the caret on a line outside every conflict, Accept Yours changes nothing and says that the
    /// caret is not inside a conflict.
    #[gpui_kit::test]
    async fn with_the_caret_outside_a_conflict_accept_changes_nothing_and_says_so(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, merged_with_conflicts);
        let editor = open_in_editor(&workbench, &root, "c.txt", cx);
        let before = text(&editor, cx);
        let at = offset_of(&editor, "3\n", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_YOURS, None);
        assert_eq!(text(&editor, cx), before);
        let notice = notice_titled(&workbench, "The caret is not inside a conflict", cx).expect("the miss is told");
        assert_eq!(notice.level, NotificationLevel::Info);
    }

    /// During a rebase git writes the commit being replayed (the user's) as the `>>>>>>>` section:
    /// Accept Yours takes it.
    #[gpui_kit::test]
    async fn accept_yours_during_a_rebase_takes_the_commit_being_replayed(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            diverged(root);
            git_in(root, &["checkout", "-q", "feature"]);
            git_fails(root, &["rebase", "main"]);
        });
        wait_long(cx, "the rebase is read", |cx| workbench.read_with(cx, |w, _| w.rebase_in_progress()));
        let editor = open_in_editor(&workbench, &root, "c.txt", cx);
        let at = offset_of(&editor, "A main", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_YOURS, None);
        assert!(text(&editor, cx).starts_with("A feature\n1\n"), "{}", text(&editor, cx));
        let at = offset_of(&editor, "B main", cx);
        caret_at(&workbench, &editor, at, cx);
        run_action(cx, ACCEPT_THEIRS, None);
        assert!(text(&editor, cx).contains("\n5\nB main\n6\n"), "{}", text(&editor, cx));
    }

    /// Next Conflict Marker moves the caret to the start of the next conflict and from the last one
    /// back to the first; Previous Conflict Marker goes the other way; a file without conflicts keeps
    /// its caret.
    #[gpui_kit::test]
    async fn next_and_previous_conflict_marker_move_the_caret_and_wrap(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, merged_with_conflicts);
        let editor = open_in_editor(&workbench, &root, "c.txt", cx);
        let second = offset_of(&editor, "<<<<<<< HEAD\nB", cx);
        let third = offset_of(&editor, "<<<<<<< HEAD\nC", cx);
        caret_at(&workbench, &editor, 0, cx);
        run_action(cx, NEXT_CONFLICT, None);
        assert_eq!(caret(&editor, cx), second);
        run_action(cx, NEXT_CONFLICT, None);
        assert_eq!(caret(&editor, cx), third);
        run_action(cx, NEXT_CONFLICT, None);
        assert_eq!(caret(&editor, cx), 0, "past the last conflict it wraps to the first");
        run_action(cx, PREV_CONFLICT, None);
        assert_eq!(caret(&editor, cx), third, "before the first conflict it wraps to the last");
        run_action(cx, PREV_CONFLICT, None);
        assert_eq!(caret(&editor, cx), second);

        let plain = open_in_editor(&workbench, &root, "a.txt", cx);
        caret_at(&workbench, &plain, 2, cx);
        run_action(cx, NEXT_CONFLICT, None);
        run_action(cx, PREV_CONFLICT, None);
        assert_eq!(caret(&plain, cx), 2, "nothing to go to");
    }

    /// The conflict actions (`editorTextFocus`) are enabled while an editor has the keyboard, and off
    /// once it has lost it.
    #[gpui_kit::test]
    async fn the_conflict_actions_are_enabled_only_while_an_editor_has_the_keyboard(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, merged_with_conflicts);
        let editor = open_in_editor(&workbench, &root, "c.txt", cx);
        let ids = [ACCEPT_YOURS, ACCEPT_THEIRS, ACCEPT_BOTH, NEXT_CONFLICT, PREV_CONFLICT];
        let at = offset_of(&editor, "3\n", cx);
        caret_at(&workbench, &editor, at, cx);
        // The moves first; the caret then goes back outside every conflict, where an Accept changes
        // nothing.
        for id in [NEXT_CONFLICT, PREV_CONFLICT] {
            assert!(enabled(id, cx), "{id} runs in the focused editor");
        }
        caret_at(&workbench, &editor, at, cx);
        for id in [ACCEPT_YOURS, ACCEPT_THEIRS, ACCEPT_BOTH] {
            assert!(enabled(id, cx), "{id} runs in the focused editor");
        }
        run_action(cx, "toolwindow.project", None);
        wait_long(cx, "the Project view has the keyboard", |cx| !context_is_on(cx, "editorTextFocus"));
        let before = text(&editor, cx);
        for id in ids {
            assert!(!enabled(id, cx), "{id} is off without an editor that has the keyboard");
        }
        assert_eq!(text(&editor, cx), before);
    }

    /// Next Change moves the caret to the start of the next changed range of the editor (an unsaved
    /// edit counts), from the last one back to the first; Previous Change goes the other way; an
    /// unchanged file keeps its caret.
    #[gpui_kit::test]
    async fn next_and_previous_change_move_through_the_editors_changed_ranges_and_wrap(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("b.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n").expect("the file is written");
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "numbers"]);
            std::fs::write(root.join("b.txt"), "1\nTWO\n3\n4\n5\n6\nSEVEN\n8\n9\n").expect("the file is written");
        });
        let editor = open_in_editor(&workbench, &root, "b.txt", cx);
        let (two, seven) = (offset_of(&editor, "TWO", cx), offset_of(&editor, "SEVEN", cx));
        caret_at(&workbench, &editor, 0, cx);
        run_action(cx, NEXT_CHANGE, None);
        wait_long(cx, "the caret goes to the first change", |cx| caret(&editor, cx) == two);
        run_action(cx, NEXT_CHANGE, None);
        wait_long(cx, "the caret goes to the second change", |cx| caret(&editor, cx) == seven);
        run_action(cx, NEXT_CHANGE, None);
        wait_long(cx, "past the last change it wraps", |cx| caret(&editor, cx) == two);
        run_action(cx, PREV_CHANGE, None);
        wait_long(cx, "before the first change it wraps", |cx| caret(&editor, cx) == seven);

        // A line typed and not saved is a change too.
        let four = offset_of(&editor, "4\n", cx);
        caret_at(&workbench, &editor, four, cx);
        cx.simulate_input("x");
        cx.run_until_parked();
        assert!(text(&editor, cx).contains("\nx4\n"));
        caret_at(&workbench, &editor, seven + 1, cx);
        run_action(cx, PREV_CHANGE, None);
        wait_long(cx, "the typed line is the previous change", |cx| caret(&editor, cx) == four);

        let plain = open_in_editor(&workbench, &root, "a.txt", cx);
        caret_at(&workbench, &plain, 2, cx);
        run_action(cx, NEXT_CHANGE, None);
        let_settle(cx);
        assert_eq!(caret(&plain, cx), 2, "an unchanged file has no change to go to");
    }

    /// While a diff viewer has the keyboard, Next Change moves between its differences and leaves the
    /// editor's caret where it is.
    #[gpui_kit::test]
    async fn next_change_moves_between_the_differences_of_the_focused_diff_viewer(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "ONE\n").expect("the file is written");
        });
        let editor = open_in_editor(&workbench, &root, "a.txt", cx);
        caret_at(&workbench, &editor, 2, cx);
        let path = root.join("a.txt").to_string_lossy().into_owned();
        let left = DiffSide::text("one\ntwo\nthree\nfour\nfive\nsix\nseven\n", "left");
        let right = DiffSide::text("one\nTWO\nthree\nfour\nfive\nsix\nSEVEN\n", "right");
        let data = DiffData::new(&path, left, right);
        workbench.update_in(cx, |w, window, cx| w.open_diff(data, window, cx));
        wait_long(cx, "the diff is shown", |cx| {
            workbench.read_with(cx, |w, cx| {
                w.diff_dialog().is_some_and(|dialog| dialog.read(cx).viewer().read(cx).difference_count() == 2)
            })
        });
        let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff window");
        workbench.update_in(cx, |_, window, cx| window.focus(&dialog.read(cx).focus_handle(cx), cx));
        wait_long(cx, "the diff has the keyboard", |cx| context_is_on(cx, DIFF_EDITOR_FOCUS));
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), None);
        run_action(cx, NEXT_CHANGE, None);
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0));
        run_action(cx, NEXT_CHANGE, None);
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(1));
        run_action(cx, PREV_CHANGE, None);
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0));
        let_settle(cx);
        assert_eq!(caret(&editor, cx), 2, "the editor's caret stays");
    }

    /// Next and Previous Change (`vcsAvailable`) are enabled in a project under version control and
    /// off in one without a repository; Show History for Selection (`vcsAvailable &&
    /// textEditorActive`) is off there even with a file open.
    #[gpui_kit::test]
    async fn the_change_actions_are_enabled_only_in_a_project_under_version_control(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "one\n")]);
        let editor = open_in_editor(&workbench, &root, "a.txt", cx);
        caret_at(&workbench, &editor, 0, cx);
        assert!(!context_is_on(cx, "vcsAvailable"));
        for id in [NEXT_CHANGE, PREV_CHANGE, SHOW_HISTORY_FOR_SELECTION] {
            assert!(!enabled(id, cx), "{id} is off without a repository");
        }
    }

    /// In a repository, Next Change is enabled without any editor; Show History for Selection only
    /// once a text editor is active.
    #[gpui_kit::test]
    async fn show_history_for_selection_is_enabled_only_with_a_text_editor_active(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        wait_long(cx, "the repository is read", |cx| context_is_on(cx, "vcsAvailable"));
        assert!(enabled(NEXT_CHANGE, cx), "Next Change runs without an editor, doing nothing");
        assert!(!enabled(SHOW_HISTORY_FOR_SELECTION, cx), "no text editor is active");
        open_in_editor(&workbench, &root, "a.txt", cx);
        wait_long(cx, "the editor is active", |cx| context_is_on(cx, "textEditorActive"));
        assert!(enabled(SHOW_HISTORY_FOR_SELECTION, cx));
        wait_long(cx, "the tab opens", |cx| workbench.read_with(cx, |w, _| w.selection_history_view().is_some()));
    }

    /// Show History for Selection opens History for Selection on the selected lines: the edit not
    /// saved yet first as Local Changes against HEAD's block, then the commits that changed the lines,
    /// newest first, each diffed as the block before and after it.
    #[gpui_kit::test]
    async fn show_history_for_selection_lists_the_commits_that_changed_the_selected_lines(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, |root| {
            init_repo(root);
            for (text, message) in [
                ("one\ntwo\nthree\nfour\n", "add b"),
                ("one\nTWO\nthree\nfour\n", "Shout two"),
                ("one\nTWO\nTHREE\nfour\n", "Shout three"),
            ] {
                std::fs::write(root.join("b.txt"), text).expect("the file is written");
                git_in(root, &["add", "."]);
                git_in(root, &["commit", "-q", "-m", message]);
            }
        });
        let editor = open_in_editor(&workbench, &root, "b.txt", cx);
        let three = offset_of(&editor, "THREE", cx) + 5;
        caret_at(&workbench, &editor, three, cx);
        cx.simulate_input("!");
        cx.run_until_parked();
        assert_eq!(text(&editor, cx), "one\nTWO\nTHREE!\nfour\n");
        // From the start of line 2 to the start of line 4: lines 2 and 3.
        let (from, to) = (offset_of(&editor, "TWO", cx), offset_of(&editor, "four", cx));
        workbench
            .update_in(cx, |_, _, cx| editor.update(cx, |editor, cx| editor.set_selections(vec![(from, to)], 0, cx)));
        run_action(cx, SHOW_HISTORY_FOR_SELECTION, None);

        wait_long(cx, "the history is read", |cx| {
            let view = workbench.read_with(cx, |w, _| w.selection_history_view());
            view.is_some_and(|view| view.read_with(cx, |view, _| !view.is_loading()))
        });
        let view = workbench.read_with(cx, |w, _| w.selection_history_view()).expect("the tab is shown");
        assert_eq!(view.read_with(cx, |view, _| view.error().map(str::to_owned)), None);
        assert_eq!(view.read_with(cx, |view, _| view.range()), "b.txt:2-3");
        assert_eq!(view.read_with(cx, |view, _| view.data().lines), (1, 2));
        let summaries: Vec<String> =
            view.read_with(cx, |view, _| view.rows().into_iter().map(|row| row.summary).collect());
        assert!(summaries.len() >= 3, "{summaries:?}");
        assert_eq!(&summaries[..3], ["Local Changes", "Shout three", "Shout two"], "{summaries:?}");
        let shown = view.read_with(cx, |view, _| view.shown().cloned()).expect("a diff is shown");
        assert_eq!((shown.left_label.as_str(), shown.left.as_str()), ("HEAD", "TWO\nTHREE\n"));
        assert_eq!((shown.right_label.as_str(), shown.right.as_str()), ("Local Changes", "TWO\nTHREE!\n"));

        view.update(cx, |view, cx| view.select(2, cx));
        let shown = view.read_with(cx, |view, _| view.shown().cloned()).expect("a diff is shown");
        assert!(shown.left_label.ends_with('^'), "{shown:?}");
        assert_eq!((shown.left.as_str(), shown.right.as_str()), ("two\nthree\n", "TWO\nthree\n"));
        assert!(view.read_with(cx, |view, _| view.pending_request().is_none()), "the request is answered");
        // The tab is no text editor: the action is off there.
        assert!(!enabled(SHOW_HISTORY_FOR_SELECTION, cx));
    }

    /// Open External Git Log… asks for folders and opens the log of the picked repository in a tab
    /// titled with its root; a folder outside any repository is told and opens nothing; a cancelled
    /// question does nothing.
    #[gpui_kit::test]
    async fn open_external_git_log_shows_the_log_of_the_picked_repository(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root) = git_project(cx, init_repo);
        let outside = tempfile::tempdir().expect("a temporary folder");
        let external = outside.path().canonicalize().expect("the folder exists");
        init_repo(&external);
        std::fs::write(external.join("a.txt"), "two\n").expect("the file is written");
        git_in(&external, &["commit", "-q", "-am", "second"]);

        run_action(cx, OPEN_EXTERNAL_LOG, None);
        wait_long(cx, "the folders are asked for", |cx| cx.did_prompt_for_paths());
        let picked = external.clone();
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.directories && !options.files && options.multiple, "folders, several at once");
            Some(vec![picked])
        });
        wait_long(cx, "the log is read", |cx| {
            let view = workbench.read_with(cx, |w, _| w.external_log_view());
            view.is_some_and(|view| view.read_with(cx, |view, _| !view.is_loading()))
        });
        let view = workbench.read_with(cx, |w, _| w.external_log_view()).expect("the log tab is shown");
        let root = to_ipc(&external);
        assert_eq!(view.read_with(cx, |view, _| view.roots().to_vec()), [root.clone()]);
        let summaries: Vec<String> =
            view.read_with(cx, |view, _| view.rows().into_iter().map(|row| row.summary).collect());
        assert_eq!(summaries, ["second", "first"]);
        assert!(view.read_with(cx, |view, _| view.rows()).iter().all(|row| row.repo.is_none()));

        let plain = tempfile::tempdir().expect("a temporary folder");
        let plain = plain.path().canonicalize().expect("the folder exists");
        run_action(cx, OPEN_EXTERNAL_LOG, None);
        wait_long(cx, "the folders are asked for again", |cx| cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(move |_| Some(vec![plain]));
        wait_for_notice(&workbench, "Could not open Git log", cx);
        let notice = notice_titled(&workbench, "Could not open Git log", cx).expect("the failure is told");
        assert_eq!(notice.level, NotificationLevel::Error);
        assert!(notice.body.as_deref().is_some_and(|body| !body.is_empty()), "{notice:?}");
        let shown = workbench.read_with(cx, |w, _| w.external_log_view());
        assert_eq!(shown.map(|shown| shown.entity_id()), Some(view.entity_id()), "no other tab opens");

        let told = notification_titles(&workbench, cx).len();
        run_action(cx, OPEN_EXTERNAL_LOG, None);
        wait_long(cx, "the folders are asked for once more", |cx| cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        let_settle(cx);
        assert_eq!(notification_titles(&workbench, cx).len(), told, "a cancelled question tells nothing");
        let shown = workbench.read_with(cx, |w, _| w.external_log_view());
        assert_eq!(shown.map(|shown| shown.entity_id()), Some(view.entity_id()));
    }
}
