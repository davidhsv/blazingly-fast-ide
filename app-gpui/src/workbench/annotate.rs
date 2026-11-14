//! Annotate with Git Blame for the editors of a project window.
//!
//! The action shows the blame of the active editor's file (or of the file its argument names, which
//! it opens first) in the editor's annotation column, or takes the column away again. While shown,
//! the column is blamed again 800 ms after the text last changed (the backend blames the unsaved
//! text) and 100 ms after the head commit moved (a commit, Undo Commit, an update, a checkout). A
//! click on a line's entry shows its commit in the Git log; the column's menu also copies the commit
//! id and closes the column.

use super::Workbench;
use super::branches::{This, finished, run_action_later};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::editor::view::annotations::{AnnotationEvent, Annotations};
use crate::editor::view::{Editor, EditorEvent};
use crate::platform::actions::ActionRegistry;
use crate::platform::context::{set_context, window_context};
use crate::platform::date_format::{DateFormatOptions, date_format_options};
use crate::platform::settings;
use crate::vcs_diff::inline_blame::SHOW_COMMIT_ACTION;
use crate::vcs_ops::actions::arg_path;
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{
    App, AsyncWindowContext, ClipboardItem, Context, Entity, EntityId, Pixels, Point, Subscription, Task, WeakEntity,
    Window,
};
use ide_vcs::BlameLine;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

/// The action that shows or closes the annotations.
pub(super) const ANNOTATE: &str = "vcs.annotate";
/// The window's context key that holds whether the active editor shows annotations: the state of
/// the action's toggle, which a menu reads while the workbench is busy building it.
const SHOWN_KEY: &str = "vcsAnnotationsShown";
/// How long the text must be still before it is blamed again.
const EDIT_DELAY: Duration = Duration::from_millis(800);
/// How long after the head commit moved the file is blamed again.
const HEAD_DELAY: Duration = Duration::from_millis(100);
/// How often, and how many times, Annotate looks for the editor of the file it opened.
const OPEN_POLL: Duration = Duration::from_millis(50);
const OPEN_POLLS: u32 = 200;

/// What a row of the column's menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AnnotationCommand {
    /// Show Commit in Git Log, with the full commit id.
    ShowCommit(String),
    /// Copy Revision Number: the full commit id goes to the clipboard.
    CopyRevision(String),
    Close,
}

/// An editor that shows annotations.
struct Annotated {
    editor: WeakEntity<Editor>,
    path: PathBuf,
    /// The blame waiting for the text to be still, or for the head to settle; dropping it cancels it.
    _reblame: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// What the window keeps for the annotations of its editors.
#[derive(Default)]
pub(super) struct AnnotateSlot {
    editors: HashMap<EntityId, Annotated>,
    /// The head commit of the selected repository, as the last repository read found it.
    head: Option<String>,
}

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle_toggle(
        ANNOTATE,
        |window, cx| window_context(window, cx).is_truthy(SHOWN_KEY),
        |call, window, cx| {
            let path = arg_path(call.arg).filter(|path| !path.is_empty()).map(PathBuf::from);
            on_workbench(window, cx, move |workbench, window, cx| workbench.run_annotate(path, window, cx))
        },
    );
}

/// The Date Formats settings the column writes its dates with.
pub(super) fn date_options(cx: &App) -> DateFormatOptions {
    date_format_options(serde_json::to_value(&settings::global(cx).general.date_format).ok().as_ref())
}

/// The blame of `path` (its unsaved text, against HEAD) for the window `label`, read on the backend.
fn blame(
    label: String,
    path: PathBuf,
    cx: &App,
) -> impl Future<Output = Result<Vec<BlameLine>, String>> + 'static + use<> {
    let call = cx.try_global::<Backend>().map(|backend| {
        backend
            .with_host(move |host| async move { ide_host::ops::vcs::vcs_blame(&host, &label, path, None, None).await })
    });
    async move {
        match call {
            None => Err("The backend is not running".to_owned()),
            Some(call) => match call.await {
                Ok(Ok(lines)) => Ok(lines),
                Ok(Err(error)) => Err(error.message),
                Err(error) => Err(error.to_string()),
            },
        }
    }
}

