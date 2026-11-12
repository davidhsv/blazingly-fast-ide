//! Context menu and "New" menu of the project view, as data: the rows a selection gets and what
//! choosing one does.

use crate::platform::i18n::{I18n, Param};
use crate::platform::keymap::{format_binding, host_binding};
use crate::platform::misc::copy_path::PathKind;
use crate::platform::misc::file_manager::file_manager_name;
use crate::platform::misc::platform_info::Os;
use crate::platform::project_roots::{FolderRole, OpenProject, RootFolder};
use crate::platform::virtual_docs::relative_path;
use crate::project_tree_model::{PlaceholderKind, ProjectNode};
use crate::project_tree_paths::{PackageKind, is_same_or_inside, package_kind_at};
use crate::project_view::actions::select_in_project_view;
use crate::project_view::mark_directory::directory_role;
use crate::project_view::operations::{
    Effect, Env, Flow, Step, begin_new, can_paste, copy_paths, copy_selection, delete_selection, find_file_usages,
    immediate, open_copy_dialog, open_move_dialog, open_rename_dialog, paste, paths_for, reload_from_disk,
    target_folder, vcs_arg,
};
use crate::project_view::store::ProjectViewState;
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// A row of the project view's menus.
pub type Item = MenuItem<ProjectCommand>;

/// Keys handled by the project view itself (the default keymap has no entries for them): action
/// id and binding, in macOS spelling.
pub const LOCAL_KEYS: [(&str, &str); 9] = [
    ("project.newMenu", "Meta+N"),
    ("project.rename", "Shift+F6"),
    ("project.move", "F6"),
    ("project.delete", "Delete"),
    ("project.copy", "Meta+C"),
    ("project.cut", "Meta+X"),
    ("project.paste", "Meta+V"),
    ("project.copyAbsolutePath", "Meta+Shift+C"),
    ("project.open", "F4"),
];

/// What New creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    File,
    Directory,
    /// A package: a dotted name creates nested folders.
    Package(PackageKind),
}

/// What choosing a row does. Rows that act on "the nodes" mean the nodes the menu was built for.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectCommand {
    /// Open the inline editor in `parent`; with `show_view` bring up the project view first.
    New {
        kind: NewKind,
        parent: Option<String>,
        show_view: bool,
    },
    /// Run another feature's action.
    Run {
        action: &'static str,
        arg: Value,
    },
    /// Cut / Copy the selection of the tree.
    Cut,
    Copy,
    /// Copy that form of the nodes' paths.
    CopyPaths(PathKind),
    Paste {
        target: Option<String>,
    },
    OpenFiles(Vec<String>),
    FindUsages(String),
    /// The Rename dialog for the one node.
    Rename,
    /// The Move dialog for the nodes that are not roots.
    Move,
    /// The Copy dialog for the nodes that are not roots.
    CopyTo,
    /// Delete the nodes, after asking.
    Delete,
    /// Show the paths in the OS file manager.
    Reveal(Vec<String>),
    /// Read the nodes' folders again.
    Reload,
    MarkDirectory {
        path: String,
        role: Option<FolderRole>,
    },
    /// Select the path in the project view.
    SelectInProjectView(String),
}

/// What contributed menu groups see of a selection: its paths, and which of them are folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuGroupContext {
    pub paths: Vec<String>,
    pub dirs: Vec<bool>,
}

/// The selected paths (placeholders left out) for the contributed menu groups.
pub fn menu_group_context(nodes: &[&ProjectNode]) -> MenuGroupContext {
    let real = nodes.iter().filter(|node| node.placeholder.is_none());
    MenuGroupContext {
        paths: real.clone().map(|node| node.path.clone()).collect(),
        dirs: real.map(|node| node.dir).collect(),
    }
}

/// The rows other features contribute, each non-empty group followed by a separator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contributed {
    /// The `new` group for the folder the menu creates in: file templates, HTTP Request, …
    pub new: Vec<Item>,
    /// The `projectView` group for [`menu_group_context`] of the nodes: Run / Debug '…'.
    pub project_view: Vec<Item>,
    /// The `openIn` group for the same context.
    pub open_in: Vec<Item>,
    /// The `selectIn` group for the path: targets besides the project view.
    pub select_in: Vec<Item>,
}

/// What the menus know of their surroundings.
#[derive(Debug, Clone, Copy)]
pub struct MenuContext<'a> {
    pub state: &'a ProjectViewState,
    pub project: Option<OpenProject<'a>>,
    /// The project is a Git repository.
    pub is_git: bool,
    /// The effective roots of the project, and the ones detected from build files.
    pub roots: &'a [RootFolder],
    pub detected: &'a [RootFolder],
    pub os: Os,
    /// The keymap spells bindings as macOS does.
    pub mac: bool,
    /// The ids of the registered actions.
    pub actions: &'a BTreeSet<String>,
    /// The shortcut the keymap shows for an action, by action id.
    pub shortcuts: &'a BTreeMap<String, String>,
    pub contributed: &'a Contributed,
}

/// The name of the OS file manager, as the Open In menu shows it.
pub fn reveal_label(os: Os) -> &'static str {
    file_manager_name(os)
}

/// The shortcut shown for the action `id`: the keymap's, else the view's own key.
pub fn key_label(ctx: &MenuContext<'_>, id: &str) -> Option<String> {
    let local = || LOCAL_KEYS.iter().find(|(action, _)| *action == id);
    let own = || local().map(|(_, key)| format_binding(&host_binding(key, ctx.mac), ctx.mac));
    ctx.shortcuts.get(id).cloned().or_else(own)
}

fn with_shortcut(item: Item, shortcut: Option<String>) -> Item {
    match shortcut {
        Some(shortcut) => item.shortcut(shortcut),
        None => item,
    }
}

/// A row for the view's own command `run`, with the shortcut of the action `id`.
fn row(ctx: &MenuContext<'_>, label: &str, run: ProjectCommand, id: &str) -> Item {
    with_shortcut(MenuItem::item(label, run), key_label(ctx, id))
}

