//! The diff window of a project: Show Diff on the changes of the Commit tool window (Enter, a
//! double click, its context menu, or the action) shows the diff of a changed file in front of the
//! window, with the other files of its group to move along. The texts of both sides load off the
//! window's thread: a revision from the repository, the working file from its open editor or from
//! disk. Jump to Source opens the file at the line shown and closes the diff; Escape closes it.
//!
//! The diff window is a layer of its own over the window, not the popup slot: the menus of its
//! toolbar open in the popup slot over it.

use super::services_window::on_workbench;
use super::{COMMIT_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::editor::view::HighlightSpan;
use crate::platform::actions::ActionRegistry;
use crate::platform::context::set_context;
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::vcs::commit_view::{CommitPanel, CommitPanelEvent};
use crate::vcs_diff::compare::action;
use crate::vcs_diff::dialog_view::{DiffDialog, DiffDialogEvent};
use crate::vcs_diff::diff_input::{DiffData, SideLoad, diff_key, side_file, side_load};
use crate::vcs_diff::diff_navigation::{ChainSearch, ChainStep};
use crate::vcs_diff::diff_options::{
    DIFF_OPTIONS_KEY, HighlightPolicy, IgnorePolicy, Side, idea_chunks, load_diff_options, save_diff_options,
};
use crate::vcs_diff::doc::{Direction, Doc};
use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, viewer_settings_menu};
use crate::vcs_diff::view::{DiffView, DiffViewEvent, map_line};
use crate::vcs_more::show_diff::{ChangesView, SHOW_DIFF, ShowDiffStart, ShownDiffs, show_diff_files, show_diff_start};
use crate::vcs_ops::actions::{PathContext, commit_window_focused};
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{
    AnyElement, App, Bounds, Context, CursorStyle, Entity, FocusHandle, Focusable, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, Size, Subscription, Task, WeakEntity,
    Window, div, prelude::*, px,
};
use ide_host::Host;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where the size the diff window was last resized to is kept, as `{"w":…,"h":…}`.
const SIZE_KEY: &str = "openidea.vcs.diffDialogSize";
/// The size the diff window opens at before it was ever resized, at most: the window less this room.
const DEFAULT_SIZE: (f32, f32) = (1100.0, 720.0);
const DEFAULT_ROOM: f32 = 64.0;
/// The room a diff window leaves inside the window, whatever its size.
const EDGE_ROOM: f32 = 24.0;
/// The smallest size the grip resizes the diff window to.
const MIN_SIZE: (f32, f32) = (480.0, 280.0);
/// The side of the resize grip at the bottom right corner.
const GRIP_SIZE: f32 = 16.0;
/// Where the grip's strokes start along its left and top edges.
const GRIP_STROKES: [f32; 2] = [2.7, 8.2];

/// The diff shown in front of the window, with what loads for it.
pub(super) struct DiffWindow {
    dialog: Entity<DiffDialog>,
    /// What had the keyboard before the diff window opened.
    restore: Option<FocusHandle>,
    /// The load of the shown diff's texts.
    loading: Option<Task<()>>,
    /// The search along the chain for the next or previous file with changes.
    finding: Option<Task<()>>,
    /// A diff of two revisions whose file is in the working tree: Jump to Source opens it there.
    working_copy: bool,
    /// Where the diff window is drawn, in window coordinates.
    bounds: Bounds<Pixels>,
    /// While the grip is dragged: where the press was and the size then.
    resizing: Option<(Point<Pixels>, Size<Pixels>)>,
    _events: Subscription,
}

