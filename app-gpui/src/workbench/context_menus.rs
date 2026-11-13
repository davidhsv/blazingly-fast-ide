//! The context menus built from the features' menu groups, and the menus around them that a window
//! opens on a right click or an Options button.
//!
//! - **Menu groups.** [`MENU_GROUPS`] lists every group the features add to the context menus: its
//!   id, the menus it adds to, its order and, where this window has what the group reads, the
//!   function that makes its rows. A menu takes the groups of its kind in order (then by id), each
//!   non-empty one followed by a separator ([`MenuGroups`]). Rows are registered actions, so a row
//!   whose action this window does not register is left out and one whose `when` fails is greyed
//!   out, with the action's title and the shortcut the active keymap gives it.
//! - **The editor's menu.** A right click in the text moves the caret there unless it lands in a
//!   selection; the menu then offers the clipboard, Go To, Folding, Refactor, the groups, Open In,
//!   Local History, Git and Compare. A right click left of the text opens the gutter's menu.
//! - **Additions to existing menus.** The editor tab's file rows (the groups, Open In, Local
//!   History, Git), the Problems and Find results' groups, the Project view's groups.
//! - **Tool windows.** The Options menu of a tool window's header and the menu of its stripe button.
//! - **Toolbar slots.** The buttons plugins put in the editor's tab strip (`editor.title`), the main
//!   toolbar (`mainToolbar`) and above the commit message (`commit.message`).

use super::Workbench;
use super::notifications::{NotificationLevel, NotificationRequest};
use super::plugin_contributions::{PluginMenu, plugin_menu_items, plugin_slot_items};
use super::tool_windows::{Effects, Slot, ToolWindowLayout};
use crate::code_insight::problems::{ProblemCommand, ProblemTarget};
use crate::editor::buffer;
use crate::editor::view::Editor;
use crate::everywhere::bookmarks_store::{has_bookmark, path_bookmark_at};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from, is_action_enabled};
use crate::platform::context::{evaluate_when, window_context};
use crate::platform::input::Keymap;
use crate::platform::keymap::{format_binding, host_binding, is_mac_host};
use crate::platform::misc::file_manager::file_manager_name;
use crate::platform::misc::platform_info::Os;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::project_view::context_menu::{Contributed, Item as ProjectItem, MenuGroupContext as ProjectSelection};
use crate::run_producers::context_run::{ContextMenu, MenuGroup, MenuGroupContext, MenuGroups};
use crate::theme;
use crate::ui::popup_menu::PopupMenu;
use crate::ui::tooltip::WithTooltip as _;
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, Focusable as _, Pixels, Point, SharedString, Window, div,
    prelude::*, px,
};
use ide_workspace::settings::SettingsScope;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::rc::Rc;

/// A row a menu group makes: a registered action, a separator, or a submenu of those.
pub type GroupRow = MenuItem<Infallible>;

/// Makes a group's rows for one menu from what the menu is for and what the window knows.
pub type GroupRows = fn(ContextMenu, &MenuGroupContext<'_>, &GroupFacts) -> Vec<GroupRow>;

/// A menu group of a feature.
pub struct GroupDecl {
    pub id: &'static str,
    pub menus: &'static [ContextMenu],
    /// Lower first; groups without an order are 0.
    pub order: i32,
    /// `None` while this window lacks what the group's rows are made from (its feature's state).
    pub rows: Option<GroupRows>,
}

/// Every menu group of the features, in the order of their ids.
pub const MENU_GROUPS: [GroupDecl; 25] = [
    GroupDecl {
        id: "agents.send",
        menus: &[ContextMenu::Editor, ContextMenu::ProjectView],
        order: 80,
        rows: Some(agents_send),
    },
    GroupDecl { id: "build.module", menus: &[ContextMenu::ProjectView], order: 40, rows: Some(build_module) },
    GroupDecl { id: "debugger.evaluate", menus: &[ContextMenu::Editor], order: 0, rows: Some(debugger_evaluate) },
    // The class files of the paths that can be reloaded into the paused session.
    GroupDecl { id: "debugger.hotswap", menus: &[ContextMenu::Editor], order: 0, rows: None },
    // One row per live debug session in the Stop chooser.
    GroupDecl { id: "debugger.stop", menus: &[ContextMenu::StopProcess], order: 0, rows: None },
    // Upload / Download / Compare with the deployment server of the path.
    GroupDecl {
        id: "deployment",
        menus: &[ContextMenu::ProjectView, ContextMenu::Editor, ContextMenu::EditorTab],
        order: 60,
        rows: None,
    },
    // Show Review Diff while the file belongs to the pull request under review.
    GroupDecl { id: "github.reviewMode", menus: &[ContextMenu::Editor], order: 60, rows: None },
    GroupDecl {
        id: "go.modules",
        menus: &[ContextMenu::ProjectView, ContextMenu::EditorTab, ContextMenu::Editor],
        order: 0,
        rows: None,
    },
    // Analyze Dependencies… of a Gradle build script the build model knows.
    GroupDecl {
        id: "gradle.analyzeDependencies",
        menus: &[ContextMenu::Editor, ContextMenu::EditorTab, ContextMenu::ProjectView],
        order: 30,
        rows: None,
    },
    GroupDecl { id: "http.new", menus: &[ContextMenu::New], order: 20, rows: Some(http_new) },
    // The Spring endpoints of the editor's file.
    GroupDecl { id: "http.spring", menus: &[ContextMenu::Editor], order: 40, rows: None },
    // Analyze Dependencies… of a pom.xml the build model knows.
    GroupDecl {
        id: "maven.analyzeDependencies",
        menus: &[ContextMenu::Editor, ContextMenu::EditorTab, ContextMenu::ProjectView],
        order: 30,
        rows: None,
    },
    GroupDecl {
        id: "plugins",
        menus: &[
            ContextMenu::Editor,
            ContextMenu::ProjectView,
            ContextMenu::EditorTab,
            ContextMenu::Problems,
            ContextMenu::SearchResults,
        ],
        order: 90,
        rows: Some(plugins),
    },
    // Run / Debug '…' and More Run/Debug: its actions run the producers' context.
    GroupDecl {
        id: "run.context",
        menus: &[ContextMenu::Editor, ContextMenu::ProjectView, ContextMenu::EditorTab],
        order: 0,
        rows: None,
    },
    GroupDecl { id: "scratch.new", menus: &[ContextMenu::New], order: 0, rows: Some(scratch_new) },
    GroupDecl { id: "search.bookmarks", menus: &[ContextMenu::EditorGutter], order: 20, rows: Some(line_bookmark) },
    GroupDecl {
        id: "search.bookmarks.files",
        menus: &[ContextMenu::ProjectView, ContextMenu::EditorTab],
        order: 60,
        rows: Some(file_bookmark),
    },
    GroupDecl {
        id: "search.structure.selectIn",
        menus: &[ContextMenu::SelectIn],
        order: 0,
        rows: Some(structure_select_in),
    },
    // Override File Type › for a file.
    GroupDecl {
        id: "settings.fileType",
        menus: &[ContextMenu::ProjectView, ContextMenu::EditorTab],
        order: 90,
        rows: None,
    },
    GroupDecl { id: "templates.emmet", menus: &[ContextMenu::Editor], order: 60, rows: Some(emmet_update_tag) },
    // One row per enabled file template.
    GroupDecl { id: "templates.new", menus: &[ContextMenu::New], order: 10, rows: None },
    GroupDecl {
        id: "templates.newFromTemplate",
        menus: &[ContextMenu::New],
        order: 100,
        rows: Some(new_from_template),
    },
    GroupDecl { id: "vcs.gutter", menus: &[ContextMenu::EditorGutter], order: 10, rows: Some(annotate) },
    GroupDecl {
        id: "web.open",
        menus: &[ContextMenu::Editor, ContextMenu::EditorTab, ContextMenu::ProjectView],
        order: 5,
        rows: Some(web_open),
    },
    // Open In › Browser › with the browsers found on the machine and the built-in preview.
    GroupDecl { id: "web.openIn", menus: &[ContextMenu::OpenIn], order: 0, rows: None },
];

/// What the groups read of the window when a menu opens.
#[derive(Clone, Debug, Default)]
pub struct GroupFacts {
    pub project_open: bool,
    pub vcs_available: bool,
    /// The active debug session is paused.
    pub debug_paused: bool,
    /// The file of the active editor.
    pub editor_path: Option<String>,
    /// The menu's path has a line bookmark on the menu's line.
    pub line_bookmarked: bool,
    /// Every one of the menu's paths that is a file of the disk has a file bookmark.
    pub paths_bookmarked: bool,
    /// Blame annotations are shown for the menu's path.
    pub annotations_shown: bool,
    /// The plugins' rows for the menu being built.
    pub plugins: Vec<GroupRow>,
}

/// The groups that make rows in this window, each reading `facts`.
pub fn native_groups(facts: &Rc<GroupFacts>) -> MenuGroups<GroupRow> {
    let mut groups = MenuGroups::default();
    for decl in &MENU_GROUPS {
        let Some(rows) = decl.rows else { continue };
        let facts = Rc::clone(facts);
        groups.register(MenuGroup {
            id: decl.id.to_owned(),
            menus: decl.menus.to_vec(),
            order: decl.order,
            items: Box::new(move |menu: ContextMenu, ctx: &MenuGroupContext<'_>| Ok(rows(menu, ctx, &facts))),
        });
    }
    groups
}

/// The rows of `items` in another menu's vocabulary.
pub fn into_menu<C>(items: Vec<GroupRow>) -> Vec<MenuItem<C>> {
    items.into_iter().map(|item| item.map(&|never: Infallible| -> C { match never {} })).collect()
}

/// The menu's one path when it is a file (not a folder).
fn single_file<'a>(ctx: &MenuGroupContext<'a>) -> Option<&'a str> {
    match (ctx.paths, ctx.dirs.first().copied()) {
        ([path], None | Some(false)) => Some(path.as_str()),
        _ => None,
    }
}