/// A row bound to another feature's action: the menu shows it disabled when the action is off.
/// None while the action is not registered.
fn act(ctx: &MenuContext<'_>, action: &'static str, arg: Option<Value>, label: &str) -> Option<Item> {
    let item = ActionItem { action, arg, label: Some(label.to_owned()), disabled: false };
    ctx.actions.contains(action).then_some(MenuItem::Action(item))
}

/// A row that runs another feature's action directly (the project view decides whether it
/// applies), with the action's shortcut unless `shortcut` is given.
fn delegated(
    ctx: &MenuContext<'_>,
    action: &'static str,
    arg: Value,
    label: &str,
    icon: Option<&'static str>,
    shortcut: Option<String>,
) -> Option<Item> {
    if !ctx.actions.contains(action) {
        return None;
    }
    let item = MenuItem::item(label, ProjectCommand::Run { action, arg });
    let item = match icon {
        Some(icon) => item.icon(icon),
        None => item,
    };
    Some(with_shortcut(item, shortcut.or_else(|| ctx.shortcuts.get(action).cloned())))
}

/// A submenu of the rows that are there, or nothing when none is left: never an empty submenu.
fn submenu(label: &str, rows: Vec<Option<Item>>, icon: Option<&'static str>) -> Option<Item> {
    let rows: Vec<Item> = rows.into_iter().flatten().collect();
    if rows.iter().all(|item| matches!(item, MenuItem::Separator)) {
        return None;
    }
    let menu = MenuItem::submenu(label, rows);
    Some(match icon {
        Some(icon) => menu.icon(icon),
        None => menu,
    })
}

/// The rows as a menu shows them: rows of actions that are not registered left out, runs of
/// separators as one and none at either end, in submenus too.
pub fn shown_items(items: &[Item], actions: &BTreeSet<String>) -> Vec<Item> {
    let mut shown: Vec<Item> = Vec::new();
    for item in items {
        match item {
            MenuItem::Separator => {
                if shown.last().is_some_and(|last| !matches!(last, MenuItem::Separator)) {
                    shown.push(MenuItem::Separator);
                }
            }
            MenuItem::Action(action) => {
                if actions.contains(action.action) {
                    shown.push(item.clone());
                }
            }
            MenuItem::Item(entry) => {
                let mut entry = entry.clone();
                entry.submenu = shown_items(&entry.submenu, actions);
                shown.push(MenuItem::Item(entry));
            }
        }
    }
    if matches!(shown.last(), Some(MenuItem::Separator)) {
        shown.pop();
    }
    shown
}

/// New › rows for `parent`. With `outside_tree` (New invoked in the editor) File asks for the
/// name in a dialog, and the inline Directory / Package editors bring up the project view.
pub fn new_menu_items(ctx: &MenuContext<'_>, parent: Option<&str>, outside_tree: bool) -> Vec<Item> {
    let rel = match (parent, ctx.project) {
        (Some(parent), Some(project)) if parent == project.root => Some(String::new()),
        (Some(parent), Some(project)) => Some(relative_path(project.root, parent).into_owned()),
        _ => None,
    };
    let pkg = rel.and_then(|rel| package_kind_at(&rel, ctx.roots, ctx.detected));
    let parent_folder = parent.map(str::to_owned);
    let new = |kind: NewKind| ProjectCommand::New { kind, parent: parent_folder.clone(), show_view: outside_tree };
    let dialog = parent.filter(|_| outside_tree && ctx.actions.contains("templates.new.file"));
    let file = match dialog {
        Some(directory) => {
            let arg = json!({ "directory": directory });
            ProjectCommand::Run { action: "templates.new.file", arg }
        }
        None => new(NewKind::File),
    };
    let mut items = vec![MenuItem::item("File", file).icon("file")];
    // A Java / Kotlin source root offers "Package" instead of "Directory".
    items.push(if pkg == Some(PackageKind::Jvm) {
        MenuItem::item("Package", new(NewKind::Package(PackageKind::Jvm))).icon("package")
    } else {
        MenuItem::item("Directory", new(NewKind::Directory)).icon("folder")
    });
    if pkg == Some(PackageKind::Python) {
        items.push(MenuItem::item("Python Package", new(NewKind::Package(PackageKind::Python))).icon("package"));
    }
    if !ctx.contributed.new.is_empty() {
        items.push(MenuItem::Separator);
        items.extend(ctx.contributed.new.iter().cloned());
    }
    items
}

fn copy_path_menu(ctx: &MenuContext<'_>, kinds: &[(&str, PathKind)]) -> Item {
    let rows = kinds.iter().map(|(label, kind)| {
        let item = MenuItem::item(*label, ProjectCommand::CopyPaths(*kind));
        let shortcut = (*kind == PathKind::Absolute).then(|| key_label(ctx, "project.copyAbsolutePath")).flatten();
        with_shortcut(item, shortcut)
    });
    MenuItem::submenu("Copy Path/Reference…", rows.collect())
}

fn paths_of(nodes: &[&ProjectNode]) -> Vec<String> {
    nodes.iter().map(|node| node.path.clone()).collect()
}

/// The roles "Mark Directory as" offers, with their titles and icons.
const DIRECTORY_ROLES: [(FolderRole, &str, &str); 5] = [
    (FolderRole::Source, "Sources Root", "folder-source"),
    (FolderRole::Test, "Test Sources Root", "folder-test"),
    (FolderRole::Resources, "Resources Root", "folder-resources"),
    (FolderRole::TestResources, "Test Resources Root", "folder-test-resources"),
    (FolderRole::Excluded, "Excluded", "folder-excluded"),
];

fn mark_directory_menu(ctx: &MenuContext<'_>, i18n: &mut I18n, project_root: &str, path: &str) -> Item {
    let current = directory_role(ctx.roots, project_root, path);
    let mark = |role: Option<FolderRole>| ProjectCommand::MarkDirectory { path: path.to_owned(), role };
    let mut rows: Vec<Item> = DIRECTORY_ROLES
        .iter()
        .filter(|(role, _, _)| Some(*role) != current)
        .map(|(role, label, icon)| MenuItem::item(*label, mark(Some(*role))).icon(icon))
        .collect();
    if let Some((role, label, _)) = DIRECTORY_ROLES.iter().find(|(role, _, _)| Some(*role) == current) {
        let unmark = if *role == FolderRole::Excluded {
            "Cancel Exclusion".to_owned()
        } else {
            i18n.tx_built(&["Unmark as ", ""], &[Param::from(*label)])
        };
        rows.push(MenuItem::item(unmark, mark(None)));
    }
    MenuItem::submenu("Mark Directory as", rows)
}

