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
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, Side, load_diff_options, save_diff_options};
use crate::vcs_diff::doc::Direction;
use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, settings_menu};
use crate::vcs_diff::view::{DiffView, DiffViewEvent};
use crate::vcs_more::show_diff::{ChangesView, SHOW_DIFF, ShowDiffStart, ShownDiffs, show_diff_files, show_diff_start};
use crate::vcs_ops::actions::{PathContext, commit_window_focused};
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, Pixels, Point, Subscription, Task,
    WeakEntity, Window, div, prelude::*, px,
};
use ide_host::Host;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// How far the diff window keeps from the edges of the window.
const MARGIN_X: f32 = 72.0;
const MARGIN_Y: f32 = 56.0;

/// The diff shown in front of the window, with what loads for it.
pub(super) struct DiffWindow {
    dialog: Entity<DiffDialog>,
    /// What had the keyboard before the diff window opened.
    restore: Option<FocusHandle>,
    /// The load of the shown diff's texts.
    loading: Option<Task<()>>,
    /// The search along the chain for the next or previous file with changes.
    finding: Option<Task<()>>,
    _events: Subscription,
}

/// Both texts of a diff, and whether a side on disk is not text.
type LoadedSides = (String, String, bool);

/// The text `load` stands for. `open` holds the texts of the open editors by path: a working file
/// that is open shows its unsaved edits.
async fn load_text(
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
async fn load_sides(
    host: &Host,
    window: &str,
    data: &DiffData,
    open: &HashMap<String, String>,
) -> Result<LoadedSides, String> {
    let left = load_text(host, window, side_load(&data.path, &data.left), open).await?;
    let right = load_text(host, window, side_load(&data.path, &data.right), open).await?;
    let mut binary = false;
    for path in disk_paths(data) {
        binary |=
            ide_host::ops::bytes::fs_probe(host, window, PathBuf::from(path)).await.map_err(|e| e.message)?.binary;
    }
    Ok((left, right, binary))
}

/// The neighbour of `from` along its chain, in `direction`, that has changes to show.
async fn find_file(
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
        (action::NEXT_DIFFERENCE, DiffCommand::NextDifference),
        (action::PREV_DIFFERENCE, DiffCommand::PreviousDifference),
        (action::JUMP_TO_SOURCE, DiffCommand::JumpToSource),
    ];
    for (id, command) in commands {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, |workbench, _, cx| workbench.run_diff_command(command, cx))
        });
    }
    for (id, direction) in [(action::NEXT_FILE, Direction::Next), (action::PREV_FILE, Direction::Previous)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, |workbench, _, cx| workbench.step_diff_file(direction, cx))
        });
    }
    registry.handle(action::SHOW_SETTINGS, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_diff_settings(window, cx))
    });
}

