//! Diffs in editor tabs. With the non-modal commit interface Show Diff opens the diff of each file
//! in an editor tab of its own instead of the diff window, the preview tab when it is the only one.
//! A diff tab is titled by its file and the revisions compared; it loads both sides off the
//! window's thread as the diff window does, loads them again on Refresh, stores the viewer
//! settings the user changes, and jumps to the source as the diff window does, staying open. Its
//! Maximize Editor hides every tool window, and Restore Size brings them back. Closing the tab
//! drops its view.

use super::Workbench;
use super::diff_window::{LoadedSides, load_sides, side_colours};
use super::tab_views::TabView;
use super::tool_windows::ToolWindowLayout;
use crate::backend::Backend;
use crate::editor::tabs::{EditorInput, OpenTabOptions};
use crate::editor::view::HighlightSpan;
use crate::platform::storage::UiStorage;
use crate::vcs_diff::diff_input::{DIFF_KIND, DiffData, diff_input, diff_key, diff_title, side_file};
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, Side, load_diff_options, save_diff_options};
use crate::vcs_diff::tab_view::{DiffTabEvent, DiffTabView};
use crate::vcs_diff::toolbar_view::DiffToolbarEvent;
use crate::vcs_diff::view::{DiffView, DiffViewEvent};
use crate::vcs_more::show_diff::DiffTab;
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::NotificationLevel;
use gpui_kit::{App, Context, Entity, WeakEntity, Window};
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
        viewer.update(cx, |viewer, cx| {
            viewer.follow_editor_settings(&label, cx);
            viewer.set_maximized(Some(maximized), cx);
        });
        shown.update(cx, |tab, cx| tab.load(cx));
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

    /// The viewer of the active tab, when it shows a diff.
    pub(super) fn diff_tab_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.active_tab_view().and_then(TabView::downcast::<DiffTabView>).map(|tab| tab.read(cx).viewer().clone())
    }

    /// Whether the active tab shows a diff that has the keyboard.
    pub(super) fn diff_tab_focused(&self, window: &Window, cx: &App) -> bool {
        let view = self.active_tab_view();
        view.is_some_and(|view| view.kind == DIFF_KIND && view.focus.contains_focused(window, cx))
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
                DiffViewEvent::Maximize => self.maximize_from_diff_tab(window, cx),
                // A diff tab has no chain to move along, its sides are not edited, and its panes have
                // no menu of their own.
                DiffViewEvent::OpenFile(_) | DiffViewEvent::Apply { .. } | DiffViewEvent::ShowMenu { .. } => {}
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

    /// Maximize Editor in a diff tab: hides every tool window, or brings back the ones it hid.
    fn maximize_from_diff_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_layout(window, cx, ToolWindowLayout::hide_all);
        let maximized = self.tool_windows().is_some_and(ToolWindowLayout::all_hidden);
        if let Some(viewer) = self.diff_tab_viewer(cx) {
            viewer.update(cx, |viewer, cx| viewer.set_maximized(Some(maximized), cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{git_project, init_repo, run_action, wait_long};
    use super::*;
    use crate::platform::settings;
    use crate::vcs_diff::toolbar_view::{DiffCommand, ToolbarItem};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use ide_workspace::settings::SettingsScope;
    use serde_json::json;

    /// A repository whose `a.txt` was changed after its first commit, under the non-modal commit
    /// interface.
    fn non_modal_project(
        cx: &mut TestAppContext,
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
        let (workbench, cx, folders, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
        });
        let label = workbench.read_with(cx, |w, _| w.label.clone());
        cx.update(|_, cx| settings::set(&label, SettingsScope::Global, "vcs.commit.nonModal", &json!(true), cx))
            .expect("the setting is written");
        wait_long(cx, "the commit interface setting", |cx| {
            cx.update(|_, cx| settings::effective(&label, cx).vcs.commit.non_modal)
        });
        (workbench, cx, folders, root)
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

    // Show Diff opens the diff in an editor tab titled by the file and the revisions, not in the diff window;
    // closing the tab drops it.
    #[gpui_kit::test]
    async fn show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()), "no diff window is shown");
        assert_eq!(diff_tabs(&workbench, cx).len(), 1);
        let titles: Vec<String> =
            workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|title| title.name).collect());
        assert!(titles.contains(&"a.txt (HEAD ↔ Your version)".to_owned()), "{titles:?}");
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.side_text(Side::B)), "two\n");
        run_action(cx, "editor.closeTab", None);
        assert!(diff_tabs(&workbench, cx).is_empty());
        assert!(workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx)).is_none());
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › maximizes a diff tab and restores the %s layout without
    // rebuilding its editor
    #[gpui_kit::test]
    async fn maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
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
        assert_eq!(maximize_button(&viewer, cx), Some(("Maximize Editor", false)));
        let still = workbench.read_with(cx, |w, cx| w.diff_tab_viewer(cx));
        assert_eq!(still.as_ref(), Some(&viewer), "the viewer is not rebuilt");
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0));
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › restores the shared layout after the diff tab is
    // remounted
    #[gpui_kit::test]
    async fn restores_the_shared_layout_after_the_diff_tab_is_remounted(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root) = non_modal_project(cx);
        let viewer = open_diff_tab(&workbench, &root, cx);
        let shown = visible_tool_windows(&workbench, cx);
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        run_action(cx, "editor.closeTab", None);
        let again = open_diff_tab(&workbench, &root, cx);
        assert_eq!(maximize_button(&again, cx), Some(("Restore Size", true)));
        again.update(cx, |viewer, cx| viewer.run_command(DiffCommand::ToggleMaximize, cx));
        cx.run_until_parked();
        assert_eq!(visible_tool_windows(&workbench, cx), shown);
    }
}