/// The Git submenu for `nodes`.
fn git_menu(ctx: &MenuContext<'_>, nodes: &[&ProjectNode], single_file: Option<&ProjectNode>) -> Option<Item> {
    let arg = vcs_arg(nodes);
    let with = || Some(arg.clone());
    let many = nodes.len() > 1 || nodes.iter().any(|node| node.dir);
    let real = !nodes.is_empty() && nodes.iter().all(|node| node.placeholder.is_none());
    let no_root = nodes.iter().all(|node| !node.root);
    let ignore = (real && no_root).then(|| {
        let shared = act(ctx, "vcs.ignoreShared", with(), ".gitignore");
        let local = act(ctx, "vcs.ignoreLocal", with(), ".git/info/exclude");
        submenu("Add to .gitignore", vec![shared, local], None)
    });
    let rows = vec![
        act(ctx, "vcs.commit", None, if many { "Commit…" } else { "Commit File…" }),
        act(ctx, "vcs.addToVcs", with(), "Add"),
        ignore.flatten(),
        Some(MenuItem::Separator),
        single_file.and_then(|file| {
            act(ctx, "vcs.annotate", Some(Value::String(file.path.clone())), "Annotate with Git Blame")
        }),
        act(ctx, "vcs.showDiff", with(), "Show Diff"),
        real.then(|| act(ctx, "vcs.compareWithBranch", with(), "Compare with Branch…")).flatten(),
        real.then(|| act(ctx, "vcs.showHistory", with(), "Show History")).flatten(),
        act(ctx, "vcs.rollback", with(), "Rollback…"),
        Some(MenuItem::Separator),
        act(ctx, "vcs.push", None, "Push…"),
        act(ctx, "vcs.update", None, "Pull…"),
        act(ctx, "vcs.fetch", None, "Fetch"),
        act(ctx, "vcs.merge", None, "Merge…"),
        act(ctx, "vcs.rebase", None, "Rebase…"),
        Some(MenuItem::Separator),
        act(ctx, "vcs.branches", None, "Branches…"),
        act(ctx, "vcs.newBranch", None, "New Branch…"),
        act(ctx, "vcs.newTag", None, "New Tag…"),
        act(ctx, "vcs.resetHead", None, "Reset HEAD…"),
        Some(MenuItem::Separator),
        act(ctx, "vcs.stash", None, "Stash Changes…"),
        act(ctx, "vcs.unstash", None, "Unstash Changes…"),
    ];
    submenu("Git", rows, Some("branch"))
}