/// Waits for the editor of `path`, which Annotate opened, and annotates it; when it does not come,
/// the active editor is annotated, or the user is told to open a file.
async fn annotate_when_open(this: This, cx: &mut AsyncWindowContext, path: PathBuf) -> anyhow::Result<()> {
    for _ in 0..OPEN_POLLS {
        let opened = this.update(cx, |workbench, cx| {
            workbench.editor().filter(|editor| editor.read(cx).path() == path.as_path()).cloned()
        })?;
        if let Some(editor) = opened {
            return this.update_in(cx, |workbench, window, cx| workbench.toggle_annotations(&editor, window, cx));
        }
        cx.background_executor().timer(OPEN_POLL).await;
    }
    this.update_in(cx, |workbench, window, cx| match workbench.editor().cloned() {
        Some(editor) => workbench.toggle_annotations(&editor, window, cx),
        None => workbench.notify(vcs_notice(NotificationLevel::Info, "Open a file to annotate it"), cx),
    })
}

/// Blames the file of `editor` and shows the result in its column, or tells why it cannot.
async fn load_annotations(
    this: This,
    cx: &mut AsyncWindowContext,
    editor: WeakEntity<Editor>,
    path: PathBuf,
) -> anyhow::Result<()> {
    let read = this.update(cx, |workbench, cx| blame(workbench.label.clone(), path.clone(), cx))?;
    let answer = read.await;
    this.update_in(cx, |workbench, window, cx| match answer {
        Ok(lines) => workbench.annotations_loaded(&editor, path, &lines, window, cx),
        Err(message) => {
            let mut request = vcs_notice(NotificationLevel::Error, "Cannot annotate");
            request.body = Some(message);
            workbench.notify(request, cx);
        }
    })
}

impl Workbench {
    /// Annotate with Git Blame: on the editor of `path` (opened first when it is not the active
    /// one), else on the active editor.
    fn run_annotate(&mut self, path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.editor().cloned();
        let elsewhere = |path: &PathBuf| active.as_ref().is_none_or(|editor| editor.read(cx).path() != path.as_path());
        if let Some(path) = path.filter(elsewhere) {
            self.open_project_file(path.clone(), window, cx);
            cx.spawn_in(window, async move |this, cx| finished(annotate_when_open(this, cx, path).await)).detach();
            return;
        }
        match active {
            Some(editor) => self.toggle_annotations(&editor, window, cx),
            None => self.notify(vcs_notice(NotificationLevel::Info, "Open a file to annotate it"), cx),
        }
    }

    /// Closes the annotations of `editor`, or blames its file and shows them.
    fn toggle_annotations(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        if editor.read(cx).annotations_shown() {
            return self.close_annotations(editor, window, cx);
        }
        let (weak, path) = (editor.downgrade(), editor.read(cx).path().to_path_buf());
        cx.spawn_in(window, async move |this, cx| finished(load_annotations(this, cx, weak, path).await)).detach();
    }

    fn close_annotations(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        editor.update(cx, |editor, cx| editor.set_annotations(None, cx));
        self.annotate.editors.remove(&editor.entity_id());
        self.publish_context(window, cx);
        cx.notify();
    }

    /// The blame of `path` arrived: `editor` shows it and is blamed again as it and the head change.
    fn annotations_loaded(
        &mut self,
        editor: &WeakEntity<Editor>,
        path: PathBuf,
        lines: &[BlameLine],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = editor.upgrade() else { return };
        let annotations = Annotations::new(lines, &date_options(cx));
        editor.update(cx, |editor, cx| editor.set_annotations(Some(annotations), cx));
        self.watch_annotations(&editor, path, window, cx);
        self.publish_context(window, cx);
        cx.notify();
    }

    fn watch_annotations(
        &mut self,
        editor: &Entity<Editor>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        if self.annotate.editors.contains_key(&id) {
            return;
        }
        let subscriptions = vec![
            cx.subscribe(editor, move |workbench, _, event: &EditorEvent, cx| {
                if let EditorEvent::Edited { .. } = event {
                    workbench.schedule_reblame(id, EDIT_DELAY, cx);
                }
            }),
            cx.subscribe_in(editor, window, |workbench, editor, event: &AnnotationEvent, window, cx| {
                let AnnotationEvent::Pressed { line } = event;
                workbench.annotation_pressed(editor, *line, window, cx);
            }),
            cx.observe_release(editor, move |workbench, _, _| {
                workbench.annotate.editors.remove(&id);
            }),
        ];
        let annotated = Annotated { editor: editor.downgrade(), path, _reblame: None, _subscriptions: subscriptions };
        self.annotate.editors.insert(id, annotated);
    }