/// The lower-case extension of `path`.
fn extension(path: &str) -> String {
    std::path::Path::new(path).extension().map(|ext| ext.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn action_row(action: &'static str, arg: Option<Value>, label: Option<&str>) -> GroupRow {
    MenuItem::Action(ActionItem { action, arg, label: label.map(str::to_owned), disabled: false })
}

fn agents_send(menu: ContextMenu, ctx: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    if ctx.paths.is_empty() || ctx.paths.iter().any(|path| is_virtual_uri(path)) {
        return Vec::new();
    }
    // The Project view sends its selection; the editor sends what it shows.
    let arg = (menu == ContextMenu::ProjectView).then(|| json!(ctx.paths));
    vec![action_row("agents.sendToAgent", arg, None)]
}

fn build_module(_: ContextMenu, ctx: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    match ctx.paths {
        [path] if facts.project_open => vec![action_row("build.buildModule", Some(json!(path)), None)],
        _ => Vec::new(),
    }
}

fn debugger_evaluate(_: ContextMenu, _: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    if facts.debug_paused { vec![action_row("debug.evaluate", None, None)] } else { Vec::new() }
}

fn http_new(_: ContextMenu, ctx: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    match ctx.paths.first() {
        Some(directory) => {
            vec![action_row("http.newRequest", Some(json!({ "directory": directory })), Some("HTTP Request"))]
        }
        None => Vec::new(),
    }
}

fn plugins(_: ContextMenu, _: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    facts.plugins.clone()
}

fn scratch_new(_: ContextMenu, _: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    vec![action_row("file.newScratchFile", None, Some("Scratch File"))]
}

fn line_bookmark(_: ContextMenu, ctx: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    let (Some(path), Some(line)) = (ctx.paths.first(), ctx.line) else { return Vec::new() };
    let label = if facts.line_bookmarked { "Remove Bookmark" } else { "Add Bookmark" };
    vec![action_row("navigate.toggleBookmark", Some(json!({ "path": path, "line": line })), Some(label))]
}

/// The menu's paths that are files or folders of the disk, with whether each is a folder.
fn bookmark_targets(ctx: &MenuGroupContext<'_>) -> Vec<(String, bool)> {
    ctx.paths
        .iter()
        .enumerate()
        .filter(|(_, path)| !is_virtual_uri(path))
        .map(|(index, path)| (path.clone(), ctx.dirs.get(index).copied().unwrap_or(false)))
        .collect()
}

fn file_bookmark(_: ContextMenu, ctx: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    let targets = bookmark_targets(ctx);
    if targets.is_empty() {
        return Vec::new();
    }
    let label = if facts.paths_bookmarked { "Remove Bookmark" } else { "Add Bookmark" };
    let targets: Vec<Value> = targets.iter().map(|(path, dir)| json!({ "path": path, "dir": dir })).collect();
    vec![action_row("bookmarks.toggleFileBookmark", Some(json!({ "targets": targets })), Some(label))]
}

fn structure_select_in(_: ContextMenu, ctx: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    // The Structure tool window follows the active editor: it can show only that file.
    match ctx.paths.first() {
        Some(path) if facts.editor_path.as_deref() == Some(path.as_str()) => {
            vec![action_row("toolwindow.structure", None, Some("Structure"))]
        }
        _ => Vec::new(),
    }
}

fn emmet_update_tag(_: ContextMenu, ctx: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    const MARKUP: [&str; 7] = ["html", "htm", "xhtml", "shtml", "vue", "svelte", "xml"];
    match ctx.paths.first() {
        Some(path) if MARKUP.contains(&extension(path).as_str()) => vec![action_row("emmet.updateTag", None, None)],
        _ => Vec::new(),
    }
}

fn new_from_template(_: ContextMenu, ctx: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    let arg = ctx.paths.first().map(|directory| json!({ "directory": directory }));
    vec![action_row("templates.newFromTemplate", arg, Some("From Template…"))]
}

fn annotate(_: ContextMenu, ctx: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    match ctx.paths.first() {
        Some(path) if facts.vcs_available => {
            let label = if facts.annotations_shown { "Close Annotations" } else { "Annotate with Git Blame" };
            vec![action_row("vcs.annotate", Some(json!(path)), Some(label))]
        }
        _ => Vec::new(),
    }
}

fn web_open(_: ContextMenu, ctx: &MenuGroupContext<'_>, _: &GroupFacts) -> Vec<GroupRow> {
    let Some(path) =
        single_file(ctx).filter(|path| ["html", "htm", "xhtml", "shtml"].contains(&extension(path).as_str()))
    else {
        return Vec::new();
    };
    let name =
        std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let label = format!("Open '{name}' in Browser");
    vec![action_row("web.openInBrowser", Some(json!({ "path": path })), Some(&label))]
}

/// The plugins' context menu of a menu kind, where plugins add to it.
fn plugin_menu(menu: ContextMenu) -> Option<PluginMenu> {
    Some(match menu {
        ContextMenu::Editor => PluginMenu::Editor,
        ContextMenu::ProjectView => PluginMenu::ProjectView,
        ContextMenu::EditorTab => PluginMenu::EditorTab,
        ContextMenu::Problems => PluginMenu::Problems,
        ContextMenu::SearchResults => PluginMenu::SearchResults,
        ContextMenu::EditorGutter
        | ContextMenu::OpenIn
        | ContextMenu::StopProcess
        | ContextMenu::SelectIn
        | ContextMenu::New => return None,
    })
}

// -------------------------------------------------------------------------------------------------
// The menus' rows

/// What choosing a row of the editor's menu of its own does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorMenuCommand {
    Cut,
    Copy,
    Paste,
    /// Puts a virtual document's address on the clipboard.
    CopyReference(String),
}

/// The shortcuts shown beside Cut, Copy and Paste.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardShortcuts {
    pub cut: Option<String>,
    pub copy: Option<String>,
    pub paste: Option<String>,
}

/// What the editor's menu is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorMenuInput {
    pub path: String,
    /// The main selection is not empty.
    pub has_selection: bool,
    pub read_only: bool,
    /// The document is not a file of the disk.
    pub virtual_doc: bool,
    /// The name of the OS file manager (Open In › Finder).
    pub file_manager: String,
    pub shortcuts: ClipboardShortcuts,
}

/// The shortcut the active keymap shows for action `id`.
fn keymap_shortcut(id: &str, cx: &App) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(id, is_mac_host()))
}

/// The clipboard rows' shortcuts: the keymap's for the editing actions, else the keys the OS gives
/// them (the keymaps leave Copy and Paste to the OS).
pub fn clipboard_shortcuts(cx: &App) -> ClipboardShortcuts {
    let mac = is_mac_host();
    let shortcut = |action: &str, os: &str| {
        keymap_shortcut(action, cx).or_else(|| Some(format_binding(&host_binding(os, mac), mac)))
    };
    ClipboardShortcuts {
        cut: shortcut("edit.cut", "Meta+X"),
        copy: shortcut("edit.copy", "Meta+C"),
        paste: shortcut("edit.paste", "Meta+V"),
    }
}

fn action<C>(id: &'static str, label: &str) -> MenuItem<C> {
    MenuItem::action(id).labelled(label)
}

fn action_with<C>(id: &'static str, arg: Value, label: &str) -> MenuItem<C> {
    MenuItem::action_with(id, arg).labelled(label)
}