/// The context menu of `nodes`: New / clipboard / find / Refactor / edit / the contributed
/// groups / Open In / Local History / Git / Compare / Mark Directory as. Rows that cannot apply
/// to the selection are left out (or disabled, like the clipboard group), so the project root
/// and scratches get a shorter menu. Show it through [`shown_items`].
pub fn context_menu_items(ctx: &MenuContext<'_>, i18n: &mut I18n, nodes: &[&ProjectNode]) -> Vec<Item> {
    if nodes.iter().any(|node| node.read_only) {
        let sources: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| !node.group).collect();
        return library_menu_items(ctx, &sources);
    }
    let lead = nodes.first().copied();
    let single = lead.filter(|_| nodes.len() == 1);
    let non_root: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| !node.root).collect();
    let folder = target_folder(ctx.state, lead);
    let files: Vec<&ProjectNode> = nodes.iter().copied().filter(|node| !node.dir).collect();
    let single_file = single.filter(|node| !node.dir);
    // Scratches and other contributed roots are outside the project: no VCS, roots or usages.
    let inside = |project: &OpenProject<'_>| nodes.iter().all(|node| is_same_or_inside(&node.path, project.root));
    let project = ctx.project.filter(inside);
    let mut items: Vec<Item> = Vec::new();

    if let (Some(single), Some(open)) = (single, ctx.project)
        && let Some(members) = open.workspace
        && single.path != open.root
        && members.contains(&single.path)
    {
        let attached = members.first().is_some_and(|primary| primary == open.root);
        let action = if attached { "project.detach" } else { "workspace.removeProject" };
        items.push(MenuItem::action_with(action, Value::String(single.path.clone())));
        items.push(MenuItem::Separator);
    }

    items.push(MenuItem::submenu("New", new_menu_items(ctx, folder.as_deref(), false)));
    items.push(MenuItem::Separator);

    // Cut / Copy / Copy Path/Reference… / Paste
    let kinds = [
        ("Absolute Path", PathKind::Absolute),
        ("File Name", PathKind::Name),
        ("Path From Content Root", PathKind::Relative),
    ];
    items.push(row(ctx, "Cut", ProjectCommand::Cut, "project.cut").icon("cut").disabled(non_root.is_empty()));
    items.push(row(ctx, "Copy", ProjectCommand::Copy, "project.copy").icon("copy").disabled(non_root.is_empty()));
    items.push(copy_path_menu(ctx, &kinds));
    let paste_into = ProjectCommand::Paste { target: folder.clone() };
    items.push(row(ctx, "Paste", paste_into, "project.paste").icon("paste").disabled(!can_paste(ctx.state)));
    items.push(MenuItem::Separator);

    // Jump to Source / Find Usages / Find in Files… / Replace in Files…
    if !files.is_empty() {
        let open = ProjectCommand::OpenFiles(paths_of(&files));
        items.push(row(ctx, "Jump to Source", open, "project.open").icon("edit"));
    }
    if let Some(file) = single_file.filter(|_| project.is_some()) {
        items.push(row(ctx, "Find Usages", ProjectCommand::FindUsages(file.path.clone()), "project.findUsages"));
    }
    if let Some(folder) = &folder {
        let within = json!({ "directory": folder });
        items.extend(delegated(ctx, "find.inPath", within.clone(), "Find in Files…", Some("search"), None));
        items.extend(delegated(ctx, "find.replaceInPath", within, "Replace in Files…", None, None));
    }
    items.push(MenuItem::Separator);

    // Refactor ›
    let kind_label = |file: &'static str, dir: &'static str, many: &'static str| match non_root.as_slice() {
        [only] if only.dir => dir,
        [_] => file,
        _ => many,
    };
    let movable = !non_root.is_empty();
    let safe_delete = format_binding(if ctx.mac { "Meta+Backspace" } else { "Delete" }, ctx.mac);
    let refactor = vec![
        single.filter(|node| !node.root).map(|_| row(ctx, "Rename…", ProjectCommand::Rename, "project.rename")),
        movable.then(|| {
            row(ctx, kind_label("Move File…", "Move Directory…", "Move…"), ProjectCommand::Move, "project.move")
        }),
        movable.then(|| {
            let label = kind_label("Copy File…", "Copy Directory…", "Copy…");
            row(ctx, label, ProjectCommand::CopyTo, "project.copyElement")
        }),
        movable.then_some(MenuItem::Separator),
        movable.then(|| MenuItem::item("Safe Delete…", ProjectCommand::Delete).shortcut(safe_delete)),
    ];
    items.extend(submenu("Refactor", refactor, None));
    items.push(MenuItem::Separator);

    // Reformat Code / Optimize Imports / Delete…
    if let Some(file) = single_file {
        let arg = json!({ "path": file.path });
        if ctx.actions.contains("code.reformatFile") {
            let reformat = ProjectCommand::Run { action: "code.reformatFile", arg: arg.clone() };
            items.push(row(ctx, "Reformat Code", reformat, "project.reformatCode"));
        }
        if ctx.actions.contains("code.optimizeImportsFile") {
            let optimize = ProjectCommand::Run { action: "code.optimizeImportsFile", arg };
            items.push(row(ctx, "Optimize Imports", optimize, "project.optimizeImports"));
        }
    }
    items.push(row(ctx, "Delete…", ProjectCommand::Delete, "project.delete").disabled(non_root.is_empty()));
    items.push(MenuItem::Separator);

    // The contributed groups (Run / Debug '…', More Run/Debug ›), each followed by a separator.
    items.extend(ctx.contributed.project_view.iter().cloned());

    // Open In ›
    let reveal = MenuItem::item(reveal_label(ctx.os), ProjectCommand::Reveal(paths_of(nodes))).icon("external-link");
    let terminal = folder.as_ref().filter(|_| ctx.actions.contains("terminal.openAt")).map(|folder| {
        let open = ProjectCommand::Run { action: "terminal.openAt", arg: Value::String(folder.clone()) };
        MenuItem::item("Terminal", open).icon("terminal")
    });
    let mut open_in = vec![Some(reveal), terminal, Some(MenuItem::Separator)];
    open_in.extend(ctx.contributed.open_in.iter().cloned().map(Some));
    items.extend(submenu("Open In", open_in, None));
    items.push(MenuItem::Separator);

    // Local History › / Reload from Disk
    if let Some(single) = single {
        let history = vec![
            delegated(ctx, "vcs.localHistory", Value::String(single.path.clone()), "Show History", None, None),
            delegated(ctx, "localHistory.putLabel", Value::Null, "Put Label…", None, None),
        ];
        items.extend(submenu("Local History", history, None));
    }
    items.push(row(ctx, "Reload from Disk", ProjectCommand::Reload, "project.reloadFromDisk").icon("refresh"));
    items.push(MenuItem::Separator);

    // Git ›
    if project.is_some() && ctx.is_git {
        items.extend(git_menu(ctx, nodes, single_file));
        items.push(MenuItem::Separator);
    }

    // Compare selections of the same kind; mixed file / folder selections cannot apply.
    if nodes.len() == 2 && nodes.iter().all(|node| node.dir && node.placeholder.is_none()) {
        let arg = Value::from(paths_of(nodes));
        items.extend(delegated(ctx, "diff.compareDirectories", arg, "Compare Directories", Some("diff"), None));
    }
    if files.len() == 2 && nodes.len() == 2 {
        let arg = Value::from(paths_of(&files));
        items.extend(delegated(ctx, "diff.compareFiles", arg, "Compare Files", Some("diff"), None));
    } else if let Some(single) = single.filter(|node| node.placeholder.is_none()) {
        let arg = if single.dir {
            json!({ "path": single.path, "directory": true })
        } else {
            Value::String(single.path.clone())
        };
        let shortcut = key_label(ctx, "project.compareWith");
        items.extend(delegated(ctx, "diff.compareWith", arg, "Compare With…", Some("diff"), shortcut));
    }

    if let (Some(project), Some(single)) = (project, single)
        && single.dir
        && !single.root
        && single.placeholder.is_none()
    {
        items.push(mark_directory_menu(ctx, i18n, project.root, &single.path));
    }
    items
}

/// Library sources (External Libraries) are read-only: copy, find and reveal only.
fn library_menu_items(ctx: &MenuContext<'_>, nodes: &[&ProjectNode]) -> Vec<Item> {
    let Some(first) = nodes.first().copied() else { return Vec::new() };
    let single = (nodes.len() == 1).then_some(first);
    let folder = single.filter(|node| node.dir);
    let mut items = vec![row(ctx, "Copy", ProjectCommand::Copy, "project.copy").icon("copy")];
    items.push(copy_path_menu(ctx, &[("Absolute Path", PathKind::Absolute), ("File Name", PathKind::Name)]));
    items.push(MenuItem::Separator);
    if let Some(folder) = folder {
        let within = json!({ "directory": folder.path });
        items.extend(delegated(ctx, "find.inPath", within, "Find in Files…", Some("search"), None));
        items.push(MenuItem::Separator);
    }
    let reveal = MenuItem::item(reveal_label(ctx.os), ProjectCommand::Reveal(paths_of(nodes))).icon("external-link");
    items.extend(submenu("Open In", vec![Some(reveal)], None));
    if let Some(file) = single.filter(|node| !node.dir) {
        let shortcut = key_label(ctx, "project.compareWith");
        let arg = Value::String(file.path.clone());
        items.extend(delegated(ctx, "diff.compareWith", arg, "Compare With…", Some("diff"), shortcut));
    }
    items
}