impl Workbench {
    /// The diff window, while one is shown.
    pub fn diff_dialog(&self) -> Option<&Entity<DiffDialog>> {
        self.diff_window.as_ref().map(|open| &open.dialog)
    }

    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone())
    }

    /// What the changes of the Commit tool window ask for.
    pub(super) fn commit_panel_event(&mut self, event: &CommitPanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            CommitPanelEvent::Open(path) => self.open_project_file(path.clone(), window, cx),
            CommitPanelEvent::ShowDiff(path) => {
                let arg = Value::String(path.to_string_lossy().into_owned());
                self.show_diff(Some(&arg), window, cx);
            }
            CommitPanelEvent::ShowMenu(position) => self.show_change_menu(*position, window, cx),
        }
    }

    /// The context menu of the selected change: Show Diff, and Jump to Source unless it was deleted.
    fn show_change_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.commit_panel() else { return };
        let (file, opens) = {
            let panel = panel.read(cx);
            (panel.selected_file(), panel.selected_file_opens())
        };
        let Some(file) = file else { return };
        let i18n = &cx.default_global::<Localization>().0;
        let items = vec![
            MenuItem::action_with(SHOW_DIFF, Value::String(file.clone())),
            MenuItem::entry(i18n.tx("Jump to Source"), opens.then(|| PathBuf::from(&file)))
                .icon("external-link")
                .shortcut("F4"),
        ];
        self.show_menu(
            &items,
            position,
            |workbench, path, window, cx| workbench.open_project_file(path, window, cx),
            window,
            cx,
        );
    }

    fn commit_panel(&self) -> Option<Entity<CommitPanel>> {
        self.tool_window_view(COMMIT_TOOL_WINDOW).and_then(|view| view.downcast::<CommitPanel>().ok())
    }

    /// Show Diff: the argument's paths, else the selected change while the Commit window is the active
    /// tool window, else the Project view's selection, else the active file. The diff opens in front of
    /// the window with the other files of the change's group as its chain.
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
        let paths = match show_diff_start(arg, context, false) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // There is no Shelf view to have the focus.
            ShowDiffStart::Shelf => return,
        };
        let view = ChangesView {
            status: &status,
            root: root.as_deref(),
            roots: &[],
            lists: None,
            group_by_dir: true,
            group_by_staging: false,
            selection_keys: &keys,
        };
        let i18n = &mut cx.default_global::<Localization>().0;
        let data = match show_diff_files(arg, &paths, view, context, false, i18n) {
            Some(ShownDiffs::FrontWindow(data)) => *data,
            Some(ShownDiffs::Tabs(tabs)) => match tabs.into_iter().next() {
                Some(tab) => tab.data,
                None => return,
            },
            None => return,
        };
        self.open_diff(data, window, cx);
    }

    /// Shows `data` in the diff window, which takes the keyboard, and loads its texts.
    pub fn open_diff(&mut self, data: DiffData, window: &mut Window, cx: &mut Context<Self>) {
        let options = load_diff_options(cx.default_global::<UiStorage>().get(DIFF_OPTIONS_KEY));
        let shown = data.clone();
        let dialog = cx.new(|cx| DiffDialog::new(shown, options, cx));
        let events = cx.subscribe_in(&dialog, window, Self::diff_dialog_event);
        // Another diff shown in its place gives the keyboard back to where it was before the first.
        let restore = match self.diff_window.take() {
            Some(previous) => previous.restore,
            None => window.focused(cx),
        };
        let viewer_focus = dialog.read(cx).viewer().read(cx).focus_handle(cx);
        window.focus(&viewer_focus, cx);
        self.diff_window = Some(DiffWindow { dialog, restore, loading: None, finding: None, _events: events });
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
    fn open_texts(&self, cx: &App) -> HashMap<String, String> {
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
        let work = backend.with_host(move |host| async move { load_sides(&host, &label, &data, &open).await });
        let task = cx.spawn(async move |workbench, cx| {
            let loaded = work.await;
            let _ = workbench.update(cx, |workbench, cx| workbench.diff_loaded(&shown, &key, loaded, cx));
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
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.diff_dialog().filter(|dialog| dialog.downgrade() == *shown).cloned() else { return };
        if diff_key(dialog.read(cx).data()) != key {
            return;
        }
        match loaded.map_err(|error| error.to_string()).and_then(|loaded| loaded) {
            Ok((left, right, binary)) => dialog.update(cx, |dialog, cx| dialog.loaded(&left, &right, binary, cx)),
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

    /// Compare Next / Previous File in the diff window.
    fn step_diff_file(&mut self, direction: Direction, cx: &mut Context<Self>) {
        if let Some(data) = self.diff_dialog().map(|dialog| dialog.read(cx).data().clone()) {
            self.find_diff_file(data, direction, cx);
        }
    }

    /// Carries out a command of the diff viewer's toolbar, its menus or its actions.
    fn run_diff_command(&mut self, command: DiffCommand, cx: &mut Context<Self>) {
        if let Some(viewer) = self.diff_viewer(cx) {
            viewer.update(cx, |viewer, cx| viewer.run_command(command, cx));
        }
    }

    /// Diff Settings: the viewer's settings menu, at the top right of the diff window.
    fn show_diff_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = self.diff_viewer(cx) else { return };
        let items = settings_menu(&viewer.read(cx).options());
        let width = f32::from(window.viewport_size().width);
        let position = gpui_kit::point(px((width - MARGIN_X - 260.0).max(0.0)), px(MARGIN_Y + 72.0));
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
                // Both panes are read-only here, and have no menu of their own.
                DiffViewEvent::Apply { .. } | DiffViewEvent::ShowMenu { .. } => {}
            },
            DiffDialogEvent::Close => self.close_diff(true, window, cx),
        }
    }

    /// Jump to Source: opens the file of `side` at its `line`; on a revision side, the working file of the
    /// other side at the line it shows there. The diff window closes, and the editor keeps the keyboard.
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
        // A diff of two revisions has no file to open.
        let Some((path, line, col)) = target else { return };
        self.close_diff(false, window, cx);
        self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
    }

    /// Publishes whether the project is under version control and whether the diff window has the
    /// keyboard.
    pub(super) fn publish_diff_context(&self, window: &mut Window, cx: &mut App) {
        let focused =
            self.diff_dialog().is_some_and(|dialog| dialog.read(cx).focus_handle(cx).contains_focused(window, cx));
        set_context(window, cx, "diffEditorFocus", focused);
        let versioned = self.project_tree().is_some()
            && cx
                .try_global::<Backend>()
                .and_then(|backend| backend.host.project(&self.label).ok())
                .is_some_and(|project| !project.repos().is_empty());
        set_context(window, cx, "vcsAvailable", versioned);
    }

    /// The diff window over the window, while one is shown.
    pub(super) fn diff_window_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.diff_dialog()?.clone();
        let window = div()
            .absolute()
            .top(px(MARGIN_Y))
            .bottom(px(MARGIN_Y))
            .left(px(MARGIN_X))
            .right(px(MARGIN_X))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .overflow_hidden()
            .child(dialog);
        let layer = div().id("diff-window").absolute().inset_0().occlude().child(window);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }
}
