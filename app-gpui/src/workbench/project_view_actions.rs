//! The Project view's navigation and clipboard actions in a project window: Select In and Select
//! Opened File, File Path, Expand All and Collapse All, the Copy Path forms, Compare With, Jump to
//! Source, Reveal in the file manager, Open in Terminal, Find in Folder, the view options that
//! change what the tree shows, Mark Directory as, and Undo of the view's last delete or move.
//!
//! They run on the models of [`crate::project_view`] through the Project view's state of
//! `project_files.rs` (`with_view`, `start_flow`): the state mirrors the tree before every step,
//! and what a step changes in the expansion or asks to reveal goes back to the tree.

use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::platform::i18n::I18n;
use crate::platform::misc::copy_path::PathKind;
use crate::platform::misc::platform_info::Os;
use crate::platform::project_roots::{FolderRole, OpenProject, RootFolder, validate_project_structure};
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::project_tree_paths::parent_of;
use crate::project_view::actions::{
    SORT_LABELS, SelectIn, context_target, copy_path_request, expand_all_from, file_path_target, folder_from,
    open_in_terminal, option_value, project_actions, select_in, select_in_project_view, select_in_targets,
    toggle_option,
};
use crate::project_view::context_menu::{Item, ProjectCommand};
use crate::project_view::file_path::{FILE_PATH_TITLE, file_path_rows};
use crate::project_view::mark_directory::mark_directory;
use crate::project_view::operations::{
    Effect, UndoKind, copy_paths, immediate, node_refs, open_selection, paths_for, undo_delete,
};
use crate::project_view::store::{EXPAND_ALL_LIMIT, ExpandAll, ExpandStep, ViewOptions};
use crate::vcs_diff::compare::compare_files_data;
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{App, Context, Focusable, Global, PathPromptOptions, Window, point, px};
use ide_core::{IdeError, IdeResult};
use ide_host::Host;
use ide_workspace::settings::SettingsScope;
use ide_workspace::{RootKind, SourceFolder};
use serde_json::Value;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

/// The view options the tree acts on, by the name they are stored under.
const TREE_TOGGLES: [&str; 4] = ["showHidden", "showExcluded", "compact", "foldersFirst"];

/// The Compare With action that asks for the file to compare with.
const COMPARE_WITH_FILE: &str = "diff.compareWith";

/// The action that reverts the last move or rename of the Project view.
const UNDO_MOVE: &str = "project.undoMove";

/// Where the OS file manager is asked to show a path. Without this global the platform's file
/// manager is; a test puts a recorder in its place, as the test platform has no file manager.
#[derive(Clone)]
pub(crate) struct FileManager(pub(crate) Rc<dyn Fn(&Path, &mut App)>);

impl Global for FileManager {}

/// Shows `paths` in the OS file manager (Finder, Explorer, the desktop's file manager).
pub(super) fn reveal_in_file_manager(paths: Vec<String>, cx: &mut App) {
    let reveal = cx.try_global::<FileManager>().map(|manager| manager.0.clone());
    for path in paths {
        match &reveal {
            Some(reveal) => reveal(Path::new(&path), cx),
            None => cx.reveal_path(Path::new(&path)),
        }
    }
}

/// Edit › Undo while the Project view has the keyboard: the view's last delete comes back from
/// Local History, or its last move or rename is reverted. False when the view has nothing to
/// revert, so that Undo goes on to what else it undoes.
pub fn undo_in_project_view(window: &mut Window, cx: &mut App) -> bool {
    let Some(workbench) = super::for_window(window, cx) else { return false };
    workbench.update(cx, |workbench, cx| workbench.undo_file_operation(window, cx))
}

/// The OS the window and the project's files are on.
fn this_os() -> Os {
    Os::from_platform(std::env::consts::OS)
}

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// Runs `work` on the backend's runtime with the host.
fn on_backend<T, F, W>(cx: &App, work: W) -> impl Future<Output = IdeResult<T>> + 'static + use<T, F, W>
where
    T: Send + 'static,
    F: Future<Output = IdeResult<T>> + Send + 'static,
    W: FnOnce(Arc<Host>) -> F + Send + 'static,
{
    let call = cx.try_global::<Backend>().map(|backend| backend.with_host(work));
    async move {
        match call {
            None => Err(IdeError::unavailable("The backend is not running")),
            Some(call) => call.await.unwrap_or_else(|error| Err(IdeError::internal(format!("{error:#}")))),
        }
    }
}

/// A root the backend detected or the user marked, as the Project view's models know it.
fn root_folder(folder: SourceFolder) -> RootFolder {
    let kind = match folder.kind {
        RootKind::Source => FolderRole::Source,
        RootKind::Test => FolderRole::Test,
        RootKind::Resources => FolderRole::Resources,
        RootKind::TestResources => FolderRole::TestResources,
        RootKind::Excluded => FolderRole::Excluded,
    };
    RootFolder { path: folder.path, kind, detected_from: folder.detected_from }
}

/// The view options as stored; the defaults for what is not.
fn stored_options(cx: &App) -> ViewOptions {
    cx.try_global::<UiStorage>().map(ViewOptions::stored).unwrap_or_default()
}

