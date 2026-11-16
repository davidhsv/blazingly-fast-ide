//! The Project view's behaviour options, its panes, and the file actions of its keymap.
//!
//! - Always Select Opened File: the tree selects the file of the active editor tab whenever it
//!   changes, and at once when the option is turned on.
//! - Autoscroll to Source, Open Files with Single Click, Enable Preview Tab and Expand Directories
//!   with Single Click are decided by the tree ([`crate::project_view::tree_clicks`]); a file it
//!   asks to open without the keyboard, or in the preview tab, opens here.
//! - The panes "Project", "Project Files", "Open Files" and "All Changed Files": the tree lists the
//!   files of the editor tabs and the files with local changes, fed from here.
//! - Find Usages of File, Reformat File, Optimize Imports in File and Reload from Disk act on the
//!   tree's selection (or the paths passed), as the context menu's rows of the same name do.

use super::services_window::on_workbench;
use super::tab_views::FILE_KIND;
use super::{Content, PROJECT_TOOL_WINDOW, Workbench};
use crate::editor::tabs::{EditorInput, OpenTabOptions};
use crate::platform::actions::ActionRegistry;
use crate::platform::storage::UiStorage;
use crate::project_tree::{OPTIONS_KEY, ProjectTree, ViewOptions};
use crate::project_view::actions::{option_value, toggle_option};
use crate::project_view::context_menu::ProjectCommand;
use crate::project_view::index::local_changed_paths;
use crate::project_view::store::ViewMode;
use crate::project_view::tree_clicks::OpenRequest;
use crate::vcs_ops::changelists::local_changelists;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{App, Context, Entity, Subscription, Window};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The behaviour options this module carries out, by the name they are stored under.
const BEHAVIOUR_TOGGLES: [&str; 5] =
    ["autoscrollFromSource", "autoscrollToSource", "singleClickOpen", "singleClickExpand", "previewTab"];

/// The panes the tree shows.
const PANES: [ViewMode; 4] = [ViewMode::Project, ViewMode::ProjectFiles, ViewMode::OpenFiles, ViewMode::ChangedFiles];

/// The Options menu's rows about what the tree shows, with their labels there.
const APPEARANCE_ROWS: [(&str, &str); 4] = [
    ("project.option.showHidden", "Show Hidden Files"),
    ("project.option.showExcluded", "Show Excluded Files"),
    ("project.option.compact", "Compact Middle Packages"),
    ("project.option.foldersFirst", "Sort Folders First"),
];

/// The Options menu's orders within a folder.
const SORT_ROWS: [(&str, &str); 3] = [
    ("project.sortBy.name", "Sort by Name"),
    ("project.sortBy.type", "Sort by Type"),
    ("project.sortBy.modified", "Sort by Modification Time"),
];

/// The Options menu's rows about how the tree behaves.
const BEHAVIOUR_ROWS: [(&str, &str); 5] = [
    ("project.option.autoscrollFromSource", "Always Select Opened File"),
    ("project.option.autoscrollToSource", "Autoscroll to Source"),
    ("project.option.singleClickOpen", "Open Files with Single Click"),
    ("project.option.singleClickExpand", "Expand Directories with Single Click"),
    ("project.option.previewTab", "Enable Preview Tab"),
];

/// The rows the Project view puts at the top of its tool window's Options menu: "Appearance" with
/// what the tree shows and the orders, then "Behavior". Each row is the option's action, checked
/// while the option is on.
pub(super) fn header_menu_rows<C>() -> Vec<MenuItem<C>> {
    let row = |(id, label): (&'static str, &str)| MenuItem::action(id).labelled(label);
    let mut rows = vec![MenuItem::entry("Appearance", None)];
    rows.extend(APPEARANCE_ROWS.map(row));
    rows.push(MenuItem::Separator);
    rows.extend(SORT_ROWS.map(row));
    rows.push(MenuItem::Separator);
    rows.push(MenuItem::entry("Behavior", None));
    rows.extend(BEHAVIOUR_ROWS.map(row));
    rows
}

/// What the window keeps for the Project view's options.
#[derive(Default)]
pub(super) struct TreeOptionsSlot {
    /// The tree's requests to open a file without the keyboard or in the preview tab.
    requests: Option<Subscription>,
    /// The file of the active editor tab the tree was last asked to follow.
    followed: Option<String>,
}

/// The view options as stored; the defaults for what is not.
fn stored_options(cx: &App) -> ViewOptions {
    cx.try_global::<UiStorage>().map(ViewOptions::stored).unwrap_or_default()
}

/// Registers the actions of this module.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for key in BEHAVIOUR_TOGGLES {
        registry.handle_toggle(
            &format!("project.option.{key}"),
            move |_, cx| option_value(&stored_options(cx), key).unwrap_or(false),
            move |_, window, cx| {
                on_workbench(window, cx, move |workbench, _, cx| workbench.toggle_tree_option(key, cx))
            },
        );
    }
    for mode in PANES {
        registry.handle(&format!("project.viewMode.{}", mode.id()), move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| workbench.show_pane(mode, window, cx))
        });
    }
    registry.handle("project.findUsages", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.find_file_usages(arg.as_ref(), window, cx))
    });
    let delegated =
        [("project.reformatCode", "code.reformatFile"), ("project.optimizeImports", "code.optimizeImportsFile")];
    for (id, action) in delegated {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.run_on_selected_file(action, arg.as_ref(), window, cx);
            })
        });
    }
    registry.handle("project.reloadFromDisk", |call, window, cx| {
        let (arg, focus) = (call.arg.cloned(), call.ctx.is_truthy("projectViewFocus"));
        on_workbench(window, cx, move |workbench, window, cx| {
            let nodes = workbench.nodes_for(arg.as_ref(), cx);
            if !nodes.is_empty() {
                workbench.run_project_command(ProjectCommand::Reload, nodes, focus, window, cx);
            }
        })
    });
}

