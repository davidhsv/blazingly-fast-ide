//! The application's menu bar.
//!
//! [`menu_spec`](super::menu_spec) says which menus there are; the focused window says what they
//! show right now: labels in its language, the shortcut of each action's first binding, check
//! marks, the items that cannot run, its tool windows and the recent projects ([`MenuState`]).
//! [`model`] puts the two together and the bar is replaced only when the result changes, since
//! replacing it redraws the macOS menu bar.
//!
//! A chosen item runs its action in the focused window ([`MenuCommand`]). The keymap stays the
//! only dispatcher of shortcuts: an item shows its action's shortcut, but a key that reaches the
//! menu's own binding was declined by the keymap first and is dropped, except for the few items
//! whose shortcut the table itself gives (Quit, window cycling).

use super::menu_spec::{
    DYNAMIC_TOOL_WINDOWS, DynamicItem, Entry, Native, OPEN_RECENT_ACTION, Os, RecentItem, action_ids, fires_natively,
    item_enabled, recent_items, spec, without_action,
};
use crate::backend::Backend;
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from, is_action_enabled};
use crate::platform::collate;
use crate::platform::context::{context_changes, window_context};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::{is_mouse_binding, normalize_binding, parse_double_modifier};
use gpui_kit::{App, Global, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, Task, Window};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub use command::MenuCommand;

mod command {
    use gpui_kit as gpui;

    /// A chosen menu item: the action it runs, or the `native.*` role it stands for, and the
    /// argument the action runs with (the project of an Open Recent item).
    #[derive(Clone, PartialEq, Debug, gpui::Action)]
    #[action(namespace = menu, no_json)]
    pub struct MenuCommand {
        pub action: String,
        pub arg: Option<String>,
    }
}

/// How long the window's state rests before the bar follows it.
pub const SYNC_DELAY: Duration = Duration::from_millis(120);
/// The action that checks for a new version; the app has no updater, so its item is left out.
const CHECK_FOR_UPDATES: &str = "app.checkForUpdates";

/// One entry of the bar as it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    Submenu {
        id: String,
        label: String,
        children: Vec<MenuNode>,
    },
    /// Runs `action` with `arg`.
    Item {
        action: String,
        arg: Option<String>,
        label: String,
        /// The shortcut shown beside the label, in keymap syntax (`Meta+Shift+O`).
        shortcut: Option<String>,
        /// Whether `shortcut` is the item's own and runs it, where the keymap binds nothing.
        native_shortcut: bool,
        checked: bool,
        enabled: bool,
    },
    /// An item the OS knows the meaning of.
    Native {
        role: Native,
        label: String,
    },
    /// A disabled line such as "No Recent Projects".
    Placeholder {
        label: String,
    },
    Separator,
}

/// What the bar shows besides the table: the state of the focused window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuState {
    /// `menu.<id>` → label, for submenus, actions and `native.*` roles.
    pub labels: Rc<HashMap<String, String>>,
    /// Action id → its first binding, where a menu can show it.
    pub accelerators: HashMap<String, String>,
    /// The items of each [`Entry::Dynamic`] slot.
    pub dynamic: HashMap<String, Vec<DynamicItem>>,
    /// The toggle actions that are on.
    pub checked: HashSet<String>,
    /// The registered actions that cannot run in the window as it is.
    pub disabled: HashSet<String>,
    pub recent: Vec<RecentItem>,
}

impl MenuState {
    fn label(&self, id: &str, default: &str) -> String {
        self.labels.get(&format!("menu.{id}")).cloned().unwrap_or_else(|| default.to_owned())
    }
}

/// Whether a menu can show `key`, the last part of a binding, as a key equivalent.
fn is_native_key(key: &str) -> bool {
    const NAMED: [&str; 16] = [
        "Space",
        "Enter",
        "Escape",
        "Tab",
        "Backspace",
        "Delete",
        "Up",
        "Down",
        "Left",
        "Right",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "NumpadAdd",
        "NumpadSubtract",
    ];
    let function = || key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n));
    let single = || {
        let mut chars = key.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_uppercase() || c.is_ascii_digit() || "[]`\\,./;'=-".contains(c))
    };
    NAMED.contains(&key) || matches!(key, "NumpadMultiply" | "NumpadDivide") || function() || single()
}

