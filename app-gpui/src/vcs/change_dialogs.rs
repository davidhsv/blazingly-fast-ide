//! The dialogs of shelving and change lists: Shelve Changes (which files, under what name),
//! Unshelve Changes (into which change list, and whether the shelf keeps what applied), and the
//! change list form of New / Edit Changelist and of Move Changes to Another Changelist (an
//! existing list or a new one).
//!
//! None of them runs anything: the answer goes to the workbench as an event. Enter presses the
//! default button when it can be pressed, Escape cancels.

use super::changes::{
    ChangelistState, EffectiveKind, FileStatus, base_name, dir_name, effective_kind, rel_to, strip_slash,
};
use super::commit_view::kind_color;
use super::prompt_dialogs::{CANCEL, OK, button, checkbox, field_frame, tx};
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::TextArea;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::changelists::validate_list_name;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use std::collections::HashSet;

/// A changed file of a dialog's checklist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistFile {
    /// Absolute path, without a trailing slash.
    pub path: String,
    pub name: String,
    /// The folder below the project root.
    pub dir: String,
    pub kind: EffectiveKind,
}

impl ChecklistFile {
    /// `file` as the checklist of the project at `root` shows it.
    pub fn of(file: &FileStatus, root: &str) -> Self {
        let path = strip_slash(&file.path).to_owned();
        let name = base_name(&path).to_owned();
        let dir = dir_name(rel_to(root, &path)).to_owned();
        Self { kind: effective_kind(file).unwrap_or(EffectiveKind::Modified), path, name, dir }
    }
}

/// A change list a dialog offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListChoice {
    pub id: String,
    /// The list's name, with " (active)" for the active one, in the UI language.
    pub label: String,
}

/// The change lists of `lists` as a dialog offers them.
pub fn list_choices(lists: &ChangelistState, cx: &mut App) -> Vec<ListChoice> {
    let i18n = &mut cx.default_global::<Localization>().0;
    lists
        .lists
        .iter()
        .map(|list| {
            let label = if list.id == lists.active {
                i18n.tx_built(&["", " (active)"], &[Param::from(list.name.as_str())])
            } else {
                list.name.clone()
            };
            ListChoice { id: list.id.clone(), label }
        })
        .collect()
}

// -------------------------------------------------------------------------------------------------
// Parts the dialogs share

/// A dialog's box with its title.
pub(super) fn dialog_frame(cx: &App, width: f32, title: String) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .bg(theme::color(cx, "bg-popup"))
        .border_1()
        .border_color(theme::color(cx, "border-popup"))
        .rounded(theme::length(cx, "radius-popup"))
        .text_size(theme::length(cx, "fs-ui"))
        .text_color(theme::color(cx, "fg"))
        // A click on the dialog is not a click beside it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .px(px(16.0))
                .pt(px(14.0))
                .pb(px(8.0))
                .text_color(theme::color(cx, "fg-strong"))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(title)),
        )
}

/// The column of a dialog's contents, below its title.
pub(super) fn dialog_body() -> Div {
    div().px(px(16.0)).py(px(4.0)).flex().flex_col().gap(px(10.0))
}

/// The row of a dialog's buttons, at its bottom right.
pub(super) fn button_row(buttons: Vec<AnyElement>) -> Div {
    div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons)
}

/// A muted line of text.
pub(super) fn muted_line(cx: &App, text: String) -> Div {
    div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(text))
}

/// Button `id` of a dialog, drawn as `<prefix>-button-<id>`; a click hands `id` to `press`.
pub(super) fn dialog_button<V: 'static>(
    cx: &mut Context<V>,
    prefix: &'static str,
    id: &'static str,
    label: &str,
    primary: bool,
    enabled: bool,
    press: fn(&mut V, &str, &mut Context<V>),
) -> AnyElement {
    let text = tx(cx, label);
    button(cx, text, primary, false, enabled)
        .id(SharedString::from(format!("{prefix}-button-{id}")))
        .debug_selector(move || format!("{prefix}-button-{id}"))
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| press(view, id, cx)))
        .into_any_element()
}