    /// Blames the editor `id` again once `delay` has passed without another reason to.
    fn schedule_reblame(&mut self, id: EntityId, delay: Duration, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(annotated) = self.annotate.editors.get_mut(&id) else { return };
        let path = annotated.path.clone();
        annotated._reblame = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let Ok(read) = this.update(cx, |_, cx| blame(label, path, cx)) else { return };
            let answer = read.await;
            if this.update(cx, |workbench, cx| workbench.reblamed(id, answer, cx)).is_err() {
                tracing::debug!("the window closed before the blame arrived");
            }
        }));
    }

    /// The editor `id` was blamed again: it shows the new blame if it still shows annotations. A
    /// failure leaves the column as it was.
    fn reblamed(&mut self, id: EntityId, answer: Result<Vec<BlameLine>, String>, cx: &mut Context<Self>) {
        let Some(annotated) = self.annotate.editors.get(&id) else { return };
        let Some(editor) = annotated.editor.upgrade() else { return };
        match answer {
            Ok(lines) => {
                let annotations = Annotations::new(&lines, &date_options(cx));
                editor.update(cx, |editor, cx| {
                    if editor.annotations_shown() {
                        editor.set_annotations(Some(annotations), cx);
                    }
                });
            }
            Err(error) => tracing::info!("{} cannot be blamed again: {error}", annotated.path.display()),
        }
    }

    /// The repository was read: when the head commit moved, every annotated editor is blamed again.
    pub(super) fn annotations_vcs_changed(&mut self, cx: &mut Context<Self>) {
        let branch = self.vcs_state().and_then(|state| state.branch.as_ref());
        let head = branch.and_then(|branch| branch.head_commit.clone());
        if head == self.annotate.head {
            return;
        }
        self.annotate.head = head;
        let ids: Vec<EntityId> = self.annotate.editors.keys().copied().collect();
        for id in ids {
            self.schedule_reblame(id, HEAD_DELAY, cx);
        }
    }

    /// Whether an editor of the file `path` shows annotations: the gutter's menu then offers to
    /// close them.
    pub(super) fn annotations_shown_for(&self, path: &str, cx: &App) -> bool {
        self.annotate.editors.values().any(|annotated| {
            annotated.path.as_path() == std::path::Path::new(path)
                && annotated.editor.upgrade().is_some_and(|editor| editor.read(cx).annotations_shown())
        })
    }

    /// Publishes whether the active editor shows annotations, the state of the action's toggle.
    pub(super) fn publish_annotate_context(&self, window: &mut Window, cx: &mut App) {
        let shown = self.editor().is_some_and(|editor| editor.read(cx).annotations_shown());
        set_context(window, cx, SHOWN_KEY, shown);
    }

    /// A click on the entry of `line`: its commit shows in the Git log; a line not committed yet
    /// opens nothing.
    fn annotation_pressed(&mut self, editor: &Entity<Editor>, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        let commit = editor.read(cx).annotation(line).filter(|row| !row.uncommitted).map(|row| row.commit.clone());
        if let Some(commit) = commit {
            run_action_later(SHOW_COMMIT_ACTION.to_owned(), Some(Value::String(commit)), window, cx);
        }
    }

    /// The right button was pressed on the annotation column of `editor`: Show Commit in Git Log and
    /// Copy Revision Number for a committed line, and Close Annotations.
    pub(super) fn show_annotation_menu(
        &mut self,
        editor: &Entity<Editor>,
        line: u32,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commit = editor.read(cx).annotation(line).filter(|row| !row.uncommitted).map(|row| row.commit.clone());
        let mut items = Vec::new();
        if let Some(commit) = commit {
            items.extend([
                MenuItem::item("Show Commit in Git Log", AnnotationCommand::ShowCommit(commit.clone())).icon("commit"),
                MenuItem::item("Copy Revision Number", AnnotationCommand::CopyRevision(commit)).icon("copy"),
                MenuItem::Separator,
            ]);
        }
        items.push(MenuItem::item("Close Annotations", AnnotationCommand::Close));
        let target = editor.downgrade();
        self.show_menu(
            &items,
            position,
            move |workbench, command, window, cx| workbench.run_annotation_command(&target, command, window, cx),
            window,
            cx,
        );
    }

    fn run_annotation_command(
        &mut self,
        editor: &WeakEntity<Editor>,
        command: AnnotationCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            AnnotationCommand::ShowCommit(commit) => {
                run_action_later(SHOW_COMMIT_ACTION.to_owned(), Some(Value::String(commit)), window, cx);
            }
            AnnotationCommand::CopyRevision(commit) => cx.write_to_clipboard(ClipboardItem::new_string(commit)),
            AnnotationCommand::Close => {
                if let Some(editor) = editor.upgrade() {
                    self.close_annotations(&editor, window, cx);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::GIT_TOOL_WINDOW;
    use super::super::context_menus::GutterCommand;
    use super::super::tests::{
        git_in, git_project, init_repo, notification_titles, run_action, wait_for_notice, wait_long,
    };
    use super::*;
    use crate::editor::marks::MarkLayer;
    use crate::editor::view::annotations::{AnnotationRow, HOVER_SELECTOR};
    use crate::platform::local_time::utc_offset_minutes;
    use crate::ui::popup_menu::PopupMenu;
    use crate::ui::tooltip::TOOLTIP_DELAY;
    use crate::vcs_diff::model::format_date;
    use gpui_kit::{Focusable, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
    use serde_json::json;
    use std::path::Path;

    /// `b.txt` added by Ada on 2024-03-01 with three lines, the second rewritten by Bob on
    /// 2025-06-15.
    fn two_authors(root: &Path) {
        init_repo(root);
        std::fs::write(root.join("b.txt"), "alpha\nbeta\ngamma\n").expect("the file is written");
        git_in(root, &["add", "b.txt"]);
        git_in(root, &["commit", "-q", "-m", "Add b", "--date=2024-03-01T10:00:00Z"]);
        std::fs::write(root.join("b.txt"), "alpha\nBETA\ngamma\n").expect("the file is written");
        let author = "--author=Bob Stone <bob@example.test>";
        git_in(root, &["commit", "-q", "-am", "Shout beta", author, "--date=2025-06-15T10:00:00Z"]);
    }

    /// Whether `a` and `b` are the same position, up to the rounding of their sums.
    fn same_x(a: Pixels, b: Pixels) -> bool {
        f32::from(a - b).abs() < 0.01
    }

    /// Opens `name` of the project and waits until its editor is active and painted.
    fn open_file(workbench: &Entity<Workbench>, root: &Path, name: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
        let path = root.join(name);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        wait_long(cx, "the file opens", |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|editor| editor.read(cx).path() == path.as_path())
                    && w.editor().is_some_and(|editor| editor.read(cx).painted_bounds().is_some())
            })
        });
        workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
    }

    /// What the column shows on each painted line: the date and the author.
    fn shown(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(u32, String, String)> {
        editor.read_with(cx, |editor, _| {
            editor.painted_annotations().into_iter().map(|(line, row)| (line, row.date, row.author)).collect()
        })
    }

    /// The row the column shows for a line last changed by commit `rev`, by `author`.
    fn expected(root: &Path, rev: &str, author: &str, cx: &mut VisualTestContext) -> (String, String) {
        let seconds: f64 = git_in(root, &["log", "-1", "--format=%at", rev]).parse().expect("a time");
        let ms = seconds * 1000.0;
        let options = cx.update(|_, cx| date_options(cx));
        (format_date(Some(ms), utc_offset_minutes(ms), &options), author.to_owned())
    }

    fn toggled(cx: &mut VisualTestContext) -> Option<bool> {
        cx.update(|window, cx| {
            cx.global::<ActionRegistry>().get(ANNOTATE).and_then(|action| action.is_toggled(window, cx))
        })
    }

    /// The middle of the annotation column on the 0-based `line`.
    fn column_point(editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
        editor.read_with(cx, |editor, cx| {
            let bounds = editor.painted_bounds().expect("the editor is painted");
            let top = editor.position_of(editor.offset_of(line, 0)).expect("the line is painted").y;
            point(bounds.left() + editor.annotation_column_width() / 2.0, top + editor.line_height(cx) / 2.0)
        })
    }

    fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    /// The rows of the root of `menu`, `-` for a separator.
    fn menu_rows<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<String> {
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        lines[0].iter().map(|line| line.trim_start_matches("> ").to_owned()).collect()
    }

    /// Clicks the row `label` of the root of `menu`.
    fn choose<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, label: &str, cx: &mut VisualTestContext) {
        let rows = menu_rows(menu, cx);
        let index = rows.iter().position(|row| row == label).unwrap_or_else(|| panic!("{label} in {rows:?}"));
        let selector: &'static str = Box::leak(format!("menu-row-0-{index}").into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn annotated(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
        editor.read_with(cx, |editor, _| editor.annotations_shown())
    }

    /// Annotate with Git Blame, run on the active editor, widens its gutter by the column, which
    /// shows each line's date and author, Ada's lines older than Bob's; the action is then on. Run
    /// again, it closes the column and the text moves back.
    #[gpui_kit::test]
    async fn annotate_shows_the_date_and_author_of_each_line_and_closes_again(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        let text_left = |cx: &mut VisualTestContext| {
            editor.read_with(cx, |editor, _| editor.position_of(0).expect("the first line is painted").x)
        };
        let before = text_left(cx);
        assert_eq!(toggled(cx), Some(false));

        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        let ada = expected(&root, "HEAD~1", "Ada", cx);
        let bob = expected(&root, "HEAD", "Bob", cx);
        let rows = shown(&editor, cx);
        assert_eq!(rows, [(0, ada.0.clone(), ada.1.clone()), (1, bob.0, bob.1), (2, ada.0, ada.1)]);
        let ranks: Vec<f32> = editor.read_with(cx, |editor, _| {
            editor.painted_annotations().into_iter().map(|(_, AnnotationRow { rank, .. })| rank).collect()
        });
        assert_eq!(ranks, [0.0, 1.0, 0.0], "Ada's commit is the older one");
        let width = editor.read_with(cx, |editor, _| editor.annotation_column_width());
        assert!(width > px(0.0));
        assert!(same_x(text_left(cx), before + width), "the text moves right by the column");
        assert_eq!(toggled(cx), Some(true));

        run_action(cx, ANNOTATE, None);
        assert!(!annotated(&editor, cx));
        assert!(shown(&editor, cx).is_empty());
        assert!(same_x(text_left(cx), before), "the text moves back");
        assert_eq!(toggled(cx), Some(false));
    }

    /// The pointer resting on an entry draws it in the strong colour and shows the commit's short id,
    /// its author's full name and its date in a tooltip, with the summary.
    #[gpui_kit::test]
    async fn resting_on_an_entry_shows_its_commit_author_and_date_in_a_tooltip(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        let at = column_point(&editor, 1, cx);
        cx.simulate_mouse_move(at, None, Modifiers::default());
        cx.run_until_parked();
        assert!(cx.debug_bounds(HOVER_SELECTOR).is_some(), "the hovered entry holds a tooltip");
        cx.simulate_mouse_move(point(at.x + px(1.0), at.y), None, Modifiers::default());
        cx.executor().advance_clock(TOOLTIP_DELAY + Duration::from_millis(10));
        cx.run_until_parked();
        let row = editor.read_with(cx, |editor, _| editor.annotation(1).cloned()).expect("Bob's entry");
        let bob = git_in(&root, &["rev-parse", "HEAD"]);
        assert!(row.title.starts_with(&format!("{}  Bob Stone  ", &bob[..8])), "{}", row.title);
        assert_eq!(row.summary, "Shout beta");
        let selector: &'static str = Box::leak(format!("tooltip-text:{}", row.title).into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "the tooltip shows {}", row.title);
    }

    /// Without an editor Annotate asks for a file; given a path it opens the file and annotates it.
    #[gpui_kit::test]
    async fn annotate_without_a_file_asks_for_one_and_with_a_path_opens_and_annotates_it(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        assert!(workbench.read_with(cx, |w, _| w.editor().is_none()));
        run_action(cx, ANNOTATE, None);
        wait_for_notice(&workbench, "Open a file to annotate it", cx);

        let path = root.join("b.txt");
        run_action(cx, ANNOTATE, Some(json!(path.to_string_lossy())));
        wait_long(cx, "the file opens annotated", |cx| {
            let editor = workbench.read_with(cx, |w, _| w.editor().cloned());
            editor.is_some_and(|editor| shown(&editor, cx).len() == 3)
        });
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        assert_eq!(editor.read_with(cx, |editor, _| editor.path().to_path_buf()), path);
        assert!(!notification_titles(&workbench, cx).contains(&"Cannot annotate".to_owned()));
    }

    /// A click on a line's entry shows that line's commit in the Git log, and sets no breakpoint.
    #[gpui_kit::test]
    async fn a_click_on_an_entry_shows_its_commit_in_the_git_log(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        let bob = git_in(&root, &["rev-parse", "HEAD"]);
        let at = column_point(&editor, 1, cx);
        cx.simulate_click(at, Modifiers::default());
        cx.run_until_parked();
        wait_long(cx, "the Git window", |cx| workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW)));
        let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
        wait_long(cx, "the commit is selected", |cx| {
            log.read_with(cx, |log, _| log.selected().map(|commit| commit.id.clone())) == Some(bob.clone())
        });
        let breakpoints = editor.read_with(cx, |editor, _| editor.marks(MarkLayer::Breakpoints).len());
        assert_eq!(breakpoints, 0, "the column's press is not the gutter's");
        assert_eq!(editor.read_with(cx, |editor, _| editor.selection()), 0..0, "the caret stays");
    }

    /// The column's menu on a committed line offers Show Commit in Git Log and Copy Revision Number,
    /// which copies the full commit id; a line changed since the commit is blamed as not committed
    /// once the text is still, and its menu offers Close Annotations only, which closes the column.
    #[gpui_kit::test]
    async fn the_columns_menu_copies_the_revision_and_closes_the_annotations(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        let open_menu = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| w.open_menu::<AnnotationCommand>()).expect("the column's menu is open")
        };

        let at = column_point(&editor, 1, cx);
        right_click(cx, at);
        let menu = open_menu(cx);
        assert_eq!(menu_rows(&menu, cx), ["Show Commit in Git Log", "Copy Revision Number", "-", "Close Annotations"]);
        choose(&menu, "Copy Revision Number", cx);
        let bob = git_in(&root, &["rev-parse", "HEAD"]);
        wait_long(cx, "the id is copied", |cx| {
            cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())) == Some(bob.clone())
        });

        // Typing in the third line: after a pause it is blamed on the unsaved text.
        let start = editor.read_with(cx, |editor, _| editor.offset_of(2, 0));
        editor.update(cx, |editor, cx| editor.select(start, start, cx));
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.simulate_input("x");
        wait_long(cx, "the edited line is blamed again", |cx| {
            editor.read_with(cx, |editor, _| editor.annotation(2).is_some_and(|row| row.uncommitted))
        });
        let rows = shown(&editor, cx);
        assert_eq!(rows.iter().map(|(line, ..)| *line).collect::<Vec<_>>(), [0, 1, 2], "{rows:?}");
        assert_eq!((rows[2].1.as_str(), rows[2].2.as_str()), ("", ""), "an uncommitted line shows nothing");

        let at = column_point(&editor, 2, cx);
        right_click(cx, at);
        let menu = open_menu(cx);
        assert_eq!(menu_rows(&menu, cx), ["Close Annotations"]);
        choose(&menu, "Close Annotations", cx);
        assert!(!annotated(&editor, cx));
        assert_eq!(toggled(cx), Some(false));
    }

    /// When the head commit moves (here the last commit is amended with another author), the column
    /// is blamed again without the text changing.
    #[gpui_kit::test]
    async fn a_new_head_commit_blames_the_file_again(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        let amend = ["commit", "-q", "--amend", "--no-edit", "--author=Cy Young <cy@example.test>"];
        git_in(&root, &[&amend[..], &["--date=2025-09-01T10:00:00Z"]].concat());
        workbench.update(cx, |w, cx| w.refresh_vcs(cx));
        wait_long(cx, "the amended commit is blamed", |cx| {
            editor.read_with(cx, |editor, _| editor.annotation(1).is_some_and(|row| row.author == "Cy"))
        });
        let cy = expected(&root, "HEAD", "Cy", cx);
        assert_eq!(shown(&editor, cx)[1], (1, cy.0, cy.1));
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "alpha\nBETA\ngamma\n");
    }

    /// The gutter's own menu says Close Annotations while the column shows, and choosing it closes
    /// the column.
    #[gpui_kit::test]
    async fn the_gutters_menu_closes_the_annotations(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = git_project(cx, two_authors);
        let editor = open_file(&workbench, &root, "b.txt", cx);
        run_action(cx, ANNOTATE, None);
        wait_long(cx, "the annotations", |cx| shown(&editor, cx).len() == 3);
        // Right of the column, on the line numbers.
        let at = editor.read_with(cx, |editor, cx| {
            let text = editor.position_of(0).expect("the first line is painted");
            point(text.x - px(crate::editor::view::FOLD_GUTTER) - px(12.0), text.y + editor.line_height(cx) / 2.0)
        });
        assert!(editor.read_with(cx, |editor, _| editor.annotation_line_at(at).is_none()), "outside the column");
        right_click(cx, at);
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<GutterCommand>()).expect("the gutter's menu is open");
        assert_eq!(menu_rows(&menu, cx)[0], "Close Annotations");
        choose(&menu, "Close Annotations", cx);
        wait_long(cx, "the column closes", |cx| !annotated(&editor, cx));
    }
}