/// Open In › for a file: the Project view, the OS file manager, the terminal, then the `openIn`
/// groups.
pub fn open_in_menu<C>(path: &str, file_manager: &str, groups: Vec<GroupRow>) -> MenuItem<C> {
    let mut rows = vec![
        action_with("navigate.selectIn", json!({ "path": path }), "Project View"),
        action_with("project.revealInFinder", json!(path), file_manager),
        action_with("project.openInTerminal", json!(path), "Terminal"),
        MenuItem::Separator,
    ];
    rows.extend(into_menu(groups));
    MenuItem::submenu("Open In", rows)
}

/// Local History › for a file.
pub fn local_history_menu<C>(path: &str) -> MenuItem<C> {
    MenuItem::submenu(
        "Local History",
        vec![
            action_with("vcs.localHistory", json!({ "path": path }), "Show History"),
            action("localHistory.putLabel", "Put Label…"),
        ],
    )
}

/// Git › for one file.
pub fn git_menu<C>(path: &str) -> MenuItem<C> {
    MenuItem::submenu(
        "Git",
        vec![
            action("vcs.commit", "Commit File…"),
            action_with("vcs.addToVcs", json!([path]), "Add"),
            MenuItem::Separator,
            action_with("vcs.annotate", json!(path), "Annotate with Git Blame"),
            action_with("vcs.showDiff", json!([path]), "Show Diff"),
            action_with("vcs.compareWithBranch", json!({ "path": path }), "Compare with Branch…"),
            action_with("vcs.showHistory", json!([path]), "Show History"),
            action("vcs.showHistoryForSelection", "Show History for Selection"),
            action_with("vcs.rollback", json!([path]), "Rollback…"),
            MenuItem::Separator,
            action("vcs.push", "Push…"),
            action("vcs.update", "Pull…"),
            action("vcs.fetch", "Fetch"),
            MenuItem::Separator,
            action("vcs.branches", "Branches…"),
        ],
    )
}

/// A clipboard row of the editor's menu.
fn clipboard_row(
    label: &str,
    icon: &'static str,
    command: EditorMenuCommand,
    shortcut: Option<&str>,
) -> MenuItem<EditorMenuCommand> {
    let row = MenuItem::item(label, command).icon(icon);
    match shortcut {
        Some(shortcut) => row.shortcut(shortcut),
        None => row,
    }
}

/// The editor's context menu: `groups` are the `editor` groups' rows, `open_in` the `openIn` ones.
pub fn editor_menu(
    input: &EditorMenuInput,
    groups: Vec<GroupRow>,
    open_in: Vec<GroupRow>,
) -> Vec<MenuItem<EditorMenuCommand>> {
    let mut items = vec![action("code.showIntentions", "Show Context Actions"), MenuItem::Separator];
    // Cut and Copy only with a selection.
    let shortcuts = &input.shortcuts;
    if input.has_selection {
        let cut = clipboard_row("Cut", "cut", EditorMenuCommand::Cut, shortcuts.cut.as_deref());
        items.push(cut.disabled(input.read_only));
        items.push(clipboard_row("Copy", "copy", EditorMenuCommand::Copy, shortcuts.copy.as_deref()));
    }
    let paste = clipboard_row("Paste", "paste", EditorMenuCommand::Paste, shortcuts.paste.as_deref());
    items.push(paste.disabled(input.read_only));
    items.extend([
        MenuItem::Separator,
        MenuItem::submenu(
            "Copy / Paste Special",
            vec![
                MenuItem::action("editor.copyPathReference"),
                MenuItem::action("editor.copyReference"),
                MenuItem::action("code.pasteAsPlainText"),
                MenuItem::action("code.pasteFromHistory"),
            ],
        ),
        action("editor.toggleColumnSelection", "Column Selection Mode"),
        MenuItem::Separator,
        MenuItem::submenu(
            "Go To",
            vec![
                action("navigate.declaration", "Declaration or Usages"),
                action("navigate.implementation", "Implementation(s)"),
                action("navigate.typeDeclaration", "Type Declaration"),
                action("navigate.superMethod", "Super Method"),
                action("navigate.gotoTest", "Test"),
                MenuItem::Separator,
                action("find.usages", "Find Usages"),
                action("navigate.callHierarchy", "Call Hierarchy"),
                action("navigate.typeHierarchy", "Type Hierarchy"),
            ],
        ),
        MenuItem::Separator,
        MenuItem::submenu(
            "Folding",
            vec![
                action("code.unfold", "Expand"),
                action("code.fold", "Collapse"),
                action("code.unfoldAll", "Expand All"),
                action("code.foldAll", "Collapse All"),
                MenuItem::Separator,
                action("code.foldSelection", "Fold Selection / Remove Region"),
            ],
        ),
        MenuItem::Separator,
        action("refactor.rename", "Rename…"),
        MenuItem::submenu(
            "Refactor",
            vec![
                action("refactor.thisMenu", "Refactor This…"),
                MenuItem::Separator,
                MenuItem::action("refactor.move"),
                MenuItem::action("refactor.changeSignature"),
                MenuItem::action("refactor.extractVariable"),
                MenuItem::action("refactor.extractConstant"),
                MenuItem::action("refactor.extractMethod"),
                MenuItem::action("refactor.inline"),
                MenuItem::action("refactor.pullMembersUp"),
                MenuItem::action("refactor.pushMembersDown"),
                MenuItem::Separator,
                MenuItem::action("refactor.safeDelete"),
            ],
        ),
        action("code.generate", "Generate…"),
        MenuItem::Separator,
    ]);
    items.extend(into_menu(groups));
    let path = input.path.as_str();
    if input.virtual_doc {
        items.extend([
            MenuItem::Separator,
            MenuItem::item("Copy Reference", EditorMenuCommand::CopyReference(input.path.clone())),
        ]);
    } else {
        items.extend([
            MenuItem::Separator,
            open_in_menu(path, &input.file_manager, open_in),
            MenuItem::Separator,
            local_history_menu(path),
            git_menu(path),
            MenuItem::Separator,
            action_with("diff.compareWith", json!(path), "Compare With…"),
            action_with("diff.compareWithClipboard", json!({ "path": path }), "Compare with Clipboard"),
        ]);
    }
    items
}

/// What choosing a row of the gutter's menu of its own does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GutterCommand {
    /// Sets the global editor setting `key` to the opposite of `on`.
    Toggle { key: &'static str, on: bool },
}

/// The gutter's Appearance › switches: label and global setting.
pub const GUTTER_APPEARANCE: [(&str, &str); 7] = [
    ("Show Line Numbers", "editor.appearance.lineNumbers"),
    ("Show Gutter Icons", "editor.showGutterIcons"),
    ("Show Folding Outline", "editor.folding.showOutline"),
    ("Show Indent Guides", "editor.appearance.indentGuides"),
    ("Show Whitespaces", "editor.showWhitespace"),
    ("Show Method Separators", "editor.appearance.methodSeparators"),
    ("Show Breadcrumbs", "editor.breadcrumbs.show"),
];

/// The value of the dotted setting `key` in `settings` (the settings as JSON).
fn setting_on(settings: &Value, key: &str) -> bool {
    settings.pointer(&format!("/{}", key.replace('.', "/"))).and_then(Value::as_bool).unwrap_or(false)
}

/// A row of a menu of its own that shows a check mark while `on`.
fn check_row<C>(label: &str, command: C, on: bool) -> MenuItem<C> {
    let row = MenuItem::item(label, command);
    if on { row.icon("check") } else { row }
}

/// The gutter's context menu: `groups` are the `editorGutter` groups' rows (Annotate, bookmarks),
/// `settings` the settings in effect as JSON.
pub fn gutter_menu(groups: Vec<GroupRow>, settings: &Value) -> Vec<MenuItem<GutterCommand>> {
    let mut items = into_menu(groups);
    let appearance = GUTTER_APPEARANCE
        .iter()
        .map(|&(label, key)| {
            let on = setting_on(settings, key);
            check_row(label, GutterCommand::Toggle { key, on }, on)
        })
        .collect();
    items.extend([
        action_with("file.settings", json!("editor.general"), "Configure Soft Wraps…"),
        MenuItem::Separator,
        MenuItem::submenu("Appearance", appearance),
        action_with("file.settings", json!("editor.general.gutterIcons"), "Configure Gutter Icons…"),
    ]);
    items
}

/// What a tool window's Options menu or stripe button menu does to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolWindowCommand {
    MoveTo(Slot),
    RestoreDefaultLayout,
    ToggleMaximize,
    Hide,
    /// Hides the window and takes its button off the stripe.
    RemoveFromStripe,
    /// Sets Show Tool Window Names to the opposite of the value it had.
    ToggleNames(bool),
}

/// The areas a tool window can move to, as Move To › lists them.
pub const MOVE_TARGETS: [(Slot, &str); 6] = [
    (Slot::Left, "Left Top"),
    (Slot::LeftSplit, "Left Bottom"),
    (Slot::Bottom, "Bottom Left"),
    (Slot::BottomSplit, "Bottom Right"),
    (Slot::RightSplit, "Right Bottom"),
    (Slot::Right, "Right Top"),
];