/// The binding as a menu shows it (`Meta+Shift+O`), or `None` when a menu cannot: chords, double
/// modifiers, mouse buttons and gestures.
pub fn menu_accelerator(binding: &str) -> Option<String> {
    if is_mouse_binding(binding) || parse_double_modifier(binding).is_some() {
        return None;
    }
    let stroke = normalize_binding(binding);
    if stroke.contains(' ') {
        return None;
    }
    stroke.rsplit('+').next().is_some_and(is_native_key).then_some(stroke)
}

/// A shortcut the table gives an item (`CmdOrCtrl+Q`, `Cmd+Shift+\``) in keymap syntax.
fn table_accelerator(accel: &str, os: Os) -> String {
    let parts: Vec<&str> = accel
        .split('+')
        .map(|part| match part {
            "CmdOrCtrl" if os == Os::Mac => "Meta",
            "CmdOrCtrl" => "Ctrl",
            "Cmd" => "Meta",
            other => other,
        })
        .collect();
    normalize_binding(&parts.join("+"))
}

/// The bar for `entries` in `state`.
pub fn model(entries: &[Entry], state: &MenuState, os: Os) -> Vec<MenuNode> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Submenu { id, label, children } => out.push(MenuNode::Submenu {
                id: (*id).to_owned(),
                label: state.label(id, label),
                children: model(children, state, os),
            }),
            Entry::Action { id, label, accel, check, .. } => out.push(MenuNode::Item {
                action: (*id).to_owned(),
                arg: None,
                label: state.label(id, label),
                shortcut: accel
                    .map(|accel| table_accelerator(accel, os))
                    .or_else(|| state.accelerators.get(*id).cloned()),
                native_shortcut: accel.is_some(),
                checked: *check && state.checked.contains(*id),
                enabled: item_enabled(id, &state.disabled),
            }),
            Entry::Dynamic(slot) => {
                out.extend(state.dynamic.get(*slot).into_iter().flatten().map(|item| MenuNode::Item {
                    action: item.action.clone(),
                    arg: item.arg.clone(),
                    label: item.label.clone(),
                    // An item with an argument is not what the action's shortcut runs.
                    shortcut: item.arg.is_none().then(|| state.accelerators.get(&item.action).cloned()).flatten(),
                    native_shortcut: false,
                    checked: item.checked == Some(true),
                    enabled: item_enabled(&item.action, &state.disabled),
                }));
            }
            Entry::Native(role) => {
                out.push(MenuNode::Native { role: *role, label: state.label(role.key(), role.default_label()) });
            }
            Entry::Separator => out.push(MenuNode::Separator),
            Entry::RecentProjects if state.recent.is_empty() => {
                out.push(MenuNode::Placeholder { label: state.label("recent.none", "No Recent Projects") });
            }
            Entry::RecentProjects => out.extend(state.recent.iter().map(|recent| MenuNode::Item {
                action: OPEN_RECENT_ACTION.to_owned(),
                arg: Some(recent.path.clone()),
                label: recent.label.clone(),
                shortcut: None,
                native_shortcut: false,
                checked: false,
                enabled: true,
            })),
        }
    }
    out
}

/// A keymap stroke (`Meta+Shift+O`) as GPUI spells it (`cmd-shift-o`), or `None` for a key GPUI
/// has no name for.
pub fn gpui_keystroke(stroke: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for part in stroke.split('+') {
        parts.push(match part {
            "Meta" => "cmd".to_owned(),
            "Ctrl" => "ctrl".to_owned(),
            "Alt" => "alt".to_owned(),
            "Shift" => "shift".to_owned(),
            key if key.starts_with("Numpad") => return None,
            key => key.to_lowercase(),
        });
    }
    Some(parts.join("-"))
}

/// The binding that has the bar show `stroke` beside the item of `action`; `None` for a stroke
/// GPUI cannot bind.
fn binding(stroke: &str, action: String, cx: &App) -> Option<KeyBinding> {
    let command = Box::new(MenuCommand { action, arg: None });
    KeyBinding::load(&gpui_keystroke(stroke)?, command, None, false, None, cx).ok()
}

