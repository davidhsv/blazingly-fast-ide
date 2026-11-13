//! View Breakpoints (the Breakpoints dialog): every breakpoint of the project, grouped as Line
//! Breakpoints (by file), Method Breakpoints and Field Watchpoints, each with its enable checkbox
//! and where it is; Mute breakpoints, Add Method Breakpoint and Remove All above the list, the
//! exception filters of the debug adapters below it, and the properties of the selected breakpoint
//! beside it: Enabled, Suspend, the condition, the hit count, the "Breakpoint hit" message,
//! Evaluate and log with its template and Remove once hit for a line breakpoint; the method or
//! the accesses that suspend, the condition and the hit count for the others. Go to Source opens
//! the breakpoint and closes the dialog, Done closes it.
//!
//! ↑ / ↓ move through the breakpoints, Space turns the selected one on or off, Enter goes to its
//! source and Escape closes. Text is kept when another breakpoint is selected, on Enter in a
//! field and when the dialog closes.
//!
//! [`BreakpointsDialog`] reads the [`Debugger`] and changes nothing in it: every change goes out
//! as a [`BreakpointsDialogEvent`] for the workbench to apply to [`Debugger::breakpoints`].

use crate::debugger::actions::Debugger;
use crate::debugger::breakpoints::exceptions::{ExceptionGroup, exception_groups};
use crate::debugger::breakpoints::model::{BreakpointEntry, BreakpointStatus, is_conditional};
use crate::debugger::breakpoints::special::{AccessType, SpecialBreakpoint, SpecialPatch};
use crate::debugger::overlays::BreakpointSelection;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::virtual_docs::{basename, relative_path};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use std::rc::Rc;

const WIDTH: f32 = 760.0;
const BODY_HEIGHT: f32 = 420.0;
const LIST_WIDTH: f32 = 360.0;
const FIELD_HEIGHT: f32 = 26.0;
const BUTTON_HEIGHT: f32 = 26.0;

/// Changes to a line breakpoint; `None` leaves a property as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinePatch {
    pub enabled: Option<bool>,
    pub suspend: Option<bool>,
    pub condition: Option<Option<String>>,
    pub hit_condition: Option<Option<String>>,
    pub log_message: Option<Option<String>>,
    pub log_hit: Option<bool>,
    pub temporary: Option<bool>,
}

impl LinePatch {
    /// Applies the patch: the change [`crate::debugger::breakpoints::store::BreakpointStore::update_breakpoint`]
    /// makes for [`BreakpointsDialogEvent::UpdateLine`].
    pub fn apply(&self, entry: &mut BreakpointEntry) {
        entry.enabled = self.enabled.unwrap_or(entry.enabled);
        entry.suspend = self.suspend.unwrap_or(entry.suspend);
        entry.log_hit = self.log_hit.unwrap_or(entry.log_hit);
        entry.temporary = self.temporary.unwrap_or(entry.temporary);
        if let Some(condition) = &self.condition {
            entry.condition.clone_from(condition);
        }
        if let Some(hit_condition) = &self.hit_condition {
            entry.hit_condition.clone_from(hit_condition);
        }
        if let Some(log_message) = &self.log_message {
            entry.log_message.clone_from(log_message);
        }
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the dialog asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointsDialogEvent {
    /// `breakpoints.lines.update_breakpoint(path, id, now, |b| patch.apply(b))`.
    UpdateLine { path: String, id: String, patch: LinePatch },
    /// `breakpoints.special.update_special(id, patch, now)`.
    UpdateSpecial { id: String, patch: SpecialPatch },
    /// `breakpoints.lines.remove_breakpoint(path, id, now)`.
    RemoveLine { path: String, id: String },
    /// `breakpoints.special.remove_special(id, now)`.
    RemoveSpecial(String),
    /// Remove every line breakpoint, method breakpoint and watchpoint.
    RemoveAll,
    /// `breakpoints.special.add_method_breakpoint("", None, now)`, then [`BreakpointsDialog::select`]
    /// the new one so its name can be typed.
    AddMethodBreakpoint,
    /// `Breakpoints::set_muted`.
    SetMuted(bool),
    /// `breakpoints.exceptions.set_exception_filter(adapter, filter, enabled, can_sync)`.
    SetExceptionFilter { adapter: String, filter: String, enabled: bool },
    /// The dialog opened: ask the live sessions for their exception filters
    /// (`breakpoints.exceptions.refresh(desktop)`).
    RefreshExceptionFilters,
    /// Open `path` at the 0-based `line`; the dialog closes too.
    GoToSource { path: String, line: u32 },
    /// Close the dialog (`overlays.breakpoints = None`).
    Close,
}

/// A row of the list: a section's title, or a breakpoint with what the adapter made of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogRow {
    Section(&'static str),
    Line { path: String, entry: BreakpointEntry, status: Option<BreakpointStatus> },
    Special { bp: SpecialBreakpoint, status: Option<BreakpointStatus> },
}

impl DialogRow {
    /// The breakpoint's id; `None` for a section.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Section(_) => None,
            Self::Line { entry, .. } => Some(&entry.id),
            Self::Special { bp, .. } => Some(bp.id()),
        }
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Section(_) => false,
            Self::Line { entry, .. } => entry.enabled,
            Self::Special { bp, .. } => bp.enabled(),
        }
    }

    /// Where Go to Source opens; `None` for a method or watchpoint without a source location.
    fn source(&self) -> Option<(String, u32)> {
        match self {
            Self::Section(_) => None,
            Self::Line { path, entry, .. } => Some((path.clone(), entry.line)),
            Self::Special { bp, .. } => {
                let path = match bp {
                    SpecialBreakpoint::Method(method) => method.path.clone(),
                    SpecialBreakpoint::Field(field) => field.path.clone(),
                };
                path.zip(bp.line())
            }
        }
    }

    fn status(&self) -> Option<&BreakpointStatus> {
        match self {
            Self::Section(_) => None,
            Self::Line { status, .. } | Self::Special { status, .. } => status.as_ref(),
        }
    }
}

/// The name a method breakpoint or watchpoint is listed under.
pub fn special_title(bp: &SpecialBreakpoint) -> String {
    match bp {
        SpecialBreakpoint::Method(method) if method.name.is_empty() => "(no method)".to_owned(),
        SpecialBreakpoint::Method(method) => method.name.clone(),
        SpecialBreakpoint::Field(field) => field.label.clone(),
    }
}

