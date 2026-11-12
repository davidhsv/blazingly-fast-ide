//! The project view's actions (Find Action, keymap, other features): the descriptions of the
//! ones that are computed, and what an action acts on when it is invoked with or without an
//! argument.

use crate::platform::actions::ActionMeta;
use crate::platform::i18n::{I18n, Param};
use crate::platform::misc::copy_path::PathKind;
use crate::platform::misc::file_manager::reveal_in_file_manager_label;
use crate::platform::misc::platform_info::Os;
use crate::project_tree_model::ProjectNode;
use crate::project_tree_paths::{name_of, parent_of};
use crate::project_view::context_menu::{Item, MenuContext, ProjectCommand, reveal_label};
use crate::project_view::operations::{Effect, lead_node, selected_nodes, selection_target_folder};
use crate::project_view::store::{ProjectViewState, VIEW_MODES, ViewMode, ViewOptions};
use crate::vcs_ops::menu::MenuItem;
use serde_json::Value;
use std::sync::Arc;

/// The view options that are toggles, by the name they are stored under, with their titles: the
/// first five are about what the tree looks like, the rest about how it behaves.
pub const OPTION_LABELS: [(&str, &str); 10] = [
    ("showHidden", "Show Hidden Files"),
    ("showExcluded", "Show Excluded Files"),
    ("showMembers", "Show Members"),
    ("compact", "Compact Middle Packages"),
    ("foldersFirst", "Sort Folders First"),
    ("autoscrollFromSource", "Always Select Opened File"),
    ("autoscrollToSource", "Autoscroll to Source"),
    ("singleClickOpen", "Open Files with Single Click"),
    ("singleClickExpand", "Expand Directories with Single Click"),
    ("previewTab", "Enable Preview Tab"),
];

/// The orders within a folder, as the options store them, with their titles.
pub const SORT_LABELS: [(&str, &str); 3] =
    [("name", "Sort by Name"), ("type", "Sort by Type"), ("modified", "Sort by Modification Time")];

fn option_mut<'a>(options: &'a mut ViewOptions, key: &str) -> Option<&'a mut bool> {
    Some(match key {
        "showHidden" => &mut options.show_hidden,
        "showExcluded" => &mut options.show_excluded,
        "showMembers" => &mut options.show_members,
        "compact" => &mut options.compact,
        "foldersFirst" => &mut options.folders_first,
        "autoscrollFromSource" => &mut options.autoscroll_from_source,
        "autoscrollToSource" => &mut options.autoscroll_to_source,
        "singleClickOpen" => &mut options.single_click_open,
        "singleClickExpand" => &mut options.single_click_expand,
        "previewTab" => &mut options.preview_tab,
        _ => return None,
    })
}

/// Whether the toggle option `key` is on; none for a name that is no toggle.
pub fn option_value(options: &ViewOptions, key: &str) -> Option<bool> {
    option_mut(&mut options.clone(), key).copied()
}

/// Flips the toggle option `key`; false for a name that is no toggle.
pub fn toggle_option(options: &mut ViewOptions, key: &str) -> bool {
    match option_mut(options, key) {
        Some(value) => {
            *value = !*value;
            true
        }
        None => false,
    }
}

/// The English title `start` + `value`, built so that the registry, which shows titles through
/// the catalog of source texts, finds its translation.
fn built_title(i18n: &mut I18n, start: &str, value: &str) -> String {
    i18n.tx_built(&[start, ""], &[Param::from(value)]);
    format!("{start}{value}")
}