/// Move To › for tool window `id`: its own area checked and greyed out.
pub fn move_to_menu(layout: &ToolWindowLayout, id: &str) -> MenuItem<ToolWindowCommand> {
    let current = layout.slot_of(id);
    let mut rows: Vec<MenuItem<ToolWindowCommand>> = MOVE_TARGETS
        .iter()
        .map(|&(slot, label)| {
            let here = current == Some(slot);
            check_row(label, ToolWindowCommand::MoveTo(slot), here).disabled(here)
        })
        .collect();
    rows.extend([
        MenuItem::Separator,
        MenuItem::item("Restore Default Layout", ToolWindowCommand::RestoreDefaultLayout),
    ]);
    MenuItem::submenu("Move To", rows)
}

/// The Options menu of tool window `id`'s header: Move To, Maximize or Restore Size, Hide.
pub fn tool_window_options(
    layout: &ToolWindowLayout,
    id: &str,
    hide_shortcut: Option<String>,
) -> Vec<MenuItem<ToolWindowCommand>> {
    let maximized = layout.maximized().is_some_and(|anchor| layout.anchor_of(id) == Some(anchor));
    let (label, icon) = if maximized { ("Restore Size", "minimize") } else { ("Maximize", "maximize") };
    let hide = MenuItem::item("Hide", ToolWindowCommand::Hide).icon("hide");
    vec![
        move_to_menu(layout, id),
        MenuItem::item(label, ToolWindowCommand::ToggleMaximize).icon(icon),
        MenuItem::Separator,
        match hide_shortcut {
            Some(shortcut) => hide.shortcut(shortcut),
            None => hide,
        },
    ]
}

/// The menu of tool window `id`'s stripe button: Hide (taking the button off), Move To, Show Tool
/// Window Names.
pub fn stripe_button_menu(layout: &ToolWindowLayout, id: &str, show_names: bool) -> Vec<MenuItem<ToolWindowCommand>> {
    vec![
        MenuItem::item("Hide", ToolWindowCommand::RemoveFromStripe),
        MenuItem::Separator,
        move_to_menu(layout, id),
        MenuItem::Separator,
        check_row("Show Tool Window Names", ToolWindowCommand::ToggleNames(show_names), show_names),
    ]
}

/// The setting Show Tool Window Names is kept under.
const SHOW_NAMES_KEY: &str = "appearance.showToolWindowNames";

/// The 0-based line whose gutter (left of the text) is under `position`.
fn gutter_line_at(editor: &Editor, position: Point<Pixels>) -> Option<u32> {
    let offset = editor.offset_at(position)?;
    let (line, _) = buffer::line_col(editor.rope(), offset);
    let text_left = editor.position_of(editor.offset_of(line, 0))?.x;
    (position.x < text_left).then_some(line)
}

/// A right click outside every selection puts the caret where it was pressed; inside one, the
/// selection stays for the menu to act on.
fn place_caret_for_menu(editor: &mut Editor, position: Point<Pixels>, cx: &mut Context<Editor>) {
    let Some(offset) = editor.offset_at(position) else { return };
    let inside =
        editor.selections().iter().any(|&(anchor, head)| anchor.min(head) <= offset && offset <= anchor.max(head));
    if !inside {
        editor.select(offset, offset, cx);
    }
}

// -------------------------------------------------------------------------------------------------
// The window