/// Registers the actions of the Project view this module carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("navigate.selectInProjectView", |call, window, cx| {
        let path = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.select_in_project_tree(path, true, window, cx);
        })
    });
    registry.handle("project.reveal", |call, window, cx| {
        let path = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.select_in_project_tree(path, false, window, cx);
        })
    });
    registry.handle("navigate.selectIn", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.select_in(arg.as_ref(), window, cx))
    });
    registry.handle("navigate.showFilePath", |call, window, cx| {
        let (arg, focus) = (call.arg.cloned(), call.ctx.is_truthy("projectViewFocus"));
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.show_file_path(arg.as_ref(), focus, window, cx);
        })
    });
    registry.handle("project.expandAll", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.expand_all(window, cx))
    });
    registry.handle("project.collapseAll", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.collapse_all(window, cx))
    });
    registry.handle("project.copyPath", |call, window, cx| {
        let (kind, paths) = copy_path_request(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.copy_path_form(kind, paths.as_ref(), window, cx);
        })
    });
    for (id, kind) in [
        ("project.copyRelativePath", PathKind::Relative),
        ("project.copyFileName", PathKind::Name),
        ("project.copySourceRootPath", PathKind::Source),
    ] {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.copy_path_form(kind, arg.as_ref(), window, cx);
            })
        });
    }
    registry.handle("project.compareWith", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.compare_with(arg.as_ref(), window, cx))
    });
    registry.handle("project.open", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            let effects = workbench.with_view(true, window, cx, |state, _| open_selection(state));
            workbench.start_flow(immediate(effects), true, None, window, cx);
        })
    });
    registry.handle("project.openInTerminal", |call, window, cx| {
        let (arg, focus) = (call.arg.cloned(), call.ctx.is_truthy("projectViewFocus"));
        on_workbench(window, cx, move |workbench, window, cx| {
            let active = workbench.active_file_path(cx);
            let effects = workbench.with_view(focus, window, cx, |state, _| {
                open_in_terminal(state, arg.as_ref(), focus, active.as_deref())
            });
            workbench.start_flow(immediate(effects), focus, None, window, cx);
        })
    });
    registry.handle("project.findInFolder", |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            let folder = workbench.with_view(true, window, cx, |state, _| folder_from(state, arg.as_ref()));
            let Some(directory) = folder else { return };
            let find = Effect::RunAction { id: "find.inPath", arg: serde_json::json!({ "directory": directory }) };
            workbench.start_flow(immediate(vec![find]), true, None, window, cx);
        })
    });
    register_computed(registry);
}

/// The actions whose descriptions are computed (they are not in the action manifest): Reveal in
/// the file manager, named after it, and the view options as toggles (Find Action, the View menu):
/// the ones the tree acts on, and the orders within a folder.
fn register_computed(registry: &mut ActionRegistry) {
    let metas: Vec<ActionMeta> = project_actions(&mut I18n::default(), this_os());
    let meta_of = |id: &str| metas.iter().find(|meta| meta.id == id).cloned();
    if let Some(meta) = meta_of("project.revealInFinder") {
        registry.register(Action::new(meta, |call, window, cx| {
            let (arg, focus) = (call.arg.cloned(), call.ctx.is_truthy("projectViewFocus"));
            on_workbench(window, cx, move |workbench, _, cx| workbench.reveal_in_finder(arg.as_ref(), focus, cx))
        }));
    }
    for key in TREE_TOGGLES {
        let Some(meta) = meta_of(&format!("project.option.{key}")) else { continue };
        let action = Action::new(meta, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| {
                workbench.change_view_options(cx, |options| {
                    toggle_option(options, key);
                });
            })
        })
        .toggled(move |_, cx| option_value(&stored_options(cx), key).unwrap_or(false));
        registry.register(action);
    }
    for (sort_by, _) in SORT_LABELS {
        let Some(meta) = meta_of(&format!("project.sortBy.{sort_by}")) else { continue };
        let action = Action::new(meta, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| {
                workbench.change_view_options(cx, |options| sort_by.clone_into(&mut options.sort_by));
            })
        })
        .toggled(move |_, cx| stored_options(cx).sort_by == sort_by);
        registry.register(action);
    }
}

impl Workbench {
    /// Where a chooser of the Project view opens: centred near the top of the window.
    fn chooser_position(window: &Window) -> gpui_kit::Point<gpui_kit::Pixels> {
        let width = f32::from(window.viewport_size().width);
        point(px((width / 2.0 - 100.0).max(0.0)), px(80.0))
    }

    /// Selects `path` (without one, the edited file) in the Project view, which shows; `focus`
    /// gives the tree the keyboard. False when there is nothing to select.
    fn select_in_project_tree(
        &mut self,
        path: Option<String>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let active = self.active_file_path(cx);
        let effects = self.with_view(focus, window, cx, |state, env| {
            select_in_project_view(state, path.as_deref(), active.as_deref(), focus, env.stored_expanded).1
        });
        if effects.is_empty() {
            return false;
        }
        self.start_flow(immediate(effects), focus, None, window, cx);
        true
    }

    /// Select In…: a path passed is selected in the Project view; without one the chooser of the
    /// places that can show the edited file (else the selection) opens.
    fn select_in(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.active_file_path(cx);
        let chosen = self.with_view(false, window, cx, |state, _| select_in(state, arg, active.as_deref()));
        match chosen {
            SelectIn::Nothing => {}
            SelectIn::Reveal(path) => {
                self.select_in_project_tree(Some(path), true, window, cx);
            }
            SelectIn::Choose(path) => {
                let menu = self.with_menu_context(cx, |ctx, _| select_in_targets(ctx, &path, active.as_deref()));
                let mut items: Vec<Item> = vec![MenuItem::entry(menu.title, None)];
                items.extend(menu.items);
                self.show_project_chooser(&items, window, cx);
            }
        }
    }

    /// Navigate › File Path: the file (the selection's while the tree has the keyboard, else the
    /// edited one) and the folders above it; choosing one shows it in the file manager.
    fn show_file_path(&mut self, arg: Option<&Value>, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.active_file_path(cx);
        let target =
            self.with_view(focus, window, cx, |state, _| file_path_target(state, arg, focus, editor.as_deref()));
        let Some((path, is_dir)) = target else { return };
        let mut items: Vec<Item> = vec![MenuItem::entry(FILE_PATH_TITLE, None)];
        for row in file_path_rows(&path, is_dir) {
            items.push(MenuItem::item(row.label, ProjectCommand::Reveal(vec![row.path])).icon(row.icon));
        }
        self.show_project_chooser(&items, window, cx);
    }

