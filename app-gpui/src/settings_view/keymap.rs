//! Settings › Keymap: the keymap selector with its gear menu (Duplicate… / Rename… / Restore
//! Defaults / Delete), the search by action name or by a recorded shortcut, and the action tree
//! grouped as IntelliJ IDEA groups it. A row's menu (double-click, Enter, right-click) adds a
//! keyboard or mouse shortcut, removes one or resets the row; the Keyboard Shortcut dialog
//! records one or two strokes and lists the actions they are already assigned to.
//!
//! Every edit is staged in the Settings dialog's model, in the global scope: `keymap` (the
//! selected keymap), `keymapOverrides` (the edits of a bundled keymap) and `customKeymaps` (the
//! user keymaps, which hold their own edits). Nothing is written before Apply.

use super::controls::{button, check_box, tx};
use crate::platform::actions::ActionRegistry;
use crate::platform::collate;
use crate::platform::dispatcher::{mouse_stroke, stroke};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input::{self, button_index, key_name};
use crate::platform::keymap::{
    Keymap, PRESET_TITLES, default_preset, format_binding, is_mac_host, normalize_binding, preset,
};
use crate::settings_model::assign_shortcut::{CUSTOM_KEYMAPS_KEY, KEYMAP_OVERRIDES_KEY, KeymapSettings};
use crate::settings_model::keymap_groups::{group_icon, group_path};
use crate::settings_model::keymap_model::{
    Conflict, ConflictKind, CustomKeymaps, KeymapAction, KeymapDraft, active_draft, add_binding, copy_name,
    delete_keymap, duplicate_keymap, find_conflicts, is_modified, keymap_name, remove_binding, remove_conflicts,
    rename_keymap, reset_bindings, resolve_bindings, switch_preset, validate_keymap_name, with_custom_overrides,
};
use crate::settings_model::keymap_page::{KeymapNode, build_keymap_tree, group_keys, keymap_search_labels};
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::theme;
use crate::ui::popup_menu::{Choice, PopupMenu, PopupMenuEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton,
    MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful, Subscription,
    UniformListScrollHandle, Window, WindowId, deferred, div, point, prelude::*, px, uniform_list,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// The id of the page in the Settings tree; edits are staged for it.
pub const KEYMAP_PAGE: &str = "keymap";
/// The setting that names the selected keymap.
pub const KEYMAP_KEY: &str = "keymap";
/// Strokes pressed within this time of each other in the shortcut search make one chord.
const CHORD: Duration = Duration::from_millis(1200);
/// The indent of a tree level.
const INDENT: f32 = 16.0;

/// The actions of `registry` as the keymap lists them, titled in the language of `i18n`.
pub fn keymap_actions(registry: &ActionRegistry, i18n: &I18n) -> Vec<KeymapAction> {
    registry.all().map(|action| KeymapAction::of(&action.meta, action.meta.localized(i18n))).collect()
}

/// The registered actions of the running app, in its language.
pub fn registered_actions(cx: &App) -> Vec<KeymapAction> {
    let Some(registry) = cx.try_global::<ActionRegistry>() else { return Vec::new() };
    match cx.try_global::<Localization>() {
        Some(localization) => keymap_actions(registry, &localization.0),
        None => keymap_actions(registry, &I18n::default()),
    }
}

/// What a menu of the page does when a row is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapCommand {
    SelectKeymap(String),
    Duplicate,
    Rename,
    RestoreDefaults,
    Delete,
    AddKeyboardShortcut(String),
    AddMouseShortcut(String),
    RemoveShortcut { action: String, binding: String },
    ResetShortcuts(String),
}

/// A row of the action tree as it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapRow {
    pub depth: usize,
    /// The group's key (`grp:` and its path) or the action's id.
    pub key: String,
    pub label: String,
    /// The group's path; `None` for an action.
    pub group: Option<Vec<String>>,
    pub expanded: bool,
    pub bindings: Vec<String>,
    pub modified: bool,
    /// Byte offsets of the title's characters the name search matched.
    pub hits: Option<Vec<usize>>,
}

impl KeymapRow {
    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }
}

/// The selected keymap as the staged settings say: its id, whether it is a user keymap, the
/// user keymaps and the draft being edited.
struct Selection {
    id: String,
    custom: bool,
    customs: CustomKeymaps,
    draft: KeymapDraft,
}

/// The Keyboard Shortcut (or Mouse Shortcut) dialog of one action.
struct Recording {
    action_id: String,
    title: String,
    mouse: bool,
    first: String,
    second: String,
    use_second: bool,
    /// OK was pressed with conflicts: the dialog asks whether to remove the other assignments.
    asking: bool,
}