fn keywords(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The actions whose descriptions are computed: one per toggle option, pane and order, and the
/// one that shows a file in the file manager of `os`, which is named after it.
pub fn project_actions(i18n: &mut I18n, os: Os) -> Vec<ActionMeta> {
    let mut actions: Vec<ActionMeta> = Vec::new();
    for (key, label) in OPTION_LABELS {
        let title = built_title(i18n, "Project View: ", label);
        let mut meta = ActionMeta::new(&format!("project.option.{key}"), &title);
        meta.keywords = keywords(&["project", "tree", "toggle"]);
        // "Autoscroll from Source" is the former name of Always Select Opened File.
        if key == "autoscrollFromSource" {
            meta.keywords.push("autoscroll from source".to_owned());
        }
        meta.toggle = true;
        actions.push(meta.category("View"));
    }
    for (mode, label) in VIEW_MODES {
        let id = format!("project.viewMode.{}", mode.id());
        let mut meta = ActionMeta::new(&id, &built_title(i18n, "Project View: Show ", label));
        meta.keywords = keywords(&["project", "pane", "scope", "view as"]);
        actions.push(meta.category("View").when("projectOpen"));
    }
    for (sort_by, label) in SORT_LABELS {
        let title = built_title(i18n, "Project View: ", label);
        let mut meta = ActionMeta::new(&format!("project.sortBy.{sort_by}"), &title);
        meta.keywords = keywords(&["project", "tree", "sort", "order"]);
        meta.toggle = true;
        actions.push(meta.category("View"));
    }
    let mut reveal = ActionMeta::new("project.revealInFinder", reveal_in_file_manager_label(os));
    reveal.icon = Some("external-link".to_owned());
    reveal.keywords = keywords(&["show in explorer", "open in finder"]);
    actions.push(reveal.category("Project View").when("projectOpen"));
    actions
}

/// The pane the action `id` shows, if it is one of the `project.viewMode.*` actions.
pub fn view_mode_of(id: &str) -> Option<ViewMode> {
    id.strip_prefix("project.viewMode.").and_then(ViewMode::from_id)
}

/// The nodes an action acts on: the paths passed as its argument (one, or a list), else the
/// selection of the tree. A path the tree does not show gets a node of its own.
pub fn nodes_from(state: &ProjectViewState, arg: Option<&Value>) -> Vec<Arc<ProjectNode>> {
    let paths: Vec<&str> = match arg {
        Some(Value::String(path)) => vec![path.as_str()],
        Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).collect(),
        _ => return selected_nodes(state),
    };
    let model = state.model();
    let node_of = |path: &str| {
        let shown = model.key_of_path.get(path).and_then(|key| model.by_key.get(key));
        match shown.filter(|node| node.path == path) {
            Some(node) => node.clone(),
            None => Arc::new(ProjectNode {
                key: path.to_owned(),
                path: path.to_owned(),
                chain: vec![path.to_owned()],
                label: name_of(path).to_owned(),
                dir: state.listings.contains_key(path),
                root: state.root.as_deref() == Some(path),
                ..ProjectNode::default()
            }),
        }
    };
    paths.into_iter().map(node_of).collect()
}

/// The target of an action invoked without one: the selection of the project view while it has
/// the focus (`None`), otherwise the file of the active editor, so that a shortcut pressed in the
/// editor acts on the edited file.
pub fn context_target(arg: Option<&Value>, project_view_focus: bool, active_path: Option<&str>) -> Option<Value> {
    match arg.filter(|arg| !arg.is_null()) {
        Some(arg) => Some(arg.clone()),
        None if project_view_focus => None,
        None => active_path.map(|path| Value::String(path.to_owned())),
    }
}

/// The folder an action creates or searches in: the folder passed as its argument (or the folder
/// of the file passed), else the one the selection targets.
pub fn folder_from(state: &ProjectViewState, arg: Option<&Value>) -> Option<String> {
    match arg.and_then(Value::as_str) {
        Some(path) if state.listings.contains_key(path) => Some(path.to_owned()),
        Some(path) => Some(parent_of(path).to_owned()),
        None => selection_target_folder(state),
    }
}

/// Selects `path` (without one, the file of the active editor) in the project view, switching to
/// the "Project" pane when the pane shown cannot show it. Whether the tree can show the path,
/// and what brings it up; nothing without a path. `stored_expanded` is what the storage holds
/// as the expansion of the open project.
pub fn select_in_project_view(
    state: &mut ProjectViewState,
    path: Option<&str>,
    active_path: Option<&str>,
    focus: bool,
    stored_expanded: Option<&str>,
) -> (bool, Vec<Effect>) {
    let Some(target) = path.or(active_path) else { return (false, Vec::new()) };
    let mut effects = Vec::new();
    if !state.mode_shows(target)
        && let Some(write) = state.set_view_mode(ViewMode::Project, stored_expanded)
    {
        effects.push(Effect::Store(write));
    }
    effects.push(Effect::ShowProjectView { focus });
    let folders = state.reveal_path(target, focus);
    let shown = folders.is_some();
    effects.extend(folders.into_iter().flatten().map(|path| Effect::LoadDir { path, force: false }));
    (shown, effects)
}