/// The role of the OS a `native.*` item stands for, where GPUI has a selector for it.
fn os_action(role: Native) -> Option<OsAction> {
    Some(match role {
        Native::Undo => OsAction::Undo,
        Native::Redo => OsAction::Redo,
        Native::Cut => OsAction::Cut,
        Native::Copy => OsAction::Copy,
        Native::Paste => OsAction::Paste,
        Native::SelectAll => OsAction::SelectAll,
        _ => return None,
    })
}

/// The shortcut the OS gives a role, in keymap syntax.
fn role_shortcut(role: Native) -> Option<&'static str> {
    Some(match role {
        Native::Undo => "Meta+Z",
        Native::Redo => "Meta+Shift+Z",
        Native::Cut => "Meta+X",
        Native::Copy => "Meta+C",
        Native::Paste => "Meta+V",
        Native::SelectAll => "Meta+A",
        Native::Hide => "Meta+H",
        Native::HideOthers => "Meta+Alt+H",
        Native::Minimize => "Meta+M",
        _ => return None,
    })
}

/// Whether the shortcut of the `native.*` item `id` is the OS's to run: the keymap binds
/// nothing to hiding the app or minimizing a window.
fn role_fires_natively(id: &str) -> bool {
    matches!(id, "native.hide" | "native.hideOthers" | "native.minimize")
}

/// The editing action a role runs in the focused window.
fn role_action(role: &str) -> Option<&'static str> {
    Some(match role {
        "native.undo" => "edit.undo",
        "native.redo" => "edit.redo",
        "native.cut" => "edit.cut",
        "native.copy" => "edit.copy",
        "native.paste" => "edit.paste",
        "native.selectAll" => "edit.selectAll",
        _ => return None,
    })
}

fn items(nodes: &[MenuNode]) -> Vec<MenuItem> {
    nodes
        .iter()
        .map(|node| match node {
            MenuNode::Submenu { label, children, .. } => {
                MenuItem::Submenu(Menu { name: label.clone().into(), items: items(children), disabled: false })
            }
            MenuNode::Item { action, arg, label, checked, enabled, .. } => MenuItem::Action {
                name: label.clone().into(),
                action: Box::new(MenuCommand { action: action.clone(), arg: arg.clone() }),
                os_action: None,
                checked: *checked,
                disabled: !enabled,
            },
            MenuNode::Native { role: Native::Services, label } => {
                MenuItem::os_submenu(label.clone(), SystemMenuType::Services)
            }
            MenuNode::Native { role, label } => MenuItem::Action {
                name: label.clone().into(),
                action: Box::new(MenuCommand { action: role.key().to_owned(), arg: None }),
                os_action: os_action(*role),
                checked: false,
                disabled: false,
            },
            MenuNode::Placeholder { label } => MenuItem::Action {
                name: label.clone().into(),
                action: Box::new(MenuCommand { action: String::new(), arg: None }),
                os_action: None,
                checked: false,
                disabled: true,
            },
            MenuNode::Separator => MenuItem::Separator,
        })
        .collect()
}

/// The shortcut of every item that shows one: `(stroke, action)`, in bar order.
fn shortcuts(nodes: &[MenuNode], out: &mut Vec<(String, String)>) {
    for node in nodes {
        match node {
            MenuNode::Submenu { children, .. } => shortcuts(children, out),
            MenuNode::Item { action, arg: None, shortcut: Some(shortcut), .. } => {
                out.push((shortcut.clone(), action.clone()));
            }
            MenuNode::Native { role, .. } => {
                out.extend(role_shortcut(*role).map(|shortcut| (shortcut.to_owned(), role.key().to_owned())));
            }
            _ => {}
        }
    }
}

/// The bar that is installed, and what it was built from.
#[derive(Default)]
pub struct MenuBar {
    shown: Option<Vec<MenuNode>>,
    /// The labels of the locale they were read in: formatting them again is the costly part.
    labels: Option<(&'static str, Rc<HashMap<String, String>>)>,
    recent: Vec<RecentItem>,
    pending: Option<Task<()>>,
    /// Grows when something the bar shows changed that no window's context tells of: the
    /// settings (labels, shortcuts, check marks) or the recent projects.
    changes: u64,
    /// What the bar was last asked to follow: the window, its context and `changes` then.
    followed: Option<(gpui_kit::WindowId, u64, u64)>,
}

impl Global for MenuBar {}

impl MenuBar {
    /// The bar as it was last installed.
    pub fn shown(&self) -> Option<&[MenuNode]> {
        self.shown.as_deref()
    }