/// The size a diff window opens at in a window of `viewport`: the size `stored` remembers, else the
/// usual one, kept inside the window.
fn opening_size(stored: Option<&str>, viewport: Size<Pixels>) -> Size<Pixels> {
    let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
    let remembered = stored.and_then(|text| serde_json::from_str::<Value>(text).ok()).and_then(|value| {
        let side = |key: &str| value.get(key).and_then(Value::as_f64).map(|n| n as f32);
        Some((side("w")?, side("h")?))
    });
    let (w, h) =
        remembered.unwrap_or((DEFAULT_SIZE.0.min(width - DEFAULT_ROOM), DEFAULT_SIZE.1.min(height - DEFAULT_ROOM)));
    gpui_kit::size(px(w.min(width - EDGE_ROOM).max(0.0)), px(h.min(height - EDGE_ROOM).max(0.0)))
}

/// The size the grip dragged from `start` (when the size was `from`) to `at` gives, in a window of
/// `viewport`: no smaller than the least size, no larger than the window allows.
fn dragged_size(from: Size<Pixels>, start: Point<Pixels>, at: Point<Pixels>, viewport: Size<Pixels>) -> Size<Pixels> {
    let clamp = |size: Pixels, delta: Pixels, least: f32, room: Pixels| {
        px(f32::from(size + delta).max(least).min(f32::from(room) - EDGE_ROOM))
    };
    gpui_kit::size(
        clamp(from.width, at.x - start.x, MIN_SIZE.0, viewport.width),
        clamp(from.height, at.y - start.y, MIN_SIZE.1, viewport.height),
    )
}

/// What the load of a diff brings back.
pub(super) struct LoadedSides {
    pub(super) left: String,
    pub(super) right: String,
    /// A side on disk is not text.
    pub(super) binary: bool,
    /// Neither side is a file, and the diff's path is one in the working tree.
    pub(super) working_copy: bool,
}

/// The text `load` stands for. `open` holds the texts of the open editors by path: a working file
/// that is open shows its unsaved edits.
pub(super) async fn load_text(
    host: &Host,
    window: &str,
    load: SideLoad,
    open: &HashMap<String, String>,
) -> Result<String, String> {
    match load {
        SideLoad::Text(text) => Ok(text),
        SideLoad::WorkingText { path } => match open.get(&path) {
            Some(text) => Ok(text.clone()),
            None => ide_host::ops::fs::read_text(host, window, PathBuf::from(path)).await.map_err(|e| e.message),
        },
        // A file that is not in that revision is empty.
        SideLoad::FileAt { path, rev } => ide_host::ops::vcs::vcs_file_at(host, window, PathBuf::from(path), Some(rev))
            .await
            .map(Option::unwrap_or_default)
            .map_err(|e| e.message),
        SideLoad::History { id } => ide_host::ops::doc::history_content(host, window, id).map_err(|e| e.message),
    }
}

/// The paths of `data`'s sides that are files on disk.
fn disk_paths(data: &DiffData) -> Vec<String> {
    let mut paths: Vec<String> = [side_file(&data.left, &data.path), side_file(&data.right, &data.path)]
        .into_iter()
        .flatten()
        .map(str::to_owned)
        .collect();
    paths.dedup();
    paths
}

/// Loads both sides of `data`, and probes its files on disk for binary content.
pub(super) async fn load_sides(
    host: &Host,
    window: &str,
    data: &DiffData,
    open: &HashMap<String, String>,
) -> Result<LoadedSides, String> {
    let left = load_text(host, window, side_load(&data.path, &data.left), open).await?;
    let right = load_text(host, window, side_load(&data.path, &data.right), open).await?;
    let mut binary = false;
    let disk = disk_paths(data);
    for path in &disk {
        binary |=
            ide_host::ops::bytes::fs_probe(host, window, PathBuf::from(path)).await.map_err(|e| e.message)?.binary;
    }
    // A diff of two revisions jumps to the working copy of its file while there is one; a file that
    // is gone or unreadable has none.
    let working_copy =
        disk.is_empty() && ide_host::ops::bytes::fs_probe(host, window, PathBuf::from(&data.path)).await.is_ok();
    Ok(LoadedSides { left, right, binary, working_copy })
}