/// The Select In chooser: its title and the places that can show a file.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectInMenu {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// The places that can show `path`: the project view, the contributed targets, the navigation
/// bar (for the file of the active editor), the OS file manager and the terminal.
pub fn select_in_targets(ctx: &MenuContext<'_>, path: &str, active_path: Option<&str>) -> SelectInMenu {
    let select = ProjectCommand::SelectInProjectView(path.to_owned());
    let mut items = vec![MenuItem::item("Project View", select).icon("toolwindow-project")];
    items.extend(ctx.contributed.select_in.iter().cloned());
    if ctx.actions.contains("navigate.navigationBar") && active_path == Some(path) {
        items.push(MenuItem::action("navigate.navigationBar").labelled("Navigation Bar"));
    }
    let reveal = ProjectCommand::Reveal(vec![path.to_owned()]);
    items.push(MenuItem::item(reveal_label(ctx.os), reveal).icon("external-link"));
    if ctx.actions.contains("terminal.openAt") {
        let folder = Value::String(parent_of(path).to_owned());
        let open = ProjectCommand::Run { action: "terminal.openAt", arg: folder };
        items.push(MenuItem::item("Terminal", open).icon("terminal"));
    }
    SelectInMenu { title: "Select In", items }
}

/// What the Select In… action does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectIn {
    /// Select the path in the project view ([`select_in_project_view`]).
    Reveal(String),
    /// Show the chooser ([`select_in_targets`]) for the path.
    Choose(String),
    Nothing,
}

/// Select In…: a path argument (a string, or an object with a `path`) is selected in the project
/// view; without one the chooser opens for the file of the active editor, else for the selection.
pub fn select_in(state: &ProjectViewState, arg: Option<&Value>, active_path: Option<&str>) -> SelectIn {
    let given = arg.and_then(|arg| arg.as_str().or_else(|| arg.get("path").and_then(Value::as_str)));
    if let Some(path) = given.filter(|path| !path.is_empty()) {
        return SelectIn::Reveal(path.to_owned());
    }
    let target = active_path.map(str::to_owned).or_else(|| lead_node(state).map(|node| node.path.clone()));
    target.map_or(SelectIn::Nothing, SelectIn::Choose)
}

/// Open in Terminal: the folder of the target, in a terminal.
pub fn open_in_terminal(
    state: &ProjectViewState,
    arg: Option<&Value>,
    project_view_focus: bool,
    active_path: Option<&str>,
) -> Vec<Effect> {
    let target = context_target(arg, project_view_focus, active_path);
    let folder = folder_from(state, target.as_ref());
    folder.map(|dir| Effect::RunAction { id: "terminal.openAt", arg: Value::String(dir) }).into_iter().collect()
}

/// The node Rename File… renames: the one passed, else the file of the active editor unless the
/// project view has the focus, else the selection's.
pub fn rename_target(
    state: &ProjectViewState,
    arg: Option<&Value>,
    project_view_focus: bool,
    active_path: Option<&str>,
) -> Option<Arc<ProjectNode>> {
    if arg.is_some() {
        return nodes_from(state, arg).into_iter().next();
    }
    match active_path.filter(|_| !project_view_focus) {
        Some(active) => nodes_from(state, Some(&Value::String(active.to_owned()))).into_iter().next(),
        None => lead_node(state),
    }
}

/// The nodes Copy… copies: the ones passed or selected, or outside the project view the file of
/// the active editor.
pub fn copy_element_nodes(
    state: &ProjectViewState,
    arg: Option<&Value>,
    project_view_focus: bool,
    editor_path: Option<&str>,
) -> Vec<Arc<ProjectNode>> {
    if arg.is_some() || project_view_focus {
        return nodes_from(state, arg);
    }
    editor_path.map_or_else(Vec::new, |path| nodes_from(state, Some(&Value::String(path.to_owned()))))
}

/// What Navigate › File Path shows the folders of: the path, and whether it is a folder.
pub fn file_path_target(
    state: &ProjectViewState,
    arg: Option<&Value>,
    project_view_focus: bool,
    editor_path: Option<&str>,
) -> Option<(String, bool)> {
    let passed = arg.is_some_and(Value::is_string);
    let node = if passed || project_view_focus { nodes_from(state, arg).into_iter().next() } else { None };
    match node {
        Some(node) => Some((node.path.clone(), node.dir)),
        None => editor_path.map(|path| (path.to_owned(), false)),
    }
}

/// The argument of the hidden Copy Path action: which form to copy, and of which paths (the
/// selection's without any).
pub fn copy_path_request(arg: Option<&Value>) -> (PathKind, Option<Value>) {
    let kind = match arg.and_then(|arg| arg.get("kind")).and_then(Value::as_str) {
        Some("relative") => PathKind::Relative,
        Some("name") => PathKind::Name,
        Some("source") => PathKind::Source,
        _ => PathKind::Absolute,
    };
    (kind, arg.and_then(|arg| arg.get("paths")).cloned())
}

