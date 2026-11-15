//! Diffs in editor tabs. With the non-modal commit interface Show Diff opens the diff of each file
//! in an editor tab of its own instead of the diff window, the preview tab when it is the only one.
//! A diff tab is titled by its file and the revisions compared; it loads both sides off the
//! window's thread as the diff window does, loads them again on Refresh, stores the viewer
//! settings the user changes, and jumps to the source as the diff window does, staying open.
//! Past the first or last difference it moves along its chain: the next file with changes to show
//! opens in the preview tab. Its Maximize Editor hides every tool window, and Restore Size brings
//! them back; the button follows the layout however it changes. Closing the tab drops its view.

use super::Workbench;
use super::diff_window::{LoadedSides, find_file, load_sides, side_colours};
use super::tab_views::TabView;
use super::tool_windows::ToolWindowLayout;
use crate::backend::Backend;
use crate::editor::tabs::{EditorInput, OpenTabOptions};
use crate::editor::view::HighlightSpan;
use crate::platform::storage::UiStorage;
use crate::vcs_diff::diff_input::{DIFF_KIND, DiffData, diff_input, diff_key, diff_title, side_file};
use crate::vcs_diff::diff_navigation::ChainSearch;
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, Side, load_diff_options, save_diff_options};
use crate::vcs_diff::doc::Direction;
use crate::vcs_diff::tab_view::{DiffTabEvent, DiffTabView};
use crate::vcs_diff::toolbar_view::{DiffToolbarEvent, viewer_settings_menu};
use crate::vcs_diff::view::{DiffView, DiffViewEvent};
use crate::vcs_more::show_diff::DiffTab;
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{App, Context, Entity, WeakEntity, Window, prelude::*};
use std::path::PathBuf;

/// The diff a tab's input holds; `None` for an input without one.
fn tab_data(input: &EditorInput) -> Option<DiffData> {
    serde_json::from_value(input.data.clone()?).ok()
}

/// The title of a diff tab: the file and the revisions compared.
pub(super) fn diff_tab_title(input: &EditorInput) -> String {
    match tab_data(input) {
        Some(data) => diff_title(&data),
        None => {
            let path = input.path.as_deref().unwrap_or(&input.key);
            path.rsplit('/').next().unwrap_or(path).to_owned()
        }
    }
}

pub(super) fn diff_tab_icon(_: &EditorInput) -> String {
    "diff".to_owned()
}

/// Builds the view of a diff tab: its viewer draws as the editor of this window does, shows
/// Restore Size while the tool windows are hidden, and the tab asks for its texts.
pub(super) fn build_diff_tab(input: &EditorInput, window: &mut Window, cx: &mut Context<Workbench>) -> Option<TabView> {
    let data = tab_data(input)?;
    let options = load_diff_options(cx.default_global::<UiStorage>().get(DIFF_OPTIONS_KEY));
    let view = cx.new(|cx| DiffTabView::new(data, options, false, cx));
    let events = cx.subscribe_in(&view, window, |workbench, view, event: &DiffTabEvent, window, cx| {
        workbench.diff_tab_event(view, event, window, cx);
    });
    let shown = view.clone();
    cx.defer_in(window, move |workbench, _, cx| {
        let label = workbench.label.clone();
        let maximized = workbench.tool_windows().is_some_and(ToolWindowLayout::all_hidden);
        let viewer = shown.read(cx).viewer().clone();
        viewer.update(cx, |viewer, cx| viewer.follow_editor_settings(&label, cx));
        shown.update(cx, |tab, cx| {
            tab.set_maximized(maximized, cx);
            tab.load(cx);
        });
    });
    Some(TabView::new(DIFF_KIND, input.clone(), &view, cx).with_subscription(events))
}