impl Recording {
    /// The binding recorded so far; empty until it is complete.
    fn binding(&self) -> String {
        if self.mouse || !self.use_second {
            self.first.clone()
        } else if self.first.is_empty() || self.second.is_empty() {
            String::new()
        } else {
            format!("{} {}", self.first, self.second)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameMode {
    Duplicate,
    Rename,
}

struct NameDialog {
    mode: NameMode,
    field: Entity<TextField>,
    _events: Subscription,
}

/// What covers the page.
enum Overlay {
    Record(Recording),
    Name(NameDialog),
    ConfirmDelete,
}

struct OpenMenu {
    menu: Entity<PopupMenu<KeymapCommand>>,
    position: Point<Pixels>,
    _events: Subscription,
}

pub struct KeymapView {
    model: Entity<SettingsDialog>,
    actions: Vec<KeymapAction>,
    /// The default shortcuts plugins declare for their actions.
    extensions: Keymap,
    focus: FocusHandle,
    search: Entity<TextField>,
    query: String,
    /// The Settings search last seen: one that names an action filters the tree too.
    seen_settings_query: Option<String>,
    by_shortcut: bool,
    shortcut: String,
    shortcut_at: Option<Instant>,
    filter_focus: FocusHandle,
    first_focus: FocusHandle,
    second_focus: FocusHandle,
    pad_focus: FocusHandle,
    ask_focus: FocusHandle,
    confirm_focus: FocusHandle,
    collapsed: BTreeSet<String>,
    lead: Option<String>,
    overlay: Option<Overlay>,
    menu: Option<OpenMenu>,
    scroll: UniformListScrollHandle,
    claims: Vec<u64>,
    /// Shortcut dispatch is paused while a recorder has the keyboard, in this window.
    suspended: Vec<WindowId>,
    _subscriptions: Vec<Subscription>,
}

/// What the view does with a key pressed while one of its parts has the keyboard; true when the
/// key is taken.
type KeyHandler = fn(&mut KeymapView, &Keystroke, &mut Window, &mut Context<KeymapView>) -> bool;

/// The canonical stroke of `keystroke`; `None` for a modifier pressed alone.
fn recorded_stroke(keystroke: &Keystroke) -> Option<String> {
    let key = keystroke.key.as_str();
    if matches!(key, "" | "shift" | "control" | "alt" | "platform" | "cmd" | "function" | "fn" | "capslock") {
        return None;
    }
    Some(normalize_binding(&stroke(&key_name(key), input::mods(&keystroke.modifiers))))
}

/// `value` as a settings value. Keymaps hold only strings and lists of strings, which always
/// serialize.
fn json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `parts` joined around `params`, translated.
fn built<S: AsRef<str>>(cx: &mut App, parts: &[&str], params: &[S]) -> String {
    let params: Vec<Param> = params.iter().map(|param| Param::from(param.as_ref())).collect();
    cx.default_global::<Localization>().0.tx_built(parts, &params)
}

/// `text` cut into runs, each marked when its bytes are among `hits`.
fn hit_segments(text: &str, hits: &[usize]) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for (at, c) in text.char_indices() {
        let hit = hits.contains(&at);
        match out.last_mut() {
            Some((run, marked)) if *marked == hit => run.push(c),
            _ => out.push((c.to_string(), hit)),
        }
    }
    out
}

fn flatten_rows(nodes: &[KeymapNode], depth: usize, open: &dyn Fn(&str) -> bool, out: &mut Vec<KeymapRow>) {
    for node in nodes {
        match node {
            KeymapNode::Group(group) => {
                let expanded = open(&group.key);
                out.push(KeymapRow {
                    depth,
                    key: group.key.clone(),
                    label: group.name.clone(),
                    group: Some(group.path.clone()),
                    expanded,
                    bindings: Vec::new(),
                    modified: false,
                    hits: None,
                });
                if expanded {
                    flatten_rows(&group.children, depth + 1, open, out);
                }
            }
            KeymapNode::Action(action) => out.push(KeymapRow {
                depth,
                key: action.action.id.clone(),
                label: action.action.title.clone(),
                group: None,
                expanded: false,
                bindings: action.bindings.clone(),
                modified: action.modified,
                hits: action.hits.clone(),
            }),
        }
    }
}

/// A shortcut drawn as a key cap.
fn key_cap(cx: &App, binding: &str) -> Div {
    div()
        .flex_none()
        .h(px(18.0))
        .px(px(5.0))
        .flex()
        .items_center()
        .rounded(px(3.0))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-field"))
        .text_size(theme::length(cx, "fs-small"))
        .text_color(theme::color(cx, "fg-muted"))
        .whitespace_nowrap()
        .child(SharedString::from(format_binding(binding, is_mac_host())))
}

/// A square button showing `icon`; faint when it cannot be used, filled when `pressed`.
fn icon_button(cx: &App, id: &'static str, icon: &str, enabled: bool, pressed: bool) -> Stateful<Div> {
    let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex_none()
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A field that shows a recorded binding, or `placeholder` while there is none.
fn recorder(
    cx: &App,
    id: &'static str,
    focus: &FocusHandle,
    focused: bool,
    value: &str,
    placeholder: &str,
) -> Stateful<Div> {
    let empty = value.is_empty();
    let text = if empty { tx(cx, placeholder) } else { format_binding(value, is_mac_host()) };
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .track_focus(focus)
        .flex_1()
        .min_w(px(160.0))
        .h(theme::length(cx, "h-control"))
        .flex()
        .items_center()
        .px(px(6.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if focused { "accent" } else { "border-strong" }))
        .bg(theme::color(cx, "bg-input"))
        .text_color(theme::color(cx, if empty { "fg-subtle" } else { "fg" }))
        .child(SharedString::from(text))
}

/// A dialog panel over the page: its title, its body and its buttons.
fn panel(cx: &App, id: &'static str, title: &str, body: Vec<AnyElement>, buttons: Vec<AnyElement>) -> Stateful<Div> {
    let panel = div()
        .w(px(440.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .p(px(16.0))
        .rounded(theme::length(cx, "radius-popup"))
        .border_1()
        .border_color(theme::color(cx, "border-popup"))
        .bg(theme::color(cx, "bg-dialog"))
        .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title.to_owned())))
        .children(body)
        .child(div().flex().justify_end().gap(px(8.0)).children(buttons));
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .child(panel)
}

/// The conflicts of a binding: each action with its place in the tree and the binding it has.
fn conflict_list(cx: &App, conflicts: &[Conflict]) -> Div {
    let muted = theme::color(cx, "fg-muted");
    div().flex().flex_col().gap(px(4.0)).children(conflicts.iter().map(|conflict| {
        let place = group_path(&conflict.action_id, conflict.category.as_deref()).join(" | ");
        let prefix = (conflict.kind == ConflictKind::Prefix)
            .then(|| div().text_color(muted).child(SharedString::from(tx(cx, "chord prefix"))));
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(div().child(SharedString::from(conflict.title.clone())))
            .child(div().text_color(muted).child(SharedString::from(format!("({place})"))))
            .child(div().flex_1())
            .children(prefix)
            .child(key_cap(cx, &conflict.binding))
    }))
}

impl KeymapView {
    /// The page over the dialog's `model`, listing `actions`; `extensions` are the default
    /// shortcuts plugins declare.
    pub fn new(
        model: Entity<SettingsDialog>,
        actions: Vec<KeymapAction>,
        extensions: Keymap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = tx(cx, "Search actions");
        let search = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let focus = cx.focus_handle();
        let [filter_focus, first_focus, second_focus, pad_focus, ask_focus, confirm_focus] =
            std::array::from_fn(|_| cx.focus_handle());
        let mut subscriptions = vec![
            cx.observe(&model, |view, _, cx| {
                view.sync_settings_query(cx);
                cx.notify();
            }),
            cx.subscribe(&search, |view, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    view.query = field.read(cx).text().to_owned();
                    cx.notify();
                }
                TextFieldEvent::Cancelled if !view.query.is_empty() => view.set_query("", cx),
                TextFieldEvent::Submitted | TextFieldEvent::Cancelled => {}
            }),
        ];
        // Shortcuts do not fire while a recorder has the keyboard.
        for recording in [&filter_focus, &first_focus, &second_focus, &pad_focus] {
            subscriptions.push(cx.on_focus(recording, window, |view, window, cx| {
                let id = window.window_handle().window_id();
                input::suspend(id, cx);
                view.suspended.push(id);
            }));
            subscriptions.push(cx.on_blur(recording, window, |view, window, cx| {
                let id = window.window_handle().window_id();
                if let Some(at) = view.suspended.iter().position(|suspended| *suspended == id) {
                    view.suspended.remove(at);
                    input::resume(id, cx);
                }
            }));
        }
        let claims = vec![
            Self::claim(&focus, Self::tree_key, cx),
            Self::claim(&search.focus_handle(cx), Self::search_key, cx),
            Self::claim(&filter_focus, Self::filter_key, cx),
            Self::claim(&first_focus, Self::first_key, cx),
            Self::claim(&second_focus, Self::second_key, cx),
            Self::claim(&pad_focus, Self::pad_key, cx),
            Self::claim(&ask_focus, Self::ask_key, cx),
            Self::claim(&confirm_focus, Self::confirm_key, cx),
        ];
        cx.on_release(|view, cx| {
            for claim in &view.claims {
                input::release_keys(*claim, cx);
            }
            for window in view.suspended.drain(..) {
                input::resume(window, cx);
            }
        })
        .detach();
        let mut view = Self {
            model,
            actions,
            extensions,
            focus,
            search,
            query: String::new(),
            seen_settings_query: None,
            by_shortcut: false,
            shortcut: String::new(),
            shortcut_at: None,
            filter_focus,
            first_focus,
            second_focus,
            pad_focus,
            ask_focus,
            confirm_focus,
            collapsed: BTreeSet::new(),
            lead: None,
            overlay: None,
            menu: None,
            scroll: UniformListScrollHandle::new(),
            claims,
            suspended: Vec::new(),
            _subscriptions: subscriptions,
        };
        view.sync_settings_query(cx);
        view
    }