/// A checkbox with its label, drawn as `selector`; a click runs `toggle` unless it is disabled.
pub(super) fn check_row<V: 'static>(
    cx: &mut Context<V>,
    selector: String,
    label: &str,
    on: bool,
    enabled: bool,
    toggle: fn(&mut V, &mut Context<V>),
) -> AnyElement {
    let text = tx(cx, label);
    let id = SharedString::from(selector.clone());
    let row = checkbox(cx, text, on).id(id).debug_selector(move || selector);
    if enabled {
        row.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| toggle(view, cx))).into_any_element()
    } else {
        row.opacity(0.5).into_any_element()
    }
}

/// A radio button with its label.
pub(super) fn radio(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let accent = theme::color(cx, "bg-button-primary");
    let dot = on.then(|| div().size(px(6.0)).rounded_full().bg(theme::color(cx, "fg-button-primary")));
    let row = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if on { accent } else { theme::color(cx, "border-checkbox") })
                .bg(if on { accent } else { theme::color(cx, "bg-checkbox") })
                .children(dot),
        )
        .child(SharedString::from(label));
    if enabled { row } else { row.opacity(0.5) }
}

/// The rows of `choices` to pick one from; the chosen one is highlighted. Row `i` is drawn as
/// `<prefix>-choice-<i>`.
pub(super) fn choice_rows<V: 'static>(
    cx: &mut Context<V>,
    prefix: &'static str,
    choices: &[ListChoice],
    chosen: &str,
    enabled: bool,
    choose: fn(&mut V, &str, &mut Context<V>),
) -> AnyElement {
    let selected = theme::color(cx, "bg-tree-selected");
    let rows: Vec<AnyElement> = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let id = choice.id.clone();
            let row = div()
                .id(SharedString::from(format!("{prefix}-choice-{index}")))
                .debug_selector(move || format!("{prefix}-choice-{index}"))
                .h(px(22.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .whitespace_nowrap()
                .when(choice.id == chosen, |el| el.bg(selected))
                .child(SharedString::from(choice.label.clone()));
            if enabled {
                row.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| choose(view, &id, cx))).into_any_element()
            } else {
                row.opacity(0.5).into_any_element()
            }
        })
        .collect();
    div()
        .id(SharedString::from(format!("{prefix}-choices")))
        .max_h(px(140.0))
        .overflow_y_scroll()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-input"))
        .children(rows)
        .into_any_element()
}

/// The files of a dialog, each with its checkbox, icon, name in the colour of its change and
/// folder. Row clicks hand the file's path to `toggle`; a row is drawn as `<prefix>-file-<name>`.
pub(super) fn checklist<V: 'static>(
    cx: &mut Context<V>,
    prefix: &'static str,
    files: &[ChecklistFile],
    checked: &HashSet<String>,
    toggle: fn(&mut V, &str, &mut Context<V>),
) -> AnyElement {
    let muted = theme::color(cx, "fg-muted");
    let rows: Vec<AnyElement> = files
        .iter()
        .map(|file| {
            let color = theme::color(cx, kind_color(file.kind));
            let (path, name) = (file.path.clone(), file.name.clone());
            div()
                .id(SharedString::from(format!("{prefix}-file:{}", file.path)))
                .debug_selector(move || format!("{prefix}-file-{name}"))
                .h(px(22.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .whitespace_nowrap()
                .child(checkbox(cx, String::new(), checked.contains(&file.path)))
                .child(theme::icon(cx, theme::icons::file_icon_name(&file.name), 14.0, color))
                .child(div().text_color(color).child(SharedString::from(file.name.clone())))
                .child(div().text_color(muted).child(SharedString::from(file.dir.clone())))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| toggle(view, &path, cx)))
                .into_any_element()
        })
        .collect();
    div()
        .id(SharedString::from(format!("{prefix}-files")))
        .max_h(px(220.0))
        .overflow_y_scroll()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-input"))
        .children(rows)
        .into_any_element()
}