fn special_icon(bp: &SpecialBreakpoint) -> &'static str {
    match bp {
        SpecialBreakpoint::Method(_) => "breakpoint-method",
        SpecialBreakpoint::Field(_) => "breakpoint-field",
    }
}

/// The list as the debugger holds it: line breakpoints by path, then method breakpoints, then
/// watchpoints, each group under its title and left out when empty.
fn dialog_rows(debugger: &Debugger) -> Vec<DialogRow> {
    let breakpoints = &debugger.breakpoints;
    let lines: Vec<DialogRow> = breakpoints
        .lines
        .by_file()
        .iter()
        .flat_map(|(path, list)| {
            list.iter().map(|entry| DialogRow::Line {
                path: path.clone(),
                entry: entry.clone(),
                status: breakpoints.lines.status(&entry.id).cloned(),
            })
        })
        .collect();
    let special = |bp: SpecialBreakpoint| {
        let status = breakpoints.special.status(bp.id()).cloned();
        DialogRow::Special { bp, status }
    };
    let methods: Vec<DialogRow> =
        breakpoints.special.methods().iter().cloned().map(SpecialBreakpoint::Method).map(special).collect();
    let fields: Vec<DialogRow> =
        breakpoints.special.fields().iter().cloned().map(SpecialBreakpoint::Field).map(special).collect();
    let mut rows = Vec::new();
    for (title, list) in [("Line Breakpoints", lines), ("Method Breakpoints", methods), ("Field Watchpoints", fields)] {
        if !list.is_empty() {
            rows.push(DialogRow::Section(title));
            rows.extend(list);
        }
    }
    rows
}

/// A list row's text: the breakpoint's name, where it is, and its marks (`if`, `log`, `temp`,
/// `unverified`).
struct RowParts {
    name: String,
    place: Option<String>,
    marks: Vec<String>,
}

/// The translated texts a frame of the detail pane draws.
struct DetailLabels {
    go: String,
    remove: String,
    enabled: String,
    suspend: String,
    condition: String,
    hit_count: String,
    log_hit: String,
    log_eval: String,
    temporary: String,
    method: String,
    suspend_on: String,
    modification: String,
    access: String,
    verified: String,
    not_verified: String,
    /// "Watching Order.total" for the current watchpoint.
    watching: Option<String>,
}