/// Carries out the row `command` of a menu built for `nodes`. `project` and `roots` are the open
/// project and its effective roots, for the paths Copy Path copies.
pub fn run_command(
    command: ProjectCommand,
    nodes: &[&ProjectNode],
    state: &mut ProjectViewState,
    env: &mut Env<'_>,
    project: Option<OpenProject<'_>>,
    roots: &[RootFolder],
) -> (Flow, Step) {
    let non_root = || nodes.iter().copied().filter(|node| !node.root).collect::<Vec<&ProjectNode>>();
    match command {
        ProjectCommand::New { kind, parent, show_view } => {
            let (node_kind, pkg) = match kind {
                NewKind::File => (PlaceholderKind::File, None),
                NewKind::Directory => (PlaceholderKind::Dir, None),
                NewKind::Package(pkg) => (PlaceholderKind::Dir, Some(pkg)),
            };
            let (flow, mut step) = begin_new(state, env, node_kind, parent.as_deref(), pkg);
            if show_view {
                step.effects.insert(0, Effect::ShowProjectView { focus: false });
            }
            (flow, step)
        }
        ProjectCommand::Run { action, arg } => immediate(vec![Effect::RunAction { id: action, arg }]),
        ProjectCommand::Cut => immediate(copy_selection(state, env.remote, true)),
        ProjectCommand::Copy => immediate(copy_selection(state, env.remote, false)),
        ProjectCommand::CopyPaths(kind) => copy_paths(&paths_for(state, kind, nodes, project, roots)),
        ProjectCommand::Paste { target } => paste(state, target.as_deref()),
        ProjectCommand::OpenFiles(paths) => {
            immediate(paths.into_iter().map(|path| Effect::OpenFile { path, preview: false, focus: true }).collect())
        }
        ProjectCommand::FindUsages(path) => find_file_usages(&path),
        ProjectCommand::Rename => open_rename_dialog(nodes.first().copied()),
        ProjectCommand::Move => immediate(open_move_dialog(state, &non_root(), None)),
        ProjectCommand::CopyTo => immediate(open_copy_dialog(state, &non_root())),
        ProjectCommand::Delete => delete_selection(state, env, nodes),
        ProjectCommand::Reveal(paths) => immediate(vec![Effect::RevealInFileManager(paths)]),
        ProjectCommand::Reload => reload_from_disk(state, nodes),
        ProjectCommand::MarkDirectory { path, role } => immediate(vec![Effect::MarkDirectory { path, role }]),
        ProjectCommand::SelectInProjectView(path) => {
            immediate(select_in_project_view(state, Some(&path), None, true, env.stored_expanded).1)
        }
    }
}

#[cfg(test)]
pub mod testing {
    //! A menu context over a project `/p`, and the rows of a menu as it shows them.

    use super::*;
    use crate::vcs_ops::menu::MenuEntry;

    pub struct Surroundings {
        pub state: ProjectViewState,
        pub root: String,
        pub members: Option<Vec<String>>,
        /// Whether a project is open at all.
        pub open: bool,
        pub is_git: bool,
        pub roots: Vec<RootFolder>,
        pub os: Os,
        pub actions: BTreeSet<String>,
        pub shortcuts: BTreeMap<String, String>,
        pub contributed: Contributed,
    }

    impl Surroundings {
        /// The Git project `/p` with the actions `registered`.
        pub fn new(state: ProjectViewState, registered: &[&str]) -> Self {
            Self {
                state,
                root: "/p".to_owned(),
                members: None,
                open: true,
                is_git: true,
                roots: Vec::new(),
                os: Os::Macos,
                actions: registered.iter().map(|id| (*id).to_owned()).collect(),
                shortcuts: BTreeMap::new(),
                contributed: Contributed::default(),
            }
        }