/// Syntax colours of the two sides of a diff of `path`, from the grammar of the file's language
/// (none for a language without one); the editor colours the same file the same way.
pub(super) fn side_colours(host: &Host, path: &str, left: &str, right: &str) -> [Vec<HighlightSpan>; 2] {
    let first_line = right.lines().next().or_else(|| left.lines().next());
    let language = host.languages.detect(Path::new(path), first_line);
    let syntax = host.syntax();
    let spans = |text: &str| -> Vec<HighlightSpan> {
        let Some(highlights) = syntax.highlights(&language, text) else { return Vec::new() };
        let (triples, _) = highlights.data.as_chunks::<3>();
        triples
            .iter()
            .filter_map(|&[from, to, kind]| {
                Some(HighlightSpan { from, to, kind: *highlights.kinds.get(kind as usize)? })
            })
            .collect()
    };
    [spans(left), spans(right)]
}

/// The neighbour of `from` along its chain, in `direction`, that has changes to show.
pub(super) async fn find_file(
    host: &Host,
    window: &str,
    mut search: ChainSearch,
    open: &HashMap<String, String>,
) -> Option<Box<DiffData>> {
    loop {
        match search.step() {
            ChainStep::Probe { paths } => {
                let mut probes = Some(Vec::with_capacity(paths.len()));
                for path in paths {
                    match ide_host::ops::bytes::fs_probe(host, window, PathBuf::from(path)).await {
                        Ok(probe) => {
                            if let Some(probes) = probes.as_mut() {
                                probes.push(probe);
                            }
                        }
                        // A file of a stale chain that is gone or unreadable is passed over.
                        Err(_) => {
                            probes = None;
                            break;
                        }
                    }
                }
                search.probed(probes.as_deref());
            }
            ChainStep::Load { left, right } => {
                let texts =
                    match (load_text(host, window, left, open).await, load_text(host, window, right, open).await) {
                        (Ok(left), Ok(right)) => Some((left, right)),
                        // A side that cannot be loaded has no changes to show: the search moves on.
                        _ => None,
                    };
                search.loaded(texts.as_ref().map(|(left, right)| (left.as_str(), right.as_str())));
            }
            ChainStep::Done(found) => return found,
        }
    }
}

/// Registers Show Diff and the diff viewer's actions the window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(SHOW_DIFF, |call, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_diff(call.arg, window, cx))
    });
    let commands = [
        (action::NEXT_DIFFERENCE, DiffCommand::NextDifference, Some(Direction::Next)),
        (action::PREV_DIFFERENCE, DiffCommand::PreviousDifference, Some(Direction::Previous)),
        (action::JUMP_TO_SOURCE, DiffCommand::JumpToSource, None),
    ];
    for (id, command, merge) in commands {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, |workbench, window, cx| {
                // The merge editor that has the keyboard moves between its own changes and jumps to its
                // own file.
                let moved = match merge {
                    Some(direction) => workbench.merge_go_to_change(direction, window, cx),
                    None => command == DiffCommand::JumpToSource && workbench.merge_jump_to_source(window, cx),
                };
                if !moved {
                    workbench.run_diff_command(command, cx);
                }
            })
        });
    }
    for (id, direction) in [(action::NEXT_FILE, Direction::Next), (action::PREV_FILE, Direction::Previous)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, |workbench, window, cx| workbench.step_diff_file(direction, window, cx))
        });
    }
    registry.handle(action::SHOW_SETTINGS, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            // The merge editor that has the keyboard shows its own settings.
            if !workbench.merge_show_settings(window, cx) {
                workbench.show_diff_settings(window, cx);
            }
        })
    });
}

impl Workbench {
    /// The diff window, while one is shown.
    pub fn diff_dialog(&self) -> Option<&Entity<DiffDialog>> {
        self.diff_window.as_ref().map(|open| &open.dialog)
    }