fn trimmed(text: &str) -> Option<String> {
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// The Breakpoints dialog.
pub struct BreakpointsDialog {
    debugger: Entity<Debugger>,
    rows: Rc<Vec<DialogRow>>,
    muted: bool,
    /// A debug session is live: breakpoints it did not verify are marked.
    live: bool,
    root: String,
    exceptions: Vec<ExceptionGroup>,
    /// The breakpoint the dialog opened on.
    initial: Option<String>,
    selected: Option<String>,
    /// The breakpoint the fields were filled for.
    loaded: Option<String>,
    /// Evaluate and log is checked (its template field shows).
    log_eval: bool,
    condition: Entity<TextField>,
    hit_count: Entity<TextField>,
    log_message: Entity<TextField>,
    method_name: Entity<TextField>,
    focus: FocusHandle,
    key_claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BreakpointsDialogEvent> for BreakpointsDialog {}

impl BreakpointsDialog {
    /// The dialog over `debugger`, on the breakpoint `select` names (else the first). Call
    /// [`BreakpointsDialog::opened`] once it shows.
    pub fn new(debugger: Entity<Debugger>, select: Option<BreakpointSelection>, cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| TextField::new(cx).with_placeholder(placeholder))
        };
        let condition = field("e.g. order.total > 100", cx);
        let hit_count = field("e.g. 5 or >= 3", cx);
        let log_message = field("Value: {expression}", cx);
        let method_name = field("com.example.Service#process, main.run, parse", cx);
        let mut subscriptions = vec![cx.observe(&debugger, |dialog, _, cx| dialog.refresh(cx))];
        for entity in [&condition, &hit_count, &log_message, &method_name] {
            subscriptions.push(cx.subscribe(entity, |dialog: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {}
                TextFieldEvent::Submitted => dialog.commit(cx),
                TextFieldEvent::Cancelled => dialog.close(cx),
            }));
        }
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let mut dialog = Self {
            debugger,
            rows: Rc::default(),
            muted: false,
            live: false,
            root: String::new(),
            exceptions: Vec::new(),
            initial: select.map(|select| select.id),
            selected: None,
            loaded: None,
            log_eval: false,
            condition,
            hit_count,
            log_message,
            method_name,
            focus,
            key_claim,
            _subscriptions: subscriptions,
        };
        dialog.refresh(cx);
        dialog
    }

    /// The dialog shows: the exception filters of the live sessions are asked for.
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        cx.emit(BreakpointsDialogEvent::RefreshExceptionFilters);
    }

    /// Selects breakpoint `id` (a method breakpoint just added), keeping what was typed for the last.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.commit(cx);
        self.selected = Some(id.to_owned());
        self.load_fields(cx);
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    pub fn rows(&self) -> &[DialogRow] {
        &self.rows
    }

    /// The rows as text: a section's title, or a breakpoint's name, place and marks.
    pub fn row_texts(&self, i18n: &I18n) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let parts = self.row_parts(row, i18n);
                let mut shown: Vec<String> = vec![parts.name];
                shown.extend(parts.place);
                shown.extend(parts.marks);
                shown.join("  ")
            })
            .collect()
    }

    fn row_parts(&self, row: &DialogRow, i18n: &I18n) -> RowParts {
        match row {
            DialogRow::Section(title) => RowParts { name: i18n.tx(title), place: None, marks: Vec::new() },
            DialogRow::Line { path, entry, .. } => {
                let mut marks = Vec::new();
                if is_conditional(entry) {
                    marks.push((if entry.suspend { "if" } else { "log" }).to_owned());
                }
                if entry.temporary {
                    marks.push(i18n.tx("temp"));
                }
                RowParts {
                    name: format!("{}:{}", basename(path), entry.line + 1),
                    place: Some(relative_path(&self.root, path).into_owned()),
                    marks,
                }
            }
            DialogRow::Special { bp, .. } => RowParts {
                name: special_title(bp),
                place: self.special_place(bp),
                marks: self.unverified(row).then(|| i18n.tx("unverified")).into_iter().collect(),
            },
        }
    }

    /// The breakpoint whose properties show.
    pub fn current(&self) -> Option<&DialogRow> {
        let wanted = self.selected.as_deref().or(self.initial.as_deref());
        let items = || self.rows.iter().filter(|row| row.id().is_some());
        items().find(|row| row.id().is_some_and(|id| Some(id) == wanted)).or_else(|| items().next())
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn exception_groups(&self) -> &[ExceptionGroup] {
        &self.exceptions
    }

    /// Evaluate and log is checked.
    pub fn log_eval(&self) -> bool {
        self.log_eval
    }

    pub fn condition_field(&self) -> &Entity<TextField> {
        &self.condition
    }

    pub fn hit_count_field(&self) -> &Entity<TextField> {
        &self.hit_count
    }

    pub fn log_message_field(&self) -> &Entity<TextField> {
        &self.log_message
    }

    pub fn method_name_field(&self) -> &Entity<TextField> {
        &self.method_name
    }

    /// What the empty list says; `None` while it lists breakpoints.
    pub fn empty_text(&self) -> Option<&'static str> {
        self.rows.is_empty().then_some("No breakpoints. Click a line number to add one.")
    }

    // ---------------------------------------------------------------------------------------------
    // Internals

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let debugger = self.debugger.read(cx);
        let breakpoints = &debugger.breakpoints;
        self.rows = Rc::new(dialog_rows(debugger));
        self.muted = breakpoints.lines.muted();
        self.live = debugger.state.active_session().is_some_and(|session| session.is_live());
        self.root = breakpoints.lines.root().unwrap_or_default().to_owned();
        self.exceptions = exception_groups(breakpoints.exceptions.known(), breakpoints.exceptions.choices());
        let current = self.current().and_then(DialogRow::id).map(str::to_owned);
        if current != self.loaded {
            self.load_fields(cx);
        }
        cx.notify();
    }

    fn special_place(&self, bp: &SpecialBreakpoint) -> Option<String> {
        let path = match bp {
            SpecialBreakpoint::Method(method) => method.path.as_deref(),
            SpecialBreakpoint::Field(field) => field.path.as_deref(),
        }?;
        Some(match bp.line() {
            Some(line) => format!("{}:{}", basename(path), line + 1),
            None => basename(path).into_owned(),
        })
    }

    fn unverified(&self, row: &DialogRow) -> bool {
        self.live && row.status().is_some_and(|status| !status.verified)
    }

    /// Fills the fields with the current breakpoint's text.
    fn load_fields(&mut self, cx: &mut Context<Self>) {
        let current = self.current().cloned();
        let (condition, hit, log, name) = match &current {
            Some(DialogRow::Line { entry, .. }) => {
                (entry.condition.clone(), entry.hit_condition.clone(), entry.log_message.clone(), None)
            }
            Some(DialogRow::Special { bp: SpecialBreakpoint::Method(method), .. }) => {
                (method.condition.clone(), method.hit_condition.clone(), None, Some(method.name.clone()))
            }
            Some(DialogRow::Special { bp: SpecialBreakpoint::Field(field), .. }) => {
                (field.condition.clone(), field.hit_condition.clone(), None, None)
            }
            Some(DialogRow::Section(_)) | None => (None, None, None, None),
        };
        self.log_eval = log.as_deref().is_some_and(|log| !log.trim().is_empty());
        let fill = |field: &Entity<TextField>, text: Option<String>, cx: &mut Context<Self>| {
            field.update(cx, |field, cx| field.set_text(text.as_deref().unwrap_or_default(), cx));
        };
        fill(&self.condition, condition, cx);
        fill(&self.hit_count, hit, cx);
        fill(&self.log_message, log, cx);
        fill(&self.method_name, name, cx);
        self.loaded = current.as_ref().and_then(DialogRow::id).map(str::to_owned);
    }

    /// Keeps what was typed for the breakpoint the fields were filled for.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.clone() else { return };
        let Some(row) = self.rows.iter().find(|row| row.id() == Some(loaded.as_str())).cloned() else { return };
        let condition = trimmed(self.condition.read(cx).text());
        let hit = trimmed(self.hit_count.read(cx).text());
        match row {
            DialogRow::Line { path, entry, .. } => {
                let log = if self.log_eval { trimmed(self.log_message.read(cx).text()) } else { None };
                let changed = |new: Option<String>, old: &Option<String>| (new != *old).then_some(new);
                let patch = LinePatch {
                    condition: changed(condition, &entry.condition),
                    hit_condition: changed(hit, &entry.hit_condition),
                    log_message: changed(log, &entry.log_message),
                    ..LinePatch::default()
                };
                if !patch.is_empty() {
                    cx.emit(BreakpointsDialogEvent::UpdateLine { path, id: entry.id, patch });
                }
            }
            DialogRow::Special { bp, .. } => {
                let (old_condition, old_hit) = match &bp {
                    SpecialBreakpoint::Method(method) => (&method.condition, &method.hit_condition),
                    SpecialBreakpoint::Field(field) => (&field.condition, &field.hit_condition),
                };
                let name = match &bp {
                    SpecialBreakpoint::Method(method) => {
                        let name = self.method_name.read(cx).text().trim().to_owned();
                        (name != method.name).then_some(name)
                    }
                    SpecialBreakpoint::Field(_) => None,
                };
                let patch = SpecialPatch {
                    condition: (condition != *old_condition).then_some(condition),
                    hit_condition: (hit != *old_hit).then_some(hit),
                    name,
                    ..SpecialPatch::default()
                };
                if patch != SpecialPatch::default() {
                    cx.emit(BreakpointsDialogEvent::UpdateSpecial { id: bp.id().to_owned(), patch });
                }
            }
            DialogRow::Section(_) => {}
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.commit(cx);
        cx.emit(BreakpointsDialogEvent::Close);
    }

    fn go_to_source(&mut self, cx: &mut Context<Self>) {
        let Some((path, line)) = self.current().and_then(DialogRow::source) else { return };
        self.commit(cx);
        cx.emit(BreakpointsDialogEvent::GoToSource { path, line });
    }

    fn remove_current(&mut self, cx: &mut Context<Self>) {
        match self.current() {
            Some(DialogRow::Line { path, entry, .. }) => {
                let (path, id) = (path.clone(), entry.id.clone());
                cx.emit(BreakpointsDialogEvent::RemoveLine { path, id });
            }
            Some(DialogRow::Special { bp, .. }) => {
                let id = bp.id().to_owned();
                cx.emit(BreakpointsDialogEvent::RemoveSpecial(id));
            }
            Some(DialogRow::Section(_)) | None => {}
        }
    }

    /// Turns the breakpoint of row `index` on or off.
    fn set_enabled(&mut self, index: usize, enabled: bool, cx: &mut Context<Self>) {
        match self.rows.get(index) {
            Some(DialogRow::Line { path, entry, .. }) => {
                let patch = LinePatch { enabled: Some(enabled), ..LinePatch::default() };
                cx.emit(BreakpointsDialogEvent::UpdateLine { path: path.clone(), id: entry.id.clone(), patch });
            }
            Some(DialogRow::Special { bp, .. }) => {
                let patch = SpecialPatch { enabled: Some(enabled), ..SpecialPatch::default() };
                cx.emit(BreakpointsDialogEvent::UpdateSpecial { id: bp.id().to_owned(), patch });
            }
            Some(DialogRow::Section(_)) | None => {}
        }
    }

    /// A checkbox of the current line breakpoint.
    fn patch_current_line(&mut self, patch: LinePatch, cx: &mut Context<Self>) {
        if let Some(DialogRow::Line { path, entry, .. }) = self.current() {
            let (path, id) = (path.clone(), entry.id.clone());
            cx.emit(BreakpointsDialogEvent::UpdateLine { path, id, patch });
        }
    }

    fn toggle_log_eval(&mut self, cx: &mut Context<Self>) {
        self.log_eval = !self.log_eval;
        self.commit(cx);
        cx.notify();
    }

    /// Field modification / Field access of the current watchpoint.
    fn toggle_access(&mut self, modification: bool, cx: &mut Context<Self>) {
        let Some(DialogRow::Special { bp: SpecialBreakpoint::Field(field), .. }) = self.current() else { return };
        let access = field.access_type;
        let next = if modification {
            if access != Some(AccessType::Read) { AccessType::Read } else { AccessType::ReadWrite }
        } else if matches!(access, Some(AccessType::Read | AccessType::ReadWrite)) {
            AccessType::Write
        } else {
            AccessType::ReadWrite
        };
        let patch = SpecialPatch { access_type: Some(Some(next)), ..SpecialPatch::default() };
        cx.emit(BreakpointsDialogEvent::UpdateSpecial { id: field.id.clone(), patch });
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(id) = self.rows.get(index).and_then(DialogRow::id).map(str::to_owned) {
            self.select(&id, cx);
        }
    }

    fn current_index(&self) -> Option<usize> {
        let id = self.current().and_then(DialogRow::id)?;
        self.rows.iter().position(|row| row.id() == Some(id))
    }

    /// The keys the list handles while it has the keyboard, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let items: Vec<usize> = (0..self.rows.len()).filter(|index| self.rows[*index].id().is_some()).collect();
        let at = self.current_index().and_then(|current| items.iter().position(|index| *index == current));
        match keystroke.key.as_str() {
            "escape" => self.close(cx),
            "down" => {
                if let Some(next) = at.map_or(items.first(), |at| items.get(at + 1)).copied() {
                    self.select_index(next, cx);
                }
            }
            "up" => {
                if let Some(previous) = at.and_then(|at| at.checked_sub(1)).and_then(|at| items.get(at)).copied() {
                    self.select_index(previous, cx);
                }
            }
            "home" => {
                if let Some(first) = items.first().copied() {
                    self.select_index(first, cx);
                }
            }
            "end" => {
                if let Some(last) = items.last().copied() {
                    self.select_index(last, cx);
                }
            }
            "space" => {
                if let Some(index) = self.current_index() {
                    let enabled = self.rows[index].enabled();
                    self.set_enabled(index, !enabled, cx);
                }
            }
            "enter" => self.go_to_source(cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for BreakpointsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A checkbox with its label; `selector` names it for clicks.
fn checkbox(cx: &App, selector: SharedString, label: Option<String>, on: bool) -> Stateful<Div> {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    let name = selector.clone();
    div()
        .id(selector)
        .debug_selector(move || name.to_string())
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
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .children(label.map(SharedString::from))
}

/// A small button of the detail pane or the footer.
fn button(cx: &App, selector: &'static str, label: String, primary: bool) -> Stateful<Div> {
    let face = div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .whitespace_nowrap()
        .child(SharedString::from(label));
    if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.min_w(px(72.0)).bg(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.text_color(theme::color(cx, "fg-strong")).hover(|el| el.bg(theme::color(cx, "bg-hover")))
    }
}

/// A header icon button, greyed when it does nothing now.
fn icon_button(cx: &App, selector: &'static str, icon: &str, enabled: bool) -> Stateful<Div> {
    let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .size(px(22.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A labelled text field.
fn field_row(cx: &App, selector: &'static str, label: String, field: &Entity<TextField>) -> Div {
    div().flex().flex_col().gap(px(4.0)).child(div().text_color(theme::color(cx, "fg-subtle")).child(label)).child(
        div()
            .debug_selector(move || selector.to_owned())
            .w_full()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .child(field.clone()),
    )
}

/// The breakpoint mark of a line row: filled when on, a ring when off or muted, grey when the
/// live session did not verify it.
fn dot(cx: &App, enabled: bool, unverified: bool) -> Div {
    let danger = theme::color(cx, "danger");
    let mark = div().flex_none().size(px(9.0)).rounded(px(4.5));
    if !enabled {
        mark.border_1().border_color(danger)
    } else if unverified {
        mark.bg(theme::color(cx, "fg-gutter"))
    } else {
        mark.bg(danger)
    }
}

impl Render for BreakpointsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &mut cx.default_global::<Localization>().0;
        let watching = match self.current() {
            Some(DialogRow::Special { bp: SpecialBreakpoint::Field(field), .. }) => {
                Some(i18n.tx_built(&["Watching ", ""], &[Param::from(field.data_id.as_str())]))
            }
            _ => None,
        };
        let parts: Vec<RowParts> = self.rows.iter().map(|row| self.row_parts(row, i18n)).collect();
        let tx = |text: &str| i18n.tx(text);
        let labels = DetailLabels {
            go: tx("Go to Source"),
            remove: tx("Remove"),
            enabled: tx("Enabled"),
            suspend: tx("Suspend"),
            condition: tx("Condition"),
            hit_count: tx("Hit count"),
            log_hit: tx("\"Breakpoint hit\" message"),
            log_eval: tx("Evaluate and log"),
            temporary: tx("Remove once hit"),
            method: tx("Method"),
            suspend_on: tx("Suspend on"),
            modification: tx("Field modification"),
            access: tx("Field access"),
            verified: tx("Verified"),
            not_verified: tx("Not verified"),
            watching,
        };
        let [title, mute, exceptions_title, exceptions_empty, done] = [
            "Breakpoints",
            "Mute breakpoints",
            "Exception Breakpoints",
            "Start a debug session to see the exception filters of its adapter.",
            "Done",
        ]
        .map(tx);
        let empty = self.empty_text().map(tx);
        let (border, subtle) = (theme::color(cx, "border-popup"), theme::color(cx, "fg-subtle"));
        let has_items = self.rows.iter().any(|row| row.id().is_some());

        let header =
            div()
                .flex_none()
                .h(theme::length(cx, "h-header"))
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(12.0))
                .pr(px(8.0))
                .border_b_1()
                .border_color(border)
                .child(checkbox(cx, "breakpoints-mute".into(), Some(mute), self.muted).on_click(cx.listener(
                    |dialog, _: &ClickEvent, _, cx| cx.emit(BreakpointsDialogEvent::SetMuted(!dialog.muted)),
                )))
                .child(div().flex_1())
                .child(icon_button(cx, "breakpoints-add", "plus", true).on_click(
                    cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(BreakpointsDialogEvent::AddMethodBreakpoint)),
                ))
                .child(icon_button(cx, "breakpoints-remove-all", "trash", has_items).when(has_items, |el| {
                    el.on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(BreakpointsDialogEvent::RemoveAll)))
                }));

        let list = match empty {
            Some(empty) => div()
                .debug_selector(|| "breakpoints-empty".to_owned())
                .flex_1()
                .p(px(12.0))
                .text_color(subtle)
                .child(SharedString::from(empty))
                .into_any_element(),
            None => div()
                .id("breakpoints-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_focus(&self.focus)
                .children(
                    self.rows
                        .iter()
                        .zip(&parts)
                        .enumerate()
                        .map(|(index, (row, parts))| self.list_row(index, row, parts, cx)),
                )
                .into_any_element(),
        };

        let several = self.exceptions.len() > 1;
        let exceptions = div()
            .id("breakpoints-exceptions")
            .flex_none()
            .max_h(px(170.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .p(px(10.0))
            .border_t_1()
            .border_color(border)
            .child(div().text_color(theme::color(cx, "fg-emphasis")).child(SharedString::from(exceptions_title)))
            .when(self.exceptions.is_empty(), |el| el.child(div().text_color(subtle).child(exceptions_empty)))
            .children(self.exceptions.iter().enumerate().map(|(group_index, group)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .when(several, |el| el.child(div().pt(px(4.0)).text_color(subtle).child(group.adapter.clone())))
                    .children(group.filters.iter().enumerate().map(|(filter_index, filter)| {
                        let selector = format!("breakpoints-exception-{group_index}-{filter_index}");
                        let adapter = group.adapter.clone();
                        let (id, enabled) = (filter.filter.filter.clone(), filter.enabled);
                        let label = Some(filter.filter.label.clone());
                        checkbox(cx, selector.into(), label, enabled).on_click(cx.listener(
                            move |_, _: &ClickEvent, _, cx| {
                                cx.emit(BreakpointsDialogEvent::SetExceptionFilter {
                                    adapter: adapter.clone(),
                                    filter: id.clone(),
                                    enabled: !enabled,
                                });
                            },
                        ))
                    }))
            }));

        let list_pane = div()
            .w(px(LIST_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(border)
            .child(header)
            .child(list)
            .child(exceptions);
        let detail = div()
            .id("breakpoints-detail")
            .debug_selector(|| "breakpoints-detail".to_owned())
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .children(self.detail(labels, cx));
        let footer =
            div().flex_none().flex().justify_end().px(px(14.0)).py(px(10.0)).border_t_1().border_color(border).child(
                button(cx, "breakpoints-done", done, true)
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.close(cx))),
            );
        let body =
            div().h(px(BODY_HEIGHT)).flex().min_h_0().border_t_1().border_color(border).child(list_pane).child(detail);
        div()
            .debug_selector(|| "breakpoints-dialog".to_owned())
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(border)
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(
                div()
                    .flex_none()
                    .px(px(14.0))
                    .py(px(10.0))
                    .text_color(theme::color(cx, "fg-emphasis"))
                    .child(SharedString::from(title)),
            )
            .child(body)
            .child(footer)
    }
}

impl BreakpointsDialog {
    fn list_row(&self, index: usize, row: &DialogRow, parts: &RowParts, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.current_index() == Some(index);
        let line = div()
            .id(SharedString::from(format!("breakpoints-row-{index}")))
            .debug_selector(move || format!("breakpoints-row-{index}"))
            .h(theme::length(cx, "h-row"))
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .whitespace_nowrap()
            .overflow_hidden();
        if let DialogRow::Section(_) = row {
            return line
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(parts.name.clone()));
        }
        let enabled = row.enabled();
        let check = checkbox(cx, format!("breakpoints-row-check-{index}").into(), None, enabled).on_click(cx.listener(
            move |dialog, _: &ClickEvent, _, cx| {
                dialog.set_enabled(index, !enabled, cx);
                cx.stop_propagation();
            },
        ));
        let icon = match row {
            DialogRow::Special { bp, .. } => {
                theme::icon(cx, special_icon(bp), 14.0, theme::color(cx, "fg-muted")).into_any_element()
            }
            DialogRow::Line { .. } | DialogRow::Section(_) => {
                dot(cx, enabled && !self.muted, self.unverified(row)).into_any_element()
            }
        };
        let place = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .justify_end()
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-subtle"))
            .children(parts.place.clone());
        let keyword = theme::color(cx, "syn-keyword");
        let tiny = theme::length(cx, "fs-tiny");
        line.child(check)
            .child(icon)
            .child(div().flex_none().text_color(theme::color(cx, "fg-emphasis")).child(parts.name.clone()))
            .child(place)
            .children(
                parts
                    .marks
                    .iter()
                    .map(|mark| div().flex_none().text_size(tiny).text_color(keyword).child(mark.clone())),
            )
            .when(selected, |el| el.bg(theme::color(cx, "bg-list-selected")))
            .on_click(cx.listener(move |dialog, event: &ClickEvent, window, cx| {
                window.focus(&dialog.focus, cx);
                dialog.select_index(index, cx);
                if event.click_count() >= 2 {
                    dialog.go_to_source(cx);
                }
            }))
    }

    /// A checkbox of the current line breakpoint that applies `patch`.
    fn line_box(
        &self,
        selector: &'static str,
        label: String,
        checked: bool,
        patch: LinePatch,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        checkbox(cx, selector.into(), Some(label), checked)
            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.patch_current_line(patch.clone(), cx)))
    }

    /// The properties of the current breakpoint.
    fn detail(&self, labels: DetailLabels, cx: &mut Context<Self>) -> Option<Div> {
        let row = self.current()?.clone();
        let subtle = theme::color(cx, "fg-subtle");
        let (title, icon) = match &row {
            DialogRow::Line { path, entry, .. } => (format!("{}:{}", basename(path), entry.line + 1), None),
            DialogRow::Special { bp, .. } => (special_title(bp), Some(special_icon(bp))),
            DialogRow::Section(_) => return None,
        };
        let go = row.source().is_some().then(|| {
            button(cx, "breakpoints-go-to-source", labels.go.clone(), false)
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.go_to_source(cx)))
        });
        let title_bar = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .children(icon.map(|icon| theme::icon(cx, icon, 14.0, theme::color(cx, "fg-muted"))))
            .child(div().text_color(theme::color(cx, "fg-emphasis")).child(SharedString::from(title)))
            .child(div().flex_1())
            .children(go)
            .child(
                button(cx, "breakpoints-remove", labels.remove.clone(), false)
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.remove_current(cx))),
            );
        let status = row.status().map(|status| {
            let text = match (status.verified, &status.message) {
                (true, _) => labels.verified.clone(),
                (false, Some(message)) => message.clone(),
                (false, None) => labels.not_verified.clone(),
            };
            div()
                .debug_selector(|| "breakpoints-status".to_owned())
                .text_size(theme::length(cx, "fs-small"))
                .text_color(subtle)
                .child(SharedString::from(text))
        });
        let pane = div().flex().flex_col().gap(px(10.0)).px(px(14.0)).py(px(12.0)).child(title_bar).children(status);
        let pane = match &row {
            DialogRow::Line { entry, .. } => {
                let (on, suspends, hit, temp) = (entry.enabled, entry.suspend, entry.log_hit, entry.temporary);
                let patch = LinePatch::default;
                let log_field = self.log_eval.then(|| {
                    div()
                        .debug_selector(|| "breakpoints-log-message".to_owned())
                        .w_full()
                        .h(px(FIELD_HEIGHT))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border-strong"))
                        .child(self.log_message.clone())
                });
                pane.child(self.line_box(
                    "breakpoints-enabled",
                    labels.enabled,
                    on,
                    LinePatch { enabled: Some(!on), ..patch() },
                    cx,
                ))
                .child(self.line_box(
                    "breakpoints-suspend",
                    labels.suspend,
                    suspends,
                    LinePatch { suspend: Some(!suspends), ..patch() },
                    cx,
                ))
                .child(field_row(cx, "breakpoints-condition", labels.condition, &self.condition))
                .child(field_row(cx, "breakpoints-hit-count", labels.hit_count, &self.hit_count))
                .child(self.line_box(
                    "breakpoints-log-hit",
                    labels.log_hit,
                    hit,
                    LinePatch { log_hit: Some(!hit), ..patch() },
                    cx,
                ))
                .child(
                    checkbox(cx, "breakpoints-log-eval".into(), Some(labels.log_eval), self.log_eval)
                        .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_log_eval(cx))),
                )
                .children(log_field)
                .child(self.line_box(
                    "breakpoints-temporary",
                    labels.temporary,
                    temp,
                    LinePatch { temporary: Some(!temp), ..patch() },
                    cx,
                ))
            }
            DialogRow::Special { bp, .. } => {
                let id = bp.id().to_owned();
                let on = bp.enabled();
                let enabled = checkbox(cx, "breakpoints-enabled".into(), Some(labels.enabled), on).on_click(
                    cx.listener(move |_, _: &ClickEvent, _, cx| {
                        let patch = SpecialPatch { enabled: Some(!on), ..SpecialPatch::default() };
                        cx.emit(BreakpointsDialogEvent::UpdateSpecial { id: id.clone(), patch });
                    }),
                );
                let pane = pane.child(enabled);
                let pane = match bp {
                    SpecialBreakpoint::Method(_) => {
                        pane.child(field_row(cx, "breakpoints-method-name", labels.method, &self.method_name))
                    }
                    SpecialBreakpoint::Field(field) => {
                        let modifies = field.access_type != Some(AccessType::Read);
                        let reads = matches!(field.access_type, Some(AccessType::Read | AccessType::ReadWrite));
                        let modification =
                            checkbox(cx, "breakpoints-field-modification".into(), Some(labels.modification), modifies)
                                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_access(true, cx)));
                        let access = checkbox(cx, "breakpoints-field-access".into(), Some(labels.access), reads)
                            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_access(false, cx)));
                        pane.children(labels.watching.map(|watching| {
                            div().text_size(theme::length(cx, "fs-small")).text_color(subtle).child(watching)
                        }))
                        .child(div().text_color(subtle).child(labels.suspend_on))
                        .child(div().flex().gap(px(16.0)).child(modification).child(access))
                    }
                };
                pane.child(field_row(cx, "breakpoints-condition", labels.condition, &self.condition)).child(field_row(
                    cx,
                    "breakpoints-hit-count",
                    labels.hit_count,
                    &self.hit_count,
                ))
            }
            DialogRow::Section(_) => return None,
        };
        Some(pane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::breakpoints::model::{BreakpointProps, make_breakpoint};
    use crate::debugger::breakpoints::special::MethodBreakpoint;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BreakpointsDialogEvent>>>;
    type FieldOf = fn(&BreakpointsDialog) -> &Entity<TextField>;

    const A: &str = "/p/a.py";
    const B: &str = "/p/src/b.py";

    /// The project `/p` with line breakpoints in `src/b.py` (line 9, conditional) and `a.py`
    /// (line 3), the method breakpoint `run` declared in `a.py` and the watchpoint `Order.total`.
    fn debugger() -> (Debugger, [String; 4]) {
        let mut debugger = Debugger::default();
        debugger.breakpoints.load(Some("/p"), |_: &str| None, false, 0.0);
        let lines = &mut debugger.breakpoints.lines;
        let props = BreakpointProps { condition: Some("x > 1".to_owned()), ..BreakpointProps::default() };
        let b = lines.add_breakpoint(B, 9, props, 0.0).id;
        let a = lines.add_breakpoint(A, 3, BreakpointProps::default(), 0.0).id;
        let special = &mut debugger.breakpoints.special;
        let method = special.add_method_breakpoint("run", Some((A, 2)), 0.0).id;
        let field = special.add_field_watchpoint("Order.total", "Order.total", None, None, 0.0).id;
        debugger.breakpoints.take_effects();
        (debugger, [a, b, method, field])
    }

    fn open(
        cx: &mut TestAppContext,
        debugger: Debugger,
        select: Option<BreakpointSelection>,
    ) -> (Entity<BreakpointsDialog>, Entity<Debugger>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let model = cx.update(|cx| cx.new(|_| debugger));
        let shared = model.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| BreakpointsDialog::new(shared, select, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &BreakpointsDialogEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            window.activate_window();
        });
        focus_list(&view, cx);
        (view, model, events, cx)
    }

    /// Gives the list the keyboard.
    fn focus_list(view: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn texts(view: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.row_texts(&I18n::default()))
    }

    fn current_id(view: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.current().and_then(DialogRow::id).map(str::to_owned))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn type_into(view: &Entity<BreakpointsDialog>, field: FieldOf, text: &str, cx: &mut VisualTestContext) {
        let field = view.read_with(cx, |view, _| field(view).clone());
        field.update(cx, |field, cx| field.set_text(text, cx));
    }

    fn field_text(view: &Entity<BreakpointsDialog>, field: FieldOf, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| field(view).read(cx).text().to_owned())
    }

    fn last(events: &Events) -> Option<BreakpointsDialogEvent> {
        events.borrow().last().cloned()
    }

    fn update_line(id: &str, path: &str, patch: LinePatch) -> Option<BreakpointsDialogEvent> {
        Some(BreakpointsDialogEvent::UpdateLine { path: path.to_owned(), id: id.to_owned(), patch })
    }

    fn update_special(id: &str, patch: SpecialPatch) -> Option<BreakpointsDialogEvent> {
        Some(BreakpointsDialogEvent::UpdateSpecial { id: id.to_owned(), patch })
    }

    fn patch() -> LinePatch {
        LinePatch::default()
    }

    // Not from a spec: the list groups the breakpoints by kind, line breakpoints by file with their
    // folder and marks, and opens on the breakpoint at the caret.
    #[gpui_kit::test]
    async fn lists_breakpoints_by_kind_and_opens_on_the_one_at_the_caret(cx: &mut TestAppContext) {
        let (debugger, [a, b, method, _]) = debugger();
        let select = BreakpointSelection { path: B.to_owned(), id: b.clone() };
        let (view, _, _, cx) = open(cx, debugger, Some(select));
        assert_eq!(
            texts(&view, cx),
            [
                "Line Breakpoints",
                "a.py:4  a.py",
                "b.py:10  src/b.py  if",
                "Method Breakpoints",
                "run  a.py:3",
                "Field Watchpoints",
                "Order.total",
            ]
        );
        assert_eq!(current_id(&view, cx), Some(b));
        assert_eq!(field_text(&view, BreakpointsDialog::condition_field, cx), "x > 1");
        cx.simulate_keystrokes("up");
        assert_eq!(current_id(&view, cx), Some(a));
        cx.simulate_keystrokes("down down");
        assert_eq!(current_id(&view, cx), Some(method), "the section title is skipped");
        assert_eq!(field_text(&view, BreakpointsDialog::method_name_field, cx), "run");
    }

    // Not from a spec: the checkboxes of a line breakpoint change it, and what was typed is kept
    // when another breakpoint is selected or the dialog closes.
    #[gpui_kit::test]
    async fn edits_a_line_breakpoint(cx: &mut TestAppContext) {
        let (debugger, [a, b, _, _]) = debugger();
        let (view, _, events, cx) = open(cx, debugger, None);
        assert_eq!(current_id(&view, cx), Some(a.clone()));
        click(cx, "breakpoints-enabled");
        assert_eq!(last(&events), update_line(&a, A, LinePatch { enabled: Some(false), ..patch() }));
        click(cx, "breakpoints-suspend");
        assert_eq!(last(&events), update_line(&a, A, LinePatch { suspend: Some(false), ..patch() }));
        click(cx, "breakpoints-log-hit");
        assert_eq!(last(&events), update_line(&a, A, LinePatch { log_hit: Some(true), ..patch() }));
        click(cx, "breakpoints-temporary");
        assert_eq!(last(&events), update_line(&a, A, LinePatch { temporary: Some(true), ..patch() }));
        click(cx, "breakpoints-row-check-2");
        assert_eq!(last(&events), update_line(&b, B, LinePatch { enabled: Some(false), ..patch() }));
        assert_eq!(current_id(&view, cx), Some(a.clone()), "a row's checkbox does not select it");
        assert!(cx.debug_bounds("breakpoints-log-message").is_none());
        click(cx, "breakpoints-log-eval");
        assert!(view.read_with(cx, |view, _| view.log_eval()));
        assert!(cx.debug_bounds("breakpoints-log-message").is_some());
        type_into(&view, BreakpointsDialog::condition_field, "  i > 2 ", cx);
        type_into(&view, BreakpointsDialog::log_message_field, "i={i}", cx);
        focus_list(&view, cx);
        cx.simulate_keystrokes("down");
        let typed = LinePatch {
            condition: Some(Some("i > 2".to_owned())),
            log_message: Some(Some("i={i}".to_owned())),
            ..patch()
        };
        assert!(events.borrow().contains(&update_line(&a, A, typed).expect("an update")));
        assert!(!view.read_with(cx, |view, _| view.log_eval()), "b.py logs nothing");
        type_into(&view, BreakpointsDialog::hit_count_field, "3", cx);
        click(cx, "breakpoints-done");
        let counted = LinePatch { hit_condition: Some(Some("3".to_owned())), ..patch() };
        let tail: Vec<BreakpointsDialogEvent> = events.borrow().iter().rev().take(2).rev().cloned().collect();
        let expected = [update_line(&b, B, counted), Some(BreakpointsDialogEvent::Close)];
        assert_eq!(tail, expected.into_iter().flatten().collect::<Vec<_>>());
    }

    // Not from a spec: a method breakpoint's name and a watchpoint's accesses change them; Go to
    // Source opens where the breakpoint is, Remove removes it.
    #[gpui_kit::test]
    async fn edits_method_breakpoints_and_watchpoints(cx: &mut TestAppContext) {
        let (debugger, [_, _, method, field]) = debugger();
        let select = BreakpointSelection { path: String::new(), id: method.clone() };
        let (view, _, events, cx) = open(cx, debugger, Some(select));
        click(cx, "breakpoints-go-to-source");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::GoToSource { path: A.to_owned(), line: 2 }));
        type_into(&view, BreakpointsDialog::method_name_field, " main.run ", cx);
        view.update(cx, |view, cx| view.select(&field, cx));
        let renamed = SpecialPatch { name: Some("main.run".to_owned()), ..SpecialPatch::default() };
        assert!(events.borrow().contains(&update_special(&method, renamed).expect("an update")));
        cx.run_until_parked();
        assert!(cx.debug_bounds("breakpoints-go-to-source").is_none(), "the watchpoint has no source");
        let access = |access: AccessType| SpecialPatch { access_type: Some(Some(access)), ..SpecialPatch::default() };
        click(cx, "breakpoints-field-access");
        assert_eq!(last(&events), update_special(&field, access(AccessType::ReadWrite)));
        click(cx, "breakpoints-field-modification");
        assert_eq!(last(&events), update_special(&field, access(AccessType::Read)));
        click(cx, "breakpoints-remove");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::RemoveSpecial(field)));
    }

    // Not from a spec: Mute, Add Method Breakpoint, Remove All and the exception filters ask the
    // workbench; the dialog asks for the live sessions' filters when it shows; an empty project
    // says how to add a breakpoint.
    #[gpui_kit::test]
    async fn mutes_adds_removes_and_sets_exception_filters(cx: &mut TestAppContext) {
        let (view, model, events, cx) = open(cx, Debugger::default(), None);
        let empty = view.read_with(cx, |view, _| view.empty_text());
        assert_eq!(empty, Some("No breakpoints. Click a line number to add one."));
        click(cx, "breakpoints-remove-all");
        assert!(events.borrow().is_empty(), "nothing to remove");
        view.update(cx, |view, cx| view.opened(cx));
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::RefreshExceptionFilters));
        click(cx, "breakpoints-mute");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::SetMuted(true)));
        click(cx, "breakpoints-add");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::AddMethodBreakpoint));
        let added = model.update(cx, |debugger, cx| {
            let id = debugger.breakpoints.special.add_method_breakpoint("", None, 0.0).id;
            cx.notify();
            id
        });
        view.update(cx, |view, cx| view.select(&added, cx));
        assert_eq!(texts(&view, cx), ["Method Breakpoints", "(no method)"]);
        assert_eq!(current_id(&view, cx), Some(added));
        click(cx, "breakpoints-remove-all");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::RemoveAll));
        let first = view.read_with(cx, |view, _| view.exception_groups()[0].clone());
        click(cx, "breakpoints-exception-0-0");
        let filter = &first.filters[0];
        assert_eq!(
            last(&events),
            Some(BreakpointsDialogEvent::SetExceptionFilter {
                adapter: first.adapter.clone(),
                filter: filter.filter.filter.clone(),
                enabled: !filter.enabled,
            })
        );
    }

    // Not from a spec: Space turns the selected breakpoint off, Enter goes to its source, Escape
    // closes.
    #[gpui_kit::test]
    async fn the_keyboard_toggles_opens_and_closes(cx: &mut TestAppContext) {
        let (debugger, [a, _, _, _]) = debugger();
        let (_, _, events, cx) = open(cx, debugger, None);
        cx.simulate_keystrokes("space");
        assert_eq!(last(&events), update_line(&a, A, LinePatch { enabled: Some(false), ..patch() }));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::GoToSource { path: A.to_owned(), line: 3 }));
        cx.simulate_keystrokes("escape");
        assert_eq!(last(&events), Some(BreakpointsDialogEvent::Close));
    }

    // Not from a spec: a patch changes only what it names, and a method breakpoint without a name
    // is listed as "(no method)".
    #[test]
    fn a_line_patch_changes_only_what_it_names() {
        let mut entry = make_breakpoint(4, BreakpointProps::default());
        let before = entry.clone();
        LinePatch::default().apply(&mut entry);
        assert_eq!(entry, before);
        let patch = LinePatch {
            suspend: Some(false),
            condition: Some(Some("x".to_owned())),
            log_message: Some(None),
            ..LinePatch::default()
        };
        patch.apply(&mut entry);
        assert_eq!((entry.suspend, entry.condition.as_deref(), entry.enabled), (false, Some("x"), true));
        let unnamed = MethodBreakpoint {
            id: "m".to_owned(),
            name: String::new(),
            path: None,
            line: None,
            enabled: true,
            condition: None,
            hit_condition: None,
        };
        assert_eq!(special_title(&SpecialBreakpoint::Method(unnamed)), "(no method)");
    }
}