    /// Whether the bar has been asked to follow a window and has not done so yet.
    pub fn is_sync_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// The table for `os`, without what the app cannot do.
fn entries_for(os: Os) -> Vec<Entry> {
    without_action(spec(os), CHECK_FOR_UPDATES)
}

/// The table of the OS the app runs on: built once, it never changes.
fn entries() -> &'static [Entry] {
    static ENTRIES: std::sync::LazyLock<Vec<Entry>> = std::sync::LazyLock::new(|| entries_for(Os::current()));
    &ENTRIES
}

/// Handles chosen items and reads the recent projects. The bar itself follows the focused
/// window ([`schedule_sync`]).
pub fn init(cx: &mut App) {
    cx.default_global::<MenuBar>();
    refresh_recent(cx);
    cx.on_action(run);
    if cx.has_global::<Backend>() {
        cx.global_mut::<Backend>().listen(|event, cx| {
            if matches!(event.event, ide_core::IdeEvent::SettingsChanged { .. }) {
                cx.default_global::<MenuBar>().changes += 1;
                cx.refresh_windows();
            }
        });
    }
    let registry = cx.default_global::<ActionRegistry>();
    registry.handle("app.quit", |_, _, cx| {
        // Each window is asked first; one with unsaved changes keeps the app open while it asks.
        let windows = cx.windows();
        let mut all = true;
        for window in windows {
            let closes = window
                .update(cx, |_, window, cx| {
                    super::for_window(window, cx).is_none_or(|workbench| {
                        workbench.update(cx, |workbench, cx| workbench.confirm_close(window, cx))
                    })
                })
                .unwrap_or(true);
            all &= closes;
        }
        if all {
            cx.quit();
        }
        Ok(())
    });
}

/// Reads the recent projects again: the list changes when a project is opened.
pub fn refresh_recent(cx: &mut App) {
    let Some(backend) = cx.try_global::<Backend>() else { return };
    let recent = ide_workspace::RecentProjects::load(&backend.dirs.config).list();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let bar = cx.default_global::<MenuBar>();
    bar.recent = recent_items(&recent, home.as_deref());
    bar.changes += 1;
}

fn os_name(os: Os) -> &'static str {
    match os {
        Os::Mac => "macos",
        Os::Windows => "windows",
        Os::Linux => "linux",
    }
}

fn labels(i18n: &I18n, os: Os) -> HashMap<String, String> {
    let params = [("os", os_name(os).into()), ("distractionFree", "off".into())];
    i18n.keys_with_prefix("menu.").into_iter().map(|key| (key.clone(), i18n.t(&key, &params))).collect()
}

/// What `window` has the bar show.
pub fn state(window: &Window, cx: &mut App) -> MenuState {
    let os = Os::current();
    let entries = entries();
    let locale = cx.default_global::<Localization>().0.locale();
    if cx.default_global::<MenuBar>().labels.as_ref().is_none_or(|(known, _)| *known != locale) {
        let read = labels(&cx.default_global::<Localization>().0, os);
        cx.default_global::<MenuBar>().labels = Some((locale, Rc::new(read)));
    }
    let bar = cx.global::<MenuBar>();
    let (labels, recent) =
        (bar.labels.as_ref().map(|(_, labels)| Rc::clone(labels)).unwrap_or_default(), bar.recent.clone());

    let i18n = &cx.global::<Localization>().0;
    let declared: Vec<super::ToolWindow> = match super::for_window(window, cx) {
        Some(workbench) => workbench.read(cx).tool_windows().map(|layout| layout.windows().cloned().collect()),
        None => None,
    }
    .unwrap_or_else(|| super::TOOL_WINDOWS.iter().filter_map(|id| super::tool_window_declaration(id)).collect());
    let mut tool_windows: Vec<DynamicItem> = declared
        .into_iter()
        .map(|window| DynamicItem {
            action: format!("toolwindow.{}", window.id),
            arg: None,
            label: if window.verbatim { window.title } else { i18n.tx(&window.title) },
            checked: None,
        })
        .collect();
    tool_windows.sort_by(|a, b| collate::compare(&a.label, &b.label));
    let dynamic = HashMap::from([(DYNAMIC_TOOL_WINDOWS.to_owned(), tool_windows)]);

    let mut ids: Vec<&str> = action_ids(entries);
    ids.extend(dynamic.values().flatten().map(|item| item.action.as_str()));
    let keymap = &cx.global::<Keymap>().0;
    let accelerators = ids
        .iter()
        .filter_map(|id| Some(((*id).to_owned(), menu_accelerator(keymap.primary_key_binding(id)?)?)))
        .collect();
    let registry = cx.global::<ActionRegistry>();
    let context = window_context(window, cx);
    let disabled = ids
        .iter()
        .filter(|id| registry.get(id).is_some_and(|action| !is_action_enabled(&action.meta, &context)))
        .map(|id| (*id).to_owned())
        .collect();
    let checked = registry
        .all()
        .filter(|action| action.is_toggled(window, cx) == Some(true))
        .map(|action| action.meta.id.clone())
        .collect();
    MenuState { labels, accelerators, dynamic, checked, disabled, recent }
}