/// Has Enter press the default button (`OK`) and Escape cancel (`CANCEL`) while one of `focus`
/// has the keyboard, ahead of the keymap. The claims go with the dialog.
pub(super) fn claim_dialog_keys<V: 'static>(
    focus: &[FocusHandle],
    press: fn(&mut V, &str, &mut Context<V>),
    cx: &mut Context<V>,
) {
    let mut claims = Vec::with_capacity(focus.len());
    for handle in focus {
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            handle.clone(),
            move |keystroke, _, cx| {
                if keystroke.modifiers.modified() {
                    return false;
                }
                let id = match keystroke.key.as_str() {
                    "enter" => OK,
                    "escape" => CANCEL,
                    _ => return false,
                };
                claimant.update(cx, |view, cx| press(view, id, cx)).is_ok()
            },
            cx,
        );
        claims.push(claim);
    }
    cx.on_release(move |_, cx| {
        for claim in claims {
            input::release_keys(claim, cx);
        }
    })
    .detach();
}

/// A one-line text field holding `text`.
fn text_field(text: &str, cx: &mut App) -> Entity<TextField> {
    cx.new(|cx| {
        let mut field = TextField::new(cx);
        field.set_text(text, cx);
        field
    })
}

// -------------------------------------------------------------------------------------------------
// Shelve Changes

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelveDialogEvent {
    /// Shelve the changes of `paths` (absolute) as `name`.
    Shelve {
        name: String,
        paths: Vec<String>,
    },
    Cancelled,
}

/// Shelve Changes: the files to shelve, all checked at first, and the shelf's name.
pub struct ShelveDialog {
    files: Vec<ChecklistFile>,
    checked: HashSet<String>,
    name: Entity<TextField>,
    _name_events: Subscription,
}

impl EventEmitter<ShelveDialogEvent> for ShelveDialog {}

impl ShelveDialog {
    pub fn new(files: Vec<ChecklistFile>, initial_name: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let checked = files.iter().map(|file| file.path.clone()).collect();
        let name = text_field(initial_name, cx);
        let events =
            cx.subscribe_in(&name, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submitted => dialog.press(OK, cx),
                TextFieldEvent::Cancelled => dialog.press(CANCEL, cx),
            });
        claim_dialog_keys(&[name.focus_handle(cx)], Self::press, cx);
        Self { files, checked, name, _name_events: events }
    }

    pub fn files(&self) -> &[ChecklistFile] {
        &self.files
    }

    /// The checked files' paths, in the list's order.
    pub fn checked(&self) -> Vec<String> {
        self.files.iter().filter(|file| self.checked.contains(&file.path)).map(|file| file.path.clone()).collect()
    }

    pub fn name_field(&self) -> &Entity<TextField> {
        &self.name
    }

    /// The name typed, trimmed.
    pub fn name(&self, cx: &App) -> String {
        self.name.read(cx).text().trim().to_owned()
    }

    /// "1 of 2 files will be shelved and reverted in the working tree.", in the UI language.
    pub fn summary(&self, cx: &mut App) -> String {
        let unit = if self.files.len() == 1 {
            " file will be shelved and reverted in the working tree."
        } else {
            " files will be shelved and reverted in the working tree."
        };
        let values = [Param::from(self.checked.len()), Param::from(self.files.len())];
        cx.default_global::<Localization>().0.tx_built(&["", " of ", unit], &values)
    }

    /// Shelve Changes can be pressed: a file is checked and a name is typed.
    pub fn can_shelve(&self, cx: &App) -> bool {
        !self.checked.is_empty() && !self.name(cx).is_empty()
    }

    /// Checks or unchecks the file at `path`.
    pub fn toggle_path(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.checked.remove(path) {
            self.checked.insert(path.to_owned());
        }
        cx.notify();
    }

    /// Presses button `id`: [`OK`] shelves when it can, [`CANCEL`] gives up.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == CANCEL {
            cx.emit(ShelveDialogEvent::Cancelled);
        } else if self.can_shelve(cx) {
            cx.emit(ShelveDialogEvent::Shelve { name: self.name(cx), paths: self.checked() });
        }
    }
}

impl Focusable for ShelveDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