impl Workbench {
    /// Show Diff under the non-modal commit interface: each diff in an editor tab, the preview
    /// tab for a single one.
    pub(super) fn open_diff_tabs(&mut self, tabs: Vec<DiffTab>, window: &mut Window, cx: &mut Context<Self>) {
        for tab in tabs {
            let input = match diff_input(&tab.data) {
                Ok(input) => input,
                Err(error) => {
                    tracing::error!("cannot open the diff of {}: {error}", tab.data.path);
                    continue;
                }
            };
            let options = OpenTabOptions { preview: tab.preview, ..OpenTabOptions::default() };
            self.with_tabs(window, cx, |tabs, pane| tabs.open_tab(pane, input, options).is_some());
        }
    }

    /// The active tab, when it shows a diff.
    fn active_diff_tab(&self) -> Option<Entity<DiffTabView>> {
        self.active_tab_view().and_then(TabView::downcast::<DiffTabView>)
    }

    /// The viewer of the active tab, when it shows a diff.
    pub(super) fn diff_tab_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.active_diff_tab().map(|tab| tab.read(cx).viewer().clone())
    }

    /// Compare Next / Previous File in the active diff tab: the next file of its chain in
    /// `direction` with changes to show opens in the preview tab.
    pub(super) fn step_diff_tab_file(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_diff_tab() {
            self.find_diff_tab_file(&tab, direction, window, cx);
        }
    }

    /// Diff Settings in the active diff tab: the viewer's settings menu under the start of the
    /// tab's toolbar.
    pub(super) fn show_diff_tab_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_diff_tab() else { return };
        let (viewer, position) = {
            let tab = tab.read(cx);
            (tab.viewer().clone(), tab.settings_menu_position())
        };
        // A tab not drawn yet has no toolbar to open the menu under.
        let Some(position) = position else { return };
        let items = {
            let viewer = viewer.read(cx);
            viewer_settings_menu(&viewer.options(), viewer.soft_wraps())
        };
        self.show_menu(
            &items,
            position,
            move |_, command, _, cx| viewer.update(cx, |viewer, cx| viewer.run_command(command, cx)),
            window,
            cx,
        );
    }

    /// Whether the active tab shows a diff that has the keyboard.
    pub(super) fn diff_tab_focused(&self, window: &Window, cx: &App) -> bool {
        let view = self.active_tab_view();
        view.is_some_and(|view| view.kind == DIFF_KIND && view.focus.contains_focused(window, cx))
    }

    /// Every diff tab shows Restore Size while all the tool windows are hidden, Maximize Editor
    /// otherwise; called whenever the layout changes.
    pub(super) fn sync_diff_tabs_maximized(&mut self, cx: &mut Context<Self>) {
        let maximized = self.tool_windows().is_some_and(ToolWindowLayout::all_hidden);
        let Some(tabs) = self.tabs() else { return };
        let views: Vec<Entity<DiffTabView>> = tabs
            .panes
            .iter()
            .flat_map(|pane| &pane.tabs)
            .filter(|tab| tab.input.kind == DIFF_KIND)
            .filter_map(|tab| self.tab_view(&tab.id).and_then(TabView::downcast::<DiffTabView>))
            .collect();
        for view in views {
            view.update(cx, |view, cx| view.set_maximized(maximized, cx));
        }
    }

    fn diff_tab_event(
        &mut self,
        tab: &Entity<DiffTabView>,
        event: &DiffTabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DiffTabEvent::Load(data) => self.load_diff_tab(tab.downgrade(), (**data).clone(), cx),
            DiffTabEvent::Menu(DiffToolbarEvent::ShowMenu { items, position, .. }) => {
                let viewer = tab.read(cx).viewer().clone();
                self.show_menu(
                    items,
                    *position,
                    move |_, command, _, cx| viewer.update(cx, |viewer, cx| viewer.run_command(command, cx)),
                    window,
                    cx,
                );
            }
            DiffTabEvent::Viewer(event) => match event {
                &DiffViewEvent::JumpToSource { side, line, col } => {
                    self.jump_from_diff_tab(tab, side, line, col, window, cx);
                }
                DiffViewEvent::Refresh => tab.update(cx, |tab, cx| tab.load(cx)),
                DiffViewEvent::OptionsChanged(options) => match save_diff_options(options) {
                    Ok(json) => cx.default_global::<UiStorage>().set(DIFF_OPTIONS_KEY, json),
                    Err(error) => tracing::error!("cannot store the diff viewer settings: {error}"),
                },
                &DiffViewEvent::OpenFile(direction) => self.find_diff_tab_file(tab, direction, window, cx),
                DiffViewEvent::Maximize => self.with_layout(window, cx, ToolWindowLayout::hide_all),
                // The sides of a diff tab are not edited, and its panes have no menu of their own.
                DiffViewEvent::Apply { .. } | DiffViewEvent::ShowMenu { .. } => {}
            },
        }
    }

    /// Loads both sides of `data` off the window's thread and hands them to the tab while it is
    /// open and still shows `data`; a failure is told.
    fn load_diff_tab(&mut self, tab: WeakEntity<DiffTabView>, data: DiffData, cx: &mut Context<Self>) {
        let open = self.open_texts(cx);
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
        cx.spawn(async move |workbench, cx| {
            let (loaded, colours) = match work.await {
                Ok((loaded, colours)) => (loaded, colours),
                Err(error) => (Err(error.to_string()), Default::default()),
            };
            let _ = workbench.update(cx, |workbench, cx| workbench.diff_tab_loaded(&tab, &key, loaded, colours, cx));
        })
        .detach();
    }

    fn diff_tab_loaded(
        &mut self,
        tab: &WeakEntity<DiffTabView>,
        key: &str,
        loaded: Result<LoadedSides, String>,
        colours: [Vec<HighlightSpan>; 2],
        cx: &mut Context<Self>,
    ) {
        // A tab closed meanwhile shows nothing.
        let Some(tab) = tab.upgrade() else { return };
        if diff_key(tab.read(cx).data()) != key {
            return;
        }
        match loaded {
            Ok(LoadedSides { left, right, binary, working_copy }) => {
                tab.update(cx, |tab, cx| tab.loaded(&left, &right, binary, cx));
                let viewer = tab.read(cx).viewer().clone();
                let [a, b] = colours;
                viewer.update(cx, |viewer, cx| {
                    viewer.set_highlights(Side::A, a, cx);
                    viewer.set_highlights(Side::B, b, cx);
                    if working_copy {
                        viewer.set_has_source(true, cx);
                    }
                });
            }
            Err(error) => {
                let mut notice = vcs_notice(NotificationLevel::Error, "Cannot load the diff");
                notice.body = Some(error);
                self.notify(notice, cx);
            }
        }
    }

    /// Looks along the chain of the diff `tab` shows for the next file in `direction` with changes
    /// to show, passing over binary, unreadable and unchanged ones, off the window's thread. The file
    /// found opens in the preview tab while `tab` still shows the diff the search started from.
    fn find_diff_tab_file(
        &mut self,
        tab: &Entity<DiffTabView>,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (from, ignore) = {
            let tab = tab.read(cx);
            (tab.data().clone(), tab.viewer().read(cx).options().ignore)
        };
        let open = self.open_texts(cx);
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let search = ChainSearch::new(&from, direction, ignore);
        let label = self.label.clone();
        let (shown, key) = (tab.downgrade(), diff_key(&from));
        let work = backend.with_host(move |host| async move { find_file(&host, &label, search, &open).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let found = work.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| match found {
                Ok(Some(data)) => {
                    if shown.upgrade().is_some_and(|tab| diff_key(tab.read(cx).data()) == key) {
                        workbench.open_diff_tabs(vec![DiffTab { data: *data, preview: true }], window, cx);
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
        })
        .detach();
    }

    /// Jump to Source from a diff tab: the file of `side` at `line`; on a revision side, the
    /// working file of the other side at the line it shows there; for a diff of two revisions, the
    /// working copy of its file at the line the shown one has become there.
    fn jump_from_diff_tab(
        &mut self,
        tab: &Entity<DiffTabView>,
        side: Side,
        line: u32,
        col: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (data, viewer) = {
            let tab = tab.read(cx);
            (tab.data().clone(), tab.viewer().clone())
        };
        let of = |side: Side| if side == Side::A { &data.left } else { &data.right };
        if let Some(path) = side_file(of(side), &data.path) {
            return self.open_project_file_at(PathBuf::from(path), line, col, window, cx);
        }
        if let Some(path) = side_file(of(side.other()), &data.path) {
            let mapped = viewer.read(cx).map_line(side, line as usize);
            let line = u32::try_from(mapped).unwrap_or(u32::MAX);
            return self.open_project_file_at(PathBuf::from(path), line, 0, window, cx);
        }
        let shown = viewer.read(cx).side_text(side);
        self.jump_to_working_copy(data.path.clone(), shown, line, col, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{context_is_on, git_in, git_project, init_repo, run_action, wait_long};
    use super::super::tool_windows::Anchor;
    use super::super::{COMMIT_TOOL_WINDOW, GIT_TOOL_WINDOW, PROJECT_TOOL_WINDOW};
    use super::*;
    use crate::platform::settings;
    use crate::vcs::commit_view::CommitPanel;
    use crate::vcs_diff::toolbar_view::{DiffCommand, ToolbarItem, counter_text};
    use gpui_kit::{Focusable, Modifiers, TestAppContext, VisualTestContext};
    use ide_workspace::settings::SettingsScope;
    use serde_json::json;

    mod proofs18_e2e_diff_navigation;

    /// A repository whose `a.txt` was changed after its first commit, under the non-modal commit
    /// interface.
    fn non_modal_project(
        cx: &mut TestAppContext,
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
        non_modal_repository(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
        })
    }

    /// A project on the repository `prepare` makes, under the non-modal commit interface.
    fn non_modal_repository(
        cx: &mut TestAppContext,
        prepare: impl FnOnce(&std::path::Path),
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
        let (workbench, cx, folders, root) = git_project(cx, prepare);
        let label = workbench.read_with(cx, |w, _| w.label.clone());
        cx.update(|_, cx| settings::set(&label, SettingsScope::Global, "vcs.commit.nonModal", &json!(true), cx))
            .expect("the setting is written");
        wait_long(cx, "the commit interface setting", |cx| {
            cx.update(|_, cx| settings::effective(&label, cx).vcs.commit.non_modal)
        });
        (workbench, cx, folders, root)
    }

    /// A repository whose `a.txt` and `b.txt` were both changed after their commit, under the
    /// non-modal commit interface, with the Commit window listing them: one group of changes, the
    /// chain of a diff tab Show Diff opens on either.
    fn two_files_project(
        cx: &mut TestAppContext,
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
        let (workbench, cx, folders, root) = non_modal_repository(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("b.txt"), "x\ny\nz\n").expect("the file is written");
            git_in(root, &["add", "b.txt"]);
            git_in(root, &["commit", "-q", "-m", "b.txt"]);
            std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").expect("the file is written");
            std::fs::write(root.join("b.txt"), "x\nY\nz\n").expect("the file is written");
        });
        run_action(cx, "toolwindow.commit", None);
        let changes = workbench
            .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
            .and_then(|view| view.downcast::<CommitPanel>().ok())
            .expect("the Commit window holds the changes");
        wait_long(cx, "both changes to be listed", |cx| {
            changes.read_with(cx, |changes, _| changes.lines()) == ["Changes (2)", "  a.txt", "  b.txt"]
        });
        (workbench, cx, folders, root)
    }

    /// Gives the keyboard to the viewer of the active diff tab.
    fn focus_viewer(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
        let viewer = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("a diff tab is active");
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&viewer.focus_handle(cx), cx);
        });
        cx.run_until_parked();
    }

    /// The file the active editor shows and the 0-based line of its caret.
    fn edited_at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PathBuf, u32)> {
        workbench.read_with(cx, |w, cx| {
            w.editor().map(|editor| {
                let editor = editor.read(cx);
                (editor.path().to_path_buf(), editor.caret_position().0)
            })
        })
    }

    /// The keys of the diff tabs, in every pane.
    fn diff_tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        workbench.read_with(cx, |w, _| {
            let Some(tabs) = w.tabs() else { return Vec::new() };
            let all = tabs.panes.iter().flat_map(|pane| &pane.tabs);
            all.filter(|tab| tab.input.kind == DIFF_KIND).map(|tab| tab.input.key.clone()).collect()
        })
    }

    /// Show Diff of `a.txt`: the viewer of its tab, once its texts are loaded.
    fn open_diff_tab(
        workbench: &Entity<Workbench>,
        root: &std::path::Path,
        cx: &mut VisualTestContext,
    ) -> Entity<DiffView> {
        let path = root.join("a.txt").to_string_lossy().into_owned();
        run_action(cx, "vcs.showDiff", Some(json!(path)));
        wait_long(cx, "the diff tab to be loaded", |cx| {
            workbench.read_with(cx, |w, cx| {
                w.diff_tab_viewer(cx).is_some_and(|viewer| viewer.read(cx).difference_count() > 0)
            })
        });
        workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("the active tab shows the diff")
    }

    fn maximize_button(viewer: &Entity<DiffView>, cx: &mut VisualTestContext) -> Option<(&'static str, bool)> {
        viewer.read_with(cx, |viewer, cx| {
            let items = viewer.toolbar(cx);
            let button = items.iter().filter_map(ToolbarItem::button).find(|button| button.id == "diff-maximize");
            button.map(|button| (button.label, button.checked))
        })
    }

    fn visible_tool_windows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        workbench.read_with(cx, |w, _| {
            w.tool_windows().map(|layout| layout.visible().ids().map(str::to_owned).collect()).unwrap_or_default()
        })
    }

    fn maximized_side(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Anchor> {
        workbench.read_with(cx, |w, _| w.tool_windows().and_then(ToolWindowLayout::maximized))
    }

    /// The counter of the active diff tab's toolbar.
    fn counter(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        workbench
            .read_with(cx, |w, cx| w.diff_tab_viewer(cx).map(|viewer| counter_text(&viewer.read(cx).summary(), cx)))
    }

    /// The name of the active editor tab.
    fn selected_tab(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        workbench.read_with(cx, |w, _| w.tab_titles().into_iter().find(|title| title.active).map(|title| title.name))
    }

    // Show Diff opens the diff in an editor tab titled by the file and the revisions, not in the diff window;
    // closing the tab drops it.
    #[gpui_kit::test]
    async fn show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()), "no diff window is shown");
        assert_eq!(diff_tabs(&workbench, cx).len(), 1);
        assert_eq!(selected_tab(&workbench, cx).as_deref(), Some("a.txt (HEAD ↔ Your version)"));
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.side_text(Side::B)), "two\n");
        run_action(cx, "editor.closeTab", None);
        assert!(diff_tabs(&workbench, cx).is_empty());
        assert!(workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).is_none());
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › maximizes a diff tab and restores the %s layout without
    // rebuilding its editor
    /// With no side maximized, and with the bottom side maximized: Maximize Editor hides every tool window and
    /// Restore Size brings back the same windows and the same maximized side, the viewer and its caret kept.
    #[gpui_kit::test]
    async fn maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        for maximized in [None, Some(Anchor::Bottom)] {
            if maximized.is_some() {
                workbench.update_in(cx, |w, window, cx| {
                    w.with_layout(window, cx, |layout| layout.toggle_maximize(GIT_TOOL_WINDOW));
                });
                cx.run_until_parked();
            }
            assert_eq!(maximized_side(&workbench, cx), maximized);
            let shown = visible_tool_windows(&workbench, cx);
            assert!(!shown.is_empty(), "a tool window is open");
            assert_eq!(maximize_button(&viewer, cx), Some(("Maximize Editor", false)));
            viewer.update(cx, |viewer, cx| viewer.reveal_line(Side::B, 0, cx));
            viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
            cx.run_until_parked();
            assert!(visible_tool_windows(&workbench, cx).is_empty());
            assert_eq!(maximize_button(&viewer, cx), Some(("Restore Size", true)));
            viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
            cx.run_until_parked();
            assert_eq!(visible_tool_windows(&workbench, cx), shown);
            assert_eq!(maximized_side(&workbench, cx), maximized);
            assert_eq!(maximize_button(&viewer, cx), Some(("Maximize Editor", false)));
            let still = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx));
            assert_eq!(still.as_ref(), Some(&viewer), "the viewer is not rebuilt");
            assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0));
        }
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › restores the shared layout after the diff tab is
    // remounted
    #[gpui_kit::test]
    async fn restores_the_shared_layout_after_the_diff_tab_is_remounted(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        workbench.update_in(cx, |w, window, cx| {
            w.with_layout(window, cx, |layout| layout.toggle_maximize(GIT_TOOL_WINDOW));
        });
        cx.run_until_parked();
        let shown = visible_tool_windows(&workbench, cx);
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        run_action(cx, "editor.closeTab", None);
        let again = open_diff_tab(&workbench, &root, cx);
        assert_eq!(maximize_button(&again, cx), Some(("Restore Size", true)));
        again.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        assert_eq!(visible_tool_windows(&workbench, cx), shown);
        assert_eq!(maximized_side(&workbench, cx), Some(Anchor::Bottom));
    }

    // Hide All Tool Windows run from anywhere turns the tab's Maximize Editor into Restore Size, and back.
    #[gpui_kit::test]
    async fn the_maximize_button_follows_hide_all_tool_windows(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW)));
        run_action(cx, "toolwindow.hideAll", None);
        assert_eq!(maximize_button(&viewer, cx), Some(("Restore Size", true)));
        run_action(cx, "toolwindow.hideAll", None);
        assert_eq!(maximize_button(&viewer, cx), Some(("Maximize Editor", false)));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW)));
    }

    // Compare Next File and Compare Previous File from the keymap, while a diff tab has the keyboard: the next
    // and the previous changed file of its group take the preview tab.
    #[gpui_kit::test]
    async fn compares_the_next_and_previous_file_in_a_diff_tab_from_the_keymap(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = two_files_project(cx);
        open_diff_tab(&workbench, &root, cx);
        assert_eq!(selected_tab(&workbench, cx).as_deref(), Some("a.txt (HEAD ↔ Your version)"));
        focus_viewer(&workbench, cx);
        assert!(context_is_on(cx, "diffEditorFocus"), "the diff tab has the keyboard");
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "ctrl-shift-right" } else { "alt-shift-right" });
        // The file found opens at its first difference.
        wait_long(cx, "b.txt's diff", |cx| {
            selected_tab(&workbench, cx).is_some_and(|tab| tab.starts_with("b.txt"))
                && counter(&workbench, cx).as_deref() == Some("1 of 1 differences")
        });
        assert_eq!(diff_tabs(&workbench, cx).len(), 1, "b.txt took the preview tab");
        focus_viewer(&workbench, cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "ctrl-shift-left" } else { "alt-shift-left" });
        wait_long(cx, "a.txt's diff again", |cx| {
            selected_tab(&workbench, cx).is_some_and(|tab| tab.starts_with("a.txt"))
        });
        assert_eq!(diff_tabs(&workbench, cx).len(), 1, "a.txt took the preview tab");
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!context_is_on(cx, "diffEditorFocus"), "nothing has the keyboard");
    }

    // Diff Settings from the keymap opens the viewer's settings menu under the start of the diff tab's toolbar, and
    // a row chosen there changes that tab's viewer.
    #[gpui_kit::test]
    async fn diff_settings_opens_the_settings_menu_under_the_diff_tabs_toolbar(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        focus_viewer(&workbench, cx);
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<DiffCommand>().is_none()));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-shift-d" } else { "ctrl-shift-d" });
        cx.run_until_parked();
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<DiffCommand>()).expect("the settings menu is open");
        let rows: Vec<String> = menu.read_with(cx, |menu, _| menu.lines())[0]
            .iter()
            .map(|line| line.trim_start_matches("> ").to_owned())
            .collect();
        let soft_wraps = rows.iter().position(|row| row == "Use Soft Wraps").expect("the menu offers Use Soft Wraps");
        assert!(rows.iter().any(|row| row.starts_with("Side-by-side Viewer")), "{rows:?}");
        let toolbar = cx.debug_bounds("diff-toolbar").expect("the toolbar is drawn");
        let first = cx.debug_bounds("menu-row-0-0").expect("the menu is drawn");
        assert!(first.top() >= toolbar.bottom() - gpui_kit::px(1.0), "the menu opens under the toolbar");
        assert!((first.left() - toolbar.left()) < gpui_kit::px(40.0), "the menu opens at the toolbar's start");
        let wrapped = viewer.read_with(cx, |viewer, _| viewer.soft_wraps());
        let selector: &'static str = Box::leak(format!("menu-row-0-{soft_wraps}").into_boxed_str());
        let row = cx.debug_bounds(selector).expect("the row is drawn");
        cx.simulate_click(row.center(), Modifiers::default());
        cx.run_until_parked();
        assert_ne!(
            viewer.read_with(cx, |viewer, _| viewer.soft_wraps()),
            wrapped,
            "Use Soft Wraps reached the tab's viewer"
        );
        assert_eq!(diff_tabs(&workbench, cx).len(), 1, "the tab stays");
    }

    // Jump to Source from the keymap in a diff tab opens the working file at the caret's line and column; the diff
    // tabs stay open (Show Diff of two files opens regular tabs, which a new editor does not replace).
    #[gpui_kit::test]
    async fn jump_to_source_opens_the_file_at_the_caret_and_keeps_the_diff_tab(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = two_files_project(cx);
        let paths = ["a.txt", "b.txt"].map(|name| root.join(name).to_string_lossy().into_owned());
        run_action(cx, "vcs.showDiff", Some(json!(paths)));
        wait_long(cx, "b.txt's diff tab to be loaded", |cx| {
            selected_tab(&workbench, cx).is_some_and(|tab| tab.starts_with("b.txt"))
                && counter(&workbench, cx).as_deref() == Some("1 difference")
        });
        assert_eq!(diff_tabs(&workbench, cx).len(), 2);
        let viewer = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).expect("a diff tab is active");
        viewer.update(cx, |viewer, cx| viewer.reveal_line(Side::B, 1, cx));
        focus_viewer(&workbench, cx);
        cx.simulate_keystrokes("f4");
        let file = root.join("b.txt");
        wait_long(cx, "b.txt in the editor", |cx| edited_at(&workbench, cx) == Some((file.clone(), 1)));
        assert_eq!(diff_tabs(&workbench, cx).len(), 2, "the diff tabs stay open");
        assert!(workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).is_none(), "the editor is the active tab");
    }

    // The close shortcut pressed while a diff tab has the keyboard closes that tab, as it closes any editor tab.
    #[gpui_kit::test]
    async fn the_close_shortcut_closes_a_diff_tab_that_has_the_keyboard(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        open_diff_tab(&workbench, &root, cx);
        focus_viewer(&workbench, cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-w" } else { "ctrl-f4" });
        cx.run_until_parked();
        assert!(diff_tabs(&workbench, cx).is_empty(), "the diff tab is closed");
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()));
    }
}