    /// Has `handler` see the keys pressed while `focus` is focused, before the keymap.
    fn claim(focus: &FocusHandle, handler: KeyHandler, cx: &mut Context<Self>) -> u64 {
        let view = cx.weak_entity();
        input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                view.update(cx, |view, cx| handler(view, keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        )
    }

    // -----------------------------------------------------------------------
    // State

    pub fn model(&self) -> &Entity<SettingsDialog> {
        &self.model
    }

    /// Replaces the listed actions (plugins came or went).
    pub fn set_actions(&mut self, actions: Vec<KeymapAction>, extensions: Keymap, cx: &mut Context<Self>) {
        self.actions = actions;
        self.extensions = extensions;
        cx.notify();
    }

    fn settings(&self, cx: &App) -> KeymapSettings {
        let dialog = self.model.read(cx);
        let value = |key: &str| dialog.staged_value(Scope::Global, key);
        KeymapSettings {
            keymap: value(KEYMAP_KEY).and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default(),
            keymap_overrides: value(KEYMAP_OVERRIDES_KEY)
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            custom_keymaps: value(CUSTOM_KEYMAPS_KEY)
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
        }
    }

    fn selection(&self, cx: &App) -> Selection {
        let KeymapSettings { keymap, keymap_overrides, custom_keymaps } = self.settings(cx);
        let custom = preset(&keymap).is_none() && custom_keymaps.contains_key(&keymap);
        let id = if custom || preset(&keymap).is_some() { keymap } else { default_preset(is_mac_host()).to_owned() };
        let mut draft = active_draft(&id, &keymap_overrides, &custom_keymaps);
        draft.extensions = self.extensions.clone();
        Selection { id, custom, customs: custom_keymaps, draft }
    }

    fn filtering(&self) -> bool {
        !self.query.trim().is_empty() || (self.by_shortcut && !self.shortcut.is_empty())
    }

    fn nodes(&self, draft: &KeymapDraft) -> Vec<KeymapNode> {
        let shortcut = if self.by_shortcut { self.shortcut.as_str() } else { "" };
        build_keymap_tree(&self.actions, draft, self.query.trim(), shortcut)
    }

    /// The rows of the action tree as they show: every group is open while a search filters it.
    pub fn rows(&self, cx: &App) -> Vec<KeymapRow> {
        let nodes = self.nodes(&self.selection(cx).draft);
        let filtering = self.filtering();
        let open = |key: &str| filtering || !self.collapsed.contains(key);
        let mut out = Vec::new();
        flatten_rows(&nodes, 0, &open, &mut out);
        out
    }

    /// The shortcuts of action `id` in the keymap being edited.
    pub fn shortcuts_of(&self, id: &str, cx: &App) -> Vec<String> {
        resolve_bindings(&self.selection(cx).draft, id)
    }

    /// The selected keymap's id.
    pub fn selected_keymap(&self, cx: &App) -> String {
        self.selection(cx).id
    }

    /// The keymaps to choose from: the bundled ones, then the user's by name; (id, name).
    pub fn keymap_choices(&self, cx: &App) -> Vec<(String, String)> {
        let customs = self.selection(cx).customs;
        let mut own: Vec<(&String, &String)> = customs.iter().map(|(id, keymap)| (id, &keymap.name)).collect();
        own.sort_by(|x, y| collate::compare(x.1, y.1));
        let bundled = PRESET_TITLES.iter().map(|(id, title)| ((*id).to_owned(), (*title).to_owned()));
        bundled.chain(own.into_iter().map(|(id, name)| (id.clone(), name.clone()))).collect()
    }

    /// How many actions the selected keymap changes.
    pub fn modified_count(&self, cx: &App) -> usize {
        self.selection(cx).draft.overrides.len()
    }

    /// The titles of the listed actions: what the Settings search finds the page by.
    pub fn search_labels(&self, cx: &App) -> Vec<String> {
        keymap_search_labels(&self.actions, &self.selection(cx).draft)
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search
    }

    /// The shortcut the tree is filtered by, when the search is by shortcut.
    pub fn shortcut_filter(&self) -> Option<&str> {
        self.by_shortcut.then_some(self.shortcut.as_str())
    }

    /// The selected row's key.
    pub fn lead(&self) -> Option<&str> {
        self.lead.as_deref()
    }

    /// The action whose shortcut is being recorded, and whether it is a mouse shortcut.
    pub fn recording(&self) -> Option<(&str, bool)> {
        match &self.overlay {
            Some(Overlay::Record(recording)) => Some((recording.action_id.as_str(), recording.mouse)),
            _ => None,
        }
    }

    /// The binding recorded so far; empty while it is incomplete.
    pub fn recorded_binding(&self) -> Option<String> {
        match &self.overlay {
            Some(Overlay::Record(recording)) => Some(recording.binding()),
            _ => None,
        }
    }

    /// Whether the recording dialog asks to remove the other assignments of the binding.
    pub fn asks_to_remove_conflicts(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Record(recording)) if recording.asking)
    }

    /// The other actions the recorded binding is assigned to.
    pub fn conflicts(&self, cx: &App) -> Vec<Conflict> {
        let Some(Overlay::Record(recording)) = &self.overlay else { return Vec::new() };
        let binding = recording.binding();
        if binding.is_empty() {
            return Vec::new();
        }
        find_conflicts(&self.selection(cx).draft, &binding, &recording.action_id, &self.actions)
    }

    /// The recorded binding is one the action has already.
    fn already_bound(&self, cx: &App) -> bool {
        let Some(Overlay::Record(recording)) = &self.overlay else { return false };
        let binding = recording.binding();
        !binding.is_empty()
            && resolve_bindings(&self.selection(cx).draft, &recording.action_id).contains(&normalize_binding(&binding))
    }

    /// The text of the keymap name dialog, when it is open.
    pub fn name_dialog_text(&self, cx: &App) -> Option<String> {
        match &self.overlay {
            Some(Overlay::Name(dialog)) => Some(dialog.field.read(cx).text().to_owned()),
            _ => None,
        }
    }

    /// Why the name typed in the name dialog cannot be taken.
    fn name_error(&self, cx: &App) -> Option<String> {
        let Some(Overlay::Name(dialog)) = &self.overlay else { return None };
        let selection = self.selection(cx);
        let own = (dialog.mode == NameMode::Rename).then_some(selection.id.as_str());
        validate_keymap_name(dialog.field.read(cx).text(), &selection.customs, own)
    }

    pub fn confirms_delete(&self) -> bool {
        matches!(self.overlay, Some(Overlay::ConfirmDelete))
    }

    /// The rows of the open menu (`> ` before the selected one, `-` for a separator).
    pub fn menu_lines(&self, cx: &App) -> Option<Vec<String>> {
        let open = self.menu.as_ref()?;
        open.menu.read(cx).lines().into_iter().next()
    }

    /// Where the open menu has its corner.
    pub fn menu_position(&self) -> Option<Point<Pixels>> {
        self.menu.as_ref().map(|open| open.position)
    }

    // -----------------------------------------------------------------------
    // Staging

    fn stage(&self, key: &str, value: Value, cx: &mut Context<Self>) {
        self.model.update(cx, |dialog, cx| {
            dialog.stage(KEYMAP_PAGE, Scope::Global, key, value);
            cx.notify();
        });
    }

    /// Stages `overrides` as the edits of the selected keymap.
    fn change(&self, overrides: Keymap, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        if selection.custom {
            let customs = with_custom_overrides(&selection.customs, &selection.id, overrides);
            self.stage(CUSTOM_KEYMAPS_KEY, json(&customs), cx);
        } else {
            self.stage(KEYMAP_OVERRIDES_KEY, json(&overrides), cx);
        }
    }

    /// Selects keymap `id`. Going from one bundled keymap to another carries the edits over,
    /// re-based on the new keymap.
    pub fn select_keymap(&mut self, id: &str, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        if id == selection.id {
            return;
        }
        if preset(id).is_some() && !selection.custom {
            let next = switch_preset(&selection.draft, id);
            self.stage(KEYMAP_KEY, Value::String(next.preset), cx);
            self.stage(KEYMAP_OVERRIDES_KEY, json(&next.overrides), cx);
        } else {
            self.stage(KEYMAP_KEY, Value::String(id.to_owned()), cx);
        }
        cx.notify();
    }

    fn sync_settings_query(&mut self, cx: &mut Context<Self>) {
        let query = self.model.read(cx).query.trim().to_owned();
        if self.seen_settings_query.as_deref() == Some(query.as_str()) {
            return;
        }
        self.seen_settings_query = Some(query.clone());
        let lowered = query.to_lowercase();
        if lowered.is_empty() {
            self.set_query("", cx);
        } else if self.search_labels(cx).iter().any(|label| label.to_lowercase().contains(&lowered)) {
            self.set_query(&query, cx);
        }
    }

    /// Searches the actions by `query`.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned();
        self.search.update(cx, |field, cx| field.set_text(query, cx));
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // The tree

    fn toggle_group(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.filtering() {
            return;
        }
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let nodes = self.nodes(&self.selection(cx).draft);
        self.collapsed = group_keys(&nodes).into_iter().collect();
        cx.notify();
    }

    fn select_row(&mut self, key: &str, cx: &mut Context<Self>) {
        self.lead = Some(key.to_owned());
        if let Some(index) = self.rows(cx).iter().position(|row| row.key == key) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Moves the selection `step` rows, within the tree; without a selection the first row is
    /// selected.
    fn step_lead(&mut self, step: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(last) = rows.len().checked_sub(1) else { return };
        let current = self.lead.as_ref().and_then(|lead| rows.iter().position(|row| &row.key == lead));
        let next = current.map_or(0, |at| at.saturating_add_signed(step).min(last));
        self.select_row(&rows[next].key, cx);
    }

    /// Selects the first row, or with `last` the last one.
    fn select_end(&mut self, last: bool, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let row = if last { rows.last() } else { rows.first() };
        if let Some(key) = row.map(|row| row.key.clone()) {
            self.select_row(&key, cx);
        }
    }

    /// Where the menu of row `index` opens when the keyboard asks for it: below the row.
    fn row_menu_position(&self, index: usize, cx: &App) -> Point<Pixels> {
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let top = list.top() + state.base_handle.offset().y + theme::length(cx, "h-row") * (index + 1) as f32;
        point(list.left() + px(INDENT * 2.0), top)
    }

    /// Enter or a double-click on a row: a group opens or closes, an action shows its menu.
    fn activate_row(
        &mut self,
        key: &str,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.rows(cx);
        let Some(index) = rows.iter().position(|row| row.key == key) else { return };
        if rows[index].is_group() {
            self.toggle_group(key, cx);
            return;
        }
        let position = position.unwrap_or_else(|| self.row_menu_position(index, cx));
        self.show_row_menu(key, position, window, cx);
    }

    fn tree_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() || self.overlay.is_some() {
            return false;
        }
        let rows = self.rows(cx);
        let current = self.lead.as_ref().and_then(|lead| rows.iter().position(|row| &row.key == lead));
        let row = current.map(|at| &rows[at]);
        match (keystroke.key.as_str(), row) {
            ("down", _) => self.step_lead(1, cx),
            ("up", _) => self.step_lead(-1, cx),
            ("home", _) => self.select_end(false, cx),
            ("end", _) => self.select_end(true, cx),
            ("right", Some(row)) if row.is_group() && !row.expanded => self.toggle_group(&row.key, cx),
            ("right", Some(row)) if row.is_group() => self.step_lead(1, cx),
            ("left", Some(row)) if row.is_group() && row.expanded => self.toggle_group(&row.key, cx),
            ("left", Some(row)) => {
                let parent = current.and_then(|at| rows[..at].iter().rev().find(|above| above.depth < row.depth));
                if let Some(parent) = parent {
                    self.select_row(&parent.key, cx);
                }
            }
            ("enter", Some(row)) => self.activate_row(&row.key.clone(), None, window, cx),
            _ => return false,
        }
        true
    }

    /// Up and Down in the search move through the tree; the keyboard stays in the search.
    fn search_key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "down" => self.step_lead(1, cx),
            "up" => self.step_lead(-1, cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------
    // Search by shortcut

    /// Switches between the search by name and the search by shortcut.
    pub fn toggle_search_by_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.by_shortcut = !self.by_shortcut;
        self.shortcut.clear();
        self.shortcut_at = None;
        let target = if self.by_shortcut { self.filter_focus.clone() } else { self.search.focus_handle(cx) };
        window.focus(&target, cx);
        cx.notify();
    }

    pub fn clear_filtering(&mut self, cx: &mut Context<Self>) {
        self.shortcut.clear();
        self.set_query("", cx);
    }

    /// A stroke pressed in the shortcut search: it replaces the shortcut, or completes a chord
    /// when it follows the first stroke closely.
    fn filter_key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(stroke) = recorded_stroke(keystroke) else { return false };
        let now = Instant::now();
        let chord = !self.shortcut.is_empty()
            && !self.shortcut.contains(' ')
            && self.shortcut_at.is_some_and(|at| now.duration_since(at) < CHORD);
        self.shortcut_at = Some(now);
        self.shortcut = if chord { format!("{} {stroke}", self.shortcut) } else { stroke };
        cx.notify();
        true
    }

    // -----------------------------------------------------------------------
    // Menus

    fn show_menu(
        &mut self,
        items: Vec<MenuItem<KeymapCommand>>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| PopupMenu::new(&items, position, window, cx));
        let events = cx.subscribe_in(&menu, window, |view, _, event: &PopupMenuEvent<KeymapCommand>, window, cx| {
            view.menu = None;
            match event {
                PopupMenuEvent::Chosen(Choice::Command(command)) => view.run(command.clone(), window, cx),
                PopupMenuEvent::Chosen(Choice::Action { .. }) | PopupMenuEvent::Dismissed => {
                    window.focus(&view.focus, cx);
                }
            }
            cx.notify();
        });
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(OpenMenu { menu, position, _events: events });
        cx.notify();
    }

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The menu of keymaps to choose from, the selected one checked.
    pub fn show_keymap_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selection(cx).id;
        let customs = self.selection(cx).customs;
        let items = self
            .keymap_choices(cx)
            .into_iter()
            .map(|(id, name)| {
                let based = customs.get(&id).map(|custom| {
                    let parent = keymap_name(&custom.parent, &CustomKeymaps::new());
                    built(cx, &["based on ", ""], &[&parent])
                });
                let item = MenuItem::item(name, KeymapCommand::SelectKeymap(id.clone()));
                let item = if id == selected { item.icon("check") } else { item };
                match based {
                    Some(based) => item.shortcut(based),
                    None => item,
                }
            })
            .collect();
        self.show_menu(items, position, window, cx);
    }

    /// The gear menu: Duplicate…, Rename… and Delete (user keymaps only), Restore Defaults.
    pub fn show_gear_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        let mut items = vec![
            MenuItem::item("Duplicate…", KeymapCommand::Duplicate).icon("copy"),
            MenuItem::entry("Rename…", selection.custom.then_some(KeymapCommand::Rename)).icon("edit"),
            MenuItem::entry(
                "Restore Defaults",
                (!selection.draft.overrides.is_empty()).then_some(KeymapCommand::RestoreDefaults),
            )
            .icon("undo"),
        ];
        // Bundled keymaps cannot be deleted.
        if selection.custom {
            items.push(MenuItem::Separator);
            items.push(MenuItem::item("Delete", KeymapCommand::Delete).icon("trash").danger());
        }
        self.show_menu(items, position, window, cx);
    }

    /// The menu of action `id`: add a keyboard or mouse shortcut, remove each, reset.
    pub fn show_row_menu(&mut self, id: &str, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.selection(cx).draft;
        let bindings = resolve_bindings(&draft, id);
        let mut items = vec![
            MenuItem::item("Add Keyboard Shortcut", KeymapCommand::AddKeyboardShortcut(id.to_owned())).icon("keyboard"),
            MenuItem::item("Add Mouse Shortcut", KeymapCommand::AddMouseShortcut(id.to_owned())),
            MenuItem::Separator,
        ];
        if bindings.is_empty() {
            items.push(MenuItem::entry("Remove", None));
        }
        for binding in bindings {
            let label = built(cx, &["Remove ", ""], &[&format_binding(&binding, is_mac_host())]);
            let command = KeymapCommand::RemoveShortcut { action: id.to_owned(), binding };
            items.push(MenuItem::item(label, command).icon("minus"));
        }
        items.push(MenuItem::Separator);
        let reset = is_modified(&draft, id).then(|| KeymapCommand::ResetShortcuts(id.to_owned()));
        items.push(MenuItem::entry("Reset Shortcuts", reset).icon("undo"));
        self.lead = Some(id.to_owned());
        self.show_menu(items, position, window, cx);
    }

    /// Carries out a menu's command.
    pub fn run(&mut self, command: KeymapCommand, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        match command {
            KeymapCommand::SelectKeymap(id) => self.select_keymap(&id, cx),
            KeymapCommand::Duplicate => {
                let name = copy_name(&keymap_name(&selection.id, &selection.customs), &selection.customs);
                self.open_name_dialog(NameMode::Duplicate, &name, window, cx);
                return;
            }
            KeymapCommand::Rename => {
                let name = keymap_name(&selection.id, &selection.customs);
                self.open_name_dialog(NameMode::Rename, &name, window, cx);
                return;
            }
            KeymapCommand::RestoreDefaults => self.change(Keymap::new(), cx),
            KeymapCommand::Delete => {
                self.overlay = Some(Overlay::ConfirmDelete);
                window.focus(&self.confirm_focus, cx);
                cx.notify();
                return;
            }
            KeymapCommand::AddKeyboardShortcut(id) => {
                self.start_recording(&id, false, window, cx);
                return;
            }
            KeymapCommand::AddMouseShortcut(id) => {
                self.start_recording(&id, true, window, cx);
                return;
            }
            KeymapCommand::RemoveShortcut { action, binding } => {
                self.change(remove_binding(&selection.draft, &action, &binding), cx);
            }
            KeymapCommand::ResetShortcuts(id) => self.change(reset_bindings(&selection.draft, &id), cx),
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Recording a shortcut

    /// Opens the Keyboard Shortcut (or, with `mouse`, the Mouse Shortcut) dialog of action `id`.
    pub fn start_recording(&mut self, id: &str, mouse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let action = self.actions.iter().find(|action| action.id == id);
        let title = action.map_or_else(|| id.to_owned(), |action| action.title.clone());
        self.overlay = Some(Overlay::Record(Recording {
            action_id: id.to_owned(),
            title,
            mouse,
            first: String::new(),
            second: String::new(),
            use_second: false,
            asking: false,
        }));
        let target = if mouse { self.pad_focus.clone() } else { self.first_focus.clone() };
        window.focus(&target, cx);
        cx.notify();
    }

    fn recording_mut(&mut self) -> Option<&mut Recording> {
        match &mut self.overlay {
            Some(Overlay::Record(recording)) => Some(recording),
            _ => None,
        }
    }

    /// Keys are recorded as pressed, Tab and Escape included: the buttons finish the dialog.
    fn first_key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(stroke) = recorded_stroke(keystroke) else { return false };
        let Some(recording) = self.recording_mut().filter(|recording| !recording.asking) else { return false };
        recording.first = stroke;
        cx.notify();
        true
    }

    fn second_key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(stroke) = recorded_stroke(keystroke) else { return false };
        let Some(recording) = self.recording_mut().filter(|recording| recording.use_second && !recording.asking) else {
            return false;
        };
        recording.second = stroke;
        cx.notify();
        true
    }

    /// The mouse pad takes no keys but Enter (OK) and Escape (Cancel).
    fn pad_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match keystroke.key.as_str() {
            "escape" => self.cancel_overlay(window, cx),
            "enter" => self.confirm_recording(window, cx),
            _ => return false,
        }
        true
    }

    /// A click on the mouse pad records its button and modifiers: a left click with Ctrl, Alt or
    /// Command held, or a side button.
    fn pad_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(stroke) = mouse_stroke(button_index(event.button), input::mods(&event.modifiers)) else { return };
        if let Some(recording) = self.recording_mut() {
            recording.first = stroke;
            cx.notify();
        }
    }

    fn toggle_second_stroke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recording) = self.recording_mut() else { return };
        recording.use_second = !recording.use_second;
        let second = recording.use_second;
        let target = if second { self.second_focus.clone() } else { self.first_focus.clone() };
        window.focus(&target, cx);
        cx.notify();
    }

    /// OK of the recording dialog: without conflicts the binding is added; with some, the dialog
    /// asks whether to remove the other assignments.
    pub fn confirm_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Record(recording)) = &self.overlay else { return };
        if recording.binding().is_empty() || self.already_bound(cx) {
            return;
        }
        if self.conflicts(cx).is_empty() {
            self.submit_recording(false, window, cx);
        } else if let Some(recording) = self.recording_mut() {
            recording.asking = true;
            window.focus(&self.ask_focus, cx);
            cx.notify();
        }
    }

    /// Adds the recorded binding; with `remove_others` the conflicting assignments lose it.
    pub fn submit_recording(&mut self, remove_others: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Record(recording)) = &self.overlay else { return };
        let (id, binding) = (recording.action_id.clone(), recording.binding());
        let conflicts = self.conflicts(cx);
        let draft = self.selection(cx).draft;
        let base = if remove_others {
            KeymapDraft { overrides: remove_conflicts(&draft, &conflicts), ..draft }
        } else {
            draft
        };
        self.change(add_binding(&base, &id, &binding), cx);
        self.overlay = None;
        self.lead = Some(id);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Enter removes the other assignments, Escape goes back to the recording.
    fn ask_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match keystroke.key.as_str() {
            "enter" => self.submit_recording(true, window, cx),
            "escape" => self.stop_asking(window, cx),
            _ => return false,
        }
        true
    }

    fn stop_asking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recording) = self.recording_mut() else { return };
        recording.asking = false;
        let mouse = recording.mouse;
        let target = if mouse { self.pad_focus.clone() } else { self.first_focus.clone() };
        window.focus(&target, cx);
        cx.notify();
    }

    /// Closes the dialog over the page without changing anything.
    pub fn cancel_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Keymap names and deletion

    fn open_name_dialog(&mut self, mode: NameMode, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(name, cx);
            field.select_all(cx);
            field
        });
        let events = cx.subscribe_in(&field, window, |view, _, event: &TextFieldEvent, window, cx| match event {
            TextFieldEvent::Changed => cx.notify(),
            TextFieldEvent::Submitted => view.submit_name(window, cx),
            TextFieldEvent::Cancelled => view.cancel_overlay(window, cx),
        });
        window.focus(&field.focus_handle(cx), cx);
        self.overlay = Some(Overlay::Name(NameDialog { mode, field, _events: events }));
        cx.notify();
    }

    /// OK of the name dialog: duplicates the selected keymap under the name and selects the copy,
    /// or renames the selected user keymap.
    pub fn submit_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_error(cx).is_some() {
            return;
        }
        let Some(Overlay::Name(dialog)) = &self.overlay else { return };
        let (mode, name) = (dialog.mode, dialog.field.read(cx).text().trim().to_owned());
        let selection = self.selection(cx);
        match mode {
            NameMode::Duplicate => {
                let (id, customs) = duplicate_keymap(&selection.draft, &selection.customs, &name);
                self.stage(CUSTOM_KEYMAPS_KEY, json(&customs), cx);
                self.stage(KEYMAP_KEY, Value::String(id), cx);
            }
            NameMode::Rename => {
                let customs = rename_keymap(&selection.customs, &selection.id, &name);
                self.stage(CUSTOM_KEYMAPS_KEY, json(&customs), cx);
            }
        }
        self.cancel_overlay(window, cx);
    }

    /// Deletes the selected user keymap and selects its parent.
    pub fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        if selection.custom {
            let (customs, parent) = delete_keymap(&selection.customs, &selection.id);
            self.stage(CUSTOM_KEYMAPS_KEY, json(&customs), cx);
            self.stage(KEYMAP_KEY, Value::String(parent), cx);
        }
        self.cancel_overlay(window, cx);
    }

    fn confirm_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match keystroke.key.as_str() {
            "enter" => self.delete_selected(window, cx),
            "escape" => self.cancel_overlay(window, cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------
    // Drawing

    fn render_header(&self, selection: &Selection, cx: &mut Context<Self>) -> Div {
        let name = keymap_name(&selection.id, &selection.customs);
        let count = selection.draft.overrides.len();
        let note = (count > 0).then(|| {
            let text = if count == 1 {
                built(cx, &["", " modified action"], &[&count.to_string()])
            } else {
                built(cx, &["", " modified actions"], &[&count.to_string()])
            };
            div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let choose = div()
            .id("keymap-preset")
            .debug_selector(|| "keymap-preset".to_owned())
            .flex_none()
            .min_w(px(220.0))
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-field"))
            .child(SharedString::from(name))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
            .on_click(cx.listener(|view, event: &ClickEvent, window, cx| {
                view.show_keymap_menu(event.position(), window, cx);
            }));
        let gear = icon_button(cx, "keymap-gear", "settings", true, false).on_click(
            cx.listener(|view, event: &ClickEvent, window, cx| view.show_gear_menu(event.position(), window, cx)),
        );
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().child(SharedString::from(tx(cx, "Keymap:"))))
            .child(choose)
            .child(gear)
            .child(div().flex_1())
            .children(note)
    }

    fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let lead_action = self.lead.as_ref().filter(|lead| self.actions.iter().any(|action| &action.id == *lead));
        let filtering = self.filtering();
        let search = if self.by_shortcut {
            let focused = self.filter_focus.is_focused(window);
            let field = recorder(
                cx,
                "keymap-shortcut-filter",
                &self.filter_focus,
                focused,
                &self.shortcut,
                "Press a shortcut to find actions",
            )
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| window.focus(&view.filter_focus, cx)));
            let clear = (!self.shortcut.is_empty()).then(|| {
                icon_button(cx, "keymap-shortcut-clear", "close", true, false).on_click(cx.listener(
                    |view, _: &ClickEvent, _, cx| {
                        view.shortcut.clear();
                        cx.notify();
                    },
                ))
            });
            div().flex_1().flex().items_center().gap(px(4.0)).child(field).children(clear).into_any_element()
        } else {
            div()
                .id("keymap-search")
                .debug_selector(|| "keymap-search".to_owned())
                .flex_1()
                .h(theme::length(cx, "h-control"))
                .flex()
                .items_center()
                .px(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .bg(theme::color(cx, "bg-input"))
                .child(self.search.clone())
                .into_any_element()
        };
        let by_shortcut = icon_button(cx, "keymap-by-shortcut", "keyboard", true, self.by_shortcut)
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_search_by_shortcut(window, cx)));
        let clear = icon_button(cx, "keymap-clear-filter", "filter", filtering, false)
            .when(filtering, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.clear_filtering(cx))));
        let expand = icon_button(cx, "keymap-expand-all", "expand-all", true, false)
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.expand_all(cx)));
        let collapse = icon_button(cx, "keymap-collapse-all", "collapse-all", true, false)
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.collapse_all(cx)));
        let add = icon_button(cx, "keymap-add-shortcut", "plus", lead_action.is_some(), false).when_some(
            lead_action.cloned(),
            |el, id| {
                el.on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    view.start_recording(&id, false, window, cx);
                }))
            },
        );
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(search)
            .child(by_shortcut)
            .child(clear)
            .child(div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(theme::color(cx, "border")))
            .child(expand)
            .child(collapse)
            .child(div().flex_1())
            .child(add)
    }

    fn render_tree(&self, rows: Vec<KeymapRow>, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-selected" });
        let (fg, accent) = (theme::color(cx, "fg-strong"), theme::color(cx, "accent"));
        let icon_color = theme::color(cx, "fg-icon");
        let (mark_bg, mark_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
        let lead = self.lead.clone();
        let empty = rows.is_empty().then(|| {
            let text = tx(cx, "Nothing to show");
            div().p(px(12.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let list = uniform_list(
            "keymap-actions",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let key = row.key.clone();
                        let selector = format!("keymap-row-{key}");
                        let leading = match &row.group {
                            Some(path) => {
                                let chevron = if row.expanded { "chevron-down" } else { "chevron-right" };
                                let toggled = key.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("keymap-toggle-{key}")))
                                            .size(px(16.0))
                                            .child(theme::icon(cx, chevron, 16.0, icon_color))
                                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                                view.toggle_group(&toggled, cx);
                                                cx.stop_propagation();
                                            })),
                                    )
                                    .child(theme::icon(cx, group_icon(path, row.expanded), 14.0, icon_color))
                            }
                            None => div().w(px(36.0)).flex_none(),
                        };
                        let title_color = if row.modified { accent } else { fg };
                        let segments = match &row.hits {
                            Some(hits) => hit_segments(&row.label, hits),
                            None => vec![(row.label.clone(), false)],
                        };
                        let runs = segments.into_iter().map(|(run, hit)| {
                            let run = div().child(SharedString::from(run));
                            if hit { run.bg(mark_bg).text_color(mark_fg).rounded(px(2.0)) } else { run }
                        });
                        let title = div().flex().text_color(title_color).children(runs);
                        let bindings = row.bindings.iter().map(|binding| key_cap(cx, binding));
                        let (clicked, right_clicked) = (key.clone(), key.clone());
                        let is_group = row.is_group();
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector)
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + INDENT * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .when(lead.as_deref() == Some(key.as_str()), |el| el.bg(selected_bg))
                            .child(leading)
                            .child(title)
                            .child(div().flex_1())
                            .child(div().flex().gap(px(4.0)).children(bindings))
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.lead = Some(clicked.clone());
                                if event.click_count() >= 2 {
                                    view.activate_row(&clicked, Some(event.position()), window, cx);
                                }
                                cx.notify();
                            }))
                            .when(!is_group, |el| {
                                el.on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                        view.show_row_menu(&right_clicked, event.position, window, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                            })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .id("keymap-tree")
            .debug_selector(|| "keymap-tree".to_owned())
            .flex_1()
            .min_h_0()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .track_focus(&self.focus)
            .child(list)
            .children(empty)
    }

    fn render_recording(&self, recording: &Recording, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let title = if recording.mouse {
            built(cx, &["Mouse Shortcut: ", ""], &[&recording.title])
        } else {
            built(cx, &["Keyboard Shortcut: ", ""], &[&recording.title])
        };
        let binding = recording.binding();
        let conflicts = self.conflicts(cx);
        let muted = theme::color(cx, "fg-subtle");
        if recording.asking {
            let warning =
                tx(cx, "The shortcut is already assigned to other actions. Do you want to remove other assignments?");
            let body = vec![
                div()
                    .id("keymap-conflict-ask")
                    .track_focus(&self.ask_focus)
                    .child(SharedString::from(warning))
                    .into_any_element(),
                conflict_list(cx, &conflicts).into_any_element(),
            ];
            let remove = div()
                .id("keymap-conflict-remove")
                .debug_selector(|| "keymap-conflict-remove".to_owned())
                .child(button(cx, &tx(cx, "Remove"), true, true))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.submit_recording(true, window, cx)));
            let leave = div()
                .id("keymap-conflict-leave")
                .debug_selector(|| "keymap-conflict-leave".to_owned())
                .child(button(cx, &tx(cx, "Leave"), false, true))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.submit_recording(false, window, cx)));
            let cancel = div()
                .id("keymap-conflict-cancel")
                .debug_selector(|| "keymap-conflict-cancel".to_owned())
                .child(button(cx, &tx(cx, "Cancel"), false, true))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.stop_asking(window, cx)));
            let buttons = vec![remove.into_any_element(), leave.into_any_element(), cancel.into_any_element()];
            return panel(cx, "keymap-record", &tx(cx, "Warning"), body, buttons).into_any_element();
        }
        let mut body: Vec<AnyElement> = Vec::new();
        if recording.mouse {
            let text = if recording.first.is_empty() {
                tx(cx, "Click here with Ctrl, Alt or Command held, or use a mouse side button")
            } else {
                format_binding(&recording.first, is_mac_host())
            };
            let pad = div()
                .id("keymap-mouse-pad")
                .debug_selector(|| "keymap-mouse-pad".to_owned())
                .track_focus(&self.pad_focus)
                .h(px(64.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .bg(theme::color(cx, "bg-input"))
                .text_color(if recording.first.is_empty() { muted } else { theme::color(cx, "fg") })
                .child(SharedString::from(text))
                .on_any_mouse_down(cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    window.focus(&view.pad_focus, cx);
                    view.pad_mouse_down(event, cx);
                    cx.stop_propagation();
                }));
            body.push(pad.into_any_element());
        } else {
            let first = recorder(
                cx,
                "keymap-first-stroke",
                &self.first_focus,
                self.first_focus.is_focused(window),
                &recording.first,
                "Press a shortcut",
            )
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| window.focus(&view.first_focus, cx)));
            let second_placeholder = if recording.use_second { "Press a shortcut" } else { "" };
            let second = recorder(
                cx,
                "keymap-second-stroke",
                &self.second_focus,
                self.second_focus.is_focused(window),
                &recording.second,
                second_placeholder,
            )
            .when(recording.use_second, |el| {
                el.on_click(cx.listener(|view, _: &ClickEvent, window, cx| window.focus(&view.second_focus, cx)))
            });
            let use_second = div()
                .id("keymap-use-second")
                .debug_selector(|| "keymap-use-second".to_owned())
                .w(px(130.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(check_box(cx, recording.use_second))
                .child(SharedString::from(tx(cx, "Second stroke:")))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_second_stroke(window, cx)));
            body.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().w(px(130.0)).child(SharedString::from(tx(cx, "First stroke:"))))
                    .child(first)
                    .into_any_element(),
            );
            body.push(div().flex().items_center().gap(px(8.0)).child(use_second).child(second).into_any_element());
            let hint = tx(cx, "Keys are recorded as pressed, including Tab and Esc. Use the buttons to finish.");
            body.push(
                div()
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(muted)
                    .child(SharedString::from(hint))
                    .into_any_element(),
            );
        }
        body.push(div().text_color(muted).child(SharedString::from(tx(cx, "Conflicts:"))).into_any_element());
        let already = self.already_bound(cx);
        let status = if already {
            Some("The action already has this shortcut")
        } else if binding.is_empty() {
            Some("Press a shortcut")
        } else if conflicts.is_empty() {
            Some("No conflicts")
        } else {
            None
        };
        match status {
            Some(status) => {
                let text = tx(cx, status);
                body.push(div().text_color(muted).child(SharedString::from(text)).into_any_element());
            }
            None => {
                let assigned = tx(cx, "is already assigned to:");
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(theme::icon(cx, "warning", 14.0, theme::color(cx, "warning")))
                        .child(key_cap(cx, &binding))
                        .child(SharedString::from(assigned))
                        .into_any_element(),
                );
                body.push(conflict_list(cx, &conflicts).into_any_element());
            }
        }
        let can_ok = !binding.is_empty() && !already;
        let ok = div()
            .id("keymap-record-ok")
            .debug_selector(|| "keymap-record-ok".to_owned())
            .child(button(cx, &tx(cx, "OK"), true, can_ok))
            .when(can_ok, |el| {
                el.on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.confirm_recording(window, cx)))
            });
        let cancel = div()
            .id("keymap-record-cancel")
            .debug_selector(|| "keymap-record-cancel".to_owned())
            .child(button(cx, &tx(cx, "Cancel"), false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.cancel_overlay(window, cx)));
        let buttons = vec![ok.into_any_element(), cancel.into_any_element()];
        panel(cx, "keymap-record", &title, body, buttons).into_any_element()
    }

    fn render_name_dialog(&self, dialog: &NameDialog, cx: &mut Context<Self>) -> AnyElement {
        let title = tx(cx, if dialog.mode == NameMode::Duplicate { "Duplicate Keymap" } else { "Rename Keymap" });
        let error = self.name_error(cx);
        let typed = !dialog.field.read(cx).text().trim().is_empty();
        let field = div()
            .id("keymap-name-field")
            .debug_selector(|| "keymap-name-field".to_owned())
            .flex_1()
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if error.is_some() { "danger" } else { "border-strong" }))
            .bg(theme::color(cx, "bg-input"))
            .child(dialog.field.clone());
        let mut body = vec![
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(SharedString::from(tx(cx, "Name:")))
                .child(field)
                .into_any_element(),
        ];
        if let Some(error) = error.as_ref().filter(|_| typed) {
            let text = tx(cx, error);
            body.push(div().text_color(theme::color(cx, "danger")).child(SharedString::from(text)).into_any_element());
        }
        let can_ok = error.is_none();
        let ok = div()
            .id("keymap-name-ok")
            .debug_selector(|| "keymap-name-ok".to_owned())
            .child(button(cx, &tx(cx, "OK"), true, can_ok))
            .when(can_ok, |el| {
                el.on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.submit_name(window, cx)))
            });
        let cancel = div()
            .id("keymap-name-cancel")
            .debug_selector(|| "keymap-name-cancel".to_owned())
            .child(button(cx, &tx(cx, "Cancel"), false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.cancel_overlay(window, cx)));
        let buttons = vec![ok.into_any_element(), cancel.into_any_element()];
        panel(cx, "keymap-name", &title, body, buttons).into_any_element()
    }

    fn render_confirm_delete(&self, selection: &Selection, cx: &mut Context<Self>) -> AnyElement {
        let name = keymap_name(&selection.id, &selection.customs);
        let message = built(cx, &["Delete keymap '", "'?"], &[&name]);
        let message =
            div().id("keymap-delete-message").track_focus(&self.confirm_focus).child(SharedString::from(message));
        let body = vec![message.into_any_element()];
        let delete = div()
            .id("keymap-delete-ok")
            .debug_selector(|| "keymap-delete-ok".to_owned())
            .child(button(cx, &tx(cx, "Delete"), true, true))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.delete_selected(window, cx)));
        let cancel = div()
            .id("keymap-delete-cancel")
            .debug_selector(|| "keymap-delete-cancel".to_owned())
            .child(button(cx, &tx(cx, "Cancel"), false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.cancel_overlay(window, cx)));
        let title = tx(cx, "Delete Keymap");
        let buttons = vec![delete.into_any_element(), cancel.into_any_element()];
        panel(cx, "keymap-delete", &title, body, buttons).into_any_element()
    }
}