impl Render for ShelveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Shelve Changes");
        let summary = self.summary(cx);
        let can = self.can_shelve(cx);
        let list = checklist(cx, "shelve", &self.files, &self.checked, Self::toggle_path);
        let label = tx(cx, "Name:");
        let field = field_frame(cx, false).child(div().flex_1().min_w_0().child(self.name.clone()));
        let buttons = vec![
            dialog_button(cx, "shelve", OK, "Shelve Changes", true, can, Self::press),
            dialog_button(cx, "shelve", CANCEL, "Cancel", false, true, Self::press),
        ];
        dialog_frame(cx, 540.0, title)
            .child(
                dialog_body().child(SharedString::from(summary)).child(list).child(muted_line(cx, label)).child(field),
            )
            .child(button_row(buttons))
    }
}

// -------------------------------------------------------------------------------------------------
// Unshelve Changes

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnshelveDialogEvent {
    /// Unshelve into change list `target`; with `remove`, what applied leaves the shelf.
    Unshelve {
        target: String,
        remove: bool,
    },
    Cancelled,
}

/// Unshelve Changes: the change list the changes go to (the active one at first) and "Remove
/// successfully applied files from the shelf".
pub struct UnshelveDialog {
    heading: String,
    choices: Vec<ListChoice>,
    target: String,
    remove: bool,
    focus: FocusHandle,
}

impl EventEmitter<UnshelveDialogEvent> for UnshelveDialog {}

impl UnshelveDialog {
    /// `heading` names what is unshelved and how many files; `target` is the list chosen at first.
    pub fn new(
        heading: String,
        choices: Vec<ListChoice>,
        target: String,
        remove: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        claim_dialog_keys(std::slice::from_ref(&focus), Self::press, cx);
        Self { heading, choices, target, remove, focus }
    }

    pub fn heading(&self) -> &str {
        &self.heading
    }

    pub fn choices(&self) -> &[ListChoice] {
        &self.choices
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn remove(&self) -> bool {
        self.remove
    }

    pub fn choose(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.choices.iter().any(|choice| choice.id == id) {
            self.target = id.to_owned();
            cx.notify();
        }
    }

    pub fn toggle_remove(&mut self, cx: &mut Context<Self>) {
        self.remove = !self.remove;
        cx.notify();
    }

    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == CANCEL {
            cx.emit(UnshelveDialogEvent::Cancelled);
        } else {
            cx.emit(UnshelveDialogEvent::Unshelve { target: self.target.clone(), remove: self.remove });
        }
    }
}

impl Focusable for UnshelveDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for UnshelveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Unshelve Changes");
        let label = tx(cx, "Changelist:");
        let choices = choice_rows(cx, "unshelve", &self.choices, &self.target, true, Self::choose);
        let remove = check_row(
            cx,
            "unshelve-remove".to_owned(),
            "Remove successfully applied files from the shelf",
            self.remove,
            true,
            Self::toggle_remove,
        );
        let buttons = vec![
            dialog_button(cx, "unshelve", OK, "Unshelve", true, true, Self::press),
            dialog_button(cx, "unshelve", CANCEL, "Cancel", false, true, Self::press),
        ];
        dialog_frame(cx, 480.0, title)
            .track_focus(&self.focus)
            .child(
                dialog_body()
                    .child(SharedString::from(self.heading.clone()))
                    .child(muted_line(cx, label))
                    .child(choices)
                    .child(remove),
            )
            .child(button_row(buttons))
    }
}

// -------------------------------------------------------------------------------------------------
// New / Edit Changelist, Move Changes to Another Changelist

/// What the change list dialog is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangelistDialogMode {
    New,
    /// Edit list `id`.
    Edit(String),
    /// A target for changes: an existing list (but the one they are all in) or a new one.
    Target {
        title: String,
        hint: Option<String>,
        /// The lists the changes are in now.
        exclude: Vec<String>,
    },
}