impl Workbench {
    /// Listens to the tree's requests to open a file; called once the project's tree exists.
    pub(super) fn watch_tree_requests(
        &mut self,
        tree: &Entity<ProjectTree>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tree_options.requests = Some(cx.subscribe_in(tree, window, Self::tree_open_request));
        self.tree_options.followed = None;
    }

    /// The tree asks for a file to open: in the preview tab or a regular one, the keyboard staying
    /// in the tree unless the request gives it to the editor.
    fn tree_open_request(
        &mut self,
        _: &Entity<ProjectTree>,
        request: &OpenRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let OpenRequest { path, preview, focus } = request.clone();
        if focus && !preview {
            self.open_project_file(path, window, cx);
        } else {
            self.open_text_tab_from_tree(path, preview, focus, window, cx);
        }
    }

    /// Opens the text tab of `path` in the active pane (as its preview tab with `preview`), or
    /// brings it to the front; the editor takes the keyboard only with `focus`.
    fn open_text_tab_from_tree(
        &mut self,
        path: PathBuf,
        preview: bool,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The editor's "Enable preview tab" setting decides whether a preview tab exists at all.
        let preview = preview && crate::platform::settings::effective(&self.label, cx).editor.tabs.preview_tab;
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let key = path.to_string_lossy().into_owned();
        let pane = tabs.active_pane.clone();
        let existing = tabs.find_tab(&key, Some(&pane)).map(|(_, tab)| tab.id.clone());
        let fresh = existing.is_none();
        let tab_id = match existing {
            Some(id) => {
                tabs.activate_tab(&pane, &id);
                id
            }
            None => {
                let input = EditorInput { kind: FILE_KIND.to_owned(), key: key.clone(), path: Some(key), data: None };
                let options = OpenTabOptions { preview, ..OpenTabOptions::default() };
                let Some(id) = tabs.open_tab(&pane, input, options) else { return };
                id
            }
        };
        if !editors.contains_key(&tab_id) {
            // A new tab is probed first: binary content shows in the hex view.
            self.load_tab_probed(tab_id, path, focus, fresh, window, cx);
        }
        self.tabs_changed(focus, window, cx);
    }

    /// The editor tabs changed: the "Open Files" pane lists their files, and with Always Select
    /// Opened File the tree selects the file of the active tab.
    pub(super) fn project_view_tabs_changed(&mut self, cx: &mut Context<Self>) {
        let Some(tree) = self.project_tree().cloned() else { return };
        let mut open: Vec<String> = Vec::new();
        for tab in self.tabs().into_iter().flat_map(|tabs| tabs.panes.iter().flat_map(|pane| &pane.tabs)) {
            if tab.input.kind == FILE_KIND
                && let Some(path) = &tab.input.path
                && !open.contains(path)
            {
                open.push(path.clone());
            }
        }
        tree.update(cx, |tree, cx| tree.set_open_files(&open, cx));
        self.follow_active_file(cx);
    }

    /// With Always Select Opened File, selects the file of the active editor tab in the tree once
    /// the tab changes; the keyboard stays where it is.
    fn follow_active_file(&mut self, cx: &mut Context<Self>) {
        // The active tab's file: its editor may still be loading.
        let active = self.tabs().and_then(|tabs| {
            let tab = tabs.pane(&tabs.active_pane).and_then(|pane| pane.active_tab())?;
            tab.input.path.clone().filter(|_| tab.input.kind == FILE_KIND)
        });
        if active == self.tree_options.followed {
            return;
        }
        self.tree_options.followed.clone_from(&active);
        let (Some(path), Some(tree)) = (active, self.project_tree().cloned()) else { return };
        if tree.read(cx).options().autoscroll_from_source {
            tree.update(cx, |tree, cx| {
                tree.reveal_path(&path, cx);
            });
        }
    }