impl Focusable for KeymapView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for KeymapView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.selection(cx);
        let rows = self.rows(cx);
        let header = self.render_header(&selection, cx);
        let toolbar = self.render_toolbar(window, cx);
        let tree = self.render_tree(rows, window, cx);
        let overlay = match &self.overlay {
            Some(Overlay::Record(recording)) => Some(self.render_recording(recording, window, cx)),
            Some(Overlay::Name(dialog)) => Some(self.render_name_dialog(dialog, cx)),
            Some(Overlay::ConfirmDelete) => Some(self.render_confirm_delete(&selection, cx)),
            None => None,
        };
        let menu = self.menu.as_ref().map(|open| deferred(open.menu.clone()).with_priority(2));
        div()
            .id("keymap-page")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, window, cx| view.close_menu(window, cx)),
            )
            .child(header)
            .child(toolbar)
            .child(tree)
            .children(overlay)
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::{default_settings, set_at};
    use gpui_kit::{Modifiers, MouseUpEvent, NavigationDirection, TestAppContext, VisualTestContext};
    use serde_json::json;

    type Opened<'a> = (Entity<KeymapView>, &'a mut VisualTestContext);

    /// The page in a window of its own, over settings that select the bundled macOS keymap.
    fn open(cx: &mut TestAppContext, actions: Vec<KeymapAction>) -> Opened<'_> {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let settings = set_at(&default_settings(), KEYMAP_KEY, json!("intellij-macos"));
            let model = cx.new(|_| {
                let mut dialog = SettingsDialog::default();
                dialog.open(Some(KEYMAP_PAGE), false, settings);
                dialog
            });
            KeymapView::new(model, actions, Keymap::new(), window, cx)
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn center(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center()
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let position = center(cx, selector);
        cx.simulate_click(position, Modifiers::default());
        cx.run_until_parked();
    }

    fn double_click(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        let position = center(cx, selector);
        let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
        cx.run_until_parked();
        position
    }

    fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
        let position = center(cx, selector);
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    fn mouse_down(cx: &mut VisualTestContext, selector: &'static str, button: MouseButton, modifiers: Modifiers) {
        let position = center(cx, selector);
        cx.simulate_mouse_down(position, button, modifiers);
        cx.run_until_parked();
    }

    fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
        cx.simulate_keystrokes(keystrokes);
        cx.run_until_parked();
    }

    /// The value staged for `key`; `None` when nothing is.
    fn staged(view: &Entity<KeymapView>, key: &str, cx: &mut VisualTestContext) -> Option<Value> {
        view.read_with(cx, |view, cx| {
            let dialog = view.model().read(cx);
            dialog.staged().find(|change| change.key == key).map(|change| change.value.clone())
        })
    }

    fn menu(view: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.menu_lines(cx)).unwrap_or_default()
    }

    fn recording(view: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Option<(String, bool)> {
        view.read_with(cx, |view, _| view.recording().map(|(id, mouse)| (id.to_owned(), mouse)))
    }

    fn recorded(view: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.recorded_binding())
    }

    fn row_keys(view: &Entity<KeymapView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.rows(cx).into_iter().map(|row| row.key).collect())
    }

    fn start_recording(view: &Entity<KeymapView>, id: &str, mouse: bool, cx: &mut VisualTestContext) {
        cx.update(|window, cx| view.update(cx, |view, cx| view.start_recording(id, mouse, window, cx)));
        cx.run_until_parked();
    }

    fn code_actions() -> Vec<KeymapAction> {
        vec![
            KeymapAction::new("code.duplicateLine", "Duplicate Line").category("Code").when("editorFocus"),
            KeymapAction::new("t.mine", "My Action").category("Code").when("editorFocus"),
        ]
    }

    // Settings dialog › records a shortcut, shows conflicts and applies it to the keymap on OK
    #[gpui_kit::test]
    async fn records_a_shortcut_shows_conflicts_and_applies_it_to_the_keymap_on_ok(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, code_actions());
        double_click(cx, "keymap-row-t.mine");
        assert_eq!(menu(&view, cx).first().map(String::as_str), Some("> Add Keyboard Shortcut"));
        keys(cx, "enter");
        assert_eq!(recording(&view, cx), Some(("t.mine".to_owned(), false)));
        keys(cx, "cmd-d");
        assert_eq!(recorded(&view, cx).as_deref(), Some("Meta+D"));
        let conflicts = view.read_with(cx, |view, cx| view.conflicts(cx));
        let titles: Vec<&str> = conflicts.iter().map(|conflict| conflict.title.as_str()).collect();
        assert_eq!(titles, ["Duplicate Line"]);
        click(cx, "keymap-record-ok");
        assert!(view.read_with(cx, |view, _| view.asks_to_remove_conflicts()), "OK asks about the other assignments");
        click(cx, "keymap-conflict-leave");
        assert_eq!(recording(&view, cx), None);
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "t.mine": ["Meta+D"] })));
        // Leave keeps the other assignment.
        assert_eq!(view.read_with(cx, |view, cx| view.shortcuts_of("code.duplicateLine", cx)), ["Meta+D"]);
        let pending = view.read_with(cx, |view, cx| view.model().read(cx).pending_changes());
        let written: Vec<(String, String)> = pending.iter().map(|change| (change.key.clone(), change.json())).collect();
        assert_eq!(written, [(KEYMAP_OVERRIDES_KEY.to_owned(), r#"{"t.mine":["Meta+D"]}"#.to_owned())]);
    }

    /// The row menu opened by `activation` (a double-click, or Enter on the selected row) adds a
    /// mouse shortcut; the next menu adds a keyboard one again.
    async fn opens_the_shortcut_menu_and_clears_mouse_recording_mode(cx: &mut TestAppContext, double: bool) {
        let actions = vec![KeymapAction::new("t.activate", "Activation Target").category("Code")];
        let (view, cx) = open(cx, actions);
        let activate = |cx: &mut VisualTestContext| {
            if double {
                Some(double_click(cx, "keymap-row-t.activate"))
            } else {
                click(cx, "keymap-row-t.activate");
                keys(cx, "enter");
                None
            }
        };
        let at = activate(cx);
        let position = view.read_with(cx, |view, _| view.menu_position());
        assert!(position.is_some(), "the menu is open");
        if let Some(at) = at {
            assert_eq!(position, Some(at), "a double-click opens the menu where it happened");
        }
        assert_eq!(recording(&view, cx), None, "the menu comes first");
        assert_eq!(
            menu(&view, cx),
            ["> Add Keyboard Shortcut", "Add Mouse Shortcut", "-", "Remove", "-", "Reset Shortcuts"]
        );
        keys(cx, "down");
        assert_eq!(menu(&view, cx)[1], "> Add Mouse Shortcut");
        keys(cx, "enter");
        assert_eq!(recording(&view, cx), Some(("t.activate".to_owned(), true)));
        mouse_down(cx, "keymap-mouse-pad", MouseButton::Left, Modifiers { alt: true, ..Modifiers::default() });
        click(cx, "keymap-record-ok");
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "t.activate": ["Alt+Button1"] })));

        activate(cx);
        keys(cx, "enter");
        assert_eq!(recording(&view, cx), Some(("t.activate".to_owned(), false)), "keys are recorded again");
        assert!(cx.debug_bounds("keymap-first-stroke").is_some());
        click(cx, "keymap-record-cancel");
        right_click(cx, "keymap-row-t.activate");
        click(cx, "menu-row-0-1");
        assert_eq!(recording(&view, cx), Some(("t.activate".to_owned(), true)));
        click(cx, "keymap-record-cancel");
        click(cx, "keymap-add-shortcut");
        assert_eq!(recording(&view, cx), Some(("t.activate".to_owned(), false)));
    }

    // Settings dialog › opens the shortcut menu on double-click and clears mouse recording mode
    #[gpui_kit::test]
    async fn opens_the_shortcut_menu_on_double_click_and_clears_mouse_recording_mode(cx: &mut TestAppContext) {
        opens_the_shortcut_menu_and_clears_mouse_recording_mode(cx, true).await;
    }

    // Settings dialog › opens the shortcut menu on Enter and clears mouse recording mode
    #[gpui_kit::test]
    async fn opens_the_shortcut_menu_on_enter_and_clears_mouse_recording_mode(cx: &mut TestAppContext) {
        opens_the_shortcut_menu_and_clears_mouse_recording_mode(cx, false).await;
    }

    // Settings dialog › duplicates the keymap from the gear menu and edits the copy
    #[gpui_kit::test]
    async fn duplicates_the_keymap_from_the_gear_menu_and_edits_the_copy(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("t.dup", "Dup Target").category("Code")]);
        click(cx, "keymap-gear");
        assert_eq!(menu(&view, cx), ["> Duplicate…", "Rename…", "Restore Defaults"]);
        keys(cx, "enter");
        assert_eq!(view.read_with(cx, |view, cx| view.name_dialog_text(cx)).as_deref(), Some("macOS copy"));
        click(cx, "keymap-name-ok");
        assert_eq!(staged(&view, KEYMAP_KEY, cx), Some(json!("custom.macos-copy")));
        let copy = json!({
            "custom.macos-copy": { "name": "macOS copy", "parent": "intellij-macos", "overrides": {} },
        });
        assert_eq!(staged(&view, CUSTOM_KEYMAPS_KEY, cx), Some(copy));
        assert_eq!(view.read_with(cx, |view, cx| view.selected_keymap(cx)), "custom.macos-copy");

        double_click(cx, "keymap-row-t.dup");
        keys(cx, "enter");
        keys(cx, "ctrl-alt-shift-cmd-f9");
        assert_eq!(recorded(&view, cx).as_deref(), Some("Ctrl+Alt+Shift+Meta+F9"));
        click(cx, "keymap-record-ok");
        let edited = json!({
            "custom.macos-copy": {
                "name": "macOS copy",
                "parent": "intellij-macos",
                "overrides": { "t.dup": ["Ctrl+Alt+Shift+Meta+F9"] },
            },
        });
        assert_eq!(staged(&view, CUSTOM_KEYMAPS_KEY, cx), Some(edited));
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), None, "the bundled keymap is left alone");
    }

    // Mouse Shortcut dialog › records and submits Option+Command+left click
    #[gpui_kit::test]
    async fn records_and_submits_option_command_left_click(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("my.action", "My Action").category("Test")]);
        start_recording(&view, "my.action", true, cx);
        let held = Modifiers { alt: true, platform: true, ..Modifiers::default() };
        mouse_down(cx, "keymap-mouse-pad", MouseButton::Left, held);
        assert_eq!(recorded(&view, cx).as_deref(), Some("Alt+Meta+Button1"));
        assert!(view.read_with(cx, |view, cx| view.conflicts(cx)).is_empty());
        click(cx, "keymap-record-ok");
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "my.action": ["Alt+Meta+Button1"] })));
    }

    // Mouse Shortcut dialog › records a side-button click with modifiers and submits it
    #[gpui_kit::test]
    async fn records_a_side_button_click_with_modifiers_and_submits_it(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("my.action", "My Action").category("Test")]);
        start_recording(&view, "my.action", true, cx);
        mouse_down(cx, "keymap-mouse-pad", MouseButton::Left, Modifiers::default());
        assert_eq!(recorded(&view, cx).as_deref(), Some(""), "a plain left click keeps its own meaning");
        click(cx, "keymap-record-ok");
        assert_eq!(recording(&view, cx), Some(("my.action".to_owned(), true)), "OK is disabled");
        let back = MouseButton::Navigate(NavigationDirection::Back);
        mouse_down(cx, "keymap-mouse-pad", back, Modifiers { alt: true, ..Modifiers::default() });
        click(cx, "keymap-record-ok");
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "my.action": ["Alt+Button4"] })));
    }

    // shortcut recording + normalization › records strokes in canonical form
    #[gpui_kit::test]
    async fn records_strokes_in_canonical_form(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("my.action", "My Action")]);
        start_recording(&view, "my.action", false, cx);
        for (pressed, stroke) in [("shift-cmd-d", "Shift+Meta+D"), ("cmd-,", "Meta+,"), ("alt-f12", "Alt+F12")] {
            keys(cx, pressed);
            assert_eq!(recorded(&view, cx).as_deref(), Some(stroke), "{pressed}");
        }
        assert_eq!(normalize_binding("Meta+Shift+D"), "Shift+Meta+D");
        assert_eq!(normalize_binding("meta+k   META+c"), "Meta+K Meta+C");
        // The second stroke makes a chord once it is turned on.
        click(cx, "keymap-use-second");
        keys(cx, "cmd-c");
        assert_eq!(recorded(&view, cx).as_deref(), Some("Alt+F12 Meta+C"));
    }

    // shortcut search › filters the keymap tree by name and by shortcut
    #[gpui_kit::test]
    async fn filters_the_keymap_tree_by_name_and_by_shortcut(cx: &mut TestAppContext) {
        let actions = vec![
            KeymapAction::new("file.save", "Save All").category("File"),
            KeymapAction::new("navigate.gotoFile", "Go to File…").category("Navigate"),
            KeymapAction { hidden: true, ..KeymapAction::new("internal.x", "Internal") },
        ];
        let (view, cx) = open(cx, actions);
        assert_eq!(
            row_keys(&view, cx),
            ["grp:Main Menu", "grp:Main Menu/File", "file.save", "grp:Main Menu/Navigate", "navigate.gotoFile"]
        );
        click(cx, "keymap-search");
        let field = view.read_with(cx, |view, _| view.search_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input("gofi");
        cx.run_until_parked();
        assert_eq!(row_keys(&view, cx), ["grp:Main Menu", "grp:Main Menu/Navigate", "navigate.gotoFile"]);
        let hits = view.read_with(cx, |view, cx| view.rows(cx).into_iter().find_map(|row| row.hits));
        assert!(hits.is_some_and(|hits| !hits.is_empty()), "the title's matched letters are marked");
        click(cx, "keymap-clear-filter");
        assert_eq!(view.read_with(cx, |view, _| view.query().to_owned()), "");
        click(cx, "keymap-by-shortcut");
        keys(cx, "cmd-s");
        assert_eq!(view.read_with(cx, |view, _| view.shortcut_filter().map(str::to_owned)), Some("Meta+S".to_owned()));
        assert_eq!(row_keys(&view, cx), ["grp:Main Menu", "grp:Main Menu/File", "file.save"]);
        click(cx, "keymap-shortcut-clear");
        assert_eq!(row_keys(&view, cx).len(), 5);
    }

    // Not from a spec: a row's menu removes a shortcut and resets the row, the gear menu restores
    // the defaults, and choosing another bundled keymap carries the edits over.
    #[gpui_kit::test]
    async fn removes_resets_and_restores_shortcuts_and_carries_edits_to_another_keymap(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("file.save", "Save All").category("File")]);
        let shortcuts = |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.shortcuts_of("file.save", cx));
        assert_eq!(shortcuts(cx), ["Meta+S"]);
        right_click(cx, "keymap-row-file.save");
        let lines = menu(&view, cx);
        assert_eq!((lines.len(), lines[5].as_str()), (6, "Reset Shortcuts"));
        click(cx, "menu-row-0-3");
        assert!(shortcuts(cx).is_empty());
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "file.save": [] })));
        assert_eq!(view.read_with(cx, |view, cx| view.modified_count(cx)), 1);
        right_click(cx, "keymap-row-file.save");
        assert_eq!(menu(&view, cx)[3], "Remove", "nothing left to remove");
        click(cx, "menu-row-0-5");
        assert_eq!(shortcuts(cx), ["Meta+S"]);
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), None, "back to the keymap's own shortcuts");

        right_click(cx, "keymap-row-file.save");
        click(cx, "menu-row-0-3");
        click(cx, "keymap-preset");
        let choices = view.read_with(cx, |view, cx| view.keymap_choices(cx));
        let vscode = choices.iter().position(|(id, _)| id == "vscode-macos").expect("the VS Code keymap is offered");
        assert_eq!(menu(&view, cx)[vscode], "VS Code (macOS)");
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.run(KeymapCommand::SelectKeymap("vscode-macos".to_owned()), window, cx))
        });
        cx.run_until_parked();
        assert_eq!(staged(&view, KEYMAP_KEY, cx), Some(json!("vscode-macos")));
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), Some(json!({ "file.save": [] })));
        click(cx, "keymap-gear");
        keys(cx, "down enter");
        assert_eq!(staged(&view, KEYMAP_OVERRIDES_KEY, cx), None, "Restore Defaults drops every edit");
        assert_eq!(shortcuts(cx), ["Meta+S"]);
    }

    // Not from a spec: a user keymap is renamed, and deleted after a confirmation, which selects its
    // parent again.
    #[gpui_kit::test]
    async fn renames_and_deletes_a_user_keymap(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, vec![KeymapAction::new("t.dup", "Dup Target").category("Code")]);
        click(cx, "keymap-gear");
        keys(cx, "enter");
        click(cx, "keymap-name-ok");
        click(cx, "keymap-gear");
        assert_eq!(menu(&view, cx), ["> Duplicate…", "Rename…", "Restore Defaults", "-", "Delete"]);
        click(cx, "menu-row-0-1");
        assert_eq!(view.read_with(cx, |view, cx| view.name_dialog_text(cx)).as_deref(), Some("macOS copy"));
        cx.simulate_input("macOS");
        cx.run_until_parked();
        click(cx, "keymap-name-ok");
        assert!(view.read_with(cx, |view, cx| view.name_dialog_text(cx)).is_some(), "a bundled keymap's name is taken");
        cx.simulate_input(" ours");
        keys(cx, "enter");
        let renamed = json!({
            "custom.macos-copy": { "name": "macOS ours", "parent": "intellij-macos", "overrides": {} },
        });
        assert_eq!(staged(&view, CUSTOM_KEYMAPS_KEY, cx), Some(renamed));

        click(cx, "keymap-gear");
        click(cx, "menu-row-0-4");
        assert!(view.read_with(cx, |view, _| view.confirms_delete()));
        click(cx, "keymap-delete-ok");
        assert_eq!(view.read_with(cx, |view, cx| view.selected_keymap(cx)), "intellij-macos");
        assert_eq!(view.read_with(cx, |view, cx| view.keymap_choices(cx)).len(), PRESET_TITLES.len());
    }

    // Not from a spec: the tree opens and closes groups from the keyboard and with Expand All /
    // Collapse All, and a Settings search that names an action filters the tree.
    #[gpui_kit::test]
    async fn walks_the_tree_and_follows_the_settings_search(cx: &mut TestAppContext) {
        let actions = vec![
            KeymapAction::new("file.save", "Save All").category("File"),
            KeymapAction::new("edit.undo", "Undo").category("Edit"),
        ];
        let (view, cx) = open(cx, actions);
        click(cx, "keymap-row-grp:Main Menu");
        keys(cx, "left");
        assert_eq!(row_keys(&view, cx), ["grp:Main Menu"]);
        keys(cx, "right right");
        assert_eq!(view.read_with(cx, |view, _| view.lead().map(str::to_owned)).as_deref(), Some("grp:Main Menu/File"));
        keys(cx, "end");
        assert_eq!(view.read_with(cx, |view, _| view.lead().map(str::to_owned)).as_deref(), Some("edit.undo"));
        click(cx, "keymap-collapse-all");
        assert_eq!(row_keys(&view, cx), ["grp:Main Menu"]);
        click(cx, "keymap-expand-all");
        assert_eq!(row_keys(&view, cx).len(), 5);

        let model = view.read_with(cx, |view, _| view.model().clone());
        model.update(cx, |dialog, cx| {
            dialog.query = "undo".to_owned();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.query().to_owned()), "undo");
        assert_eq!(row_keys(&view, cx), ["grp:Main Menu", "grp:Main Menu/Edit", "edit.undo"]);
        model.update(cx, |dialog, cx| {
            dialog.query = "appearance".to_owned();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.query().to_owned()), "undo", "no action has that name");
    }

    // Not from a spec: runs of a title follow the matched bytes.
    #[test]
    fn cuts_a_title_into_marked_runs() {
        let runs = hit_segments("Go to File", &[0, 6]);
        let expected = [("G", true), ("o to ", false), ("F", true), ("ile", false)];
        assert_eq!(runs, expected.map(|(run, hit)| (run.to_owned(), hit)));
        assert!(hit_segments("", &[0]).is_empty());
    }
}