/// Makes the bar show `window`'s state, now. Not from inside an update of the window's
/// workbench: whether an action is on may be read from it.
pub fn sync(window: &mut Window, cx: &mut App) {
    let os = Os::current();
    let state = state(window, cx);
    let nodes = model(entries(), &state, os);
    if cx.global::<MenuBar>().shown.as_ref() == Some(&nodes) {
        return;
    }
    let mut bound = Vec::new();
    shortcuts(&nodes, &mut bound);
    // The bar shows the shortcut of the binding its item's command has; the last one bound for a
    // command wins, so the ones bound for an earlier keymap do no harm.
    let bindings: Vec<KeyBinding> =
        bound.into_iter().filter_map(|(stroke, action)| binding(&stroke, action, cx)).collect();
    cx.bind_keys(bindings);
    cx.set_menus(nodes.iter().filter_map(|node| match node {
        MenuNode::Submenu { label, children, .. } => {
            Some(Menu { name: label.clone().into(), items: items(children), disabled: false })
        }
        _ => None,
    }));
    cx.global_mut::<MenuBar>().shown = Some(nodes);
}

/// Has the bar follow `window` once its state has rested: called whenever it may have changed.
pub fn schedule_sync(window: &mut Window, cx: &mut App) {
    // Called on every draw of the focused window: nothing is done unless what the bar shows
    // could have changed since it was last asked.
    let following = (window.window_handle().window_id(), context_changes(cx), cx.default_global::<MenuBar>().changes);
    let bar = cx.global_mut::<MenuBar>();
    if bar.followed == Some(following) || bar.pending.is_some() {
        return;
    }
    bar.followed = Some(following);
    let handle = window.window_handle();
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(SYNC_DELAY).await;
        cx.update(|cx| {
            cx.global_mut::<MenuBar>().pending = None;
            // The window may be gone; the next one to be focused asks again.
            let _ = handle.update(cx, |_, window, cx| sync(window, cx));
        });
    });
    cx.global_mut::<MenuBar>().pending = Some(task);
}