    /// The repository was read: "All Changed Files" lists the changed files still on disk, of
    /// every change list (unversioned and ignored files are left out).
    pub(super) fn feed_changed_files(&mut self, cx: &mut Context<Self>) {
        let Some(tree) = self.project_tree().cloned() else { return };
        let paths = match self.vcs_state() {
            Some(vcs) if vcs.available && vcs.status_loaded => {
                let root = vcs.root.clone().unwrap_or_default();
                local_changed_paths(&local_changelists(&vcs.status, &root, self.changelists()), None, true)
            }
            _ => Vec::new(),
        };
        tree.update(cx, |tree, cx| tree.set_changed_files(&paths, cx));
    }

    /// Flips the behaviour option `key`, for every Project view from now on. Turning Always Select
    /// Opened File on selects the edited file at once.
    fn toggle_tree_option(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let Some(tree) = self.project_tree().cloned() else {
            // Without a project the choice is stored for the next tree.
            let mut options = stored_options(cx);
            toggle_option(&mut options, key);
            match serde_json::to_string(&options) {
                Ok(text) if cx.has_global::<UiStorage>() => cx.global_mut::<UiStorage>().set(OPTIONS_KEY, text),
                Ok(_) => {}
                Err(error) => tracing::warn!("project view: cannot store the options: {error}"),
            }
            return;
        };
        tree.update(cx, |tree, cx| {
            tree.set_options(
                |options| {
                    toggle_option(options, key);
                },
                cx,
            );
        });
        if key == "autoscrollFromSource" && tree.read(cx).options().autoscroll_from_source {
            self.tree_options.followed = None;
            self.follow_active_file(cx);
        }
    }

    /// Project View: Show …: the tree shows the pane `mode`, remembered across sessions, and the
    /// Project view shows without taking the keyboard. Each pane keeps its own expansion.
    fn show_pane(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let write = self.with_view(false, window, cx, |state, env| state.set_view_mode(mode, env.stored_expanded));
        if let Some(write) = write
            && cx.has_global::<UiStorage>()
        {
            cx.global_mut::<UiStorage>().set(&write.key, write.value);
        }
        self.with_layout(window, cx, |layout| layout.show(PROJECT_TOOL_WINDOW, false));
    }

    /// Find Usages of File: the usages of the selected file's top-level symbols in other files.
    fn find_file_usages(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let nodes = self.nodes_for(arg, cx);
        let Some(node) = nodes.first().filter(|node| !node.dir).cloned() else { return };
        let command = ProjectCommand::FindUsages(node.path.clone());
        self.run_project_command(command, vec![node], true, window, cx);
    }