    /// The viewer of the diff window, else of the active diff tab.
    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone()).or_else(|| self.diff_tab_viewer(cx))
    }

    /// What the changes of the Commit tool window ask for.
    pub(super) fn commit_panel_event(&mut self, event: &CommitPanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            CommitPanelEvent::Open(path) => self.open_project_file(path.clone(), window, cx),
            CommitPanelEvent::ShowDiff(path) => {
                let arg = Value::String(path.to_string_lossy().into_owned());
                self.show_diff(Some(&arg), window, cx);
            }
            CommitPanelEvent::ShowMenu(position) => self.show_changes_menu(*position, window, cx),
            CommitPanelEvent::SelectTab(tab) => self.select_commit_tab(tab, window, cx),
            CommitPanelEvent::MoveToChangelist { paths, list } => self.move_dropped_changes(paths, list, cx),
        }
    }

    fn commit_panel(&self) -> Option<Entity<CommitPanel>> {
        self.tool_window_view(COMMIT_TOOL_WINDOW).and_then(|view| view.downcast::<CommitPanel>().ok())
    }

    /// Show Diff: the argument's paths, else the selected change while the Commit window is the active
    /// tool window, else the Project view's selection, else the active file. Under the modal commit
    /// interface the diff opens in front of the window with the other files of the change's group as its
    /// chain; under the non-modal one each file opens in an editor tab, the preview tab for a single one.
    pub fn show_diff(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.commit_panel();
        let active_tool_window = self.tool_windows().and_then(|layout| layout.focused()).map(str::to_owned);
        let (selection, keys, status, root) = match &panel {
            Some(panel) => {
                let panel = panel.read(cx);
                (
                    panel.selected_file().into_iter().collect::<Vec<_>>(),
                    panel.selected_key().map(str::to_owned).into_iter().collect::<Vec<_>>(),
                    panel.status().to_vec(),
                    Some(panel.root().to_owned()),
                )
            }
            None => {
                (Vec::new(), Vec::new(), Vec::new(), self.project_root(cx).map(|r| r.to_string_lossy().into_owned()))
            }
        };
        let project: Vec<String> =
            self.project_tree().and_then(|tree| tree.read(cx).selected_path().map(str::to_owned)).into_iter().collect();
        let active = self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned());
        let context = PathContext {
            commit_selection: &selection,
            commit_window_focused: commit_window_focused(active_tool_window.as_deref(), ""),
            project_selection: &project,
            active_path: active.as_deref(),
        };
        let paths = match show_diff_start(arg, context, self.shelf_focused(window, cx)) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // The Shelf tab has the keyboard: the diff of its selection.
            ShowDiffStart::Shelf => return self.show_shelf_diff(None, false, false, window, cx),
        };
        let view = ChangesView {
            status: &status,
            root: root.as_deref(),
            roots: &[],
            lists: Some(self.changelists()),
            group_by_dir: true,
            group_by_staging: false,
            selection_keys: &keys,
        };
        let non_modal = crate::platform::settings::effective(&self.label, cx).vcs.commit.non_modal;
        let i18n = &mut cx.default_global::<Localization>().0;
        let data = match show_diff_files(arg, &paths, view, context, non_modal, i18n) {
            Some(ShownDiffs::FrontWindow(data)) => *data,
            Some(ShownDiffs::Tabs(tabs)) => return self.open_diff_tabs(tabs, window, cx),
            None => return,
        };
        self.open_diff(data, window, cx);
    }

    /// Shows `data` in the diff window, which takes the keyboard, and loads its texts.
    pub fn open_diff(&mut self, data: DiffData, window: &mut Window, cx: &mut Context<Self>) {
        let options = load_diff_options(cx.default_global::<UiStorage>().get(DIFF_OPTIONS_KEY));
        let shown = data.clone();
        let dialog = cx.new(|cx| DiffDialog::new(shown, options, cx));
        let viewer = dialog.read(cx).viewer().clone();
        viewer.update(cx, |viewer, cx| viewer.follow_editor_settings(&self.label, cx));
        let events = cx.subscribe_in(&dialog, window, Self::diff_dialog_event);
        // Another diff shown in its place gives the keyboard back to where it was before the first,
        // and keeps its place on the screen.
        let (restore, bounds) = match self.diff_window.take() {
            Some(previous) => (previous.restore, previous.bounds),
            None => {
                let viewport = window.viewport_size();
                let size = opening_size(cx.default_global::<UiStorage>().get(SIZE_KEY), viewport);
                let origin =
                    gpui_kit::point((viewport.width - size.width) / 2.0, (viewport.height - size.height) / 2.0);
                (window.focused(cx), Bounds { origin, size })
            }
        };
        let viewer_focus = dialog.read(cx).viewer().read(cx).focus_handle(cx);
        window.focus(&viewer_focus, cx);
        self.diff_window = Some(DiffWindow {
            dialog,
            restore,
            loading: None,
            finding: None,
            working_copy: false,
            bounds,
            resizing: None,
            _events: events,
        });
        self.load_diff(data, cx);
        cx.notify();
    }

    /// Closes the diff window; `restore` gives the keyboard back to what had it before.
    fn close_diff(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.diff_window.take() else { return };
        if restore && let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// The texts of the open editors, by path.
    pub(super) fn open_texts(&self, cx: &App) -> HashMap<String, String> {
        self.editors()
            .into_iter()
            .map(|editor| {
                let editor = editor.read(cx);
                (editor.path().to_string_lossy().into_owned(), editor.text())
            })
            .collect()
    }

    /// Loads both sides of `data` off the window's thread and hands them to the diff window, if it
    /// still shows `data` then.
    fn load_diff(&mut self, data: DiffData, cx: &mut Context<Self>) {
        let open = self.open_texts(cx);
        let Some(shown) = self.diff_window.as_ref().map(|open| open.dialog.downgrade()) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        let key = diff_key(&data);
        let work = backend.with_host(move |host| async move {
            let loaded = load_sides(&host, &label, &data, &open).await;
            let colours = match &loaded {
                Ok(LoadedSides { left, right, binary: false, .. }) => {
                    let (path, left, right) = (data.path.clone(), left.clone(), right.clone());
                    let colour = move || side_colours(&host, &path, &left, &right);
                    tokio::task::spawn_blocking(colour).await.unwrap_or_default()
                }
                _ => Default::default(),
            };
            (loaded, colours)
        });
        let task = cx.spawn(async move |workbench, cx| {
            let (loaded, colours) = match work.await {
                Ok((loaded, colours)) => (Ok(loaded), colours),
                Err(error) => (Err(error), Default::default()),
            };
            let _ = workbench.update(cx, |workbench, cx| workbench.diff_loaded(&shown, &key, loaded, colours, cx));
        });
        if let Some(open) = self.diff_window.as_mut() {
            open.loading = Some(task);
        }
    }

    fn diff_loaded(
        &mut self,
        shown: &WeakEntity<DiffDialog>,
        key: &str,
        loaded: anyhow::Result<Result<LoadedSides, String>>,
        colours: [Vec<HighlightSpan>; 2],
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.diff_dialog().filter(|dialog| dialog.downgrade() == *shown).cloned() else { return };
        if diff_key(dialog.read(cx).data()) != key {
            return;
        }
        match loaded.map_err(|error| error.to_string()).and_then(|loaded| loaded) {
            Ok(LoadedSides { left, right, binary, working_copy }) => {
                dialog.update(cx, |dialog, cx| dialog.loaded(&left, &right, binary, cx));
                let [a, b] = colours;
                let viewer = dialog.read(cx).viewer().clone();
                viewer.update(cx, |viewer, cx| {
                    viewer.set_highlights(Side::A, a, cx);
                    viewer.set_highlights(Side::B, b, cx);
                    if working_copy {
                        viewer.set_has_source(true, cx);
                    }
                });
                if let Some(open) = self.diff_window.as_mut() {
                    open.working_copy = working_copy;
                }
            }
            Err(error) => {
                let mut notice = vcs_notice(NotificationLevel::Error, "Cannot load the diff");
                notice.body = Some(error);
                self.notify(notice, cx);
            }
        }
    }

    /// Looks along the chain of `from` for the next file with changes in `direction`, and shows it.
    fn find_diff_file(&mut self, from: DiffData, direction: Direction, cx: &mut Context<Self>) {
        let open = self.open_texts(cx);
        let Some(viewer) = self.diff_viewer(cx) else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let search = ChainSearch::new(&from, direction, viewer.read(cx).options().ignore);
        let label = self.label.clone();
        let Some(shown) = self.diff_dialog().map(Entity::downgrade) else { return };
        let work = backend.with_host(move |host| async move { find_file(&host, &label, search, &open).await });
        let task = cx.spawn(async move |workbench, cx| {
            let found = work.await;
            let _ = workbench.update(cx, |workbench, cx| match found {
                Ok(Some(data)) => {
                    if let Some(dialog) = workbench.diff_dialog().filter(|dialog| dialog.downgrade() == shown).cloned()
                    {
                        dialog.update(cx, |dialog, cx| dialog.show(*data, cx));
                    }
                }
                // No file further along has changes to show.
                Ok(None) => {}
                Err(error) => {
                    let mut notice = vcs_notice(NotificationLevel::Error, "Cannot load the diff");
                    notice.body = Some(error.to_string());
                    workbench.notify(notice, cx);
                }
            });
        });
        if let Some(open) = self.diff_window.as_mut() {
            open.finding = Some(task);
        }
    }

    /// Compare Next / Previous File in the diff window, else in the active diff tab.
    fn step_diff_file(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        match self.diff_dialog().map(|dialog| dialog.read(cx).data().clone()) {
            Some(data) => self.find_diff_file(data, direction, cx),
            None => self.step_diff_tab_file(direction, window, cx),
        }
    }

    /// Carries out a command of the diff viewer's toolbar, its menus or its actions.
    fn run_diff_command(&mut self, command: DiffCommand, cx: &mut Context<Self>) {
        if let Some(viewer) = self.diff_viewer(cx) {
            viewer.update(cx, |viewer, cx| viewer.run_command(command, cx));
        }
    }

    /// Diff Settings: the viewer's settings menu, at the top right of the diff window, else under the
    /// toolbar of the active diff tab.
    fn show_diff_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diff_dialog().is_none() {
            self.show_diff_tab_settings(window, cx);
            return;
        }
        let Some(viewer) = self.diff_viewer(cx) else { return };
        let items = {
            let viewer = viewer.read(cx);
            viewer_settings_menu(&viewer.options(), viewer.soft_wraps())
        };
        let Some(bounds) = self.diff_window.as_ref().map(|open| open.bounds) else { return };
        let position = gpui_kit::point((bounds.right() - px(260.0)).max(px(0.0)), bounds.top() + px(72.0));
        self.show_menu(
            &items,
            position,
            |workbench, command, _, cx| workbench.run_diff_command(command, cx),
            window,
            cx,
        );
    }

    fn diff_dialog_event(
        &mut self,
        dialog: &Entity<DiffDialog>,
        event: &DiffDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.diff_dialog() != Some(dialog) {
            return;
        }
        match event {
            DiffDialogEvent::Load(data) => self.load_diff((**data).clone(), cx),
            DiffDialogEvent::FindFile { from, direction } => self.find_diff_file((**from).clone(), *direction, cx),
            DiffDialogEvent::Menu(DiffToolbarEvent::ShowMenu { items, position, .. }) => {
                self.show_menu(
                    items,
                    *position,
                    |workbench, command, _, cx| workbench.run_diff_command(command, cx),
                    window,
                    cx,
                );
            }
            DiffDialogEvent::Viewer(event) => match event {
                &DiffViewEvent::JumpToSource { side, line, col } => {
                    self.jump_to_diff_source(side, line, col, window, cx)
                }
                DiffViewEvent::Refresh => {
                    let data = dialog.read(cx).data().clone();
                    self.load_diff(data, cx);
                }
                DiffViewEvent::OptionsChanged(options) => match save_diff_options(options) {
                    Ok(json) => cx.default_global::<UiStorage>().set(DIFF_OPTIONS_KEY, json),
                    Err(error) => tracing::error!("cannot store the diff viewer settings: {error}"),
                },
                &DiffViewEvent::OpenFile(direction) => {
                    let data = dialog.read(cx).data().clone();
                    self.find_diff_file(data, direction, cx);
                }
                // An arrow, a hover Revert or Accept Left / Right Side on an editable side.
                DiffViewEvent::Apply { target, change } => self.apply_diff_edit(*target, change, cx),
                // The panes have no menu of their own, and the diff window offers no Maximize Editor.
                DiffViewEvent::ShowMenu { .. } | DiffViewEvent::Maximize => {}
            },
            DiffDialogEvent::Close => self.close_diff(true, window, cx),
        }
    }

    /// Jump to Source: opens the file of `side` at its `line`; on a revision side, the working file of the
    /// other side at the line it shows there. A diff of two revisions opens the working copy of its file,
    /// at the line the shown one has become there. The diff window closes, and the editor keeps the
    /// keyboard.
    fn jump_to_diff_source(&mut self, side: Side, line: u32, col: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.diff_dialog().cloned() else { return };
        let (data, viewer) = {
            let dialog = dialog.read(cx);
            (dialog.data().clone(), dialog.viewer().clone())
        };
        let of = |side: Side| if side == Side::A { &data.left } else { &data.right };
        let target = match side_file(of(side), &data.path) {
            Some(path) => Some((path.to_owned(), line, col)),
            None => side_file(of(side.other()), &data.path).map(|path| {
                let mapped = viewer.read(cx).map_line(side, line as usize);
                (path.to_owned(), u32::try_from(mapped).unwrap_or(u32::MAX), 0)
            }),
        };
        if let Some((path, line, col)) = target {
            self.close_diff(false, window, cx);
            self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
            return;
        }
        // A diff of two revisions without a working copy has no file to open.
        if !self.diff_window.as_ref().is_some_and(|open| open.working_copy) {
            return;
        }
        let shown = viewer.read(cx).side_text(side);
        self.jump_to_working_copy(data.path.clone(), shown, line, col, window, cx);
        self.close_diff(false, window, cx);
    }

    /// Opens the working copy of `path` at the line that `line` of `shown`, another version of the file,
    /// has become there: through a diff of the two, once the working text is loaded off the window's
    /// thread. The column is kept when the line reads the same in both.
    pub(super) fn jump_to_working_copy(
        &mut self,
        path: String,
        shown: String,
        line: u32,
        col: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.open_texts(cx);
        let label = self.label.clone();
        let file = path.clone();
        let work = cx.try_global::<Backend>().map(|backend| {
            backend.with_host(move |host| async move {
                load_text(&host, &label, SideLoad::WorkingText { path: file }, &open).await
            })
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let working = match work {
                Some(work) => work.await.map_err(|error| error.to_string()).and_then(|loaded| loaded),
                None => Err("no backend".to_owned()),
            };
            let (line, col) = match &working {
                Ok(working) => {
                    let (from, to) = (Doc::new(&shown), Doc::new(&working.replace("\r\n", "\n")));
                    let chunks = idea_chunks(&from, &to, IgnorePolicy::None, HighlightPolicy::Lines).chunks;
                    let mapped = map_line(&chunks, Side::A, [&from, &to], line as usize);
                    let same = from.lines() > line as usize
                        && mapped < to.lines()
                        && from.line_text(from.line(line as usize + 1)) == to.line_text(to.line(mapped + 1));
                    (u32::try_from(mapped).unwrap_or(u32::MAX), if same { col } else { 0 })
                }
                // The working copy cannot be read: its file opens at the line shown, as it would be.
                Err(_) => (line, col),
            };
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                workbench.open_project_file_at(PathBuf::from(path), line, col, window, cx);
            });
        })
        .detach();
    }

    /// Publishes whether the project is under version control and whether the diff window has the
    /// keyboard.
    pub(super) fn publish_diff_context(&self, window: &mut Window, cx: &mut App) {
        let focused =
            self.diff_dialog().is_some_and(|dialog| dialog.read(cx).focus_handle(cx).contains_focused(window, cx))
                || self.merge_editor_focused(window, cx)
                || self.diff_tab_focused(window, cx);
        set_context(window, cx, "diffEditorFocus", focused);
        let versioned = self.project_tree().is_some()
            && cx
                .try_global::<Backend>()
                .and_then(|backend| backend.host.project(&self.label).ok())
                .is_some_and(|project| !project.repos().is_empty());
        set_context(window, cx, "vcsAvailable", versioned);
    }

    /// A press on the diff window's grip starts resizing it.
    fn start_diff_resize(&mut self, at: Point<Pixels>) {
        if let Some(open) = self.diff_window.as_mut() {
            open.resizing = Some((at, open.bounds.size));
        }
    }

    /// The pointer moved while the grip is dragged: the diff window takes the size it points at, its
    /// top left corner staying where it is. A move without the button ends the drag.
    fn diff_resize_moved(&mut self, event: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        let Some(open) = self.diff_window.as_mut() else { return };
        let Some((start, from)) = open.resizing else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            return self.end_diff_resize(cx);
        }
        open.bounds.size = dragged_size(from, start, event.position, window.viewport_size());
        cx.notify();
    }

    /// The grip is let go: the size is remembered for the next diff window.
    fn end_diff_resize(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.diff_window.as_mut() else { return };
        if open.resizing.take().is_none() {
            return;
        }
        let size = open.bounds.size;
        let stored = serde_json::json!({ "w": f32::from(size.width), "h": f32::from(size.height) });
        cx.default_global::<UiStorage>().set(SIZE_KEY, stored.to_string());
    }

    /// The diff window over the window, while one is shown, with its resize grip.
    pub(super) fn diff_window_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.diff_window.as_ref()?;
        let bounds = open.bounds;
        let stroke = theme::color(cx, "fg-faint");
        // Two short diagonal strokes across the corner, as a resize grip is drawn.
        let strokes = gpui_kit::canvas(
            |_, _, _| {},
            move |corner: Bounds<Pixels>, _, window, _| {
                for reach in GRIP_STROKES {
                    let mut line = PathBuilder::stroke(px(1.2));
                    line.move_to(corner.origin + gpui_kit::point(px(reach), px(GRIP_SIZE)));
                    line.line_to(corner.origin + gpui_kit::point(px(GRIP_SIZE), px(reach)));
                    if let Ok(path) = line.build() {
                        window.paint_path(path, stroke);
                    }
                }
            },
        )
        .size_full();
        let grip = div()
            .id("diff-window-resize")
            .debug_selector(|| "diff-window-resize".to_owned())
            .absolute()
            .right_0()
            .bottom_0()
            .size(px(GRIP_SIZE))
            .cursor(CursorStyle::ResizeUpLeftDownRight)
            .opacity(0.55)
            .hover(|grip| grip.opacity(1.0))
            .child(strokes)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|workbench, event: &MouseDownEvent, _, cx| {
                    workbench.start_diff_resize(event.position);
                    cx.stop_propagation();
                }),
            );
        let frame = div()
            .absolute()
            .left(bounds.left())
            .top(bounds.top())
            .w(bounds.size.width)
            .h(bounds.size.height)
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .shadow(theme::shadow(cx, "shadow-dialog"))
            .overflow_hidden()
            .child(open.dialog.clone())
            .child(grip);
        let layer = div()
            .id("diff-window")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_move(cx.listener(|workbench, event: &MouseMoveEvent, window, cx| {
                workbench.diff_resize_moved(event, window, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|workbench, _: &MouseUpEvent, _, cx| workbench.end_diff_resize(cx)),
            )
            .child(frame);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }
}