/// How the change list dialog was answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangelistAnswer {
    /// The existing list `id` was chosen.
    Existing(String),
    /// The form was filled in: a new list, or the edited one.
    Form { name: String, comment: String, active: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangelistDialogEvent {
    Done(ChangelistAnswer),
    Cancelled,
}

/// The change list form (Name, Comment, Set active), and for a target the choice between an
/// existing list and a new one.
pub struct ChangelistDialog {
    title: String,
    lists: ChangelistState,
    /// The list being edited.
    editing: Option<String>,
    hint: Option<String>,
    /// The lists a target can be, when the dialog asks for one.
    choices: Option<Vec<ListChoice>>,
    /// An existing list is chosen rather than a new one.
    existing: bool,
    target: String,
    name: Entity<TextField>,
    comment: Entity<TextArea>,
    active: bool,
    /// The edited list is the active one: it stays active.
    active_disabled: bool,
    focus: FocusHandle,
    _name_events: Subscription,
}

impl EventEmitter<ChangelistDialogEvent> for ChangelistDialog {}

impl ChangelistDialog {
    /// The dialog for `mode` over `lists`; `suggested_name` prefills the name of a new target list.
    pub fn new(
        lists: ChangelistState,
        mode: ChangelistDialogMode,
        suggested_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let choices_of = list_choices(&lists, cx);
        let (title, editing, hint, choices, name, comment, active) = match mode {
            ChangelistDialogMode::New => {
                ("New Changelist".to_owned(), None, None, None, String::new(), String::new(), false)
            }
            ChangelistDialogMode::Edit(id) => {
                let list = lists.lists.iter().find(|list| list.id == id);
                let (name, comment) = list.map(|list| (list.name.clone(), list.comment.clone())).unwrap_or_default();
                let active = lists.active == id;
                ("Edit Changelist".to_owned(), Some(id), None, None, name, comment, active)
            }
            ChangelistDialogMode::Target { title, hint, exclude } => {
                let current: HashSet<&str> = exclude.iter().map(String::as_str).collect();
                let others: Vec<ListChoice> = choices_of
                    .into_iter()
                    .filter(|choice| !(current.len() == 1 && current.contains(choice.id.as_str())))
                    .collect();
                (title, None, hint, Some(others), suggested_name.to_owned(), String::new(), false)
            }
        };
        let existing = choices.as_ref().is_some_and(|others| !others.is_empty());
        let target = choices
            .as_ref()
            .and_then(|others| others.iter().find(|choice| choice.id == lists.active).or(others.first()))
            .map_or_else(|| lists.active.clone(), |choice| choice.id.clone());
        let active_disabled = editing.as_deref().is_some_and(|id| id == lists.active);
        let name = text_field(&name, cx);
        let comment = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_rows(3, 3, cx);
            area.set_text(&comment, cx);
            area
        });
        let events =
            cx.subscribe_in(&name, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submitted => dialog.press(OK, cx),
                TextFieldEvent::Cancelled => dialog.press(CANCEL, cx),
            });
        let focus = cx.focus_handle();
        claim_dialog_keys(&[name.focus_handle(cx), focus.clone()], Self::press, cx);
        Self {
            title,
            lists,
            editing,
            hint,
            choices,
            existing,
            target,
            name,
            comment,
            active,
            active_disabled,
            focus,
            _name_events: events,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn name_field(&self) -> &Entity<TextField> {
        &self.name
    }

    pub fn comment_area(&self) -> &Entity<TextArea> {
        &self.comment
    }

    /// The lists a target can be; `None` when the dialog is a plain form.
    pub fn choices(&self) -> Option<&[ListChoice]> {
        self.choices.as_deref()
    }

    pub fn existing(&self) -> bool {
        self.existing
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    /// Set active is checked.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Set active cannot be changed (the edited list is the active one).
    pub fn active_disabled(&self) -> bool {
        self.active_disabled
    }

    /// The form is shown: always, but for a target while an existing list is chosen.
    pub fn shows_form(&self) -> bool {
        self.choices.is_none() || !self.existing
    }

    /// Why the typed name cannot name a list, in English.
    fn problem(&self, cx: &App) -> Option<String> {
        if !self.shows_form() {
            return None;
        }
        validate_list_name(&self.lists, self.name.read(cx).text(), self.editing.as_deref())
    }

    /// The problem shown under the name, in the UI language: only once a name is typed.
    pub fn error(&self, cx: &mut App) -> Option<String> {
        if self.name.read(cx).text().is_empty() {
            return None;
        }
        let problem = self.problem(cx)?;
        Some(cx.default_global::<Localization>().0.tx(&problem))
    }

    pub fn can_confirm(&self, cx: &App) -> bool {
        self.problem(cx).is_none()
    }

    /// Chooses an existing list (`true`) or a new one; a new one takes the keyboard to its name.
    pub fn set_existing(&mut self, existing: bool, window: &mut Window, cx: &mut Context<Self>) {
        let offered = self.choices.as_ref().is_some_and(|others| !others.is_empty());
        self.existing = existing && offered;
        let target = if self.existing { self.focus.clone() } else { self.name.focus_handle(cx) };
        window.focus(&target, cx);
        cx.notify();
    }

    pub fn choose(&mut self, id: &str, cx: &mut Context<Self>) {
        let known = self.choices.as_ref().is_some_and(|others| others.iter().any(|choice| choice.id == id));
        if known && self.existing {
            self.target = id.to_owned();
            cx.notify();
        }
    }

    pub fn toggle_active(&mut self, cx: &mut Context<Self>) {
        if !self.active_disabled {
            self.active = !self.active;
            cx.notify();
        }
    }

    /// Presses button `id`: [`OK`] answers when the form has no problem, [`CANCEL`] gives up.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == CANCEL {
            cx.emit(ChangelistDialogEvent::Cancelled);
            return;
        }
        if !self.can_confirm(cx) {
            return;
        }
        let answer = if self.shows_form() {
            ChangelistAnswer::Form {
                name: self.name.read(cx).text().trim().to_owned(),
                comment: self.comment.read(cx).text().to_owned(),
                active: self.active,
            }
        } else {
            ChangelistAnswer::Existing(self.target.clone())
        };
        cx.emit(ChangelistDialogEvent::Done(answer));
    }

    fn render_form(&self, cx: &mut Context<Self>) -> Div {
        let error = self.error(cx);
        let name_label = tx(cx, "Name:");
        let comment_label = tx(cx, "Comment:");
        let field = field_frame(cx, error.is_some()).child(div().flex_1().min_w_0().child(self.name.clone()));
        let problem = error.map(|error| {
            let small = theme::length(cx, "fs-small");
            div().text_color(theme::color(cx, "danger")).text_size(small).child(SharedString::from(error))
        });
        let comment = div()
            .p(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-input"))
            .child(self.comment.clone());
        let active = check_row(
            cx,
            "changelist-active".to_owned(),
            "Set active",
            self.active,
            !self.active_disabled,
            Self::toggle_active,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(muted_line(cx, name_label))
            .child(field)
            .children(problem)
            .child(muted_line(cx, comment_label))
            .child(comment)
            .child(active)
    }
}

impl Focusable for ChangelistDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.shows_form() { self.name.focus_handle(cx) } else { self.focus.clone() }
    }
}