    /// Runs the editor's `action` (Reformat File, Optimize Imports in File) on the selected file,
    /// which opens for it; a folder is left alone.
    fn run_on_selected_file(
        &mut self,
        action: &'static str,
        arg: Option<&Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let nodes = self.nodes_for(arg, cx);
        let Some(node) = nodes.first().filter(|node| !node.dir).cloned() else {
            #[cfg(test)]
            crate::platform::actions::test_trace::note(format!("{action}: no file is selected in the tree"));
            return;
        };
        let command = ProjectCommand::Run { action, arg: json!({ "path": node.path }) };
        self.run_project_command(command, vec![node], true, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{git_in, git_project, init_repo, notification_titles, run_action, start, wait_long};
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::editor::intelligence::fake_server::FakeServer;
    use crate::platform::actions::action_toggled;
    use crate::platform::context::window_context;
    use crate::project_view::store::MODE_KEY;
    use gpui_kit::{
        Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
    };
    use std::path::Path;

    fn text_of(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn project_tree_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectTree> {
        workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree")
    }

    /// The labels of the tree's rows below the project.
    fn tree_labels(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) -> Vec<String> {
        tree.read_with(cx, |tree, _| tree.rows().iter().skip(1).map(|row| row.node.label.clone()).collect())
    }

    fn wait_loaded(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) {
        wait_long(cx, "the project to be listed", |cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
    }

    fn toggled(id: &str, cx: &mut VisualTestContext) -> Option<bool> {
        cx.update(|window, cx| {
            let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
            action_toggled(&action, window, cx)
        })
    }

    fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {
        cx.update(|window, cx| window_context(window, cx).is_truthy(key))
    }

    /// The editor tabs by file name, with whether each is the preview tab.
    fn open_tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().into_iter().flat_map(|tabs| tabs.panes.iter().flat_map(|pane| &pane.tabs));
            tabs.map(|tab| {
                let name = Path::new(&tab.input.key).file_name().map(|name| name.to_string_lossy().into_owned());
                (name.unwrap_or_default(), tab.preview)
            })
            .collect()
        })
    }

    /// Clicks the row of `path` `count` times in a row, as one burst.
    fn click_row(path: &Path, count: usize, cx: &mut VisualTestContext) {
        let selector: &'static str = Box::leak(format!("project-row:{}", text_of(path)).into_boxed_str());
        let position = cx.debug_bounds(selector).expect("the row is drawn").center();
        let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
        for click_count in 1..=count {
            cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count, first_mouse: false });
            cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count });
        }
        cx.run_until_parked();
    }

    /// Selects `path` in the tree and gives the tree the keyboard.
    fn select_in_tree(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
        let tree = project_tree_of(workbench, cx);
        let at = text_of(path);
        wait_loaded(&tree, cx);
        assert!(tree.update(cx, |tree, cx| tree.reveal_path(&at, cx)), "{at} is in the project");
        wait_long(cx, "the row to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    /// Opens `file` in the editor and gives the editor the keyboard.
    fn edit(workbench: &Entity<Workbench>, file: &Path, cx: &mut VisualTestContext) {
        let at = file.to_path_buf();
        workbench.update_in(cx, |w, window, cx| w.open_project_file(at, window, cx));
        wait_long(cx, "the file to open", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file))
        });
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        cx.update(|window, cx| {
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn editor_text(workbench: &Entity<Workbench>, file: &Path, cx: &mut VisualTestContext) -> Option<String> {
        workbench.read_with(cx, |w, cx| {
            w.editor().filter(|editor| editor.read(cx).path() == file).map(|editor| editor.read(cx).text())
        })
    }

    /// Closes the window's project and opens it again; the new tree once it lists the project (a pane
    /// of some files only lists them once they are known). The caller must hold no handle on the old
    /// tree, which writes its expansion when it is released.
    fn reopen(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Entity<ProjectTree> {
        let before = project_tree_of(workbench, cx).entity_id();
        workbench.update_in(cx, |w, window, cx| w.close_project(window, cx));
        cx.run_until_parked();
        let at = root.to_path_buf();
        workbench.update_in(cx, |w, window, cx| w.open_project(at, window, cx));
        wait_long(cx, "the project to open again", |cx| {
            workbench.read_with(cx, |w, _| w.project_tree().is_some_and(|tree| tree.entity_id() != before))
        });
        let tree = project_tree_of(workbench, cx);
        if matches!(tree.read_with(cx, |tree, _| tree.pane().mode()), ViewMode::Project | ViewMode::ProjectFiles) {
            wait_loaded(&tree, cx);
        }
        tree
    }

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// The app as [`start`] sets it up, in Full mode, with the fake language server for `*.mock`
    /// files installed before the backend starts.
    fn start_with_mock_server(cx: &mut TestAppContext) -> (tempfile::TempDir, FakeServer) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        let dirs = AppDirs::under(home.path());
        let server = FakeServer::install(&dirs.config, home.path());
        cx.update(|cx| {
            cx.set_global(crate::theme::ActiveTheme::default());
            crate::platform::input::init(cx);
            crate::editor::actions::register(cx);
            super::super::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(dirs, cx).expect("the backend starts");
            super::super::menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            let host = cx.global::<Backend>().host.clone();
            let global = ide_workspace::settings::SettingsScope::Global;
            host.global_settings.set(global, "ide.mode", json!("full")).expect("the mode is set");
        });
        (home, server)
    }

    // src/features/project-tree/ProjectView.test.tsx › opens files on double click and previews on
    // single click only when enabled
    #[gpui_kit::test]
    async fn opens_files_on_double_click_and_previews_on_single_click_only_when_enabled(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("README.md", "# Acme\n"), ("notes.md", "notes\n"), ("todo.md", "todo\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        wait_long(cx, "the files", |cx| tree_labels(&tree, cx) == ["notes.md", "README.md", "todo.md"]);
        let (readme, notes, todo) = (root.join("README.md"), root.join("notes.md"), root.join("todo.md"));

        click_row(&readme, 1, cx);
        assert!(open_tabs(&workbench, cx).is_empty(), "a single click only selects");
        click_row(&readme, 2, cx);
        wait_long(cx, "README.md to open", |cx| open_tabs(&workbench, cx) == [("README.md".to_owned(), false)]);
        wait_long(cx, "the editor to take the keyboard", |cx| context_is_on(cx, "editorFocus"));

        assert_eq!(toggled("project.option.previewTab", cx), Some(false));
        run_action(cx, "project.option.previewTab", None);
        assert_eq!(toggled("project.option.previewTab", cx), Some(true));
        click_row(&notes, 1, cx);
        wait_long(cx, "notes.md in the preview tab", |cx| {
            open_tabs(&workbench, cx).contains(&("notes.md".to_owned(), true))
        });
        assert!(context_is_on(cx, "projectViewFocus"), "the keyboard stays in the tree");

        run_action(cx, "project.option.previewTab", None);
        assert_eq!(toggled("project.option.previewTab", cx), Some(false));
        click_row(&todo, 1, cx);
        assert!(!open_tabs(&workbench, cx).iter().any(|(name, _)| name == "todo.md"), "off: a click only selects");

        run_action(cx, "project.option.singleClickOpen", None);
        assert_eq!(toggled("project.option.singleClickOpen", cx), Some(true));
        click_row(&todo, 1, cx);
        wait_long(cx, "todo.md in a tab of its own", |cx| {
            open_tabs(&workbench, cx).contains(&("todo.md".to_owned(), false))
        });
        assert!(context_is_on(cx, "projectViewFocus"), "the keyboard stays in the tree");
        run_action(cx, "project.option.singleClickOpen", None);
        assert_eq!(toggled("project.option.singleClickOpen", cx), Some(false));
        close_project(cx);
    }

    // src/features/project-tree/ProjectView.test.tsx › expands and collapses folders on single
    // click only when enabled (with src/features/project-tree/store.test.ts › single-click expansion
    // › is off by default and its option persists across reloads)
    #[gpui_kit::test]
    async fn expands_and_collapses_folders_on_single_click_only_when_enabled(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("src/App.kt", "class App\n"), ("src/Util.kt", "object Util\n"), ("README.md", "")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        wait_long(cx, "the files", |cx| tree_labels(&tree, cx) == ["src", "README.md"]);
        let src = root.join("src");
        let shows_app = |tree: &Entity<ProjectTree>, cx: &mut VisualTestContext| {
            tree_labels(tree, cx).contains(&"App.kt".to_owned())
        };

        click_row(&src, 1, cx);
        assert!(!shows_app(&tree, cx), "off: a single click only selects");
        assert_eq!(toggled("project.option.singleClickExpand", cx), Some(false));
        run_action(cx, "project.option.singleClickExpand", None);
        assert_eq!(toggled("project.option.singleClickExpand", cx), Some(true));
        click_row(&src, 1, cx);
        wait_long(cx, "src to open", |cx| shows_app(&tree, cx));
        click_row(&src, 1, cx);
        assert!(!shows_app(&tree, cx), "a second click closes it");
        // A double click toggles once: its second click is left alone.
        click_row(&src, 2, cx);
        wait_long(cx, "src to open once", |cx| shows_app(&tree, cx));

        // The choice is remembered: the project opened again behaves the same.
        drop(tree);
        let tree = reopen(&workbench, &root, cx);
        assert_eq!(toggled("project.option.singleClickExpand", cx), Some(true));
        wait_long(cx, "src as it was left", |cx| shows_app(&tree, cx));
        click_row(&src, 1, cx);
        assert!(!shows_app(&tree, cx), "a single click still closes it");

        run_action(cx, "project.option.singleClickExpand", None);
        assert_eq!(toggled("project.option.singleClickExpand", cx), Some(false));
        click_row(&src, 1, cx);
        assert!(!shows_app(&tree, cx), "off again: a single click only selects");
        close_project(cx);
    }

    // src/features/project-tree/ProjectView.test.tsx › follows tab switches with Always Select
    // Opened File, hiding Select Opened File
    #[gpui_kit::test]
    async fn follows_tab_switches_with_always_select_opened_file(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("README.md", "# Acme\n"), ("src/Util.kt", "object Util\n"), ("src/App.kt", "class App\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        wait_loaded(&tree, cx);
        let (readme, util, app) = (root.join("README.md"), root.join("src/Util.kt"), root.join("src/App.kt"));
        let selected = |tree: &Entity<ProjectTree>, cx: &mut VisualTestContext| {
            tree.read_with(cx, |tree, _| tree.selected_path().map(str::to_owned))
        };
        edit(&workbench, &util, cx);
        assert!(!tree_labels(&tree, cx).contains(&"Util.kt".to_owned()), "the tree leaves src closed");

        assert_eq!(toggled("project.option.autoscrollFromSource", cx), Some(false));
        run_action(cx, "project.option.autoscrollFromSource", None);
        assert_eq!(toggled("project.option.autoscrollFromSource", cx), Some(true));
        wait_long(cx, "Util.kt to be selected", |cx| selected(&tree, cx) == Some(text_of(&util)));
        assert!(context_is_on(cx, "editorFocus"), "the keyboard stays in the editor");
        edit(&workbench, &readme, cx);
        wait_long(cx, "README.md to be selected", |cx| selected(&tree, cx) == Some(text_of(&readme)));
        edit(&workbench, &app, cx);
        wait_long(cx, "App.kt to be selected", |cx| selected(&tree, cx) == Some(text_of(&app)));

        run_action(cx, "project.option.autoscrollFromSource", None);
        assert_eq!(toggled("project.option.autoscrollFromSource", cx), Some(false));
        edit(&workbench, &readme, cx);
        assert_eq!(selected(&tree, cx), Some(text_of(&app)), "off: the selection stays");
        close_project(cx);
    }

    /// Autoscroll to Source opens the file the arrow keys move the selection to, the keyboard staying
    /// in the tree; with Enable Preview Tab it opens in the preview tab. Off, moving opens nothing.
    #[gpui_kit::test]
    async fn autoscroll_to_source_opens_the_file_the_selection_moves_to(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        select_in_tree(&workbench, &root.join("a.txt"), cx);
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert!(open_tabs(&workbench, cx).is_empty(), "off: moving opens nothing");

        assert_eq!(toggled("project.option.autoscrollToSource", cx), Some(false));
        run_action(cx, "project.option.autoscrollToSource", None);
        assert_eq!(toggled("project.option.autoscrollToSource", cx), Some(true));
        cx.simulate_keystrokes("down");
        wait_long(cx, "c.txt to open", |cx| open_tabs(&workbench, cx) == [("c.txt".to_owned(), false)]);
        assert!(context_is_on(cx, "projectViewFocus"), "the keyboard stays in the tree");

        run_action(cx, "project.option.previewTab", None);
        cx.simulate_keystrokes("up");
        wait_long(cx, "b.txt in the preview tab", |cx| open_tabs(&workbench, cx).contains(&("b.txt".to_owned(), true)));

        run_action(cx, "project.option.autoscrollToSource", None);
        assert_eq!(toggled("project.option.autoscrollToSource", cx), Some(false));
        let before = open_tabs(&workbench, cx);
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(open_tabs(&workbench, cx), before, "off again: a.txt does not open");
        close_project(cx);
    }

    /// The Project tool window's Options menu lists what the tree shows under "Appearance" and how
    /// it behaves under "Behavior"; a row chosen there turns its option on. Every behaviour option
    /// is remembered when the project is opened again, on and off.
    #[gpui_kit::test]
    async fn the_options_menu_lists_the_view_options_and_they_survive_reopening_the_project(cx: &mut TestAppContext) {
        use super::super::context_menus::ToolWindowCommand;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
        wait_loaded(&project_tree_of(&workbench, cx), cx);
        let more = cx.debug_bounds("more:project").expect("the Options button is drawn").center();
        cx.simulate_mouse_down(more, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(more, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<ToolWindowCommand>()).expect("the Options menu");
        let rows = menu.read_with(cx, |menu, _| menu.lines()).remove(0);
        let expected = [
            "Appearance",
            "> Show Hidden Files",
            "Show Excluded Files",
            "Compact Middle Packages",
            "Sort Folders First",
            "-",
            "Sort by Name",
            "Sort by Type",
            "Sort by Modification Time",
            "-",
            "Behavior",
            "Always Select Opened File",
            "Autoscroll to Source",
            "Open Files with Single Click",
            "Expand Directories with Single Click",
            "Enable Preview Tab",
        ];
        assert_eq!(rows[..expected.len()], expected.map(str::to_owned));
        let preview = "> Enable Preview Tab".to_owned();
        for _ in 0..expected.len() {
            if menu.read_with(cx, |menu, _| menu.lines().remove(0).contains(&preview)) {
                break;
            }
            cx.simulate_keystrokes("down");
        }
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(toggled("project.option.previewTab", cx), Some(true), "the row turns the option on");

        for key in BEHAVIOUR_TOGGLES.into_iter().filter(|key| *key != "previewTab") {
            run_action(cx, &format!("project.option.{key}"), None);
        }
        let states = |cx: &mut VisualTestContext| -> Vec<Option<bool>> {
            BEHAVIOUR_TOGGLES.into_iter().map(|key| toggled(&format!("project.option.{key}"), cx)).collect()
        };
        assert_eq!(states(cx), [Some(true); 5]);
        reopen(&workbench, &root, cx);
        assert_eq!(states(cx), [Some(true); 5], "on, they stay on");
        for key in BEHAVIOUR_TOGGLES {
            run_action(cx, &format!("project.option.{key}"), None);
        }
        reopen(&workbench, &root, cx);
        assert_eq!(states(cx), [Some(false); 5], "off, they stay off");
        close_project(cx);
    }

    // src/features/project-tree/viewModes.test.ts › project view modes › switches panes and
    // remembers the choice (with › lists open and changed files, expanded once)
    #[gpui_kit::test]
    async fn switches_panes_and_remembers_the_choice(cx: &mut TestAppContext) {
        let (workbench, cx, _dirs, root) = git_project(cx, |root| {
            init_repo(root);
            std::fs::create_dir_all(root.join("src")).expect("the folder is made");
            for (path, text) in [("a.txt", "a\n"), ("b.txt", "b\n"), ("src/c.txt", "c\n")] {
                std::fs::write(root.join(path), text).expect("the file is written");
            }
            git_in(root, &["add", "."]);
            git_in(root, &["commit", "-q", "-m", "files"]);
            std::fs::write(root.join("b.txt"), "b changed\n").expect("the file is written");
        });
        let tree = project_tree_of(&workbench, cx);
        wait_long(cx, "the project's files", |cx| tree_labels(&tree, cx) == ["src", "a.txt", "b.txt"]);
        edit(&workbench, &root.join("src/c.txt"), cx);
        let stored_mode =
            |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<UiStorage>().get(MODE_KEY).map(str::to_owned));
        let pane =
            |tree: &Entity<ProjectTree>, cx: &mut VisualTestContext| tree.read_with(cx, |tree, _| tree.pane().mode());

        run_action(cx, "project.viewMode.openFiles", None);
        assert_eq!(pane(&tree, cx), ViewMode::OpenFiles);
        wait_long(cx, "the open file, its folder open", |cx| tree_labels(&tree, cx) == ["src", "c.txt"]);
        assert_eq!(stored_mode(cx).as_deref(), Some("\"openFiles\""));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PROJECT_TOOL_WINDOW)));
        assert!(context_is_on(cx, "editorFocus"), "the keyboard stays in the editor");

        run_action(cx, "project.viewMode.changedFiles", None);
        wait_long(cx, "the changed file", |cx| tree_labels(&tree, cx) == ["b.txt"]);
        assert_eq!(stored_mode(cx).as_deref(), Some("\"changedFiles\""));
        // A file of the pane opens on a double click like any other.
        let changed = root.join("b.txt");
        click_row(&changed, 2, cx);
        wait_long(cx, "b.txt to open", |cx| editor_text(&workbench, &changed, cx).as_deref() == Some("b changed\n"));

        run_action(cx, "project.viewMode.projectFiles", None);
        wait_long(cx, "every file", |cx| tree_labels(&tree, cx) == ["src", "a.txt", "b.txt"]);
        run_action(cx, "project.viewMode.project", None);
        assert_eq!(pane(&tree, cx), ViewMode::Project);
        assert_eq!(stored_mode(cx).as_deref(), Some("\"project\""));
        wait_long(cx, "every file", |cx| tree_labels(&tree, cx) == ["src", "a.txt", "b.txt"]);

        // The pane is remembered: the project opened again shows "All Changed Files".
        run_action(cx, "project.viewMode.changedFiles", None);
        drop(tree);
        let tree = reopen(&workbench, &root, cx);
        assert_eq!(pane(&tree, cx), ViewMode::ChangedFiles);
        wait_long(cx, "the changed file again", |cx| tree_labels(&tree, cx) == ["b.txt"]);
    }

    /// Reload from Disk on a file of the tree reads it again into its editor; an editor with unsaved
    /// changes keeps them and a warning says why.
    #[gpui_kit::test]
    async fn reload_from_disk_reads_the_selected_file_again(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("notes.txt", "one\n"), ("clean.txt", "first\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let (notes, clean) = (root.join("notes.txt"), root.join("clean.txt"));
        edit(&workbench, &notes, cx);
        cx.simulate_input("x");
        cx.run_until_parked();
        std::fs::write(&notes, "two\n").expect("the file is written");
        select_in_tree(&workbench, &notes, cx);
        run_action(cx, "project.reloadFromDisk", None);
        wait_long(cx, "the warning", |cx| {
            notification_titles(&workbench, cx).contains(&"notes.txt has unsaved changes".to_owned())
        });
        let kept = editor_text(&workbench, &notes, cx).expect("the editor shows the file");
        assert!(kept.contains('x') && kept.contains("one"), "{kept:?}");

        // A file without unsaved changes, passed as the argument, is read again.
        edit(&workbench, &clean, cx);
        std::fs::write(&clean, "second\n").expect("the file is written");
        run_action(cx, "project.reloadFromDisk", Some(Value::from(text_of(&clean))));
        wait_long(cx, "the new text", |cx| editor_text(&workbench, &clean, cx).as_deref() == Some("second\n"));
        close_project(cx);
    }

    /// Find Usages of File searches the usages of the selected file's symbols; for a file no
    /// language server can search it says so. A folder is not searched.
    #[gpui_kit::test]
    async fn find_usages_of_file_searches_the_selected_file(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("docs/notes.txt", "words\n")]);
        let titled = |cx: &mut VisualTestContext| {
            notification_titles(&workbench, cx).into_iter().filter(|title| title.contains("usages of")).count()
        };
        select_in_tree(&workbench, &root.join("docs"), cx);
        run_action(cx, "project.findUsages", None);
        cx.run_until_parked();
        assert_eq!(titled(cx), 0, "a folder is not searched");

        select_in_tree(&workbench, &root.join("docs/notes.txt"), cx);
        run_action(cx, "project.findUsages", None);
        wait_long(cx, "the search to answer", |cx| {
            notification_titles(&workbench, cx).contains(&"Cannot find usages of 'notes.txt'".to_owned())
        });
        close_project(cx);
    }

    /// Reformat File and Optimize Imports in File, run in the tree, open the selected file and
    /// apply the language server's edits to it.
    #[gpui_kit::test]
    async fn reformat_and_optimize_imports_in_the_tree_change_the_selected_file(cx: &mut TestAppContext) {
        let (_home, server) = start_with_mock_server(cx);
        let files = [("b.mock", "fn  f\n"), ("c.mock", "import y\nimport x\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let (b, c) = (root.join("b.mock"), root.join("c.mock"));
        let range = |line: u32, from: u32, to: u32| {
            let at = |character: u32| json!({ "line": line, "character": character });
            json!({ "start": at(from), "end": at(to) })
        };
        let capabilities = json!({ "documentFormattingProvider": true });
        server.answer(json!({
            "capabilities": capabilities.clone(),
            "textDocument/formatting": [{ "range": range(0, 2, 4), "newText": " " }],
        }));
        select_in_tree(&workbench, &b, cx);
        run_action(cx, "project.reformatCode", None);
        wait_long(cx, "b.mock formatted", |cx| editor_text(&workbench, &b, cx).as_deref() == Some("fn f\n"));

        let whole = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 8 } });
        let organize = json!({
            "title": "Organize imports",
            "kind": "source.organizeImports",
            "edit": {
                "changes": { format!("file://{}", c.display()): [{ "range": whole, "newText": "import x\nimport y" }] },
            },
        });
        server.answer(json!({ "capabilities": capabilities, "textDocument/codeAction": [organize] }));
        select_in_tree(&workbench, &c, cx);
        run_action(cx, "project.optimizeImports", None);
        wait_long(cx, "c.mock's imports sorted", |cx| {
            editor_text(&workbench, &c, cx).as_deref() == Some("import x\nimport y\n")
        });
        close_project(cx);
    }

    /// The tree read again after a file is written beside the selected one keeps that selection, and
    /// Optimize Imports in the tree still acts on it; the removal of the selected file clears the
    /// selection.
    #[gpui_kit::test]
    async fn a_tree_read_again_after_a_change_on_disk_keeps_the_selected_file(cx: &mut TestAppContext) {
        let (_home, server) = start_with_mock_server(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("c.mock", "import y\nimport x\n")]);
        let c = root.join("c.mock");
        let whole = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 8 } });
        let organize = json!({
            "title": "Organize imports",
            "kind": "source.organizeImports",
            "edit": {
                "changes": { format!("file://{}", c.display()): [{ "range": whole, "newText": "import x\nimport y" }] },
            },
        });
        let capabilities = json!({ "documentFormattingProvider": true });
        server.answer(json!({ "capabilities": capabilities, "textDocument/codeAction": [organize] }));
        select_in_tree(&workbench, &c, cx);
        let tree = project_tree_of(&workbench, cx);
        let reads = tree.read_with(cx, |tree, _| tree.folder_reads());

        std::fs::write(root.join("d.mock"), "new\n").expect("the file is written");
        wait_long(cx, "the tree to read the project again", |cx| {
            tree.read_with(cx, |tree, _| tree.folder_reads() > reads)
                && tree_labels(&tree, cx).contains(&"d.mock".into())
        });
        assert_eq!(tree.read_with(cx, |tree, _| tree.selected_path().map(str::to_owned)), Some(text_of(&c)));
        run_action(cx, "project.optimizeImports", None);
        wait_long(cx, "c.mock's imports sorted", |cx| {
            editor_text(&workbench, &c, cx).as_deref() == Some("import x\nimport y\n")
        });

        // The removal reaches the tree as the watcher's removal event, so the test does not depend on
        // how the watcher reports a file removed right after it was created.
        let d = text_of(&root.join("d.mock"));
        select_in_tree(&workbench, &root.join("d.mock"), cx);
        std::fs::remove_file(&d).expect("the file is removed");
        tree.update(cx, |tree, cx| tree.apply_fs_change(&[], &[], std::slice::from_ref(&d), cx));
        assert_eq!(tree.read_with(cx, |tree, _| tree.selected().map(str::to_owned)), None);
        wait_long(cx, "the removed file to leave the tree", |cx| !tree_labels(&tree, cx).contains(&"d.mock".into()));
        close_project(cx);
    }
}