    /// A chooser of Project view rows; the one chosen is carried out on the selection.
    fn show_project_chooser(&mut self, items: &[Item], window: &mut Window, cx: &mut Context<Self>) {
        let position = Self::chooser_position(window);
        let run = |workbench: &mut Self, command: ProjectCommand, window: &mut Window, cx: &mut Context<Self>| {
            workbench.run_project_command(command, Vec::new(), true, window, cx);
        };
        self.show_menu(items, position, run, window, cx);
    }

    /// Reveal in the file manager: the paths passed, else the selection while the tree has the
    /// keyboard, else the edited file.
    fn reveal_in_finder(&mut self, arg: Option<&Value>, focus: bool, cx: &mut Context<Self>) {
        let target = context_target(arg, focus, self.active_file_path(cx).as_deref());
        let paths: Vec<String> = self.nodes_for(target.as_ref(), cx).iter().map(|node| node.path.clone()).collect();
        reveal_in_file_manager(paths, cx);
    }

    /// Expand All: every folder below the selected one (below the project without one), read as
    /// it goes, up to [`EXPAND_ALL_LIMIT`] folders. Excluded folders and symbolic links stay closed.
    fn expand_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = self.with_view(true, window, cx, |state, _| {
            let from = expand_all_from(state);
            ExpandAll::start(state, from.as_deref(), EXPAND_ALL_LIMIT)
        });
        // A filtered pane has every folder already: `start` expanded them.
        let Some(mut expand) = started else { return };
        let label = self.label.clone();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let step = this.update_in(cx, |workbench, window, cx| {
                    workbench.with_view(true, window, cx, |state, _| expand.step(state))
                })?;
                let ExpandStep::Load(path) = step else { return anyhow::Ok(()) };
                let (window_label, dir) = (label.clone(), PathBuf::from(&path));
                let read = this.update(cx, |_, cx| {
                    on_backend(
                        cx,
                        move |host| async move { ide_host::ops::fs::read_dir(&host, &window_label, dir).await },
                    )
                })?;
                let read = read.await;
                this.update(cx, |workbench, cx| {
                    if let Some(tree) = workbench.project_tree().cloned() {
                        tree.update(cx, |tree, cx| tree.put_read(&path, read, cx));
                    }
                })?;
            }
        })
        .detach();
    }

    /// Collapse All: the project root stays open, and the tree gets the keyboard.
    fn collapse_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_view(true, window, cx, |state, _| state.collapse_all());
        if let Some(tree) = self.project_tree().cloned() {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        }
    }

    /// Copies the `kind` form of the paths `arg` names (the selection's without any). The source
    /// root of a path is one of the project's roots, which the backend knows.
    fn copy_path_form(&mut self, kind: PathKind, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let nodes = self.nodes_for(arg, cx);
        if nodes.is_empty() {
            return;
        }
        let label = self.label.clone();
        let roots =
            on_backend(
                cx,
                move |host| async move { ide_host::ops::project::roots(&host, &label).map(|roots| roots.roots) },
            );
        cx.spawn_in(window, async move |this, cx| {
            let roots: Vec<RootFolder> = match roots.await {
                Ok(roots) => roots.into_iter().map(root_folder).collect(),
                // Without the roots only the source root form has nothing to copy.
                Err(error) => {
                    tracing::debug!("copy path: the project's roots are not known: {}", error.message);
                    Vec::new()
                }
            };
            this.update_in(cx, |workbench, window, cx| {
                let root = workbench.project_root(cx).map(|root| root.to_string_lossy().into_owned());
                let name = workbench.project_name(cx);
                let paths = workbench.with_view(true, window, cx, |state, _| {
                    let project = root.as_deref().map(|root| OpenProject { root, name: &name, workspace: None });
                    paths_for(state, kind, &node_refs(&nodes), project, &roots)
                });
                workbench.start_flow(copy_paths(&paths), true, None, window, cx);
            })
        })
        .detach();
    }

    /// The name of the open project, as the tree shows it.
    fn project_name(&self, cx: &App) -> String {
        let root = self.project_root(cx);
        root.and_then(|root| root.file_name().map(|name| name.to_string_lossy().into_owned())).unwrap_or_default()
    }

    /// Compare With…: two files are compared with each other; one with the file chosen for it.
    fn compare_with(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let nodes = self.nodes_for(arg, cx);
        let files: Vec<String> = nodes.iter().filter(|node| !node.dir).map(|node| node.path.clone()).collect();
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        match files.as_slice() {
            [left, right] => self.open_diff(compare_files_data(left, right, root.as_deref()), window, cx),
            [first, ..] => self.compare_with_chosen_file(first.clone(), root, window, cx),
            [] => {}
        }
    }

    /// Compares `left` with a file the user picks: through the diff feature's Compare With when it
    /// is there, else with the OS picker.
    fn compare_with_chosen_file(
        &mut self,
        left: String,
        root: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delegated = cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(COMPARE_WITH_FILE));
        if delegated {
            let compare = Effect::RunAction { id: COMPARE_WITH_FILE, arg: Value::String(left) };
            self.start_flow(immediate(vec![compare]), true, None, window, cx);
            return;
        }
        let picked =
            cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let right = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    tracing::warn!("compare with: cannot pick a file to compare {left} with: {error:#}");
                    None
                }
            };
            let Some(right) = right else { return };
            let right = right.to_string_lossy().into_owned();
            let compared = this.update_in(cx, |workbench, window, cx| {
                workbench.open_diff(compare_files_data(&left, &right, root.as_deref()), window, cx);
            });
            if let Err(error) = compared {
                tracing::info!("compare with: the window closed before {left} was compared: {error:#}");
            }
        })
        .detach();
    }

    /// Changes the view options of the tree, for every Project view from now on.
    fn change_view_options(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ViewOptions)) {
        if let Some(tree) = self.project_tree().cloned() {
            tree.update(cx, |tree, cx| tree.set_options(change, cx));
        }
    }

    /// Undo in the Project view: its latest delete comes back from Local History, else its latest
    /// move or rename is reverted (`project.undoMove`). False when there is neither.
    fn undo_file_operation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let started = self.with_view(true, window, cx, |state, env| match env.log.last() {
            Some(UndoKind::Delete) => Some(undo_delete(state, env)),
            Some(UndoKind::Refactoring) => Some(immediate(vec![Effect::RunAction { id: UNDO_MOVE, arg: Value::Null }])),
            None => None,
        });
        let Some(started) = started else { return false };
        self.start_flow(started, true, None, window, cx);
        true
    }

    /// Mark Directory as: gives the folder `path` the `role` (`None`: unmarks it) in the project's
    /// Project Structure settings. The tree reads the folder's parent again, so an excluded folder
    /// shows as one; the backend leaves it out of the search from then on.
    pub(super) fn mark_directory_as(
        &mut self,
        path: String,
        role: Option<FolderRole>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project_root(cx).map(|root| root.to_string_lossy().into_owned()) else { return };
        let label = self.label.clone();
        let detected = on_backend(cx, move |host| async move {
            ide_host::ops::project::roots(&host, &label).map(|roots| roots.detected)
        });
        cx.spawn_in(window, async move |this, cx| {
            let detected = detected.await;
            let marked = this.update(cx, |workbench, cx| match detected {
                Ok(detected) => {
                    let detected: Vec<RootFolder> = detected.into_iter().map(root_folder).collect();
                    workbench.write_directory_mark(&root, &path, role, &detected, cx);
                }
                Err(error) => workbench.notify(mark_failed_notification(&error.message), cx),
            });
            if let Err(error) = marked {
                tracing::info!("mark directory: the window closed before {path} was marked: {error:#}");
            }
        })
        .detach();
    }

    /// Writes the marks that give `path` the `role` and shows the folder as it is now.
    fn write_directory_mark(
        &mut self,
        root: &str,
        path: &str,
        role: Option<FolderRole>,
        detected: &[RootFolder],
        cx: &mut Context<Self>,
    ) {
        let current = settings::effective(&self.label, cx).project_structure;
        let structure = match serde_json::to_value(&current) {
            Ok(raw) => validate_project_structure(&raw),
            Err(error) => return self.notify(mark_failed_notification(&error.to_string()), cx),
        };
        for write in mark_directory(root, path, role, &structure, detected) {
            if let Err(error) = settings::set(&self.label, SettingsScope::Project, write.key, &write.value, cx) {
                return self.notify(mark_failed_notification(&error.message), cx);
            }
        }
        // The folder's row says whether it is excluded: its listing is read again.
        if let Some(tree) = self.project_tree().cloned() {
            let parent = parent_of(path).to_owned();
            tree.update(cx, |tree, cx| tree.load_dir(&parent, true, cx));
        }
    }

    /// Whether the Project view tool window shows.
    #[cfg(test)]
    fn project_view_shown(&self) -> bool {
        self.is_tool_window_visible(super::PROJECT_TOOL_WINDOW)
    }
}