impl Render for ChangelistDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, &self.title);
        let can = self.can_confirm(cx);
        let mut body = dialog_body();
        if let Some(choices) = self.choices.clone() {
            if let Some(hint) = self.hint.clone() {
                body = body.child(muted_line(cx, hint));
            }
            let offered = !choices.is_empty();
            let existing_label = tx(cx, "Existing changelist:");
            let new_label = tx(cx, "New changelist:");
            let existing = radio(cx, existing_label, self.existing, offered)
                .id("changelist-existing")
                .debug_selector(|| "changelist-existing".to_owned())
                .when(offered, |el| {
                    el.on_click(cx.listener(|dialog, _: &ClickEvent, window, cx| dialog.set_existing(true, window, cx)))
                });
            let rows = choice_rows(cx, "changelist", &choices, &self.target, self.existing, Self::choose);
            let new = radio(cx, new_label, !self.existing, true)
                .id("changelist-new")
                .debug_selector(|| "changelist-new".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, window, cx| dialog.set_existing(false, window, cx)));
            body = body.child(existing).child(rows).child(new);
        }
        if self.shows_form() {
            body = body.child(self.render_form(cx));
        }
        let buttons = vec![
            dialog_button(cx, "changelist", OK, "OK", true, can, Self::press),
            dialog_button(cx, "changelist", CANCEL, "Cancel", false, true, Self::press),
        ];
        let width = if self.choices.is_some() { 440.0 } else { 420.0 };
        dialog_frame(cx, width, title).track_focus(&self.focus).child(body).child(button_row(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::changes::Changelist;
    use crate::vcs_ops::changelists::{add_list, initial_changelists};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        cx.run_until_parked();
    }

    fn file(name: &str, kind: EffectiveKind) -> ChecklistFile {
        ChecklistFile { path: format!("/p/{name}"), name: name.to_owned(), dir: String::new(), kind }
    }

    fn two_lists() -> ChangelistState {
        add_list(&initial_changelists(), "feature", "Feature", "", false)
    }

    // Not from a spec: the checklist takes its folder from the project root and its kind from the
    // file's change.
    #[test]
    fn a_checklist_file_names_its_folder_below_the_root() {
        let status = FileStatus {
            path: "/p/src/a.txt".to_owned(),
            rel_path: "src/a.txt".to_owned(),
            staged: None,
            unstaged: Some(crate::vcs::changes::ChangeKind::Untracked),
            renamed_from: None,
        };
        let file = ChecklistFile::of(&status, "/p");
        assert_eq!((file.name.as_str(), file.dir.as_str()), ("a.txt", "src"));
        assert_eq!(file.kind, EffectiveKind::Untracked);
    }

    /// Shelve Changes starts with every file checked and the suggested name; unchecking a
    /// file updates the count, an empty name disables the button, and Enter answers with the
    /// checked files and the name.
    #[gpui_kit::test]
    async fn the_shelve_dialog_shelves_the_checked_files_under_the_typed_name(cx: &mut TestAppContext) {
        init(cx);
        let files = vec![file("a.txt", EffectiveKind::Modified), file("b.txt", EffectiveKind::Added)];
        let (dialog, cx) = cx.add_window_view(|window, cx| ShelveDialog::new(files, "Changes", window, cx));
        let events: Rc<RefCell<Vec<ShelveDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &ShelveDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        cx.update(|window, cx| {
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        let summary = dialog.update(cx, |dialog, cx| dialog.summary(cx));
        assert_eq!(summary, "2 of 2 files will be shelved and reverted in the working tree.");
        click(cx, "shelve-file-b.txt");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), ["/p/a.txt"]);
        let summary = dialog.update(cx, |dialog, cx| dialog.summary(cx));
        assert_eq!(summary, "1 of 2 files will be shelved and reverted in the working tree.");

        // The suggested name is replaced by what is typed.
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.name(cx)), "Changes");
        dialog.update(cx, |dialog, cx| dialog.name_field().update(cx, |field, cx| field.set_text("", cx)));
        cx.simulate_input("Work");
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.name(cx)), "Work");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            *events.borrow(),
            [ShelveDialogEvent::Shelve { name: "Work".to_owned(), paths: vec!["/p/a.txt".to_owned()] }]
        );
        click(cx, "shelve-file-a.txt");
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.can_shelve(cx)), "nothing is checked");
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&ShelveDialogEvent::Cancelled));
    }

    /// Unshelve offers the change lists with the active one marked and chosen; another list and the
    /// Remove checkbox go into the answer.
    #[gpui_kit::test]
    async fn the_unshelve_dialog_answers_with_the_chosen_list_and_the_remove_choice(cx: &mut TestAppContext) {
        init(cx);
        let lists = two_lists();
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            let choices = list_choices(&lists, cx);
            UnshelveDialog::new("Work · 2 files".to_owned(), choices, lists.active.clone(), false, cx)
        });
        let events: Rc<RefCell<Vec<UnshelveDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &UnshelveDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        let labels: Vec<String> = dialog.read_with(cx, |d, _| d.choices().iter().map(|c| c.label.clone()).collect());
        assert_eq!(labels, ["Changes (active)", "Feature"]);
        assert_eq!(dialog.read_with(cx, |d, _| d.target().to_owned()), "default");
        click(cx, "unshelve-choice-1");
        click(cx, "unshelve-remove");
        click(cx, "unshelve-button-ok");
        assert_eq!(*events.borrow(), [UnshelveDialogEvent::Unshelve { target: "feature".to_owned(), remove: true }]);
    }

    /// New Changelist refuses a taken or empty name and answers with the name, the comment and Set
    /// active; Edit keeps Set active checked and fixed for the active list.
    #[gpui_kit::test]
    async fn the_changelist_form_validates_the_name_and_answers_with_the_form(cx: &mut TestAppContext) {
        init(cx);
        let lists = two_lists();
        let (dialog, cx) = cx.add_window_view(|window, cx| {
            ChangelistDialog::new(lists.clone(), ChangelistDialogMode::New, "", window, cx)
        });
        let events: Rc<RefCell<Vec<ChangelistDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &ChangelistDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        cx.update(|window, cx| {
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        assert!(!dialog.read_with(cx, |d, cx| d.can_confirm(cx)), "an empty name is refused");
        assert_eq!(dialog.update(cx, |d, cx| d.error(cx)), None, "no problem shows before typing");
        cx.simulate_input("feature");
        assert_eq!(
            dialog.update(cx, |d, cx| d.error(cx)).as_deref(),
            Some("A changelist named 'feature' already exists")
        );
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty(), "a taken name is not accepted");
        cx.simulate_input(" two");
        dialog.update(cx, |d, cx| d.comment_area().update(cx, |area, cx| area.set_text("ticket 7", cx)));
        click(cx, "changelist-active");
        click(cx, "changelist-button-ok");
        let form =
            ChangelistAnswer::Form { name: "feature two".to_owned(), comment: "ticket 7".to_owned(), active: true };
        assert_eq!(*events.borrow(), [ChangelistDialogEvent::Done(form)]);

        let active = ChangelistState { active: "feature".to_owned(), ..lists };
        let (edit, cx) = cx.add_window_view(|window, cx| {
            ChangelistDialog::new(active, ChangelistDialogMode::Edit("feature".to_owned()), "", window, cx)
        });
        let (name, active, fixed) =
            edit.read_with(cx, |d, cx| (d.name_field().read(cx).text().to_owned(), d.active(), d.active_disabled()));
        assert_eq!((name.as_str(), active, fixed), ("Feature", true, true));
        assert!(edit.read_with(cx, |d, cx| d.can_confirm(cx)), "the list keeps its own name");
    }

    /// Move Changes to Another Changelist leaves out the one list the changes are all in, chooses an
    /// existing list by default and switches to the form for a new one.
    #[gpui_kit::test]
    async fn the_target_dialog_offers_the_other_lists_or_a_new_one(cx: &mut TestAppContext) {
        init(cx);
        let mut lists = two_lists();
        lists.lists.push(Changelist { id: "third".to_owned(), name: "Third".to_owned(), comment: String::new() });
        let mode = ChangelistDialogMode::Target {
            title: "Move Changes to Another Changelist".to_owned(),
            hint: Some("1 file".to_owned()),
            exclude: vec!["default".to_owned()],
        };
        let (dialog, cx) = cx.add_window_view(|window, cx| ChangelistDialog::new(lists, mode, "", window, cx));
        let events: Rc<RefCell<Vec<ChangelistDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &ChangelistDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        let ids: Vec<String> =
            dialog.read_with(cx, |d, _| d.choices().unwrap_or_default().iter().map(|c| c.id.clone()).collect());
        assert_eq!(ids, ["feature", "third"]);
        assert!(dialog.read_with(cx, |d, _| d.existing() && !d.shows_form()));
        assert_eq!(dialog.read_with(cx, |d, _| d.target().to_owned()), "feature");
        click(cx, "changelist-choice-1");
        click(cx, "changelist-button-ok");
        assert_eq!(*events.borrow(), [ChangelistDialogEvent::Done(ChangelistAnswer::Existing("third".to_owned()))]);

        click(cx, "changelist-new");
        assert!(dialog.read_with(cx, |d, _| d.shows_form()));
        cx.simulate_input("Fresh");
        cx.simulate_keystrokes("enter");
        let form = ChangelistAnswer::Form { name: "Fresh".to_owned(), comment: String::new(), active: false };
        assert_eq!(events.borrow().last(), Some(&ChangelistDialogEvent::Done(form)));
    }
}