impl Workbench {
    /// What the groups read of the window for `menu` on `ctx` (`column`: the UTF-16 column of a
    /// problem or a search match).
    fn group_facts(
        &self,
        menu: ContextMenu,
        ctx: &MenuGroupContext<'_>,
        column: Option<u32>,
        window: &Window,
        cx: &App,
    ) -> GroupFacts {
        let keys = window_context(window, cx);
        let data = self.bookmarks().data();
        let targets = bookmark_targets(ctx);
        let plugins = plugin_menu(menu).map_or_else(Vec::new, |plugin_menu| {
            let position = ctx.line.map(|line| (line, column.unwrap_or(0)));
            let mut rows = plugin_menu_items::<Infallible>(plugin_menu, ctx.paths, position, &keys, cx);
            // The group list puts the separator after each group.
            if matches!(rows.last(), Some(MenuItem::Separator)) {
                rows.pop();
            }
            rows
        });
        GroupFacts {
            project_open: self.project_root(cx).is_some(),
            vcs_available: self.vcs_state().is_some_and(|vcs| vcs.available),
            debug_paused: keys.is_truthy("debugSessionPaused"),
            editor_path: self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()),
            line_bookmarked: match (ctx.paths.first(), ctx.line) {
                (Some(path), Some(line)) => has_bookmark(&data.items, path, line),
                _ => false,
            },
            paths_bookmarked: !targets.is_empty()
                && targets.iter().all(|(path, _)| path_bookmark_at(&data.paths, path).is_some()),
            // No blame annotations are drawn in this window's editors.
            annotations_shown: false,
            plugins,
        }
    }

    /// The groups' rows of `menu` for `ctx`, each non-empty group followed by a separator.
    pub(super) fn group_rows(
        &self,
        menu: ContextMenu,
        ctx: &MenuGroupContext<'_>,
        column: Option<u32>,
        window: &Window,
        cx: &App,
    ) -> Vec<GroupRow> {
        let facts = Rc::new(self.group_facts(menu, ctx, column, window, cx));
        let (rows, failed) = native_groups(&facts).items(menu, ctx, || MenuItem::Separator);
        for (id, error) in failed {
            tracing::warn!("the menu group {id} made no rows: {error}");
        }
        rows
    }

    /// The `openIn` groups' rows for `path`.
    fn open_in_rows(&self, path: &str, window: &Window, cx: &App) -> Vec<GroupRow> {
        let paths = [path.to_owned()];
        let ctx = MenuGroupContext { paths: &paths, ..MenuGroupContext::default() };
        self.group_rows(ContextMenu::OpenIn, &ctx, None, window, cx)
    }

    // ---------------------------------------------------------------------------------------------
    // The editor

    /// The right button was pressed at `position` over `editor`: the editor takes the keyboard and
    /// the gutter's menu opens left of the text, the editor's menu over it (after the caret moved
    /// there unless the press is inside a selection).
    pub(super) fn open_editor_context_menu(
        &mut self,
        editor: &Entity<Editor>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&editor.focus_handle(cx), cx);
        if let Some(line) = gutter_line_at(editor.read(cx), position) {
            self.publish_context(window, cx);
            return self.show_gutter_menu(editor, line, position, window, cx);
        }
        editor.update(cx, |editor, cx| place_caret_for_menu(editor, position, cx));
        // The menu's rows are judged against the window with the editor focused.
        self.publish_context(window, cx);
        let items = self.editor_menu_items(editor, window, cx);
        self.show_menu_later(
            items,
            position,
            |workbench, command, window, cx| workbench.run_editor_menu_command(command, window, cx),
            window,
            cx,
        );
    }

    /// Opens a menu of `items` at `position` once this update is over, as [`Workbench::show_menu`]
    /// does. A row's check mark may be read through the workbench (Column Selection Mode reads the
    /// active editor), which cannot be read while it is being updated.
    fn show_menu_later<C: Clone + 'static>(
        &self,
        items: Vec<MenuItem<C>>,
        position: Point<Pixels>,
        on_command: impl Fn(&mut Self, C, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workbench = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let menu = cx.new(|cx| PopupMenu::new(&items, position, window, cx));
            let shown = workbench.update(cx, |workbench, cx| workbench.show_popup_menu(menu, on_command, window, cx));
            if shown.is_err() {
                tracing::debug!("the window closed before its menu opened");
            }
        });
    }

    /// The editor's context menu for `editor` as it is now.
    pub fn editor_menu_items(
        &self,
        editor: &Entity<Editor>,
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<EditorMenuCommand>> {
        let shown = editor.read(cx);
        let path = shown.path().to_string_lossy().into_owned();
        let (text, line) = (shown.text(), shown.caret_position().0);
        let input = EditorMenuInput {
            has_selection: !shown.selection().is_empty(),
            read_only: shown.is_read_only(),
            virtual_doc: is_virtual_uri(&path),
            file_manager: file_manager_name(Os::from_platform(std::env::consts::OS)).to_owned(),
            shortcuts: clipboard_shortcuts(cx),
            path,
        };
        let paths = [input.path.clone()];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[], text: Some(&text), line: Some(line) };
        let groups = self.group_rows(ContextMenu::Editor, &ctx, None, window, cx);
        let open_in = if input.virtual_doc { Vec::new() } else { self.open_in_rows(&input.path, window, cx) };
        editor_menu(&input, groups, open_in)
    }

    /// Carries out a row of the editor's menu of its own. The menu has closed and the editor has the
    /// keyboard again, so the editing actions act on it.
    fn run_editor_menu_command(&mut self, command: EditorMenuCommand, window: &mut Window, cx: &mut Context<Self>) {
        let action = match command {
            EditorMenuCommand::Cut => "edit.cut",
            EditorMenuCommand::Copy => "edit.copy",
            EditorMenuCommand::Paste => "edit.paste",
            EditorMenuCommand::CopyReference(path) => {
                cx.write_to_clipboard(ClipboardItem::new_string(path));
                return;
            }
        };
        // Once this update is over: the action reaches for the workbench's editor.
        window.defer(cx, move |window, cx| {
            if !execute_action_from(ActionSource::ContextMenu, action, None, window, cx) {
                tracing::warn!("the editor menu's {action} did not run");
            }
        });
    }

    /// The gutter's menu for the 0-based `line` of `editor`.
    fn show_gutter_menu(
        &mut self,
        editor: &Entity<Editor>,
        line: u32,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, text) =
            editor.read_with(cx, |editor, _| (editor.path().to_string_lossy().into_owned(), editor.text()));
        let paths = [path];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[], text: Some(&text), line: Some(line) };
        let groups = self.group_rows(ContextMenu::EditorGutter, &ctx, None, window, cx);
        let settings = serde_json::to_value(crate::platform::settings::effective(&self.label, cx)).unwrap_or_default();
        let items = gutter_menu(groups, &settings);
        self.show_menu_later(
            items,
            position,
            |workbench, command, _, cx| workbench.run_gutter_command(command, cx),
            window,
            cx,
        );
    }

    fn run_gutter_command(&mut self, command: GutterCommand, cx: &mut Context<Self>) {
        let GutterCommand::Toggle { key, on } = command;
        self.write_global_setting(key, !on, cx);
    }

    /// Writes a global switch; a failure is told.
    fn write_global_setting(&mut self, key: &str, value: bool, cx: &mut Context<Self>) {
        let written = crate::platform::settings::set(&self.label, SettingsScope::Global, key, &Value::Bool(value), cx);
        if let Err(error) = written {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot change the setting");
            request.body = Some(error.message);
            self.notify(request, cx);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Additions to the menus other parts of the window build

    /// The editor tab menu's rows for its file at `path`: the `editorTab` groups, Open In, Local
    /// History and, for a file of the project, Git; `text` is the tab's document when it has an
    /// editor.
    pub(super) fn tab_file_rows<C>(
        &self,
        path: &str,
        in_project: bool,
        text: Option<&str>,
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<C>> {
        let paths = [path.to_owned()];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[], text, line: None };
        let mut rows = into_menu(self.group_rows(ContextMenu::EditorTab, &ctx, None, window, cx));
        let file_manager = file_manager_name(Os::from_platform(std::env::consts::OS));
        rows.extend([
            open_in_menu(path, file_manager, self.open_in_rows(path, window, cx)),
            MenuItem::Separator,
            local_history_menu(path),
        ]);
        if in_project {
            rows.push(git_menu(path));
        }
        rows.push(MenuItem::Separator);
        rows
    }

    /// The Problems menu `items` with the `problems` groups for the problem they act on.
    pub(super) fn with_problem_groups(
        &self,
        mut items: Vec<MenuItem<ProblemCommand>>,
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<ProblemCommand>> {
        let target: Option<ProblemTarget> = items.iter().find_map(|item| match item.command() {
            Some(ProblemCommand::JumpToSource(target)) => Some(target.clone()),
            _ => None,
        });
        let Some(target) = target else { return items };
        let paths = [target.path];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[], text: None, line: Some(target.line) };
        items.extend(into_menu(self.group_rows(ContextMenu::Problems, &ctx, Some(target.col), window, cx)));
        items
    }

    /// The menu of a match in the Find tool window: the `searchResults` groups for its place.
    pub(super) fn show_search_result_menu(
        &mut self,
        path: String,
        line: u32,
        col: u32,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = [path];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[], text: None, line: Some(line) };
        let rows = self.group_rows(ContextMenu::SearchResults, &ctx, Some(col), window, cx);
        if rows.is_empty() {
            return;
        }
        self.show_menu(&rows, position, |_, never, _, _| match never {}, window, cx);
    }

    /// The Project view's contributed rows for `selected`: the `projectView`, `openIn` and
    /// `selectIn` groups, and the `new` groups for `new_in`, the folder New creates in.
    pub(super) fn project_view_contributed(
        &self,
        selected: &ProjectSelection,
        new_in: Option<&str>,
        window: &Window,
        cx: &App,
    ) -> Contributed {
        let ctx = MenuGroupContext { paths: &selected.paths, dirs: &selected.dirs, text: None, line: None };
        let rows = |menu: ContextMenu, ctx: &MenuGroupContext<'_>| -> Vec<ProjectItem> {
            into_menu(self.group_rows(menu, ctx, None, window, cx))
        };
        let folder: Vec<String> = new_in.map(str::to_owned).into_iter().collect();
        let dirs = vec![true; folder.len()];
        let new_ctx = MenuGroupContext { paths: &folder, dirs: &dirs, text: None, line: None };
        let first: Vec<String> = selected.paths.first().cloned().into_iter().collect();
        let select_ctx = MenuGroupContext { paths: &first, ..MenuGroupContext::default() };
        Contributed {
            new: rows(ContextMenu::New, &new_ctx),
            project_view: rows(ContextMenu::ProjectView, &ctx),
            open_in: rows(ContextMenu::OpenIn, &ctx),
            select_in: rows(ContextMenu::SelectIn, &select_ctx),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Tool windows

    /// The Options menu of tool window `id`, at `position`.
    pub(super) fn show_tool_window_options(
        &mut self,
        id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.tool_windows() else { return };
        let items = tool_window_options(layout, id, keymap_shortcut("toolwindow.hideActive", cx));
        self.show_tool_window_menu(id, &items, position, window, cx);
    }

    /// The menu of tool window `id`'s stripe button, at `position`.
    pub(super) fn show_stripe_button_menu(
        &mut self,
        id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.tool_windows() else { return };
        let settings = serde_json::to_value(crate::platform::settings::effective(&self.label, cx)).unwrap_or_default();
        let items = stripe_button_menu(layout, id, setting_on(&settings, SHOW_NAMES_KEY));
        self.show_tool_window_menu(id, &items, position, window, cx);
    }

    fn show_tool_window_menu(
        &mut self,
        id: &str,
        items: &[MenuItem<ToolWindowCommand>],
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = id.to_owned();
        self.show_menu(
            items,
            position,
            move |workbench, command, window, cx| workbench.run_tool_window_command(&id, command, window, cx),
            window,
            cx,
        );
    }

    fn run_tool_window_command(
        &mut self,
        id: &str,
        command: ToolWindowCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ToolWindowCommand::ToggleNames(on) = command {
            return self.write_global_setting(SHOW_NAMES_KEY, !on, cx);
        }
        self.with_layout(window, cx, |layout| match command {
            ToolWindowCommand::MoveTo(slot) => layout.move_window(id, slot, None),
            ToolWindowCommand::RestoreDefaultLayout => layout.reset_placements(),
            ToolWindowCommand::ToggleMaximize => layout.toggle_maximize(id),
            ToolWindowCommand::Hide => layout.hide(id),
            ToolWindowCommand::RemoveFromStripe => layout.remove_from_stripe(id),
            ToolWindowCommand::ToggleNames(_) => Effects::NONE,
        });
    }
}

// -------------------------------------------------------------------------------------------------
// Toolbar slots

/// The buttons plugins put in toolbar slot `slot` (`editor.title`, `mainToolbar`, `commit.message`)
/// whose `when` holds: each runs its action, greyed out while the action cannot run; an entry
/// whose action is not registered has no button.
pub fn slot_buttons(slot: &str, window: &Window, cx: &App) -> Vec<AnyElement> {
    let Some(registry) = cx.try_global::<ActionRegistry>() else { return Vec::new() };
    let keys = window_context(window, cx);
    let (fg, faint, hover) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"), theme::color(cx, "bg-hover"));
    plugin_slot_items(slot, cx)
        .into_iter()
        .filter(|item| evaluate_when(item.when.as_deref(), &keys).unwrap_or(false))
        .filter_map(|item| {
            let action = registry.get(&item.action)?;
            let enabled = is_action_enabled(&action.meta, &keys);
            let icon = item.icon.clone().unwrap_or_else(|| "plugin".to_owned());
            let selector = format!("slot:{slot}:{}", item.action);
            let id = item.action.clone();
            Some(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .size(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .child(theme::icon(cx, &icon, 16.0, if enabled { fg } else { faint }))
                    .with_tooltip_shortcut(item.label.clone(), item.action.clone())
                    .when(enabled, |el| {
                        el.hover(move |el| el.bg(hover)).on_click(move |_, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &id, None, window, cx);
                        })
                    })
                    .into_any_element(),
            )
        })
        .collect()
}