/// Whether AppKit is delivering a key press right now. A key nothing in the window handled is
/// offered to the menu bar, which then performs the item whose shortcut it is: that arrives as a
/// chosen item, after the keystroke's own dispatch is over, and only the current event tells it
/// from a click.
#[cfg(target_os = "macos")]
fn chosen_by_key_press() -> bool {
    let Some(main_thread) = objc2::MainThreadMarker::new() else { return false };
    objc2_app_kit::NSApplication::sharedApplication(main_thread)
        .currentEvent()
        .is_some_and(|event| event.r#type() == objc2_app_kit::NSEventType::KeyDown)
}

#[cfg(not(target_os = "macos"))]
fn chosen_by_key_press() -> bool {
    false
}

/// Runs a chosen item in the focused window.
fn run(command: &MenuCommand, cx: &mut App) {
    // A key that reaches an item's shortcut was declined by the keymap, which is the only
    // dispatcher of shortcuts: the key goes on to whatever has the keyboard.
    let native = role_fires_natively(&command.action) || fires_natively(entries(), &command.action);
    if (input::key_in_flight(cx) || chosen_by_key_press()) && !native {
        cx.propagate();
        return;
    }
    match command.action.as_str() {
        "" => return,
        "native.hide" => return cx.hide(),
        "native.hideOthers" => return cx.hide_other_apps(),
        "native.showAll" => return cx.unhide_other_apps(),
        "native.bringAllToFront" => return cx.activate(false),
        _ => {}
    }
    let Some(window) = cx.active_window() else { return };
    let command = command.clone();
    // After this dispatch: it may be the window's own, and the window is then not free to update.
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| match command.action.as_str() {
            "native.minimize" => window.minimize_window(),
            "native.zoom" => window.zoom_window(),
            action => {
                let arg = command.arg.clone().map(serde_json::Value::String);
                execute_action_from(
                    ActionSource::Menu,
                    role_action(action).unwrap_or(action),
                    arg.as_ref(),
                    window,
                    cx,
                );
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::ActionMeta;

    fn find<'a>(nodes: &'a [MenuNode], wanted: &str) -> Option<&'a MenuNode> {
        nodes.iter().find_map(|node| match node {
            MenuNode::Submenu { id, children, .. } if id != wanted => find(children, wanted),
            MenuNode::Submenu { .. } => Some(node),
            MenuNode::Item { action, .. } if action == wanted => Some(node),
            _ => None,
        })
    }

    #[test]
    fn shows_only_bindings_a_menu_can_show() {
        // Shown in the keymap's canonical form.
        assert_eq!(menu_accelerator("meta+shift+o"), Some(normalize_binding("Meta+Shift+O")));
        assert_eq!(menu_accelerator("Meta+F12").as_deref(), Some("Meta+F12"));
        assert_eq!(menu_accelerator("Meta+Alt+Left"), Some(normalize_binding("Meta+Alt+Left")));
        assert_eq!(menu_accelerator("Meta+/").as_deref(), Some("Meta+/"));
        // Chords, double modifiers and mouse buttons have no key equivalent.
        assert_eq!(menu_accelerator("Meta+K Meta+S"), None);
        assert_eq!(menu_accelerator("Shift Shift"), None);
        assert_eq!(menu_accelerator("Meta+Click"), None);
    }

    #[test]
    fn spells_strokes_as_gpui_does() {
        assert_eq!(gpui_keystroke("Meta+Shift+O").as_deref(), Some("cmd-shift-o"));
        assert_eq!(gpui_keystroke("Ctrl+Alt+PageDown").as_deref(), Some("ctrl-alt-pagedown"));
        assert_eq!(gpui_keystroke("Meta+`").as_deref(), Some("cmd-`"));
        assert_eq!(gpui_keystroke("Meta+NumpadAdd"), None);
    }

    #[test]
    fn the_bar_is_the_table_in_the_windows_state() {
        let entries = entries_for(Os::Mac);
        let state = MenuState {
            labels: Rc::new(HashMap::from([
                ("menu.view".to_owned(), "Ansicht".to_owned()),
                ("menu.native.undo".to_owned(), "Widerrufen".to_owned()),
            ])),
            accelerators: HashMap::from([
                ("navigate.gotoFile".to_owned(), "Meta+Shift+O".to_owned()),
                ("toolwindow.project".to_owned(), "Meta+1".to_owned()),
            ]),
            dynamic: HashMap::from([(
                DYNAMIC_TOOL_WINDOWS.to_owned(),
                vec![DynamicItem {
                    action: "toolwindow.project".to_owned(),
                    arg: None,
                    label: "Project".to_owned(),
                    checked: None,
                }],
            )]),
            checked: HashSet::from(["ide.toggleMode".to_owned(), "navigate.gotoFile".to_owned()]),
            disabled: HashSet::from(["navigate.gotoFile".to_owned()]),
            recent: vec![RecentItem { path: "/work/shop".to_owned(), label: "shop  —  ~/work/shop".to_owned() }],
        };
        let bar = model(&entries, &state, Os::Mac);
        assert!(matches!(find(&bar, "view"), Some(MenuNode::Submenu { label, .. }) if label == "Ansicht"));
        assert!(bar.iter().all(|node| matches!(node, MenuNode::Submenu { .. })), "the bar is made of menus");

        // An action shows its first binding and is greyed out when it cannot run. Being "on"
        // shows only on the items the table draws with a check mark.
        let Some(MenuNode::Item { shortcut, native_shortcut, enabled, checked, .. }) = find(&bar, "navigate.gotoFile")
        else {
            panic!("Go to File is on the bar")
        };
        assert_eq!(
            (shortcut.as_deref(), *native_shortcut, *enabled, *checked),
            (Some("Meta+Shift+O"), false, false, false)
        );
        // Quit's shortcut is the table's own.
        let Some(MenuNode::Item { shortcut, native_shortcut, enabled, .. }) = find(&bar, "app.quit") else {
            panic!("Quit is on the bar")
        };
        assert_eq!((shortcut.as_deref(), *native_shortcut, *enabled), (Some("Meta+Q"), true, true));
        // The window's tool windows and the recent projects are listed in their places.
        assert!(matches!(find(&bar, "toolwindow.project"), Some(MenuNode::Item { label, shortcut, .. })
            if label == "Project" && shortcut.as_deref() == Some("Meta+1")));
        assert!(matches!(find(&bar, OPEN_RECENT_ACTION), Some(MenuNode::Item { arg, label, .. })
            if arg.as_deref() == Some("/work/shop") && label.starts_with("shop")));
        // The roles of the OS carry the label of the language.
        fn roles(nodes: &[MenuNode], out: &mut Vec<(Native, String)>) {
            for node in nodes {
                match node {
                    MenuNode::Submenu { children, .. } => roles(children, out),
                    MenuNode::Native { role, label } => out.push((*role, label.clone())),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        roles(&bar, &mut found);
        assert!(found.contains(&(Native::Undo, "Widerrufen".to_owned())));
        assert!(found.contains(&(Native::Services, "Services".to_owned())));
        // No updater: its item is not offered.
        assert!(find(&bar, CHECK_FOR_UPDATES).is_none());

        // Without recent projects the list says so, greyed out.
        let none = model(&entries, &MenuState::default(), Os::Mac);
        assert!(find(&none, OPEN_RECENT_ACTION).is_none());
        fn placeholders(nodes: &[MenuNode]) -> usize {
            nodes
                .iter()
                .map(|node| match node {
                    MenuNode::Submenu { children, .. } => placeholders(children),
                    MenuNode::Placeholder { .. } => 1,
                    _ => 0,
                })
                .sum()
        }
        assert_eq!(placeholders(&none), 1);
    }

    #[gpui_kit::test]
    fn every_shortcut_of_the_bar_is_one_gpui_can_bind(cx: &mut gpui_kit::TestAppContext) {
        let os = Os::Mac;
        let entries = entries_for(os);
        let keymap = crate::platform::keymap::KeymapState::new(crate::platform::keymap::default_preset(true));
        let accelerators = action_ids(&entries)
            .into_iter()
            .filter_map(|id| Some((id.to_owned(), menu_accelerator(keymap.primary_key_binding(id)?)?)))
            .collect();
        let state = MenuState { accelerators, ..MenuState::default() };
        let mut shown = Vec::new();
        shortcuts(&model(&entries, &state, os), &mut shown);
        assert!(shown.len() > 50, "most of the bar's actions have a shortcut: {}", shown.len());
        let unbindable: Vec<(String, String)> = cx.update(|cx| {
            shown
                .into_iter()
                .filter(|(stroke, action)| !stroke.contains("Numpad") && binding(stroke, action.clone(), cx).is_none())
                .collect()
        });
        assert_eq!(unbindable, Vec::<(String, String)>::new());
    }

    #[test]
    fn a_registered_action_that_cannot_run_is_disabled() {
        let meta = ActionMeta::new("navigate.gotoFile", "Go to File…").when("projectOpen");
        let mut context = crate::platform::context::ContextKeys::new();
        assert!(!is_action_enabled(&meta, &context));
        context.set("projectOpen", true);
        assert!(is_action_enabled(&meta, &context));
        assert!(!item_enabled("navigate.gotoFile", &HashSet::from(["navigate.gotoFile".to_owned()])));
        assert!(item_enabled("dyn:toolwindow.project", &HashSet::new()));
    }
}