/// Compare With…: two files are compared with each other, one with a file to choose.
pub fn compare_with(nodes: &[&ProjectNode]) -> Vec<Effect> {
    let files: Vec<&str> = nodes.iter().filter(|node| !node.dir).map(|node| node.path.as_str()).collect();
    match files.as_slice() {
        [left, right] => vec![Effect::RunAction { id: "diff.compareFiles", arg: Value::from(vec![*left, *right]) }],
        [first, ..] => vec![Effect::RunAction { id: "diff.compareWith", arg: Value::from(*first) }],
        [] => Vec::new(),
    }
}

/// The node Expand All starts below: the selected folder, unless it is a root.
pub fn expand_all_from(state: &ProjectViewState) -> Option<String> {
    lead_node(state).filter(|node| node.dir && !node.root).map(|node| node.key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::manifest;
    use crate::project_view::context_menu::testing::{Surroundings, labels};
    use crate::project_view::store::ProjectInit;
    use serde_json::json;

    fn project_state() -> ProjectViewState {
        let mut state = ProjectViewState::default();
        state.init(Some(ProjectInit { root: "/p", name: "p", ..ProjectInit::default() }));
        state
    }

    fn reveal_action(i18n: &mut I18n, os: Os) -> ActionMeta {
        let actions = project_actions(i18n, os);
        actions.into_iter().find(|action| action.id == "project.revealInFinder").expect("the reveal action")
    }

    // src/features/project-tree/selectIn.test.ts › Open in Terminal ›
    // opens the edited file's folder unless the Project view has focus
    #[test]
    fn opens_the_edited_files_folder_unless_the_project_view_has_focus() {
        let state = project_state();
        let opened = |arg: Option<&Value>, focus: bool| open_in_terminal(&state, arg, focus, Some("/p/src/a.ts"));
        let terminal_at = |dir: &str| vec![Effect::RunAction { id: "terminal.openAt", arg: json!(dir) }];
        assert_eq!(opened(None, false), terminal_at("/p/src"));
        assert_eq!(opened(Some(&json!("/p/docs/readme.md")), false), terminal_at("/p/docs"));
        // With the focus in the project view the selection decides: without one, the project.
        assert_eq!(opened(None, true), terminal_at("/p"));
    }

    // src/features/project-tree/selectIn.test.ts › Select In (⌥F1) ›
    // uses the platform store for %s menus and action titles
    #[test]
    fn uses_the_platform_for_menus_and_action_titles() {
        let cases = [
            (Os::Macos, "Finder", "Reveal in Finder"),
            (Os::Windows, "Explorer", "Show in Explorer"),
            (Os::Linux, "Files", "Show in Files"),
        ];
        for (os, name, title) in cases {
            let mut around = Surroundings::new(project_state(), &[]);
            around.os = os;
            assert_eq!(reveal_label(os), name);
            let targets = select_in_targets(&around.ctx(), "/p/a.ts", None);
            assert!(labels(&targets.items, &around.actions).contains(&name.to_owned()));
            assert_eq!(reveal_action(&mut I18n::default(), os).title, title);
        }
    }

    // src/features/project-tree/selectIn.test.ts › Select In (⌥F1) ›
    // retranslates the file manager action after registering in Chinese
    #[test]
    fn retranslates_the_file_manager_action_after_registering_in_chinese() {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        let action = reveal_action(&mut i18n, Os::Windows);
        assert_eq!(action.localized(&i18n).title, "在资源管理器中显示");
        i18n.set_locale(Some("en"), "en");
        assert_eq!(action.localized(&i18n).title, "Show in Explorer");
    }

    // src/features/project-tree/selectIn.test.ts › Select In (⌥F1) ›
    // lists the project view, contributed targets, Finder and the terminal
    #[test]
    fn lists_the_project_view_contributed_targets_finder_and_the_terminal() {
        let mut around = Surroundings::new(project_state(), &["terminal.openAt"]);
        let structure = ProjectCommand::Run { action: "structure.select", arg: Value::Null };
        around.contributed.select_in = vec![MenuItem::item("Structure", structure), MenuItem::Separator];
        let targets = select_in_targets(&around.ctx(), "/p/src/a.ts", None);
        assert_eq!(targets.title, "Select In");
        let expected = ["Project View", "Structure", "-", reveal_label(around.os), "Terminal"];
        assert_eq!(labels(&targets.items, &around.actions), expected);
    }

    // src/features/project-tree/selectIn.test.ts › Select In (⌥F1) ›
    // without a path shows the chooser for the active file; with one selects it in the project view
    #[test]
    fn without_a_path_shows_the_chooser_for_the_active_file_and_with_one_selects_it() {
        let mut state = project_state();
        let title = manifest().get("navigate.selectInProjectView").map(|action| action.title.as_str());
        assert_eq!(title, Some("Select in Project View"));
        let active = Some("/p/src/a.ts");
        assert_eq!(select_in(&state, None, active), SelectIn::Choose("/p/src/a.ts".to_owned()));
        let around = Surroundings::new(project_state(), &[]);
        let chooser = select_in_targets(&around.ctx(), "/p/src/a.ts", active);
        assert_eq!(labels(&chooser.items, &around.actions)[0], "Project View");
        let arg = json!({ "path": "/p/src/a.ts" });
        assert_eq!(select_in(&state, Some(&arg), active), SelectIn::Reveal("/p/src/a.ts".to_owned()));
        let (shown, effects) = select_in_project_view(&mut state, Some("/p/src/a.ts"), active, true, None);
        assert!(shown && effects.contains(&Effect::ShowProjectView { focus: true }));
        assert_eq!(state.reveal_request.as_ref().map(|request| request.path.as_str()), Some("/p/src/a.ts"));
    }

    // Not from a spec: the computed actions are one per toggle option, pane and order, their
    // titles are found by the catalog, and a toggle is flipped by the name it is stored under.
    #[test]
    fn describes_the_computed_actions_and_flips_toggles_by_name() {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        let actions = project_actions(&mut i18n, Os::Linux);
        assert_eq!(actions.len(), OPTION_LABELS.len() + VIEW_MODES.len() + SORT_LABELS.len() + 1);
        let compact = actions.iter().find(|action| action.id == "project.option.compact").expect("the toggle");
        assert_eq!(compact.title, "Project View: Compact Middle Packages");
        assert!(compact.toggle && compact.localized(&i18n).title.starts_with("项目视图："));
        assert_eq!(view_mode_of("project.viewMode.openFiles"), Some(ViewMode::OpenFiles));
        let mut options = ViewOptions::default();
        for (key, _) in OPTION_LABELS {
            let before = option_value(&options, key).expect("a toggle");
            assert!(toggle_option(&mut options, key));
            assert_eq!(option_value(&options, key), Some(!before), "{key}");
        }
        assert!(!toggle_option(&mut options, "sortBy"));
    }

    // Not from a spec: an action acts on the paths passed to it, on nodes of their own when the
    // tree does not show them, and otherwise on the selection.
    #[test]
    fn acts_on_the_paths_passed_else_on_the_selection() {
        let mut state = project_state();
        let passed = nodes_from(&state, Some(&json!(["/p/a.txt", 7, "/p"])));
        let shown: Vec<(&str, bool)> = passed.iter().map(|node| (node.path.as_str(), node.root)).collect();
        assert_eq!(shown, [("/p/a.txt", false), ("/p", true)]);
        assert!(nodes_from(&state, None).is_empty());
        state.set_selection(["/p".to_owned()], Some("/p".to_owned()));
        assert_eq!(nodes_from(&state, None).len(), 1);
        assert_eq!(expand_all_from(&state), None);
        assert_eq!(context_target(Some(&Value::Null), false, Some("/p/a.txt")), Some(json!("/p/a.txt")));
        let renamed = rename_target(&state, None, false, Some("/p/a.txt")).map(|node| node.path.clone());
        assert_eq!(renamed.as_deref(), Some("/p/a.txt"));
        assert_eq!(file_path_target(&state, None, false, Some("/p/a.txt")), Some(("/p/a.txt".to_owned(), false)));
        assert_eq!(copy_element_nodes(&state, None, true, None).len(), 1);
        let (kind, paths) = copy_path_request(Some(&json!({ "kind": "name", "paths": ["/p/a.txt"] })));
        assert_eq!((kind, paths), (PathKind::Name, Some(json!(["/p/a.txt"]))));
        let files = [passed[0].as_ref()];
        assert_eq!(compare_with(&files), [Effect::RunAction { id: "diff.compareWith", arg: json!("/p/a.txt") }]);
    }
}