/// The editor's menu while it is open.
pub type EditorMenu = Entity<PopupMenu<EditorMenuCommand>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::editor::intelligence::fake_server::FakeServer;
    use crate::platform::storage::UiStorage;
    use crate::plugins::Plugins;
    use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point};
    use ide_host::plugins::{Consent, FileSecretStore};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    const LABEL: &str = "w1";

    // ---------------------------------------------------------------------------------------------
    // The rows as data

    fn input(has_selection: bool) -> EditorMenuInput {
        EditorMenuInput {
            path: "/p/a.txt".to_owned(),
            has_selection,
            read_only: false,
            virtual_doc: false,
            file_manager: "Finder".to_owned(),
            shortcuts: ClipboardShortcuts {
                cut: Some("⌘X".into()), copy: Some("⌘C".into()), paste: Some("⌘V".into())
            },
        }
    }

    /// The labels of `items`, an action row by its label (else its id), `-` for a separator.
    fn labels<C>(items: &[MenuItem<C>]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => "-".to_owned(),
                other => other
                    .label()
                    .map(str::to_owned)
                    .or_else(|| other.action_id().map(str::to_owned))
                    .unwrap_or_default(),
            })
            .collect()
    }

    /// The editor's menu has the rows in the order the IDE shows them: Cut and Copy only with a
    /// selection, the groups after Generate…, then Open In, Local History, Git and Compare.
    #[test]
    fn the_editor_menu_lists_its_rows_in_order() {
        let groups = vec![action_row("agents.sendToAgent", None, None), MenuItem::Separator];
        let items = editor_menu(&input(false), groups.clone(), Vec::new());
        assert_eq!(
            labels(&items),
            [
                "Show Context Actions",
                "-",
                "Paste",
                "-",
                "Copy / Paste Special",
                "Column Selection Mode",
                "-",
                "Go To",
                "-",
                "Folding",
                "-",
                "Rename…",
                "Refactor",
                "Generate…",
                "-",
                "agents.sendToAgent",
                "-",
                "-",
                "Open In",
                "-",
                "Local History",
                "Git",
                "-",
                "Compare With…",
                "Compare with Clipboard",
            ]
        );
        let selected = editor_menu(&input(true), groups, Vec::new());
        assert_eq!(labels(&selected)[2..5], ["Cut", "Copy", "Paste"]);
        let MenuItem::Item(copy) = &selected[3] else { panic!("Copy is a row of the menu's own") };
        assert_eq!((copy.shortcut.as_deref(), copy.run.clone()), (Some("⌘C"), Some(EditorMenuCommand::Copy)));
    }

    /// A read-only document greys out Cut and Paste; a virtual document offers Copy Reference
    /// instead of the file rows.
    #[test]
    fn read_only_and_virtual_documents_change_the_clipboard_and_file_rows() {
        let read_only = EditorMenuInput { read_only: true, ..input(true) };
        let items = editor_menu(&read_only, Vec::new(), Vec::new());
        let disabled: Vec<(String, bool)> =
            items[2..5].iter().map(|item| (item.label().unwrap_or_default().to_owned(), item.is_disabled())).collect();
        assert_eq!(disabled, [("Cut".to_owned(), true), ("Copy".to_owned(), false), ("Paste".to_owned(), true)]);
        let virtual_doc =
            EditorMenuInput { virtual_doc: true, path: "jdt://contents/A.class".to_owned(), ..input(false) };
        let items = editor_menu(&virtual_doc, Vec::new(), Vec::new());
        let last = items.last().expect("rows");
        assert_eq!(last.command(), Some(&EditorMenuCommand::CopyReference("jdt://contents/A.class".to_owned())));
        assert!(!labels(&items).contains(&"Open In".to_owned()));
    }

    /// The groups come in their order, then by id; a group with no rows adds nothing, the others a
    /// separator each.
    #[test]
    fn the_groups_of_a_menu_come_in_order_each_followed_by_a_separator() {
        let facts = Rc::new(GroupFacts { project_open: true, debug_paused: true, ..GroupFacts::default() });
        let paths = ["/p/index.html".to_owned()];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[false], text: Some(""), line: Some(0) };
        let (rows, failed) = native_groups(&facts).items(ContextMenu::Editor, &ctx, || MenuItem::Separator);
        assert!(failed.is_empty());
        // debugger.evaluate (0), web.open (5), templates.emmet (60), agents.send (80).
        assert_eq!(
            labels(&rows),
            [
                "debug.evaluate",
                "-",
                "Open 'index.html' in Browser",
                "-",
                "emmet.updateTag",
                "-",
                "agents.sendToAgent",
                "-"
            ]
        );
        let (gutter, _) = native_groups(&facts).items(ContextMenu::EditorGutter, &ctx, || MenuItem::Separator);
        assert_eq!(labels(&gutter), ["Add Bookmark", "-"], "no VCS: no Annotate");
    }

    /// Every group of the features is listed once; the menus without a group of this window are the
    /// ones whose feature state the window does not have.
    #[test]
    fn every_menu_group_is_listed_once() {
        let mut ids: Vec<&str> = MENU_GROUPS.iter().map(|group| group.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MENU_GROUPS.len());
        let missing: Vec<&str> =
            MENU_GROUPS.iter().filter(|group| group.rows.is_none()).map(|group| group.id).collect();
        assert_eq!(
            missing,
            [
                "debugger.hotswap",
                "debugger.stop",
                "deployment",
                "github.reviewMode",
                "go.modules",
                "gradle.analyzeDependencies",
                "http.spring",
                "maven.analyzeDependencies",
                "run.context",
                "settings.fileType",
                "templates.new",
                "web.openIn",
            ]
        );
    }

    // ---------------------------------------------------------------------------------------------
    // End to end

    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..1500 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within fifteen seconds");
    }

    fn settle_app(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut App) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if cx.update(|cx| done(cx)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within ten seconds");
    }

    /// Runs git in `dir` without the machine's own configuration.
    fn git_in(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// What a test window starts with.
    #[derive(Default)]
    struct Setup {
        /// The project is a Git repository with its files committed.
        git: bool,
        /// The fake language server answers this for `*.mock` files.
        server: Option<Value>,
        /// The plugin host runs, with the Hello fixture linked.
        plugin: bool,
    }

    struct Fixture {
        workbench: Entity<Workbench>,
        root: PathBuf,
        server: Option<FakeServer>,
        _dirs: (tempfile::TempDir, tempfile::TempDir),
    }

    /// The Hello fixture's manifest with entries in the editor's menu (a submenu of two) and the
    /// editor's tab strip; its Sync command only runs in Bazel workspaces.
    fn hello_menus(text: &str) -> String {
        format!(
            "{text}\n\
             [[contributes.commands]]\nid = \"sync\"\ntitle = \"Hello: Sync\"\nwhen = \"workspaceContains:MODULE.bazel\"\n\n\
             [[contributes.menus]]\nmenu = \"editor.context\"\ncommand = \"insert-date\"\nsubmenu = \"Hello\"\n\n\
             [[contributes.menus]]\nmenu = \"editor.context\"\ncommand = \"sync\"\nsubmenu = \"Hello\"\n\n\
             [[contributes.menus]]\nmenu = \"editor.title\"\ncommand = \"insert-date\"\n"
        )
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("the folder is made");
        for entry in std::fs::read_dir(from).expect("the fixture is readable").flatten() {
            let (source, target) = (entry.path(), to.join(entry.file_name()));
            if entry.file_name() == "target" {
                continue;
            }
            if source.is_dir() {
                copy_dir(&source, &target);
            } else {
                std::fs::copy(&source, &target).expect("the file is copied");
            }
        }
    }

    /// Links a copy of the Hello fixture plugin (in the temporary home) with its manifest given
    /// [`hello_menus`], and waits until its commands are actions.
    fn link_hello(cx: &mut TestAppContext, home: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/ide-plugin/tests/fixtures/hello");
        let folder = home.join("fixtures").join("hello");
        copy_dir(&source, &folder);
        let manifest = folder.join("plugin.toml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest is readable");
        std::fs::write(&manifest, hello_menus(&text)).expect("the manifest is written");
        let id = cx.update(|cx| {
            let host = cx.global::<Plugins>().host.clone();
            let info = host.link(&folder).expect("the plugin links");
            let consent = Consent { optional: Vec::new(), signer_change: false };
            let info = host.grant(&info.id, &consent).expect("consent is recorded");
            assert_eq!(info.error, None, "the plugin loads");
            info.id
        });
        let action = crate::plugins::plugin_action_id(&id, "insert-date");
        settle_app(cx, "the plugin's commands are actions", |cx| cx.global::<ActionRegistry>().contains(&action));
    }

    /// A trusted project of `files` open in an active window, as `setup` says.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
        setup: Setup,
    ) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let dirs = AppDirs::under(home.path());
        let server = setup.server.as_ref().map(|answers| {
            let server = FakeServer::install(&dirs.config, home.path());
            server.answer(answers.clone());
            server
        });
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            std::fs::write(root.join(path), text).expect("the file is written");
        }
        if setup.git {
            git_in(&root, &["init", "-q", "-b", "main"]);
            git_in(&root, &["config", "user.name", "Ada"]);
            git_in(&root, &["config", "user.email", "ada@example.test"]);
            git_in(&root, &["add", "."]);
            git_in(&root, &["commit", "-q", "-m", "first"]);
        }
        let full_mode = setup.server.is_some();
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            crate::platform::input::init(cx);
            crate::editor::actions::register(cx);
            crate::workbench::register_actions(cx);
            Backend::init(dirs, cx).expect("the backend starts");
            crate::workbench::menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            let host = cx.global::<Backend>().host.clone();
            if full_mode {
                host.global_settings.set(SettingsScope::Global, "ide.mode", json!("full")).expect("Full mode is set");
            }
            host.trust.decide(&root, true, false).expect("the project is trusted");
        });
        if setup.plugin {
            cx.update(|cx| {
                theme::appearance::init(cx);
                Plugins::init(cx).expect("the plugin host starts");
                let host = cx.global::<Plugins>().host.clone();
                host.set_secret_store(Arc::new(FileSecretStore { dir: home.path().join("secrets") }));
            });
            settle_app(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
            link_hello(cx, home.path());
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        if setup.git {
            settle(cx, "the repository is read", |cx| {
                workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|vcs| vcs.available))
            });
        }
        (Fixture { workbench, root, server, _dirs: (home, folder) }, cx)
    }

    impl Fixture {
        /// Opens `name` and waits until its editor has the keyboard.
        fn open_file(&self, name: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
            let path = self.root.join(name);
            self.workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
            settle(cx, "the file opens", |cx| {
                let editor = self.workbench.read_with(cx, |w, cx| {
                    w.editor().filter(|editor| editor.read(cx).path() == path.as_path()).cloned()
                });
                editor.is_some_and(|editor| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)))
            });
            self.editor(cx)
        }

        fn editor(&self, cx: &mut VisualTestContext) -> Entity<Editor> {
            self.workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
        }

        fn editor_menu(&self, cx: &mut VisualTestContext) -> Option<EditorMenu> {
            self.workbench.read_with(cx, |w, _| w.open_menu::<EditorMenuCommand>())
        }
    }

    /// The middle of the character at `offset` of `editor`.
    fn at_offset(editor: &Entity<Editor>, offset: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
        editor.read_with(cx, |editor, cx| {
            let at = editor.position_of(offset).expect("the line is painted");
            point(at.x + px(2.0), at.y + editor.line_height(cx) / 2.0)
        })
    }

    fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, selector: String) {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }

    /// The rows of each open level of `menu`, without the selection mark.
    fn shown<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        lines
            .into_iter()
            .map(|level| level.into_iter().map(|line| line.trim_start_matches("> ").to_owned()).collect())
            .collect()
    }

    /// Clicks the rows `path` names, one level after the other (a submenu row opens its submenu).
    fn choose<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, path: &[&str], cx: &mut VisualTestContext) {
        for (depth, label) in path.iter().enumerate() {
            let levels = shown(menu, cx);
            let index =
                levels[depth].iter().position(|line| line == label).unwrap_or_else(|| panic!("{label} in {levels:?}"));
            click(cx, format!("menu-row-{depth}-{index}"));
        }
    }

    /// Whether the row `label` of the deepest open level of `menu` is greyed out.
    fn disabled<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, label: &str, cx: &mut VisualTestContext) -> bool {
        menu.read_with(cx, |menu, _| menu.rows().iter().find(|row| row.label == label).map(|row| row.disabled))
            .unwrap_or_else(|| panic!("{label} is a row"))
    }

    /// The registered actions and their titles.
    fn registered(cx: &mut VisualTestContext) -> std::collections::HashMap<String, String> {
        cx.update(|_, cx| {
            cx.global::<ActionRegistry>()
                .all()
                .map(|action| (action.meta.id.clone(), action.meta.title.clone()))
                .collect()
        })
    }

    /// The labels the editor's menu shows at its root for what `editor_menu_items` builds, given
    /// which actions the window registers: an action row needs its action, a submenu one of its rows.
    fn expected_root(fixture: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
        let editor = fixture.editor(cx);
        let items = cx.update(|window, cx| fixture.workbench.read(cx).editor_menu_items(&editor, window, cx));
        let titles = registered(cx);
        fn shows(item: &MenuItem<EditorMenuCommand>, titles: &std::collections::HashMap<String, String>) -> bool {
            match item {
                MenuItem::Separator => false,
                MenuItem::Action(row) => titles.contains_key(row.action),
                MenuItem::Item(entry) => entry.submenu.is_empty() || entry.submenu.iter().any(|row| shows(row, titles)),
            }
        }
        items
            .iter()
            .filter(|item| shows(item, &titles))
            .map(|item| match item {
                MenuItem::Action(row) => row.label.clone().unwrap_or_else(|| titles[row.action].clone()),
                other => other.label().unwrap_or_default().to_owned(),
            })
            .collect()
    }

    fn root_rows(menu: &EditorMenu, cx: &mut VisualTestContext) -> Vec<String> {
        shown(menu, cx)[0].iter().filter(|line| *line != "-").cloned().collect()
    }

    /// A right click in a plain file's text puts the caret there and opens the editor's menu at the
    /// pointer: without a selection there is no Cut or Copy, Paste is offered, and the rows are the
    /// menu's own in order. Git's rows are greyed out outside a repository. Escape closes the menu
    /// and the editor has the keyboard again.
    #[gpui_kit::test]
    async fn right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "hello world\nsecond line\n")], Setup::default());
        let editor = f.open_file("a.txt", cx);
        let at = at_offset(&editor, 8, cx);
        right_click(cx, at);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        assert_eq!(editor.read_with(cx, |editor, _| editor.selection()), 8..8, "the caret moved to the press");
        let rows = root_rows(&menu, cx);
        assert_eq!(rows, expected_root(&f, cx));
        assert!(!rows.contains(&"Cut".to_owned()) && !rows.contains(&"Copy".to_owned()), "{rows:?}");
        let paste = rows.iter().position(|row| row == "Paste").expect("Paste is offered");
        let go_to = rows.iter().position(|row| row == "Go To").expect("Go To is offered");
        assert!(paste < go_to, "{rows:?}");
        assert!(!disabled(&menu, "Paste", cx));
        if rows.contains(&"Git".to_owned()) {
            choose(&menu, &["Git"], cx);
            assert!(disabled(&menu, "Commit File…", cx), "no repository: Git's rows cannot run");
        }

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(f.editor_menu(cx).is_none(), "Escape closes the menu");
        assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)), "the editor has the keyboard");
    }

    /// A right click inside the selection keeps it: Cut and Copy are offered with the OS's keys,
    /// and Copy puts the selected text on the clipboard.
    #[gpui_kit::test]
    async fn right_click_in_the_selection_keeps_it_and_copy_puts_it_on_the_clipboard(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "hello world\n")], Setup::default());
        let editor = f.open_file("a.txt", cx);
        editor.update(cx, |editor, cx| editor.select(0, 5, cx));
        let at = at_offset(&editor, 2, cx);
        right_click(cx, at);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        assert_eq!(editor.read_with(cx, |editor, _| editor.selection()), 0..5, "the selection stays");
        let rows = root_rows(&menu, cx);
        let cut = rows.iter().position(|row| row == "Cut").expect("Cut is offered");
        assert_eq!(rows[cut..cut + 3], ["Cut", "Copy", "Paste"], "{rows:?}");
        let expected_copy = cx.update(|_, cx| clipboard_shortcuts(cx).copy);
        let copy = menu.read_with(cx, |menu, _| {
            menu.rows().iter().find(|row| row.label == "Copy").map(|row| row.shortcut.clone())
        });
        assert_eq!(copy, Some(expected_copy));

        choose(&menu, &["Copy"], cx);
        settle(cx, "the text is copied", |cx| {
            cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())).as_deref() == Some("hello")
        });
        assert!(f.editor_menu(cx).is_none());
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "hello world\n", "Copy leaves the text");
    }

    /// In a Git repository the editor menu's Git rows can run.
    #[gpui_kit::test]
    async fn in_a_git_repository_the_editor_menus_git_rows_are_enabled(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "one\n")], Setup { git: true, ..Setup::default() });
        let editor = f.open_file("a.txt", cx);
        let at = at_offset(&editor, 1, cx);
        right_click(cx, at);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        assert_eq!(root_rows(&menu, cx), expected_root(&f, cx));
        choose(&menu, &["Git"], cx);
        assert!(!disabled(&menu, "Commit File…", cx), "vcsAvailable holds");
    }

    /// Go To › Declaration or Usages, chosen from the menu opened on a name, asks the language
    /// server for the declaration and moves the caret there.
    #[gpui_kit::test]
    async fn go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration(cx: &mut TestAppContext) {
        let files = [("a.mock", "call target here\n"), ("b.mock", "zero\none\n  target\n")];
        let (f, cx) = open(cx, &files, Setup { server: Some(json!({})), ..Setup::default() });
        let b = f.root.join("b.mock");
        let at = json!({ "line": 2, "character": 2 });
        let location = json!({ "uri": format!("file://{}", b.display()), "range": { "start": at, "end": at } });
        if let Some(server) = &f.server {
            server.answer(json!({ "textDocument/definition": [location] }));
        }
        let editor = f.open_file("a.mock", cx);
        let at = at_offset(&editor, 7, cx);
        right_click(cx, at);
        assert_eq!(editor.read_with(cx, |editor, _| editor.caret()), 7);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        choose(&menu, &["Go To", "Declaration or Usages"], cx);
        settle(cx, "the declaration opens", |cx| {
            let editor = f.editor(cx);
            editor.read_with(cx, |editor, _| editor.path() == b.as_path() && editor.caret_position() == (2, 2))
        });
    }

    /// A right click left of the text opens the gutter's menu for that line: Add Bookmark there adds
    /// a bookmark on that line, and the menu then offers to remove it.
    #[gpui_kit::test]
    async fn the_gutters_menu_adds_a_bookmark_on_the_clicked_line(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "one\ntwo\nthree\n")], Setup::default());
        let editor = f.open_file("a.txt", cx);
        let line_start = editor.read_with(cx, |editor, _| editor.offset_of(1, 0));
        let text_at = at_offset(&editor, line_start, cx);
        let gutter = point(text_at.x - px(12.0), text_at.y);
        right_click(cx, gutter);
        let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<GutterCommand>()).expect("the gutter's menu is open");
        // The bookmark group (no Annotate outside a repository), then the gutter's own rows.
        assert_eq!(
            shown(&menu, cx)[0],
            ["Add Bookmark", "-", "Configure Soft Wraps…", "-", "Appearance", "Configure Gutter Icons…"]
        );
        choose(&menu, &["Appearance"], cx);
        assert_eq!(shown(&menu, cx)[1].len(), GUTTER_APPEARANCE.len());
        choose(&menu, &["Add Bookmark"], cx);
        let path = f.root.join("a.txt").to_string_lossy().into_owned();
        settle(cx, "the bookmark is added", |cx| {
            f.workbench.read_with(cx, |w, _| has_bookmark(&w.bookmarks().data().items, &path, 1))
        });
        right_click(cx, gutter);
        let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<GutterCommand>()).expect("the gutter's menu is open");
        assert!(shown(&menu, cx)[0].contains(&"Remove Bookmark".to_owned()));
    }

    /// A tab's menu acts on the tab it was opened on, not on the active one, and lists the file's
    /// Open In and Local History rows.
    #[gpui_kit::test]
    async fn the_tab_menu_acts_on_the_tab_it_was_opened_on(cx: &mut TestAppContext) {
        use crate::workbench::TabCommand;
        let (f, cx) = open(cx, &[("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")], Setup::default());
        for name in ["a.txt", "b.txt", "c.txt"] {
            f.open_file(name, cx);
        }
        let tab_ids = |cx: &mut VisualTestContext| -> Vec<String> {
            f.workbench.read_with(cx, |w, _| {
                let tabs = w.tabs().expect("a project is open");
                tabs.pane(&tabs.active_pane).expect("the active pane").tabs.iter().map(|tab| tab.id.clone()).collect()
            })
        };
        let first = tab_ids(cx)[0].clone();
        let bounds = cx.debug_bounds(Box::leak(format!("tab:{first}").into_boxed_str())).expect("the tab is drawn");
        right_click(cx, bounds.center());
        let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).expect("the tab's menu is open");
        let rows = shown(&menu, cx)[0].clone();
        let titles = registered(cx);
        if titles.contains_key("vcs.localHistory") || titles.contains_key("localHistory.putLabel") {
            assert!(rows.contains(&"Local History".to_owned()), "{rows:?}");
        }
        choose(&menu, &["Close"], cx);
        assert_eq!(tab_ids(cx).len(), 2);
        assert!(!tab_ids(cx).contains(&first), "the clicked tab closed");
        let active = f.editor(cx).read_with(cx, |editor, _| editor.path().to_path_buf());
        assert_eq!(active, f.root.join("c.txt"), "the active tab stays");
    }

    /// A plugin's two editor menu entries naming one submenu appear under it while their `when`
    /// holds; the entry whose command cannot run is greyed out, and choosing the other runs it. Its
    /// `editor.title` entry is a button of the tab strip.
    #[gpui_kit::test]
    async fn a_plugins_editor_menu_entries_appear_under_their_submenu_and_run(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "one two\n")], Setup { plugin: true, ..Setup::default() });
        let editor = f.open_file("notes.txt", cx);
        settle(cx, "the key is answered", |cx| {
            cx.update(|window, cx| window_context(window, cx).get("workspaceContains:MODULE.bazel").is_some())
        });
        let at = at_offset(&editor, 1, cx);
        right_click(cx, at);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        let rows = root_rows(&menu, cx);
        let hello = rows.iter().position(|row| row == "Hello").expect("the plugin's submenu");
        let paste = rows.iter().position(|row| row == "Paste").expect("Paste");
        assert!(paste < hello, "the groups come after the menu's own rows: {rows:?}");
        choose(&menu, &["Hello"], cx);
        assert!(disabled(&menu, "Hello: Sync", cx), "Sync needs a Bazel workspace");
        assert!(!disabled(&menu, "Hello: Insert Date", cx));
        choose(&menu, &["Hello", "Hello: Insert Date"], cx);
        settle(cx, "the plugin inserts the date", |cx| {
            editor.read_with(cx, |editor, _| editor.text().chars().filter(char::is_ascii_digit).count() >= 8)
        });
        let id = cx.update(|_, cx| {
            cx.global::<ActionRegistry>().ids().find(|id| id.ends_with(".insert-date")).map(str::to_owned)
        });
        let id = id.expect("the plugin's command is an action");
        assert!(cx.debug_bounds(Box::leak(format!("slot:editor.title:{id}").into_boxed_str())).is_some());
    }

    /// Move To › Right Top in a tool window's Options menu docks it on the right.
    #[gpui_kit::test]
    async fn move_to_in_a_tool_windows_options_moves_it(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "a\n")], Setup::default());
        click(cx, "more:project".to_owned());
        let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<ToolWindowCommand>()).expect("the options are open");
        assert_eq!(shown(&menu, cx)[0], ["Move To", "Maximize", "-", "Hide"]);
        choose(&menu, &["Move To", "Right Top"], cx);
        let slot = f.workbench.read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.slot_of("project")));
        assert_eq!(slot, Some(Slot::Right));
    }

    /// The rows whose action this window does not register are left out of the editor's menu; the
    /// list of those actions is what the window still lacks.
    #[gpui_kit::test]
    async fn the_editor_menu_leaves_out_the_actions_the_window_does_not_register(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "a\n")], Setup::default());
        let editor = f.open_file("a.txt", cx);
        let items = cx.update(|window, cx| f.workbench.read(cx).editor_menu_items(&editor, window, cx));
        fn ids(items: &[MenuItem<EditorMenuCommand>], out: &mut Vec<&'static str>) {
            for item in items {
                match item {
                    MenuItem::Action(row) => out.push(row.action),
                    MenuItem::Item(entry) => ids(&entry.submenu, out),
                    MenuItem::Separator => {}
                }
            }
        }
        let mut all = Vec::new();
        ids(&items, &mut all);
        let titles = registered(cx);
        let missing: Vec<&str> = all.iter().copied().filter(|id| !titles.contains_key(*id)).collect();
        eprintln!("editor menu actions not registered: {missing:?}");
        let at = at_offset(&editor, 0, cx);
        right_click(cx, at);
        let menu = f.editor_menu(cx).expect("the editor's menu is open");
        let keys: Vec<String> = menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| row.key.clone()).collect());
        for id in missing {
            assert!(!keys.iter().any(|key| key == id || key.starts_with(&format!("{id}|"))), "{id} is shown");
        }
    }
}