/// What is raised when a folder's mark could not be written.
fn mark_failed_notification(message: &str) -> NotificationRequest {
    NotificationRequest {
        body: Some(message.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Error, "Cannot mark the directory")
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{notification_titles, run_action, start, wait_long};
    use super::*;
    use crate::platform::actions::{action_toggled, execute_action_or_throw};
    use crate::platform::context::window_context;
    use crate::project_tree::ProjectTree;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use ide_host::ops::search::SearchEvent;
    use ide_search::{SearchContext, SearchQuery, SearchScope};
    use serde_json::json;
    use std::cell::RefCell;
    use std::sync::Mutex;

    /// The key of the platform's keymap: ⌘ on macOS, Ctrl elsewhere.
    fn primary(key: &str) -> String {
        if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
    }

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

    /// Selects `path` in the tree and gives the tree the keyboard.
    fn select_in_tree(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
        let tree = project_tree_of(workbench, cx);
        let at = text_of(path);
        wait_long(cx, "the project to be listed", |cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
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

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {
        cx.update(|window, cx| window_context(window, cx).is_truthy(key))
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    /// The actions the keyboard runs while `keys` are pressed.
    fn actions_run_by(keys: &str, cx: &mut VisualTestContext) -> Vec<String> {
        let ran: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = ran.clone();
        let listener = cx.update(|_, cx| {
            cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
        });
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
        ran.take()
    }

    /// The paths the file manager is asked to show from now on, in order.
    fn record_reveals(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<String>>> {
        let revealed: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = revealed.clone();
        let record = move |path: &Path, _: &mut App| sink.borrow_mut().push(text_of(path));
        cx.update(|_, cx| cx.set_global(FileManager(Rc::new(record))));
        revealed
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

    /// Whether every folder row the tree shows is expanded.
    fn all_open(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) -> bool {
        tree.read_with(cx, |tree, _| tree.rows().iter().filter(|row| row.node.dir).all(|row| row.expanded))
    }

    fn open_rows(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) -> usize {
        tree.read_with(cx, |tree, _| tree.rows().iter().filter(|row| row.expanded).count())
    }

    /// Opens the context menu of `path`'s row with a right click.
    fn right_click_row(path: &Path, cx: &mut VisualTestContext) {
        let selector: &'static str = Box::leak(format!("project-row:{}", text_of(path)).into_boxed_str());
        let row = cx.debug_bounds(selector).expect("the row is drawn");
        cx.simulate_mouse_down(row.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.simulate_mouse_up(row.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
        cx.run_until_parked();
    }

    /// Chooses the row `labels` names in the open Project view menu: each label but the last opens
    /// a submenu, the last is chosen.
    fn choose_in_menu(workbench: &Entity<Workbench>, labels: &[&str], cx: &mut VisualTestContext) {
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("a menu is open");
        for (level, label) in labels.iter().enumerate() {
            let selected = format!("> {label}");
            let is_selected = |cx: &mut VisualTestContext| {
                menu.read_with(cx, |menu, _| menu.lines().get(level).is_some_and(|rows| rows.contains(&selected)))
            };
            for _ in 0..60 {
                if is_selected(cx) {
                    break;
                }
                cx.simulate_keystrokes("down");
            }
            assert!(is_selected(cx), "{label} in {:?}", menu.read_with(cx, |menu, _| menu.lines()));
            cx.simulate_keystrokes(if level + 1 < labels.len() { "right" } else { "enter" });
            cx.run_until_parked();
        }
    }

    /// The files of the project a search for `pattern` finds, project-relative and sorted.
    fn searched(pattern: &str, cx: &mut VisualTestContext) -> Vec<String> {
        let query = SearchQuery {
            pattern: pattern.to_owned(),
            is_regex: false,
            case_sensitive: true,
            whole_word: false,
            include: Vec::new(),
            exclude: Vec::new(),
            scope: SearchScope::Project,
            max_matches: 0,
            context: SearchContext::Any,
        };
        let found: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = Arc::clone(&found);
        let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
            if let SearchEvent::File { file } = event {
                sink.lock().expect("the results are kept").push(file.rel_path);
            }
        });
        cx.update(|_, cx| {
            let backend = cx.global::<Backend>();
            let host = backend.host.clone();
            backend.block_on(ide_host::ops::search::start(&host, "w1", Backend::next_request_id(), query, on_event))
        })
        .expect("the search runs");
        let mut files = found.lock().expect("the results are kept").clone();
        files.sort();
        files
    }

    /// Select in Project View shows the Project view with the edited file selected and the tree
    /// holding the keyboard; Reveal in Project View selects a file given to it and leaves the
    /// keyboard in the editor.
    #[gpui_kit::test]
    async fn select_in_project_view_selects_the_edited_file_and_reveal_leaves_the_keyboard_where_it_was(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let files = [("src/deep/main.rs", "fn main() {}\n"), ("docs/guide.md", "# Guide\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        let file = root.join("src/deep/main.rs");
        edit(&workbench, &file, cx);
        run_action(cx, "navigate.selectInProjectView", None);
        let at = text_of(&file);
        wait_long(cx, "the file to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
        assert!(workbench.read_with(cx, |w, _| w.project_view_shown()), "the Project view shows");
        assert!(context_is_on(cx, "projectViewFocus"), "the tree has the keyboard");

        edit(&workbench, &file, cx);
        let guide = text_of(&root.join("docs/guide.md"));
        run_action(cx, "project.reveal", Some(json!(guide)));
        wait_long(cx, "the guide to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(guide.as_str()))
        });
        assert!(!context_is_on(cx, "projectViewFocus"), "the keyboard stays in the editor");
        assert!(context_is_on(cx, "editorFocus"));
        close_project(cx);
    }

    /// Select In… (⌥F1) lists the places that can show the edited file, the Project view first;
    /// choosing it selects the file in the tree. A path passed to it is selected at once, without
    /// the list.
    #[gpui_kit::test]
    async fn select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it(cx: &mut TestAppContext) {
        use crate::project_view::context_menu::reveal_label;
        let _home = start(cx);
        let files = [("src/lib.rs", "pub fn a() {}\n"), ("b.txt", "")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        let file = root.join("src/lib.rs");
        edit(&workbench, &file, cx);
        assert_eq!(actions_run_by("alt-f1", cx), ["navigate.selectIn"]);
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the chooser is open");
        let rows = menu.read_with(cx, |menu, _| menu.lines()).remove(0);
        assert_eq!(rows, ["Select In", "> Project View", reveal_label(this_os()), "Terminal"]);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let at = text_of(&file);
        wait_long(cx, "the file to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).is_none());

        let other = text_of(&root.join("b.txt"));
        run_action(cx, "navigate.selectIn", Some(json!({ "path": other })));
        wait_long(cx, "b.txt to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(other.as_str()))
        });
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).is_none(), "no chooser");
        close_project(cx);
    }

    /// Select In… (⌥F1) and File Path (⌥⌘F12) need a project: in a window without one their keys
    /// run nothing.
    #[gpui_kit::test]
    async fn the_navigation_choosers_keys_run_nothing_without_a_project(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        let file_path = if cfg!(target_os = "macos") { "cmd-alt-f12" } else { "ctrl-alt-f12" };
        for keys in ["alt-f1", file_path] {
            assert_eq!(actions_run_by(keys, cx), Vec::<String>::new(), "{keys}");
        }
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).is_none());
    }

    /// File Path (⌥⌘F12) lists the edited file, then every folder above it up to the root of the
    /// disk; choosing a row shows it in the file manager.
    #[gpui_kit::test]
    async fn file_path_lists_the_file_then_every_folder_up_to_the_root(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/main.rs", "fn main() {}\n")]);
        let revealed = record_reveals(cx);
        let file = root.join("src/main.rs");
        edit(&workbench, &file, cx);
        let keys = if cfg!(target_os = "macos") { "cmd-alt-f12" } else { "ctrl-alt-f12" };
        assert_eq!(actions_run_by(keys, cx), ["navigate.showFilePath"]);
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the File Path popup");
        let rows = menu.read_with(cx, |menu, _| menu.lines()).remove(0);
        let mut expected = vec!["File Path".to_owned(), "> main.rs".to_owned(), "src".to_owned()];
        expected.extend(root.ancestors().map(|folder| {
            folder.file_name().map_or_else(|| text_of(folder), |name| name.to_string_lossy().into_owned())
        }));
        assert_eq!(rows, expected);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).is_none());
        assert!(revealed.borrow().is_empty(), "Escape shows nothing");

        run_action(cx, "navigate.showFilePath", None);
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();
        assert_eq!(*revealed.borrow(), [text_of(&root.join("src"))]);
        close_project(cx);
    }

    /// Reveal in Finder shows the tree's selection in the file manager, the edited file from the
    /// editor, and the paths given to it; the context menu's Open In › Finder asks the same.
    #[gpui_kit::test]
    async fn reveal_in_finder_shows_the_selection_or_the_edited_file_in_the_file_manager(cx: &mut TestAppContext) {
        use crate::project_view::context_menu::reveal_label;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n"), ("src/b.rs", "")]);
        let revealed = record_reveals(cx);
        let (a, b) = (root.join("a.txt"), root.join("src/b.rs"));
        select_in_tree(&workbench, &a, cx);
        run_action(cx, "project.revealInFinder", None);
        edit(&workbench, &b, cx);
        run_action(cx, "project.revealInFinder", None);
        run_action(cx, "project.revealInFinder", Some(json!([text_of(&a), text_of(&b)])));
        assert_eq!(*revealed.borrow(), [text_of(&a), text_of(&b), text_of(&a), text_of(&b)]);

        revealed.borrow_mut().clear();
        select_in_tree(&workbench, &a, cx);
        right_click_row(&a, cx);
        choose_in_menu(&workbench, &["Open In", reveal_label(this_os())], cx);
        assert_eq!(*revealed.borrow(), [text_of(&a)]);
        close_project(cx);
    }

    /// Expand All (⌘=) opens every folder below the project and Collapse All (⌘-) closes them, the
    /// project root staying open; their keys act in the tree only: in the editor neither runs.
    #[gpui_kit::test]
    async fn expand_all_and_collapse_all_open_and_close_every_folder_and_their_keys_act_in_the_tree_only(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let files = [("a/one.txt", "1\n"), ("a/b/two.txt", "2\n"), ("a/b/c/three.txt", "3\n"), ("x/four.txt", "4\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        let (expand, collapse) = (primary("="), primary("-"));
        select_in_tree(&workbench, &root, cx);
        assert_eq!(open_rows(&tree, cx), 1, "only the project is open");

        assert_eq!(actions_run_by(&expand, cx), ["project.expandAll"]);
        wait_long(cx, "every folder to open", |cx| {
            all_open(&tree, cx) && tree_labels(&tree, cx).contains(&"three.txt".to_owned())
        });
        assert!(tree_labels(&tree, cx).contains(&"four.txt".to_owned()));

        // In the editor the same keys are not the tree's.
        edit(&workbench, &root.join("x/four.txt"), cx);
        assert!(!actions_run_by(&collapse, cx).contains(&"project.collapseAll".to_owned()));
        assert!(all_open(&tree, cx), "the tree is left as it is");

        // From Find Action it acts wherever the keyboard is, and gives the tree the keyboard.
        run_action(cx, "project.collapseAll", None);
        assert_eq!(open_rows(&tree, cx), 1);
        assert!(context_is_on(cx, "projectViewFocus"));
        edit(&workbench, &root.join("x/four.txt"), cx);
        assert!(!actions_run_by(&expand, cx).contains(&"project.expandAll".to_owned()));
        assert_eq!(open_rows(&tree, cx), 1, "the editor's key leaves the tree closed");

        run_action(cx, "project.expandAll", None);
        wait_long(cx, "every folder to open again", |cx| {
            all_open(&tree, cx) && tree_labels(&tree, cx).contains(&"three.txt".to_owned())
        });
        select_in_tree(&workbench, &root, cx);
        assert_eq!(actions_run_by(&collapse, cx), ["project.collapseAll"]);
        assert_eq!(open_rows(&tree, cx), 1);
        close_project(cx);
    }

    /// The Copy Path forms put the selection's name, its path from the content root, or the form
    /// asked for of the paths given on the clipboard. Copy Path From Source Root copies the path
    /// below the folder marked as a sources root from the context menu.
    #[gpui_kit::test]
    async fn the_copy_path_forms_copy_the_selection_and_the_source_root_path_follows_mark_directory_as(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let files = [("src/app/a.txt", "a\n"), ("src/b.txt", ""), ("gen/c.txt", "")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let file = root.join("src/app/a.txt");
        select_in_tree(&workbench, &file, cx);
        run_action(cx, "project.copyFileName", None);
        wait_long(cx, "the file name", |cx| clipboard(cx).as_deref() == Some("a.txt"));
        run_action(cx, "project.copyRelativePath", None);
        wait_long(cx, "the path from the content root", |cx| clipboard(cx).as_deref() == Some("src/app/a.txt"));
        let other = text_of(&root.join("gen/c.txt"));
        run_action(cx, "project.copyPath", Some(json!({ "kind": "relative", "paths": [other] })));
        wait_long(cx, "the given path's relative form", |cx| clipboard(cx).as_deref() == Some("gen/c.txt"));
        run_action(cx, "project.copyPath", Some(json!({ "paths": [other] })));
        wait_long(cx, "the given path", |cx| clipboard(cx) == Some(other.clone()));

        let src = root.join("src");
        select_in_tree(&workbench, &src, cx);
        right_click_row(&src, cx);
        choose_in_menu(&workbench, &["Mark Directory as", "Sources Root"], cx);
        let marked = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| settings::effective("w1", cx).project_structure.source_roots)
        };
        wait_long(cx, "src to be a sources root", |cx| marked(cx) == ["src"]);
        select_in_tree(&workbench, &file, cx);
        run_action(cx, "project.copySourceRootPath", None);
        wait_long(cx, "the path from the source root", |cx| clipboard(cx).as_deref() == Some("app/a.txt"));
        close_project(cx);
    }

    /// Mark Directory as › Excluded writes the folder to the project's excluded folders: its row
    /// shows it excluded and the search no longer finds what is inside it.
    #[gpui_kit::test]
    async fn an_excluded_folder_shows_as_excluded_and_the_search_skips_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("src/a.txt", "a needle\n"), ("gen/out.txt", "a needle too\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        assert_eq!(searched("needle", cx), ["gen/out.txt", "src/a.txt"]);
        let generated = root.join("gen");
        select_in_tree(&workbench, &generated, cx);
        right_click_row(&generated, cx);
        choose_in_menu(&workbench, &["Mark Directory as", "Excluded"], cx);
        let excluded =
            |cx: &mut VisualTestContext| cx.update(|_, cx| settings::effective("w1", cx).project_structure.excluded);
        wait_long(cx, "gen to be excluded", |cx| excluded(cx) == ["gen"]);
        let at = text_of(&generated);
        wait_long(cx, "the row to show it", |cx| {
            tree.read_with(cx, |tree, _| tree.rows().iter().any(|row| row.node.path == at && row.node.excluded))
        });
        assert_eq!(searched("needle", cx), ["src/a.txt"]);
        assert!(!notification_titles(&workbench, cx).contains(&"Cannot mark the directory".to_owned()));
        close_project(cx);
    }

    /// Compare With… (⌘D in the tree) on two files shows them side by side in the diff window; with
    /// one, it asks for the other. It is the tree's: with the editor's keyboard neither the action
    /// nor its key runs.
    #[gpui_kit::test]
    async fn compare_with_shows_two_files_side_by_side_and_asks_for_the_second_of_one(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [("a.txt", "one\ntwo\nthree\n"), ("b.txt", "one\nTWO\nthree\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let (a, b) = (root.join("a.txt"), root.join("b.txt"));
        let changed = |lines: Vec<crate::vcs_diff::view::PaneLine>| -> Vec<String> {
            lines.into_iter().filter(|line| line.kind.is_some()).map(|line| line.text).collect()
        };
        let sides = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
            let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned())?;
            let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
            let left = changed(viewer.read_with(cx, |viewer, _| viewer.left_lines()));
            let right = changed(viewer.read_with(cx, |viewer, _| viewer.right_lines()));
            Some((left, right))
        };
        let expected = Some((vec!["two".to_owned()], vec!["TWO".to_owned()]));

        select_in_tree(&workbench, &a, cx);
        run_action(cx, "project.compareWith", Some(json!([text_of(&a), text_of(&b)])));
        wait_long(cx, "the two files side by side", |cx| sides(&workbench, cx) == expected);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()));

        select_in_tree(&workbench, &a, cx);
        assert_eq!(actions_run_by(&primary("d"), cx), ["project.compareWith"]);
        assert!(cx.did_prompt_for_paths(), "the file to compare with is asked for");
        cx.simulate_path_prompt_response(move |_| Some(vec![b.clone()]));
        wait_long(cx, "the chosen file side by side", |cx| sides(&workbench, cx) == expected);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        edit(&workbench, &a, cx);
        let refused = cx.update(|window, cx| execute_action_or_throw("project.compareWith", None, window, cx));
        assert!(refused.is_err(), "Compare With is the tree's");
        assert!(!actions_run_by(&primary("d"), cx).contains(&"project.compareWith".to_owned()));
        assert!(!cx.did_prompt_for_paths(), "nothing is asked for");
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()));
        close_project(cx);
    }

    /// Cut and Paste on another folder moves the file there on disk once the Move dialog is
    /// confirmed. Copy and Paste onto a folder that has a file of that name says so and copies
    /// nothing until another name is given.
    #[gpui_kit::test]
    async fn cut_and_paste_moves_the_file_and_a_copy_onto_a_taken_name_asks_for_another(cx: &mut TestAppContext) {
        use crate::project_view::file_dialogs::FileDialog;
        let _home = start(cx);
        let files = [("src/a.txt", "mine\n"), ("src/c.txt", "c\n"), ("lib/a.txt", "theirs\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let lib = root.join("lib");
        let read = |path: PathBuf| std::fs::read_to_string(path).expect("the file is read");
        // Revealing a file of `lib` reads its listing, which the Copy dialog checks names against.
        select_in_tree(&workbench, &lib.join("a.txt"), cx);
        select_in_tree(&workbench, &root.join("src/a.txt"), cx);
        run_action(cx, "project.copy", None);
        select_in_tree(&workbench, &lib, cx);
        run_action(cx, "project.paste", None);
        let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).expect("the Copy dialog");
        let name = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("a name field");
        name.update(cx, |field, cx| field.set_text("a.txt", cx));
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.error(cx)).as_deref(), Some("'a.txt' already exists"));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).is_some(), "the dialog stays");
        assert_eq!(read(lib.join("a.txt")), "theirs\n");
        name.update(cx, |field, cx| field.set_text("mine.txt", cx));
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the copy", |_| lib.join("mine.txt").is_file());
        assert_eq!(read(lib.join("mine.txt")), "mine\n");
        assert!(root.join("src/a.txt").is_file(), "a copy leaves the original");
        wait_long(cx, "the dialog to close", |cx| {
            workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>().is_none())
        });

        let moved = root.join("src/c.txt");
        select_in_tree(&workbench, &moved, cx);
        run_action(cx, "project.cut", None);
        select_in_tree(&workbench, &lib, cx);
        run_action(cx, "project.paste", None);
        assert!(workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).is_some(), "the Move dialog");
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the move", |_| lib.join("c.txt").is_file() && !moved.exists());
        assert_eq!(read(lib.join("c.txt")), "c\n");
        close_project(cx);
    }

    /// A confirmed Delete… moves the file out of the project (here into the test's own trash
    /// folder, which takes the place of the system Trash): it leaves the disk and the tree. Edit ›
    /// Undo while the tree has the keyboard brings it back from Local History and selects it.
    #[gpui_kit::test]
    async fn a_confirmed_delete_moves_the_file_to_the_trash_and_undo_brings_it_back(cx: &mut TestAppContext) {
        use crate::project_view::file_dialogs::DeleteDialog;
        let home = start(cx);
        let files = [("docs/notes.txt", "kept words\n"), ("docs/keep.txt", "")];
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
        let trash = home.path().join("trash");
        std::fs::create_dir_all(&trash).expect("the trash folder is made");
        let into = trash.clone();
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.set_trash(Some(Arc::new(move |path: &Path| {
            let name = path.file_name().ok_or_else(|| IdeError::invalid("a path without a name"))?;
            std::fs::rename(path, into.join(name)).map_err(IdeError::from)
        })));
        let tree = project_tree_of(&workbench, cx);
        let file = root.join("docs/notes.txt");
        let listed = |cx: &mut VisualTestContext| tree_labels(&tree, cx).contains(&"notes.txt".to_owned());
        select_in_tree(&workbench, &file, cx);
        run_action(cx, "project.delete", None);
        let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<DeleteDialog>()).expect("the Delete dialog");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.question().message.clone()), "Delete 'notes.txt'?");
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the file to leave the project", |cx| !file.exists() && !listed(cx));
        let trashed = std::fs::read_to_string(trash.join("notes.txt")).expect("the file is in the trash folder");
        assert_eq!(trashed, "kept words\n");
        assert!(root.join("docs/keep.txt").is_file());
        assert!(!notification_titles(&workbench, cx).contains(&"Some items could not be deleted".to_owned()));

        select_in_tree(&workbench, &root.join("docs"), cx);
        run_action(cx, "edit.undo", None);
        wait_long(cx, "the file to come back", |cx| file.is_file() && listed(cx));
        assert_eq!(std::fs::read_to_string(&file).expect("the file is back"), "kept words\n");
        let at = text_of(&file);
        wait_long(cx, "the restored file to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
        assert!(!notification_titles(&workbench, cx).contains(&"Some items could not be restored".to_owned()));
        close_project(cx);
    }

    /// Jump to Source (F4 in the tree) opens the selected file in the editor; in the editor F4 is
    /// not the tree's.
    #[gpui_kit::test]
    async fn jump_to_source_opens_the_selected_file(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/a.rs", "fn a() {}\n"), ("b.rs", "")]);
        let file = root.join("src/a.rs");
        let edited = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).path().to_path_buf()))
        };
        select_in_tree(&workbench, &file, cx);
        assert_eq!(actions_run_by("f4", cx), ["project.open"]);
        wait_long(cx, "a.rs to open", |cx| edited(&workbench, cx).as_deref() == Some(file.as_path()));
        let other = root.join("b.rs");
        select_in_tree(&workbench, &other, cx);
        run_action(cx, "project.open", None);
        wait_long(cx, "b.rs to open", |cx| edited(&workbench, cx).as_deref() == Some(other.as_path()));

        select_in_tree(&workbench, &file, cx);
        edit(&workbench, &other, cx);
        assert!(!actions_run_by("f4", cx).contains(&"project.open".to_owned()));
        close_project(cx);
    }

    /// Find in Folder… opens Find in Files on the selected folder.
    #[gpui_kit::test]
    async fn find_in_folder_opens_find_in_files_on_the_selected_folder(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/a.txt", "a\n"), ("b.txt", "")]);
        let src = root.join("src");
        select_in_tree(&workbench, &src, cx);
        run_action(cx, "project.findInFolder", None);
        wait_long(cx, "Find in Files to open", |cx| workbench.read_with(cx, |w, _| w.find_popup().is_some()));
        let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
        let directory = popup.read_with(cx, |popup, cx| popup.directory_field().read(cx).text().to_owned());
        assert_eq!(directory, text_of(&src));
        close_project(cx);
    }

    /// Show Hidden Files and Sort by Type change what the tree shows and say they are on; Sort by
    /// Name puts the order back.
    #[gpui_kit::test]
    async fn the_view_options_change_what_the_tree_shows_and_say_whether_they_are_on(cx: &mut TestAppContext) {
        let _home = start(cx);
        let files = [(".env", "A=1\n"), ("a.txt", ""), ("b.rs", "")];
        let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &files);
        let tree = project_tree_of(&workbench, cx);
        let toggled = |id: &str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
                action_toggled(&action, window, cx)
            })
        };
        wait_long(cx, "the project's files", |cx| tree_labels(&tree, cx) == ["a.txt", "b.rs"]);
        assert_eq!(toggled("project.option.showHidden", cx), Some(false));
        run_action(cx, "project.option.showHidden", None);
        assert_eq!(toggled("project.option.showHidden", cx), Some(true));
        wait_long(cx, "the hidden file", |cx| tree_labels(&tree, cx).contains(&".env".to_owned()));

        run_action(cx, "project.option.showHidden", None);
        wait_long(cx, "the hidden file to go", |cx| tree_labels(&tree, cx) == ["a.txt", "b.rs"]);
        run_action(cx, "project.sortBy.type", None);
        assert_eq!(toggled("project.sortBy.type", cx), Some(true));
        assert_eq!(toggled("project.sortBy.name", cx), Some(false));
        wait_long(cx, "the files by type", |cx| tree_labels(&tree, cx) == ["b.rs", "a.txt"]);
        run_action(cx, "project.sortBy.name", None);
        wait_long(cx, "the files by name", |cx| tree_labels(&tree, cx) == ["a.txt", "b.rs"]);
        let registered: Vec<&str> = TREE_TOGGLES
            .into_iter()
            .filter(|key| cx.update(|_, cx| cx.global::<ActionRegistry>().contains(&format!("project.option.{key}"))))
            .collect();
        assert_eq!(registered, TREE_TOGGLES, "every option the tree acts on is an action");
        close_project(cx);
    }
}