        pub fn ctx(&self) -> MenuContext<'_> {
            let project = OpenProject { root: &self.root, name: "p", workspace: self.members.as_deref() };
            MenuContext {
                state: &self.state,
                project: self.open.then_some(project),
                is_git: self.is_git,
                roots: &self.roots,
                detected: &self.roots,
                os: self.os,
                mac: true,
                actions: &self.actions,
                shortcuts: &self.shortcuts,
                contributed: &self.contributed,
            }
        }
    }

    /// The label a row shows: an action row without one of its own shows the action's id here.
    fn label(item: &Item) -> String {
        match item {
            MenuItem::Separator => "-".to_owned(),
            MenuItem::Action(action) => action.label.clone().unwrap_or_else(|| action.action.to_owned()),
            MenuItem::Item(entry) => entry.label.clone(),
        }
    }

    /// The labels of `items` as the menu shows them ("-" for a separator).
    pub fn labels(items: &[Item], actions: &BTreeSet<String>) -> Vec<String> {
        shown_items(items, actions).iter().map(label).collect()
    }

    /// The labels of the submenu of the row labelled `of`.
    pub fn sub(items: &[Item], actions: &BTreeSet<String>, of: &str) -> Option<Vec<String>> {
        let entry = items.iter().filter_map(|item| match item {
            MenuItem::Item(entry) if entry.label == of && !entry.submenu.is_empty() => Some(entry),
            _ => None,
        });
        entry.map(|entry| labels(&entry.submenu, actions)).next()
    }

    /// The row labelled `of`.
    pub fn entry_of<'a>(items: &'a [Item], of: &str) -> Option<&'a MenuEntry<ProjectCommand>> {
        items.iter().find_map(|item| match item {
            MenuItem::Item(entry) if entry.label == of => Some(entry),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Surroundings, entry_of, labels, sub};
    use super::*;
    use crate::project_view::fs::testing::load;
    use crate::project_view::operations::testing::{Driver, node, tree_node};
    use crate::project_view::operations::{begin_rename, node_refs};
    use crate::project_view::store::ProjectInit;
    use crate::project_view::store::testing::{py_key, with_contributed_roots};
    use std::sync::Arc;

    const OTHER_ACTIONS: [&str; 17] = [
        "find.inPath",
        "find.replaceInPath",
        "code.reformatFile",
        "code.optimizeImportsFile",
        "terminal.openAt",
        "vcs.localHistory",
        "localHistory.putLabel",
        "diff.compareWith",
        "diff.compareFiles",
        "diff.compareDirectories",
        "vcs.commit",
        "vcs.addToVcs",
        "vcs.annotate",
        "vcs.showDiff",
        "vcs.compareWithBranch",
        "vcs.rollback",
        "vcs.push",
    ];

    fn project_state() -> ProjectViewState {
        let mut state = ProjectViewState::default();
        state.init(Some(ProjectInit { root: "/p", name: "p", ..ProjectInit::default() }));
        state
    }

    fn surroundings(extra: &[&str]) -> Surroundings {
        let registered: Vec<&str> = OTHER_ACTIONS.iter().chain(extra).copied().collect();
        Surroundings::new(project_state(), &registered)
    }

    fn file(path: &str) -> ProjectNode {
        node(path, false)
    }

    fn dir(path: &str) -> ProjectNode {
        node(path, true)
    }

    /// The menu of `nodes`. Where the other features' actions are registered, so is their Run
    /// group: it contributes a row for a single file.
    fn items(around: &mut Surroundings, nodes: &[ProjectNode]) -> Vec<Item> {
        let nodes: Vec<&ProjectNode> = nodes.iter().collect();
        let selected = menu_group_context(&nodes);
        around.contributed.project_view = match (selected.paths.as_slice(), selected.dirs.as_slice()) {
            ([_], [false]) if around.actions.contains("find.inPath") => {
                let run = ProjectCommand::Run { action: "run.file", arg: Value::Null };
                vec![MenuItem::item("Run 'a'", run).icon("run"), MenuItem::Separator]
            }
            _ => Vec::new(),
        };
        context_menu_items(&around.ctx(), &mut I18n::default(), &nodes)
    }

    fn shown_labels(around: &mut Surroundings, nodes: &[ProjectNode]) -> Vec<String> {
        let items = items(around, nodes);
        labels(&items, &around.actions)
    }

    fn sub_of(around: &mut Surroundings, nodes: &[ProjectNode], of: &str) -> Vec<String> {
        let items = items(around, nodes);
        sub(&items, &around.actions, of).unwrap_or_default()
    }

    fn has(labels: &[String], label: &str) -> bool {
        labels.iter().any(|shown| shown == label)
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // offers directory comparison only for two folders and Compare With for a single folder
    #[test]
    fn offers_directory_comparison_only_for_two_folders_and_compare_with_for_a_single_folder() {
        let mut around = surroundings(&[]);
        assert!(has(&shown_labels(&mut around, &[dir("/p/left"), dir("/p/right")]), "Compare Directories"));
        assert!(has(&shown_labels(&mut around, &[dir("/p/left")]), "Compare With…"));
        let mixed = shown_labels(&mut around, &[dir("/p/left"), file("/p/a.txt")]);
        for label in ["Compare Directories", "Compare Files", "Compare With…"] {
            assert!(!has(&mixed, label), "{label}");
        }
        assert!(has(&shown_labels(&mut around, &[file("/p/a.txt"), file("/p/b.txt")]), "Compare Files"));
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu › follows IDEA's order for a file
    #[test]
    fn follows_ideas_order_for_a_file() {
        let mut around = surroundings(&[]);
        let nodes = [file("/p/src/a.py")];
        let expected = [
            "New",
            "-",
            "Cut",
            "Copy",
            "Copy Path/Reference…",
            "Paste",
            "-",
            "Jump to Source",
            "Find Usages",
            "Find in Files…",
            "Replace in Files…",
            "-",
            "Refactor",
            "-",
            "Reformat Code",
            "Optimize Imports",
            "Delete…",
            "-",
            "Run 'a'",
            "-",
            "Open In",
            "-",
            "Local History",
            "Reload from Disk",
            "-",
            "Git",
            "-",
            "Compare With…",
        ];
        assert_eq!(shown_labels(&mut around, &nodes), expected);
        let refactor = ["Rename…", "Move File…", "Copy File…", "-", "Safe Delete…"];
        assert_eq!(sub_of(&mut around, &nodes, "Refactor"), refactor);
        assert_eq!(sub_of(&mut around, &nodes, "Open In"), [reveal_label(Os::Macos), "Terminal"]);
        let git = sub_of(&mut around, &nodes, "Git");
        assert_eq!(git[..4], ["Commit File…", "Add", "-", "Annotate with Git Blame"]);
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // offers both ignore destinations for files, directories and multi-selection, separately from IDE exclusion
    #[test]
    fn offers_both_ignore_destinations_separately_from_ide_exclusion() {
        let mut around = surroundings(&["vcs.ignoreShared", "vcs.ignoreLocal"]);
        for nodes in [vec![file("/p/a")], vec![dir("/p/dir")], vec![file("/p/a"), dir("/p/dir")]] {
            let all = items(&mut around, &nodes);
            let git = entry_of(&all, "Git").expect("the Git submenu");
            let ignore = sub(&git.submenu, &around.actions, "Add to .gitignore");
            assert_eq!(ignore, Some(vec![".gitignore".to_owned(), ".git/info/exclude".to_owned()]));
        }
        let mut root = dir("/p");
        root.root = true;
        assert!(!has(&sub_of(&mut around, &[root], "Git"), "Add to .gitignore"));
        assert!(has(&sub_of(&mut around, &[dir("/p/dir")], "Mark Directory as"), "Excluded"));
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // offers branch comparison for files, folders, their union and the project root
    #[test]
    fn offers_branch_comparison_for_files_folders_their_union_and_the_project_root() {
        let mut around = surroundings(&[]);
        let mut root = dir("/p");
        root.root = true;
        let selections = [vec![file("/p/a.py")], vec![dir("/p/src")], vec![dir("/p/src"), dir("/p/lib")], vec![root]];
        for nodes in selections {
            assert!(has(&sub_of(&mut around, &nodes, "Git"), "Compare with Branch…"));
        }
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // offers Show History for files, directories, multi-selection and the project root
    #[test]
    fn offers_show_history_for_files_directories_multi_selection_and_the_project_root() {
        let mut around = surroundings(&["vcs.showHistory"]);
        let mut root = dir("/p");
        root.root = true;
        let selections = [vec![file("/p/a.py")], vec![dir("/p/src")], vec![file("/p/a.py"), dir("/p/src")], vec![root]];
        for nodes in selections {
            assert!(has(&sub_of(&mut around, &nodes, "Git"), "Show History"));
        }
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // shows Mark Directory as for folders, without file-only items
    #[test]
    fn shows_mark_directory_as_for_folders_without_file_only_items() {
        let mut around = surroundings(&[]);
        let nodes = [dir("/p/src")];
        let shown = shown_labels(&mut around, &nodes);
        assert!(has(&shown, "Mark Directory as"));
        assert!(has(&sub_of(&mut around, &nodes, "Refactor"), "Move Directory…"));
        for file_only in ["Find Usages", "Reformat Code", "Optimize Imports", "Run 'a'"] {
            assert!(!has(&shown, file_only), "{file_only}");
        }
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // keeps the project root menu short and disables what cannot apply
    #[test]
    fn keeps_the_project_root_menu_short_and_disables_what_cannot_apply() {
        let mut around = surroundings(&[]);
        let mut root = dir("/p");
        root.root = true;
        let all = items(&mut around, &[root]);
        let shown = labels(&all, &around.actions);
        assert!(!has(&shown, "Refactor") && !has(&shown, "Mark Directory as"));
        for label in ["Cut", "Copy", "Delete…"] {
            assert!(entry_of(&all, label).is_some_and(|entry| entry.disabled), "{label}");
        }
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // drops project-only groups for scratch files
    #[test]
    fn drops_project_only_groups_for_scratch_files() {
        let mut around = surroundings(&[]);
        let shown = shown_labels(&mut around, &[file("/config/scratches/s.py")]);
        assert!(!has(&shown, "Git") && !has(&shown, "Find Usages"));
        assert!(has(&shown, "Refactor") && has(&shown, "Reload from Disk"));
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu › offers Compare Files for two files
    #[test]
    fn offers_compare_files_for_two_files() {
        let mut around = surroundings(&[]);
        let shown = shown_labels(&mut around, &[file("/p/a.py"), file("/p/b.py")]);
        assert!(has(&shown, "Compare Files") && !has(&shown, "Compare With…"));
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu ›
    // offers Compare With… in a non-git project too
    #[test]
    fn offers_compare_with_in_a_non_git_project_too() {
        let mut around = surroundings(&[]);
        around.is_git = false;
        let shown = shown_labels(&mut around, &[file("/p/a.py")]);
        assert!(!has(&shown, "Git") && has(&shown, "Compare With…"));
        let two = shown_labels(&mut around, &[file("/p/a.py"), file("/p/b.py")]);
        assert!(has(&two, "Compare Files"));
    }

    // src/features/project-tree/contextMenu.test.ts › project view context menu › never shows an empty submenu
    #[test]
    fn never_shows_an_empty_submenu() {
        let mut around = Surroundings::new(project_state(), &[]);
        let shown = shown_labels(&mut around, &[file("/p/a.py")]);
        for label in ["Git", "Local History", "Find in Files…"] {
            assert!(!has(&shown, label), "{label}");
        }
    }

    /// The raw rows of the root node `path` in a workspace at `root` with the members `/a` and `/b`.
    fn workspace_rows(root: &str, path: &str, is_root: bool) -> Vec<Item> {
        let mut around = surroundings(&[]);
        around.root = root.to_owned();
        around.members = Some(vec!["/a".to_owned(), "/b".to_owned()]);
        around.is_git = false;
        let mut member = dir(path);
        member.root = is_root;
        items(&mut around, &[member])
    }

    fn bound_to(items: &[Item], action: &str) -> bool {
        items.iter().any(|item| item.action_id() == Some(action))
    }

    // src/features/project-tree/contextMenu.test.ts › offers workspace removal for every member root
    #[test]
    fn offers_workspace_removal_for_every_member_root() {
        let removal = MenuItem::action_with("workspace.removeProject", Value::String("/b".to_owned()));
        assert!(workspace_rows("/workspace", "/b", true).contains(&removal));
        assert!(bound_to(&workspace_rows("/workspace", "/a", true), "workspace.removeProject"));
        assert!(!bound_to(&workspace_rows("/workspace", "/b/src", false), "workspace.removeProject"));
    }

    // src/features/project-tree/contextMenu.test.ts › does not offer removal on the attached primary
    #[test]
    fn does_not_offer_removal_on_the_attached_primary() {
        assert!(!bound_to(&workspace_rows("/a", "/a", true), "project.detach"));
        assert!(bound_to(&workspace_rows("/a", "/b", true), "project.detach"));
    }

    // src/features/project-tree/roots.test.ts › creating packages ›
    // New menu shows Package instead of Directory in a Java source root
    #[test]
    fn new_menu_shows_package_instead_of_directory_in_a_java_source_root() {
        let mut around = surroundings(&[]);
        around.is_git = false;
        around.roots = [("src/main/java", FolderRole::Source), ("src/test/java", FolderRole::Test)]
            .map(|(path, kind)| RootFolder { path: path.to_owned(), kind, detected_from: Some("pom.xml".to_owned()) })
            .to_vec();
        let first_two = |parent: &str| {
            let rows = new_menu_items(&around.ctx(), Some(parent), false);
            labels(&rows, &around.actions)[..2].to_vec()
        };
        assert_eq!(first_two("/p/src/main/java"), ["File", "Package"]);
        assert_eq!(first_two("/p"), ["File", "Directory"]);
    }

    fn library_surroundings() -> (Surroundings, Driver) {
        let (state, fs) = with_contributed_roots();
        let mut around = Surroundings::new(state, &OTHER_ACTIONS);
        around.is_git = false;
        (around, Driver::new(fs))
    }

    fn rows_of(around: &Surroundings, node: &Arc<ProjectNode>) -> Vec<Item> {
        context_menu_items(&around.ctx(), &mut I18n::default(), &[node.as_ref()])
    }

    fn runs(items: &[Item], command: &ProjectCommand) -> bool {
        items.iter().any(|item| item.command() == Some(command))
    }

    fn delete(around: &mut Surroundings, driver: &mut Driver, nodes: &[Arc<ProjectNode>]) {
        driver.run(&mut around.state, |state, env| delete_selection(state, env, &node_refs(nodes)));
    }

    // src/features/project-tree/viewModes.test.ts › project view modes › keeps library sources read-only
    #[test]
    fn keeps_library_sources_read_only() {
        let (mut around, mut driver) = library_surroundings();
        load(&mut around.state, &mut driver.fs, "/py/lib/python3.12", false);
        let file = tree_node(&around.state, &py_key("/py/lib/python3.12/os.py"));
        assert!(around.state.is_read_only_path(&file.path));
        assert!(!around.state.is_read_only_path("/p/src"));
        begin_rename(&mut around.state, Some(file.as_ref()));
        assert!(around.state.editing.is_none());
        delete(&mut around, &mut driver, std::slice::from_ref(&file));
        assert!(driver.fs.has("/py/lib/python3.12/os.py") && driver.questions.is_empty());
        let rows = rows_of(&around, &file);
        assert!(!runs(&rows, &ProjectCommand::Delete) && entry_of(&rows, "New").is_none());
        assert!(entry_of(&rows, "Copy Path/Reference…").is_some());
    }

    // src/features/project-tree/viewModes.test.ts › project view modes ›
    // shows library folders inside the project in both places, read-only
    #[test]
    fn shows_library_folders_inside_the_project_in_both_places_read_only() {
        let (mut around, mut driver) = library_surroundings();
        let site = "/p/.venv/lib/site-packages";
        for dir in ["/p", "/p/.venv", "/p/.venv/lib", site, "/p/.venv/lib/site-packages/req"] {
            load(&mut around.state, &mut driver.fs, dir, false);
        }
        let m = around.state.model();
        // Under the project: path keys, read-only from the library root down.
        let venv: Vec<(&str, &str, bool)> = m
            .children
            .get("/p/.venv")
            .expect("the folder is listed")
            .iter()
            .map(|node| (node.label.as_str(), node.key.as_str(), node.read_only))
            .collect();
        let expected = [("lib/site-packages/req", "/p/.venv/lib", true), ("pyvenv.cfg", "/p/.venv/pyvenv.cfg", false)];
        assert_eq!(venv, expected);
        assert!(m.by_key.get("/p/.venv").is_some_and(|node| !node.read_only));
        let api = format!("{site}/req/api.py");
        let project_file = m.by_key.get(&api).cloned().expect("the file below the project");
        assert!(project_file.read_only);
        // Under External Libraries: the same folders in the library namespace.
        let lib_file = m.by_key.get(&py_key(&api)).cloned().expect("the file below the library");
        assert_eq!((lib_file.path.as_str(), lib_file.read_only), (api.as_str(), true));
        let below: Vec<&str> =
            m.children.get(&py_key(site)).expect("the library lists").iter().map(|node| node.key.as_str()).collect();
        assert_eq!(below, [py_key(&format!("{site}/req")).as_str()]);
        // Paths resolve to the project node (Select In, autoscroll from source).
        assert_eq!(m.key_of_path.get(&api), Some(&api));
        assert!(around.state.is_read_only_path(site));

        begin_rename(&mut around.state, Some(project_file.as_ref()));
        assert!(around.state.editing.is_none());
        delete(&mut around, &mut driver, &[project_file.clone(), lib_file]);
        assert!(driver.fs.has(&api));
        let rows = rows_of(&around, &project_file);
        assert!(!runs(&rows, &ProjectCommand::Delete) && !runs(&rows, &ProjectCommand::Rename));
        let config = tree_node(&around.state, "/p/.venv/pyvenv.cfg");
        assert!(runs(&rows_of(&around, &config), &ProjectCommand::Delete));
    }

    // Not from a spec: choosing a row carries it out: New opens the inline editor (bringing up
    // the view when asked to), Copy Path copies, a delegated row runs its action.
    #[test]
    fn choosing_a_row_carries_it_out() {
        let (mut around, mut driver) = library_surroundings();
        load(&mut around.state, &mut driver.fs, "/p", false);
        let readme = tree_node(&around.state, "/p/README.md");
        let nodes = [readme.as_ref()];
        let new = ProjectCommand::New { kind: NewKind::Directory, parent: Some("/p".to_owned()), show_view: true };
        driver.run(&mut around.state, |state, env| run_command(new, &nodes, state, env, None, &[]));
        assert_eq!(driver.shown, 1);
        assert!(around.state.editing.is_some());
        let copy = ProjectCommand::CopyPaths(PathKind::Name);
        driver.run(&mut around.state, |state, env| run_command(copy, &nodes, state, env, None, &[]));
        assert_eq!(driver.texts, ["README.md"]);
        let run = ProjectCommand::Run { action: "terminal.openAt", arg: json!("/p") };
        driver.run(&mut around.state, |state, env| run_command(run, &nodes, state, env, None, &[]));
        assert_eq!(driver.actions.last(), Some(&("terminal.openAt".to_owned(), json!("/p"))));
        let select = ProjectCommand::SelectInProjectView("/p/src/App.kt".to_owned());
        driver.run(&mut around.state, |state, env| run_command(select, &nodes, state, env, None, &[]));
        assert_eq!(around.state.reveal_request.as_ref().map(|request| request.focus), Some(true));
        assert_eq!(key_label(&around.ctx(), "project.rename").as_deref(), Some("⇧F6"));
    }
}
