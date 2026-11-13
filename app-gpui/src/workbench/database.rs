//! The Database tool window of a project: the data source tree, the result pane and the table
//! editors below it, the Data Source dialog, and the connection store's commands carried out
//! against the backend.
//!
//! The store ([`DbStore`]) is a step machine: every step returns [`Command`]s. Backend requests run
//! off the window's thread and answer the store through its ticket; questions are dialogs of a
//! layer of their own, one at a time, each answered exactly once. The editor area holds file tabs
//! only, so the result pane and the table editors share the lower half of the tool window, with a
//! strip to switch between them.

use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::{Panel, Workbench};
use crate::backend::Backend;
use crate::database::dialogs::blank_source;
use crate::database::grid_view::{GridEvent, GridView};
use crate::database::model::new_source_id;
use crate::database::nodes::{DbNode, NodeKind, TableAt};
use crate::database::objects::reference;
use crate::database::results_view::{ResultsEvent, ResultsView};
use crate::database::source_dialog::{PasswordField, SourceDialog, SourceDialogEvent, SourceForm};
use crate::database::store::{
    Answer, BINDINGS_KEY, CONSOLE_SCHEMAS_KEY, Command, DbOp, DbStore, Env, Level, Prompt, PromptAnswer, ResultsUpdate,
    SecretText, Ticket, TreeUpdate, console_append,
};
use crate::database::table_editor::SourceCaps;
use crate::database::table_editor_view::{TableEditorEvent, TableEditorView, TableRef};
use crate::database::tree_view::{DATABASE_TOOL_WINDOW, DatabaseTree, TreeAction, TreeCommand, TreeEvent};
use crate::editor::view::Editor;
use crate::platform::actions::{Action, ActionRegistry, manifest};
use crate::platform::context::set_context;
use crate::platform::i18n::Localization;
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, FieldRule, OK, PromptDialog, PromptDialogEvent};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, ConfirmRequest, PromptRequest};
use gpui_kit::{
    AnyElement, AnyView, AnyWindowHandle, App, ClickEvent, ClipboardItem, Context, Div, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, MouseButton, PathPromptOptions, Render, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};
use ide_core::{IdeError, IdeResult, RequestId};
use ide_db::DataSource;
use ide_host::Host;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How often idle connections are looked for.
const TICK_MS: u64 = 5000;
/// How long an append to a console waits for its editor to open: steps of [`EDITOR_WAIT_MS`].
const EDITOR_WAIT_STEPS: u32 = 200;
const EDITOR_WAIT_MS: u64 = 50;
/// The `when` keys of the table editor's Submit and Revert.
const TABLE_EDITOR_FOCUS: &str = "dbTableEditorFocus";
const TABLE_EDITOR_CHANGES: &str = "dbTableEditorChanges";
/// The `when` key of the tree's actions, and the key that holds while the tree has the keyboard.
const PROJECT_OPEN: &str = "projectOpen";
pub(super) const DATABASE_VIEW_FOCUS: &str = "databaseViewFocus";
/// The language id of SQL editors.
const SQL_LANGUAGE: &str = "sql";

/// Tells the slots of successive projects apart: an answer for an earlier one is dropped.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// A fresh random part for a data source id.
fn random_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn u32_of(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn len16(text: &str) -> u32 {
    u32_of(text.encode_utf16().count() as u64)
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The string argument of an action: a tree node's key.
fn text_arg(arg: Option<&serde_json::Value>) -> Option<String> {
    arg.and_then(serde_json::Value::as_str).map(str::to_owned)
}

/// The name of a store request, for the log: requests carry passwords and are never printed whole.
fn op_name(op: &DbOp) -> &'static str {
    match op {
        DbOp::LoadSources => "LoadSources",
        DbOp::SaveSources(_) => "SaveSources",
        DbOp::SetPassword { .. } => "SetPassword",
        DbOp::SetSshSecret { .. } => "SetSshSecret",
        DbOp::TrustSshHost { .. } => "TrustSshHost",
        DbOp::Connect { .. } => "Connect",
        DbOp::Disconnect { .. } => "Disconnect",
        DbOp::SetManualCommit { .. } => "SetManualCommit",
        DbOp::FinishTransaction { .. } => "FinishTransaction",
        DbOp::Databases { .. } => "Databases",
        DbOp::Schemas { .. } => "Schemas",
        DbOp::Tables { .. } => "Tables",
        DbOp::Details { .. } => "Details",
        DbOp::Ddl { .. } => "Ddl",
        DbOp::ObjectDdl { .. } => "ObjectDdl",
        DbOp::Completion { .. } => "Completion",
        DbOp::Execute { .. } => "Execute",
        DbOp::Explain { .. } => "Explain",
        DbOp::History { .. } => "History",
        DbOp::RemoveHistory { .. } => "RemoveHistory",
        DbOp::ConsolePath { .. } => "ConsolePath",
        DbOp::WriteFile { .. } => "WriteFile",
        DbOp::TablePage { .. } => "TablePage",
        DbOp::ApplyEdits { .. } => "ApplyEdits",
    }
}

/// The backend command a store request stands for, by which the trust of the window's project is
/// checked: a project in Restricted Mode reaches no database, while console paths and exports,
/// which run nothing, stay allowed.
fn ipc_command(op: &DbOp) -> &'static str {
    match op {
        DbOp::LoadSources => "db_sources",
        DbOp::SaveSources(_) => "db_save_sources",
        DbOp::SetPassword { .. } => "db_set_password",
        DbOp::SetSshSecret { .. } => "db_set_ssh_secret",
        DbOp::TrustSshHost { .. } => "db_trust_ssh_host",
        DbOp::Connect { .. } => "db_connect",
        DbOp::Disconnect { .. } => "db_disconnect",
        DbOp::SetManualCommit { .. } => "db_set_manual_commit",
        DbOp::FinishTransaction { .. } => "db_finish_transaction",
        DbOp::Databases { .. } => "db_databases",
        DbOp::Schemas { .. } => "db_schemas",
        DbOp::Tables { .. } => "db_tables",
        DbOp::Details { .. } => "db_table_details",
        DbOp::Ddl { .. } => "db_ddl",
        DbOp::ObjectDdl { .. } => "db_object_ddl",
        DbOp::Completion { .. } => "db_completion",
        DbOp::Execute { .. } => "db_execute",
        DbOp::Explain { .. } => "db_explain",
        DbOp::History { .. } => "db_history",
        DbOp::RemoveHistory { .. } => "db_remove_history",
        DbOp::ConsolePath { .. } => "db_console_path",
        DbOp::WriteFile { .. } => "db_export_file",
        DbOp::TablePage { .. } => "db_table_page",
        DbOp::ApplyEdits { .. } => "db_apply_edits",
    }
}

/// Carries out a store request on the backend; `request` is the id a cancellable one runs under.
async fn run_op(host: Arc<Host>, label: String, op: DbOp, request: Option<RequestId>) -> Answer {
    use ide_host::ops::db;
    let (host, w) = (host.as_ref(), label.as_str());
    if let Err(error) = host.authorize_command(w, ipc_command(&op)) {
        return Answer::Failed(error);
    }
    let request = request.unwrap_or_else(Backend::next_request_id);
    match op {
        DbOp::LoadSources => match (db::sources(host, w), db::connected(host, w)) {
            (Ok(sources), Ok(connected)) => Answer::Sources { sources, connected },
            (Err(error), _) | (_, Err(error)) => Answer::Failed(error),
        },
        DbOp::SaveSources(sources) => Answer::of(db::save_sources(host, w, sources).await, |()| Answer::Done),
        DbOp::SetPassword { id, password, save } => {
            let password = password.map(SecretText::into_inner);
            Answer::of(db::set_password(host, w, &id, password, save).await, |()| Answer::Done)
        }
        DbOp::SetSshSecret { id, secret, save } => {
            let secret = secret.map(SecretText::into_inner);
            Answer::of(db::set_ssh_secret(host, w, &id, secret, save).await, |()| Answer::Done)
        }
        DbOp::TrustSshHost { id } => Answer::of(db::trust_ssh_host(host, w, &id).await, |()| Answer::Done),
        DbOp::Connect { target } => Answer::of(db::connect(host, w, &target).await, |()| Answer::Done),
        DbOp::Disconnect { target } => Answer::of(db::disconnect(host, w, &target).await, |()| Answer::Done),
        DbOp::SetManualCommit { id, manual } => {
            Answer::of(db::set_manual_commit(host, w, &id, manual).await, |()| Answer::Done)
        }
        DbOp::FinishTransaction { id, commit } => {
            Answer::of(db::finish_transaction(host, w, &id, commit).await, |()| Answer::Done)
        }
        DbOp::Databases { id } => Answer::of(db::databases(host, w, &id).await, Answer::Databases),
        DbOp::Schemas { target } => Answer::of(db::schemas(host, w, &target).await, Answer::Schemas),
        DbOp::Tables { target, schema } => match db::tables(host, w, &target, &schema).await {
            Ok(tables) => {
                let objects = db::schema_objects(host, w, &target, &schema).await.ok();
                Answer::Tables { tables, objects }
            }
            Err(error) => Answer::Failed(error),
        },
        DbOp::Details { target, schema, table } => {
            Answer::of(db::table_details(host, w, &target, &schema, &table).await, Answer::Details)
        }
        DbOp::Ddl { target, schema, table } => {
            Answer::of(db::ddl(host, w, &target, &schema, &table).await, Answer::Text)
        }
        DbOp::ObjectDdl { target, schema, kind, name } => {
            Answer::of(db::object_ddl(host, w, &target, &schema, kind, &name).await, Answer::Text)
        }
        DbOp::Completion { target } => Answer::of(db::completion(host, w, &target).await, Answer::Completion),
        DbOp::Execute { target, statements, skip, max_rows, schema } => {
            let skip = u32_of(skip);
            let max_rows = u32_of(max_rows);
            let ran = db::execute(host, w, request, &target, statements, skip, max_rows, schema.as_deref()).await;
            Answer::of(ran, Answer::Outcomes)
        }
        DbOp::Explain { target, statement, schema } => {
            Answer::of(db::explain(host, w, request, &target, &statement, schema.as_deref()).await, Answer::Plan)
        }
        DbOp::History { target } => Answer::of(db::history(host, w, &target).await, Answer::History),
        DbOp::RemoveHistory { target, texts } => {
            Answer::of(db::remove_history(host, w, &target, texts).await, |()| Answer::Done)
        }
        DbOp::ConsolePath { target } => Answer::of(db::console_path(host, &target), |path| {
            Answer::Text(path.into_path_buf().to_string_lossy().into_owned())
        }),
        DbOp::WriteFile { path, text } => {
            let written = tokio::task::spawn_blocking(move || std::fs::write(&path, text)).await;
            match written.map(|result| result.map_err(IdeError::from)) {
                Ok(Ok(())) => Answer::Done,
                Ok(Err(error)) => Answer::Failed(error),
                Err(error) => Answer::Failed(IdeError::internal(format!("the file write did not complete: {error}"))),
            }
        }
        DbOp::TablePage { target, schema, table, filter, sort, offset, limit } => {
            let (offset, limit) = (u32_of(offset), u32_of(limit));
            let page = db::table_page(host, w, request, &target, &schema, &table, filter, sort, offset, limit).await;
            Answer::of(page, |page| Answer::TablePage(Box::new(page)))
        }
        DbOp::ApplyEdits { target, schema, table, edits } => {
            Answer::of(db::apply_edits(host, w, &target, &schema, &table, edits).await, Answer::Count)
        }
    }
}

/// Whether a request runs under a `RequestId` that `Command::Cancel` cancels.
fn cancellable(op: &DbOp) -> bool {
    matches!(op, DbOp::Execute { .. } | DbOp::Explain { .. } | DbOp::TablePage { .. })
}

/// What the lower half of the tool window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lower {
    Hidden,
    Results,
    Table(u64),
}

/// What the tool window's view tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DatabasePanelEvent {
    /// Table editor `id` was closed from the strip.
    TableClosed(u64),
    /// The lower half shows something else.
    Switched,
}

/// The Database tool window: the tree above; the result pane or a table editor below.
pub(super) struct DatabasePanel {
    tree: Entity<DatabaseTree>,
    results: Entity<ResultsView>,
    editors: Vec<Entity<TableEditorView>>,
    lower: Lower,
}

impl EventEmitter<DatabasePanelEvent> for DatabasePanel {}

impl DatabasePanel {
    fn new(tree: Entity<DatabaseTree>, results: Entity<ResultsView>) -> Self {
        Self { tree, results, editors: Vec::new(), lower: Lower::Hidden }
    }

    #[cfg(test)]
    pub(super) fn lower(&self) -> Lower {
        self.lower
    }

    /// Table editor `id`.
    pub(super) fn editor(&self, id: u64, cx: &App) -> Option<&Entity<TableEditorView>> {
        self.editors.iter().find(|editor| editor.read(cx).id() == id)
    }

    /// The table editor the lower half shows.
    pub(super) fn active_editor(&self, cx: &App) -> Option<&Entity<TableEditorView>> {
        match self.lower {
            Lower::Table(id) => self.editor(id, cx),
            Lower::Hidden | Lower::Results => None,
        }
    }

    fn editor_of(&self, table: &TableRef, cx: &App) -> Option<&Entity<TableEditorView>> {
        self.editors.iter().find(|editor| editor.read(cx).table() == table)
    }

    fn select(&mut self, lower: Lower, cx: &mut Context<Self>) {
        if self.lower != lower {
            self.lower = lower;
            cx.emit(DatabasePanelEvent::Switched);
        }
        cx.notify();
    }

    fn add_editor(&mut self, editor: Entity<TableEditorView>, cx: &mut Context<Self>) {
        let id = editor.read(cx).id();
        self.editors.push(editor);
        self.select(Lower::Table(id), cx);
    }

    fn close_editor(&mut self, id: u64, cx: &mut Context<Self>) {
        self.editors.retain(|editor| editor.read(cx).id() != id);
        if self.lower == Lower::Table(id) {
            let next = match self.editors.last() {
                Some(editor) => Lower::Table(editor.read(cx).id()),
                None => Lower::Results,
            };
            self.select(next, cx);
        }
        cx.emit(DatabasePanelEvent::TableClosed(id));
        cx.notify();
    }

    fn strip_tab(label: String, active: bool, cx: &App) -> Div {
        div()
            .flex_none()
            .h_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .when(active, |el| el.bg(theme::color(cx, "bg-selected")).text_color(theme::color(cx, "fg-strong")))
            .child(SharedString::from(label))
    }

    fn strip(&self, cx: &mut Context<Self>) -> Div {
        let results_label = tx(cx, "Results");
        let results = Self::strip_tab(results_label, self.lower == Lower::Results, cx)
            .id("db-lower-results")
            .debug_selector(|| "db-lower-results".to_owned())
            .on_click(cx.listener(|panel: &mut Self, _: &ClickEvent, _, cx| panel.select(Lower::Results, cx)));
        let mut strip = div()
            .flex_none()
            .h(px(26.0))
            .flex()
            .items_center()
            .border_t_1()
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_size(theme::length(cx, "fs-small"))
            .child(results);
        let muted = theme::color(cx, "fg-muted");
        for editor in &self.editors {
            let (id, name) = {
                let editor = editor.read(cx);
                (editor.id(), editor.table().table.clone())
            };
            let tab = Self::strip_tab(name, self.lower == Lower::Table(id), cx)
                .id(SharedString::from(format!("db-lower-table-{id}")))
                .debug_selector(move || format!("db-lower-table-{id}"))
                .on_click(cx.listener(move |panel: &mut Self, _: &ClickEvent, _, cx| {
                    panel.select(Lower::Table(id), cx);
                }));
            let close = div()
                .id(SharedString::from(format!("db-lower-close-{id}")))
                .debug_selector(move || format!("db-lower-close-{id}"))
                .flex_none()
                .px(px(4.0))
                .flex()
                .items_center()
                .child(theme::icon(cx, "close", 10.0, muted))
                .on_click(cx.listener(move |panel: &mut Self, _: &ClickEvent, _, cx| panel.close_editor(id, cx)));
            strip = strip.child(div().flex_none().h_full().flex().items_center().child(tab).child(close));
        }
        strip
    }
}

impl Focusable for DatabasePanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tree.focus_handle(cx)
    }
}

impl Render for DatabasePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lower: Option<AnyView> = match self.lower {
            Lower::Hidden => None,
            Lower::Results => Some(self.results.clone().into()),
            Lower::Table(id) => self.editor(id, cx).cloned().map(AnyView::from),
        };
        let strip = lower.is_some().then(|| self.strip(cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h(px(80.0)).overflow_hidden().child(self.tree.clone()))
            .children(strip)
            .children(lower.map(|view| div().flex_1().min_h(px(80.0)).overflow_hidden().child(view)))
    }
}

/// An input of a [`FieldsPrompt`]: plain text, or a secret drawn masked.
enum PromptInput {
    Text(Entity<TextField>),
    Secret(Entity<PasswordField>),
}

/// How a [`FieldsPrompt`] closed. It carries secrets: it is never printed.
#[derive(Clone)]
pub(super) enum FieldsPromptEvent {
    /// OK: the fields' values in order, and the checkbox.
    Done {
        values: Vec<String>,
        checked: bool,
    },
    Cancelled,
}

/// A question with one or more fields: a database password or SSH secret (masked), or the values
/// of query parameters.
pub(super) struct FieldsPrompt {
    title: String,
    message: Option<String>,
    inputs: Vec<(String, PromptInput)>,
    checkbox: Option<(String, bool)>,
    focus: FocusHandle,
    _events: Vec<Subscription>,
}

impl EventEmitter<FieldsPromptEvent> for FieldsPrompt {}

/// A field of a prompt as it is built: its label, whether it is secret, its first value.
struct FieldSpec {
    label: String,
    secret: bool,
    initial: String,
}

impl FieldsPrompt {
    fn new(
        title: String,
        message: Option<String>,
        fields: Vec<FieldSpec>,
        checkbox: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut inputs = Vec::new();
        let mut events = Vec::new();
        let on_field = |prompt: &mut Self, event: &TextFieldEvent, cx: &mut Context<Self>| match event {
            TextFieldEvent::Submitted => prompt.done(cx),
            TextFieldEvent::Cancelled => cx.emit(FieldsPromptEvent::Cancelled),
            TextFieldEvent::Changed => {}
        };
        for field in fields {
            if field.secret {
                let entity = cx.new(PasswordField::new);
                entity.update(cx, |input, cx| input.set_text(&field.initial, cx));
                events.push(cx.subscribe_in(
                    &entity,
                    window,
                    move |prompt: &mut Self, _, event: &TextFieldEvent, _, cx| on_field(prompt, event, cx),
                ));
                inputs.push((field.label, PromptInput::Secret(entity)));
            } else {
                let initial = field.initial;
                let entity = cx.new(|cx| {
                    let mut input = TextField::new(cx);
                    input.set_text(&initial, cx);
                    input
                });
                events.push(cx.subscribe_in(
                    &entity,
                    window,
                    move |prompt: &mut Self, _, event: &TextFieldEvent, _, cx| on_field(prompt, event, cx),
                ));
                inputs.push((field.label, PromptInput::Text(entity)));
            }
        }
        let focus = match inputs.first() {
            Some((_, PromptInput::Text(entity))) => entity.focus_handle(cx),
            Some((_, PromptInput::Secret(entity))) => entity.focus_handle(cx),
            None => cx.focus_handle(),
        };
        Self { title, message, inputs, checkbox, focus, _events: events }
    }

    /// The fields' values in order (a secret's too: the caller hands it to the store only).
    fn values(&self, cx: &App) -> Vec<String> {
        self.inputs
            .iter()
            .map(|(_, input)| match input {
                PromptInput::Text(entity) => entity.read(cx).text().to_owned(),
                PromptInput::Secret(entity) => entity.read(cx).text(cx).to_owned(),
            })
            .collect()
    }

    fn done(&mut self, cx: &mut Context<Self>) {
        let values = self.values(cx);
        let checked = self.checkbox.as_ref().is_some_and(|(_, on)| *on);
        cx.emit(FieldsPromptEvent::Done { values, checked });
    }

    /// The masked input of the prompt, when it has one.
    #[cfg(test)]
    pub(super) fn secret_input(&self) -> Option<Entity<PasswordField>> {
        self.inputs.iter().find_map(|(_, input)| match input {
            PromptInput::Secret(entity) => Some(entity.clone()),
            PromptInput::Text(_) => None,
        })
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if let Some((_, on)) = &mut self.checkbox {
            *on = !*on;
        }
        cx.notify();
    }

    fn button(cx: &App, label: String, primary: bool) -> Div {
        let face = div()
            .flex_none()
            .h(px(28.0))
            .min_w(px(72.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .whitespace_nowrap()
            .child(SharedString::from(label));
        if primary {
            let fill = theme::color(cx, "bg-button-primary");
            face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
        } else {
            face.bg(theme::color(cx, "bg-button"))
                .border_color(theme::color(cx, "border-button"))
                .text_color(theme::color(cx, "fg-strong"))
        }
    }
}

impl Focusable for FieldsPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for FieldsPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let title = tx(cx, &self.title);
        let message = self.message.clone().map(|message| div().child(SharedString::from(tx(cx, &message))));
        let mut fields = Vec::new();
        for (label, input) in &self.inputs {
            let label = div().text_color(muted).child(SharedString::from(tx(cx, label)));
            let view: AnyView = match input {
                PromptInput::Text(entity) => entity.clone().into(),
                PromptInput::Secret(entity) => entity.clone().into(),
            };
            let frame = div()
                .h(px(28.0))
                .flex()
                .items_center()
                .px(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .bg(theme::color(cx, "bg-input"))
                .child(div().flex_1().min_w_0().child(view));
            fields.push(div().flex().flex_col().gap(px(6.0)).child(label).child(frame));
        }
        let check = self.checkbox.clone().map(|(label, on)| {
            let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
            let fill = if on { "bg-button-primary" } else { "bg-checkbox" };
            let edge = if on { "bg-button-primary" } else { "border-checkbox" };
            div()
                .id("db-prompt-checkbox")
                .debug_selector(|| "db-prompt-checkbox".to_owned())
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(strong)
                .child(
                    div()
                        .flex_none()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(theme::color(cx, edge))
                        .bg(theme::color(cx, fill))
                        .children(mark),
                )
                .child(SharedString::from(tx(cx, &label)))
                .on_click(cx.listener(|prompt: &mut Self, _: &ClickEvent, _, cx| prompt.toggle(cx)))
        });
        let (ok_label, cancel_label) = (tx(cx, "OK"), tx(cx, "Cancel"));
        let ok = Self::button(cx, ok_label, true)
            .id("db-prompt-ok")
            .debug_selector(|| "db-prompt-ok".to_owned())
            .on_click(cx.listener(|prompt: &mut Self, _: &ClickEvent, _, cx| prompt.done(cx)));
        let cancel = Self::button(cx, cancel_label, false)
            .id("db-prompt-cancel")
            .debug_selector(|| "db-prompt-cancel".to_owned())
            .on_click(cx.listener(|_: &mut Self, _: &ClickEvent, _, cx| cx.emit(FieldsPromptEvent::Cancelled)));
        div()
            .w(px(420.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .children(message)
                    .children(fields)
                    .children(check),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).child(ok).child(cancel))
    }
}

/// A question waiting for its turn.
enum Question {
    /// A store prompt, answered through `DbStore::prompt_answered`.
    Store { ticket: Ticket, prompt: Prompt },
    /// Rename of a data source.
    Rename { id: String, name: String },
}

/// A dialog over the window and where the keyboard goes when it closes.
struct Layer {
    view: AnyView,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The Data Source dialog that is open.
struct SourceDialogSlot {
    dialog: Entity<SourceDialog>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// A console run asked for before the data sources were read.
struct PendingRun {
    path: String,
    text: String,
    from: u32,
    to: u32,
    explain: bool,
}

/// The Database tools of the window's project.
pub(super) struct DatabaseSlot {
    epoch: u64,
    window: AnyWindowHandle,
    store: DbStore,
    tree: Entity<DatabaseTree>,
    results: Entity<ResultsView>,
    panel: Entity<DatabasePanel>,
    /// The `RequestId` of each cancellable request under way, by ticket.
    requests: HashMap<Ticket, RequestId>,
    next_editor: u64,
    /// What each table editor's window listens to it for, by editor id.
    editor_events: HashMap<u64, Vec<Subscription>>,
    source_dialog: Option<SourceDialogSlot>,
    prompt: Option<Layer>,
    questions: VecDeque<Question>,
    pending_run: Option<PendingRun>,
    _tick: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// Registers the Database actions this window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("db.showToolWindow", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if workbench.ensure_database(window, cx) {
                workbench.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, true));
            }
        })
    });
    registry.handle("db.newDataSource", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.open_source_dialog(None, None, window, cx))
    });
    registry.handle("db.openConsole", |call, window, cx| {
        let key = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.run_tree_action(TreeAction::OpenConsole, key, window, cx);
        })
    });
    // The tree's actions act on the node the argument names (its key), else on the selection. They
    // need a project, and their shortcuts work only while the tree has the keyboard: F4, Delete or
    // Cmd+D mean something else everywhere else.
    for (id, action) in [
        ("db.properties", TreeAction::Properties),
        ("db.refresh", TreeAction::Refresh),
        ("db.disconnect", TreeAction::Disconnect),
        ("db.editData", TreeAction::EditData),
        ("db.goToDdl", TreeAction::GoToDdl),
        ("db.duplicateSource", TreeAction::DuplicateSource),
        ("db.rename", TreeAction::Rename),
        ("db.drop", TreeAction::Drop),
        ("db.copyReference", TreeAction::CopyReference),
        ("db.collapseAll", TreeAction::CollapseAll),
    ] {
        let meta = manifest()
            .get(id)
            .unwrap_or_else(|| panic!("action {id} is not in assets/gen/actions.json"))
            .clone()
            .when(PROJECT_OPEN)
            .key_when(DATABASE_VIEW_FOCUS);
        registry.register(Action::new(meta, move |call, window, cx| {
            let key = text_arg(call.arg);
            on_workbench(window, cx, move |workbench, window, cx| workbench.run_tree_action(action, key, window, cx))
        }));
    }
    registry.handle("db.submitChanges", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some(editor) = workbench.database_table_editor(cx) {
                editor.update(cx, |editor, cx| editor.submit(cx));
            }
            workbench.sync_database_context(window, cx);
        })
    });
    registry.handle("db.revertChanges", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some(editor) = workbench.database_table_editor(cx) {
                editor.update(cx, |editor, cx| editor.revert(cx));
            }
            workbench.sync_database_context(window, cx);
        })
    });
    registry.handle("db.execute", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.run_sql_editor(false, window, cx))
    });
    registry.handle("db.explainPlan", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.run_sql_editor(true, window, cx))
    });
}

impl Workbench {
    // ---- the slot ---------------------------------------------------------------------------------

    /// Builds the Database tools of the open project on first use and reads its data sources.
    /// Returns whether the window has them (a project is open).
    pub(super) fn ensure_database(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.database.is_some() {
            return true;
        }
        if self.project_root(cx).is_none() {
            return false;
        }
        let stored = |key: &str| cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned);
        let (bindings, schemas) = (stored(BINDINGS_KEY), stored(CONSOLE_SCHEMAS_KEY));
        let store = DbStore::new(bindings.as_deref(), schemas.as_deref());
        let tree = cx.new(DatabaseTree::new);
        let results = cx.new(|cx| ResultsView::new(window, cx));
        let panel = {
            let (tree, results) = (tree.clone(), results.clone());
            cx.new(|_| DatabasePanel::new(tree, results))
        };
        let grid = results.read(cx).grid().clone();
        let subscriptions = vec![
            cx.subscribe_in(&tree, window, Self::database_tree_event),
            cx.subscribe_in(&results, window, Self::database_results_event),
            cx.subscribe_in(&grid, window, Self::database_grid_event),
            cx.subscribe_in(&panel, window, Self::database_panel_event),
        ];
        let handle = window.window_handle();
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(TICK_MS)).await;
                let ticked = handle.update(cx, |_, window, cx| {
                    this.update(cx, |workbench, cx| workbench.database_tick(window, cx)).is_ok()
                });
                if !matches!(ticked, Ok(true)) {
                    return;
                }
            }
        });
        self.database = Some(DatabaseSlot {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            window: handle,
            store,
            tree,
            results,
            panel,
            requests: HashMap::new(),
            next_editor: 1,
            editor_events: HashMap::new(),
            source_dialog: None,
            prompt: None,
            questions: VecDeque::new(),
            pending_run: None,
            _tick: tick,
            _subscriptions: subscriptions,
        });
        let commands = self.db_step(cx, |store, _| store.load_sources());
        self.carry_out_db(commands, window, cx);
        true
    }

    /// The project goes: its Database tools, their dialogs and requests in flight are forgotten
    /// (late answers carry the old epoch and are dropped).
    pub(super) fn reset_database(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.database.take().is_some() {
            set_context(window, cx, TABLE_EDITOR_FOCUS, false);
            set_context(window, cx, TABLE_EDITOR_CHANGES, false);
            cx.notify();
        }
    }

    /// The tool window's panel, when a project is open.
    pub(super) fn mount_database(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        if !self.ensure_database(window, cx) {
            return None;
        }
        let panel = self.database.as_ref()?.panel.clone();
        Some(Panel::new(panel, None))
    }

    /// The backend says the project's data sources changed: they are read again.
    pub(super) fn database_sources_changed(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.database.as_ref().map(|slot| slot.window) else { return };
        cx.spawn(async move |this, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                this.update(cx, |workbench, cx| {
                    let commands = workbench.db_step(cx, |store, _| store.load_sources());
                    workbench.carry_out_db(commands, window, cx);
                })
            });
        })
        .detach();
    }

    fn database_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let commands = self.db_step(cx, |store, env| store.tick(env));
        self.carry_out_db(commands, window, cx);
    }

    /// Runs a step of the store with the clock and the UI language.
    fn db_step(
        &mut self,
        cx: &mut Context<Self>,
        step: impl FnOnce(&mut DbStore, &Env<'_>) -> Vec<Command>,
    ) -> Vec<Command> {
        let Some(slot) = self.database.as_mut() else { return Vec::new() };
        let localization = cx.default_global::<Localization>();
        let env = Env { now_ms: now_ms().max(0.0) as u64, i18n: &localization.0 };
        step(&mut slot.store, &env)
    }

    // ---- test and wiring accessors ----------------------------------------------------------------

    #[cfg(test)]
    pub(super) fn database_panel(&self) -> Option<Entity<DatabasePanel>> {
        self.database.as_ref().map(|slot| slot.panel.clone())
    }

    pub(super) fn database_tree(&self) -> Option<Entity<DatabaseTree>> {
        self.database.as_ref().map(|slot| slot.tree.clone())
    }

    #[cfg(test)]
    pub(super) fn database_results(&self) -> Option<Entity<ResultsView>> {
        self.database.as_ref().map(|slot| slot.results.clone())
    }

    /// The table editor the tool window shows.
    pub(super) fn database_table_editor(&self, cx: &App) -> Option<Entity<TableEditorView>> {
        let slot = self.database.as_ref()?;
        slot.panel.read(cx).active_editor(cx).cloned()
    }

    /// Puts the store's question `prompt` to the user, as a store command does.
    #[cfg(test)]
    pub(super) fn ask_database(&mut self, ticket: Ticket, prompt: Prompt, window: &mut Window, cx: &mut Context<Self>) {
        self.carry_out_db(vec![Command::Ask { ticket, prompt }], window, cx);
    }

    #[cfg(test)]
    pub(super) fn database_source_dialog(&self) -> Option<Entity<SourceDialog>> {
        self.database.as_ref()?.source_dialog.as_ref().map(|slot| slot.dialog.clone())
    }

    #[cfg(test)]
    /// The question that is open, as a view of type `V` (`PromptDialog` or [`FieldsPrompt`]).
    pub(super) fn database_prompt<V: 'static>(&self) -> Option<Entity<V>> {
        self.database.as_ref()?.prompt.as_ref().and_then(|layer| layer.view.clone().downcast().ok())
    }

    /// The Data Source dialog over the window, while one is open.
    pub(super) fn database_dialog_layer(&self) -> Option<AnyElement> {
        let dialog = self.database.as_ref()?.source_dialog.as_ref()?.dialog.clone();
        let layer = div()
            .id("db-dialog-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(dialog);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }

    /// The Database question over everything else, while one is open.
    pub(super) fn database_prompt_layer(&self) -> Option<AnyElement> {
        let view = self.database.as_ref()?.prompt.as_ref()?.view.clone();
        let layer = div()
            .id("db-prompt-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(view);
        Some(gpui_kit::deferred(layer).with_priority(2).into_any_element())
    }

    // ---- the driver -------------------------------------------------------------------------------

    /// Carries out what the store asks for, in order.
    fn carry_out_db(&mut self, commands: Vec<Command>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tree, results, panel)) =
            self.database.as_ref().map(|slot| (slot.tree.clone(), slot.results.clone(), slot.panel.clone()))
        else {
            return;
        };
        for command in commands {
            match command {
                Command::Backend { ticket, op } => self.db_backend(ticket, op, cx),
                Command::Cancel(ticket) => {
                    let request = self.database.as_mut().and_then(|slot| slot.requests.remove(&ticket));
                    if let (Some(request), Some(backend)) = (request, cx.try_global::<Backend>()) {
                        backend.host.cancel_request(request);
                    }
                }
                Command::Ask { ticket, prompt } => {
                    if let Some(slot) = self.database.as_mut() {
                        slot.questions.push_back(Question::Store { ticket, prompt });
                    }
                    self.next_db_question(window, cx);
                }
                Command::Tree(update) => {
                    let sources_read = matches!(update, TreeUpdate::Sources(_));
                    tree.update(cx, |tree, cx| apply_tree_update(tree, update, cx));
                    if sources_read && let Some(run) = self.database.as_mut().and_then(|slot| slot.pending_run.take()) {
                        self.run_console(run, window, cx);
                    }
                }
                Command::Results(update) => {
                    let shown = matches!(update, ResultsUpdate::Show(_));
                    results.update(cx, |results, cx| apply_results_update(results, update, cx));
                    if shown {
                        panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));
                    }
                }
                Command::Notify(notice) => {
                    let level = match notice.level {
                        Level::Info => NotificationLevel::Info,
                        Level::Error => NotificationLevel::Error,
                    };
                    let mut request = NotificationRequest::new(level, notice.title);
                    request.body = notice.body;
                    request.key = notice.key.map(str::to_owned);
                    request.group = Some("tools".to_owned());
                    self.notify(request, cx);
                }
                Command::Persist { key, value } => cx.default_global::<UiStorage>().set(key, value),
                Command::OpenFile { path } => self.open_project_file(PathBuf::from(path), window, cx),
                Command::AppendToConsole { path, text } => {
                    self.with_console_editor(
                        path,
                        move |editor, cx| {
                            editor.update(cx, |editor, cx| {
                                let doc = editor.text();
                                let end = len16(&doc);
                                let append = console_append(&doc, &text);
                                editor.insert(Some(end..end), &append.insert, cx);
                                editor.select(append.anchor, append.head, cx);
                            });
                        },
                        cx,
                    );
                }
                Command::PasteIntoConsole { target, path, text } => {
                    let open = self.editor().filter(|editor| editor.read(cx).path() == Path::new(&path)).cloned();
                    match open {
                        Some(editor) => {
                            editor.update(cx, |editor, cx| editor.insert(None, &text, cx));
                        }
                        None => {
                            let more = self.db_step(cx, |store, _| store.open_console_with(&target, &text));
                            self.carry_out_db(more, window, cx);
                        }
                    }
                }
                Command::MarkOutcomes { path, marks, .. } => {
                    let marks = marks.len();
                    tracing::debug!(%path, marks, "Database: the console's statement marks are not drawn");
                }
                Command::ShowResults => {
                    self.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, false));
                    panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));
                }
                Command::ShowToolWindow => {
                    self.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, true));
                }
                Command::ShowHistory => {
                    tracing::info!("Database: the Query History dialog is not available in this window");
                }
                Command::SourceSaved { id } => {
                    let open = self.database.as_ref().and_then(|slot| slot.source_dialog.as_ref());
                    if open.is_some_and(|slot| slot.dialog.read(cx).form().source.id == id) {
                        self.close_source_dialog(window, cx);
                    }
                }
                Command::SourceSaveFailed { id, message } => {
                    let open = self.database.as_ref().and_then(|slot| slot.source_dialog.as_ref());
                    if let Some(dialog) =
                        open.filter(|slot| slot.dialog.read(cx).form().source.id == id).map(|slot| slot.dialog.clone())
                    {
                        dialog.update(cx, |dialog, cx| dialog.save_failed(&message, cx));
                    }
                }
                Command::SqlDone { job, ok } => tracing::debug!(job, ok, "Database: statements ran"),
                Command::TablePage { editor, request, result } => {
                    if let Some(view) = panel.read(cx).editor(editor, cx).cloned() {
                        view.update(cx, |view, cx| view.page_loaded(request, result, cx));
                    }
                }
                Command::TableEdited { editor, ok } => {
                    if let Some(view) = panel.read(cx).editor(editor, cx).cloned() {
                        view.update(cx, |view, cx| view.submit_finished(ok, cx));
                    }
                    self.sync_database_context(window, cx);
                }
            }
        }
        cx.notify();
    }

    /// Runs a store request on the backend; its answer goes back to the store.
    fn db_backend(&mut self, ticket: Ticket, op: DbOp, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.database.as_mut() else { return };
        let request = cancellable(&op).then(Backend::next_request_id);
        if let Some(request) = request {
            slot.requests.insert(ticket, request);
        }
        let (epoch, handle) = (slot.epoch, slot.window);
        let name = op_name(&op);
        let call =
            cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| run_op(host, label, op, request)));
        cx.spawn(async move |this, cx| {
            let answer = match call {
                Some(call) => {
                    call.await.unwrap_or_else(|error| Answer::Failed(IdeError::internal(format!("{error:#}"))))
                }
                None => Answer::Failed(IdeError::unavailable("The backend is not running")),
            };
            let delivered = handle.update(cx, |_, window, cx| {
                this.update(cx, |workbench, cx| workbench.db_answered(epoch, ticket, answer, window, cx))
            });
            if delivered.is_err() {
                tracing::debug!(request = name, "Database: the window closed before the backend answered");
            }
        })
        .detach();
    }

    fn db_answered(&mut self, epoch: u64, ticket: Ticket, answer: Answer, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.database.as_mut().filter(|slot| slot.epoch == epoch) else { return };
        slot.requests.remove(&ticket);
        let commands = self.db_step(cx, |store, env| store.answer(ticket, answer, env));
        self.carry_out_db(commands, window, cx);
    }

    /// Runs `work` on the backend with the host and the window's label, then `then` with its result.
    fn db_call<T, F, W>(
        &mut self,
        command: &'static str,
        work: W,
        then: impl FnOnce(&mut Self, IdeResult<T>, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: Send + 'static,
        F: Future<Output = IdeResult<T>> + Send + 'static,
        W: FnOnce(Arc<Host>, String) -> F + Send + 'static,
    {
        let label = self.label.clone();
        let call = cx.try_global::<Backend>().map(|backend| {
            backend.with_host(move |host| {
                // A project in Restricted Mode reaches no database.
                let allowed = host.authorize_command(&label, command);
                let work = allowed.map(|()| work(host, label));
                async move {
                    match work {
                        Ok(work) => work.await,
                        Err(error) => Err(error),
                    }
                }
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = match call {
                None => Err(IdeError::unavailable("The backend is not running")),
                Some(call) => call.await.unwrap_or_else(|error| Err(IdeError::internal(format!("{error:#}")))),
            };
            let _ = this.update_in(cx, move |workbench, window, cx| then(workbench, result, window, cx));
        })
        .detach();
    }

    /// Runs `edit` on the editor of `path` once it is the active one (it may still be opening).
    fn with_console_editor(
        &mut self,
        path: String,
        edit: impl FnOnce(&Entity<Editor>, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let open = |workbench: &Self, cx: &App| {
            workbench.editor().filter(|editor| editor.read(cx).path() == Path::new(&path)).cloned()
        };
        if let Some(editor) = open(self, cx) {
            edit(&editor, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            for _ in 0..EDITOR_WAIT_STEPS {
                cx.background_executor().timer(Duration::from_millis(EDITOR_WAIT_MS)).await;
                let found = this.update(cx, |workbench, cx| {
                    workbench.editor().filter(|editor| editor.read(cx).path() == Path::new(&path)).cloned()
                });
                match found {
                    Ok(Some(editor)) => {
                        cx.update(|cx| edit(&editor, cx));
                        return;
                    }
                    Ok(None) => {}
                    Err(_) => return,
                }
            }
            tracing::info!(%path, "Database: the console did not open in time for its text");
        })
        .detach();
    }

    // ---- questions --------------------------------------------------------------------------------

    /// Shows the next question waiting, unless one is open.
    fn next_db_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.database.as_mut() else { return };
        if slot.prompt.is_some() {
            return;
        }
        let Some(question) = slot.questions.pop_front() else { return };
        let (view, focus, events): (AnyView, FocusHandle, Subscription) = match question {
            Question::Store { ticket, prompt } => match prompt {
                Prompt::Password { source } => {
                    let field = FieldSpec { label: "Password:".to_owned(), secret: true, initial: String::new() };
                    let checkbox = Some(("Save password".to_owned(), source.save_password));
                    let title = format!("Connect to {}", source.name);
                    self.fields_prompt(ticket, title, None, vec![field], checkbox, secret_answer, window, cx)
                }
                Prompt::SshSecret { source, message } => {
                    let label = "SSH password or key passphrase:".to_owned();
                    let field = FieldSpec { label, secret: true, initial: String::new() };
                    let save = source.ssh.as_ref().is_some_and(|ssh| ssh.save_secret);
                    let checkbox = Some(("Save password".to_owned(), save));
                    let title = format!("SSH Connection to {}", source.name);
                    self.fields_prompt(ticket, title, Some(message), vec![field], checkbox, secret_answer, window, cx)
                }
                Prompt::Params { names, values } => {
                    let fields = names
                        .iter()
                        .zip(values.into_iter().chain(std::iter::repeat(String::new())))
                        .map(|(name, initial)| FieldSpec { label: name.clone(), secret: false, initial })
                        .collect();
                    let answer = move |values: Vec<String>, _: bool| {
                        PromptAnswer::Params(names.iter().cloned().zip(values).collect::<HashMap<_, _>>())
                    };
                    self.fields_prompt(ticket, "Query Parameters".to_owned(), None, fields, None, answer, window, cx)
                }
                Prompt::TrustHost { message } => {
                    let request = ConfirmRequest {
                        title: "Unknown SSH Host Key".to_owned(),
                        message,
                        ok_label: "Trust".to_owned(),
                        danger: false,
                    };
                    let build = move |window: &mut Window, cx: &mut Context<PromptDialog>| {
                        PromptDialog::confirm(request, window, cx)
                    };
                    self.dialog_prompt(ticket, build, confirm_answer, window, cx)
                }
                Prompt::RemoveSource { name, .. } => {
                    let message = format!("Remove data source \"{name}\"?");
                    let request = ConfirmRequest::danger("Remove Data Source", message, "Remove");
                    let build = move |window: &mut Window, cx: &mut Context<PromptDialog>| {
                        PromptDialog::confirm(request, window, cx)
                    };
                    self.dialog_prompt(ticket, build, confirm_answer, window, cx)
                }
                Prompt::EndTransaction { name, .. } => {
                    let request = AskRequest {
                        title: "End Transaction".to_owned(),
                        body: format!("{name} has uncommitted changes. Commit them before switching to Auto?"),
                        warning: true,
                        width: None,
                        checkbox: None,
                        buttons: vec![
                            AskButton { id: "commit", label: "Commit", primary: true },
                            AskButton { id: "rollback", label: "Roll Back", primary: false },
                            AskButton { id: CANCEL, label: "Cancel", primary: false },
                        ],
                    };
                    let build = move |window: &mut Window, cx: &mut Context<PromptDialog>| {
                        PromptDialog::ask(request, window, cx)
                    };
                    let answer = |answer: &DialogAnswer| match answer.button.as_str() {
                        "commit" => PromptAnswer::Commit(true),
                        "rollback" => PromptAnswer::Commit(false),
                        _ => PromptAnswer::Cancel,
                    };
                    self.dialog_prompt(ticket, build, answer, window, cx)
                }
            },
            Question::Rename { id, name } => {
                let request = PromptRequest {
                    title: "Rename Data Source".to_owned(),
                    label: Some("New name:".to_owned()),
                    initial: name,
                    ok_label: "Rename".to_owned(),
                    ..PromptRequest::default()
                };
                let dialog = cx.new(|cx| {
                    PromptDialog::prompt(request, FieldRule::NonEmpty("Enter a name".to_owned()), window, cx)
                });
                let events =
                    cx.subscribe_in(&dialog, window, move |workbench, _, event: &PromptDialogEvent, window, cx| {
                        let PromptDialogEvent::Closed(answer) = event;
                        workbench.close_db_prompt(window, cx);
                        if let Some(result) = answer.prompt_result() {
                            let commands = workbench.db_step(cx, |store, _| store.rename_source(&id, &result.value));
                            workbench.carry_out_db(commands, window, cx);
                        }
                        workbench.next_db_question(window, cx);
                    });
                let focus = dialog.focus_handle(cx);
                (dialog.into(), focus, events)
            }
        };
        let restore = window.focused(cx);
        window.focus(&focus, cx);
        if let Some(slot) = self.database.as_mut() {
            slot.prompt = Some(Layer { view, restore, _events: events });
        }
        cx.notify();
    }

    /// A [`FieldsPrompt`] whose answer `answer` turns into the store's.
    #[expect(clippy::too_many_arguments, reason = "the question's parts and the window")]
    fn fields_prompt(
        &mut self,
        ticket: Ticket,
        title: String,
        message: Option<String>,
        fields: Vec<FieldSpec>,
        checkbox: Option<(String, bool)>,
        answer: impl Fn(Vec<String>, bool) -> PromptAnswer + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyView, FocusHandle, Subscription) {
        let prompt = cx.new(|cx| FieldsPrompt::new(title, message, fields, checkbox, window, cx));
        let events = cx.subscribe_in(&prompt, window, move |workbench, _, event: &FieldsPromptEvent, window, cx| {
            let reply = match event.clone() {
                FieldsPromptEvent::Done { values, checked } => answer(values, checked),
                FieldsPromptEvent::Cancelled => PromptAnswer::Cancel,
            };
            workbench.db_prompt_answered(ticket, reply, window, cx);
        });
        let focus = prompt.focus_handle(cx);
        (prompt.into(), focus, events)
    }

    /// A [`PromptDialog`] built by `build`, whose answer `answer` turns into the store's.
    fn dialog_prompt(
        &mut self,
        ticket: Ticket,
        build: impl FnOnce(&mut Window, &mut Context<PromptDialog>) -> PromptDialog,
        answer: impl Fn(&DialogAnswer) -> PromptAnswer + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyView, FocusHandle, Subscription) {
        let dialog = cx.new(|cx| build(window, cx));
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &PromptDialogEvent, window, cx| {
            let PromptDialogEvent::Closed(closed) = event;
            workbench.db_prompt_answered(ticket, answer(closed), window, cx);
        });
        let focus = dialog.focus_handle(cx);
        (dialog.into(), focus, events)
    }

    fn db_prompt_answered(
        &mut self,
        ticket: Ticket,
        answer: PromptAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_db_prompt(window, cx);
        let commands = self.db_step(cx, |store, env| store.prompt_answered(ticket, answer, env));
        self.carry_out_db(commands, window, cx);
        self.next_db_question(window, cx);
    }

    fn close_db_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layer) = self.database.as_mut().and_then(|slot| slot.prompt.take()) else { return };
        if let Some(target) = layer.restore {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    // ---- the Data Source dialog -----------------------------------------------------------------

    /// The Data Source dialog: over `source` (Properties), else a new data source of `vendor`'s type.
    fn open_source_dialog(
        &mut self,
        source: Option<DataSource>,
        vendor: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_database(window, cx) {
            return;
        }
        let loaded = self.database.as_ref().is_some_and(|slot| slot.store.loaded());
        if !loaded {
            let commands = self.db_step(cx, |store, _| store.load_sources());
            self.carry_out_db(commands, window, cx);
        }
        let form = match source {
            Some(source) => {
                let name = source.name.clone();
                SourceForm::new(source, Some(name), None)
            }
            None => {
                let mut form = SourceForm::new(blank_source(new_source_id(&random_id())), None, None);
                if let Some(vendor) = vendor {
                    form.set_type(vendor);
                }
                form
            }
        };
        let dialog = cx.new(|cx| SourceDialog::new(form, window, cx));
        let events = cx.subscribe_in(&dialog, window, Self::source_dialog_event);
        let focus = dialog.focus_handle(cx);
        let Some(slot) = self.database.as_mut() else { return };
        let restore = match slot.source_dialog.take() {
            Some(previous) => previous.restore,
            None => window.focused(cx),
        };
        window.focus(&focus, cx);
        slot.source_dialog = Some(SourceDialogSlot { dialog, restore, _events: events });
        cx.notify();
    }

    fn close_source_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.database.as_mut().and_then(|slot| slot.source_dialog.take()) else { return };
        if let Some(target) = slot.restore {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// Carries out what the Data Source dialog asks for. Its events carry passwords: they are
    /// never logged.
    fn source_dialog_event(
        &mut self,
        dialog: &Entity<SourceDialog>,
        event: &SourceDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = dialog.downgrade();
        match event.clone() {
            SourceDialogEvent::Test { source, password, ssh_secret } => self.db_call(
                "db_test",
                move |host, label| async move {
                    let (password, ssh_secret) =
                        (password.map(SecretText::into_inner), ssh_secret.map(SecretText::into_inner));
                    ide_host::ops::db::test(&host, &label, *source, password, ssh_secret).await
                },
                move |_, result, _, cx| {
                    let _ = weak.update(cx, |dialog, cx| dialog.test_finished(result, cx));
                },
                window,
                cx,
            ),
            SourceDialogEvent::TrustHost { config: None, source_id } => self.db_call(
                "db_trust_ssh_host",
                move |host, label| async move { ide_host::ops::db::trust_ssh_host(&host, &label, &source_id).await },
                move |_, result, _, cx| {
                    let _ = weak.update(cx, |dialog, cx| match result {
                        Ok(()) => dialog.trust_finished(cx),
                        Err(error) => dialog.test_finished(Err(error), cx),
                    });
                },
                window,
                cx,
            ),
            SourceDialogEvent::TrustHost { config: Some(_), .. } => {
                let error = IdeError::unavailable("Trusting the host of an SSH configuration is not available here");
                dialog.update(cx, |dialog, cx| dialog.test_finished(Err(error), cx));
            }
            SourceDialogEvent::Save { source, password, ssh_secret } => {
                // The store saves the whole list: before it is read, a save would drop the others.
                let loaded = self.database.as_ref().is_some_and(|slot| slot.store.loaded());
                if !loaded {
                    let message = tx(cx, "The data sources are still being read. Try again.");
                    dialog.update(cx, |dialog, cx| dialog.save_failed(&message, cx));
                    return;
                }
                let commands = self.db_step(cx, |store, _| store.save_source(*source, password, ssh_secret));
                self.carry_out_db(commands, window, cx);
            }
            SourceDialogEvent::LoadNames { id, databases } => self.db_call(
                if databases { "db_databases" } else { "db_schemas" },
                move |host, label| async move {
                    if databases {
                        ide_host::ops::db::databases(&host, &label, &id).await.map(|d| (d.names, Vec::new()))
                    } else {
                        ide_host::ops::db::schemas(&host, &label, &id).await.map(|names| (Vec::new(), names))
                    }
                },
                move |workbench, result, _, cx| {
                    let (databases, schemas) = match result {
                        Ok(names) => names,
                        Err(error) => {
                            let mut request =
                                NotificationRequest::new(NotificationLevel::Error, "Cannot load the names");
                            request.body = Some(error.message);
                            workbench.notify(request, cx);
                            (Vec::new(), Vec::new())
                        }
                    };
                    let _ = weak.update(cx, |dialog, cx| dialog.set_known_names(databases, schemas, cx));
                },
                window,
                cx,
            ),
            SourceDialogEvent::PickFile { field, .. } => {
                let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
                let picked = cx.prompt_for_paths(options);
                cx.spawn_in(window, async move |_, cx| {
                    let path = match picked.await {
                        Ok(Ok(Some(paths))) => paths.into_iter().next(),
                        Ok(Ok(None)) | Err(_) => None,
                        Ok(Err(error)) => {
                            tracing::warn!("database: cannot pick a file: {error:#}");
                            None
                        }
                    };
                    if let Some(path) = path {
                        let path = path.to_string_lossy().into_owned();
                        let _ = weak.update_in(cx, |dialog, window, cx| dialog.file_picked(field, &path, window, cx));
                    }
                })
                .detach();
            }
            SourceDialogEvent::EditSshConfigurations => {
                tracing::info!("Database: Settings › SSH Configurations is not available in this window");
            }
            SourceDialogEvent::Cancelled => self.close_source_dialog(window, cx),
        }
    }

    // ---- the tree ---------------------------------------------------------------------------------

    /// A tree action from the action registry: on the node `key`, else on the selection.
    fn run_tree_action(
        &mut self,
        action: TreeAction,
        key: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_database(window, cx) {
            return;
        }
        let Some(tree) = self.database_tree() else { return };
        let node = key.or_else(|| tree.read(cx).selected().map(str::to_owned));
        tree.update(cx, |tree, cx| tree.perform(TreeCommand { action, node }, cx));
    }

    /// Carries out what the tree asks for.
    fn database_tree_event(
        &mut self,
        tree: &Entity<DatabaseTree>,
        event: &TreeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            TreeEvent::Load(load) => {
                let commands = self.db_step(cx, |store, _| store.tree_load(&load));
                self.carry_out_db(commands, window, cx);
            }
            TreeEvent::LoadSources => {
                let commands = self.db_step(cx, |store, _| store.load_sources());
                self.carry_out_db(commands, window, cx);
            }
            TreeEvent::OpenTableData(at) => self.open_table_editor(at, window, cx),
            TreeEvent::OpenObjectDdl(object) => {
                let (id, schema, name, kind) = (object.id, object.schema, object.name, object.kind);
                let commands = self.db_step(cx, |store, _| store.open_ddl(&id, &schema, &name, Some(kind)));
                self.carry_out_db(commands, window, cx);
            }
            TreeEvent::ShowMenu { position, node } => {
                let items = tree.read(cx).menu_for(node.as_deref());
                let tree = tree.clone();
                let run = move |_: &mut Self, command: TreeCommand, _: &mut Window, cx: &mut Context<Self>| {
                    tree.update(cx, |tree, cx| tree.perform(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            TreeEvent::ShowNewMenu(position) => {
                let items = tree.read(cx).new_menu();
                let tree = tree.clone();
                let run = move |_: &mut Self, command: TreeCommand, _: &mut Window, cx: &mut Context<Self>| {
                    tree.update(cx, |tree, cx| tree.perform(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            TreeEvent::Run { action, node } => self.database_tree_run(action, node.map(|node| *node), window, cx),
        }
    }

    /// The data source of a node (`None`: the only one, if there is one).
    fn source_of_node(&self, node: Option<&DbNode>) -> Option<DataSource> {
        let store = &self.database.as_ref()?.store;
        match node.map(|node| &node.kind) {
            Some(NodeKind::Source(source)) => Some((**source).clone()),
            Some(_) => node.and_then(DbNode::target).and_then(|target| store.source_of(target)).cloned(),
            None => match store.sources() {
                [only] => Some(only.clone()),
                _ => None,
            },
        }
    }

    /// A toolbar button or menu row of the tree, on `node`.
    fn database_tree_run(
        &mut self,
        action: TreeAction,
        node: Option<DbNode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = node.as_ref().and_then(DbNode::target).map(str::to_owned);
        let commands = match action {
            TreeAction::OpenConsole | TreeAction::JumpToConsole => {
                let target = target.or_else(|| self.source_of_node(None).map(|source| source.id));
                match target {
                    Some(target) => self.db_step(cx, |store, _| store.open_console(&target)),
                    None => vec![Command::ShowToolWindow],
                }
            }
            TreeAction::Properties => {
                if let Some(source) = self.source_of_node(node.as_ref()) {
                    self.open_source_dialog(Some(source), None, window, cx);
                }
                Vec::new()
            }
            TreeAction::Disconnect => {
                let database = node.as_ref().is_some_and(|node| matches!(node.kind, NodeKind::Database { .. }));
                self.db_step(cx, |store, _| store.disconnect_node(target.as_deref(), database))
            }
            TreeAction::EditData => {
                if let Some(at) = node.as_ref().and_then(DbNode::table_of) {
                    self.open_table_editor(at, window, cx);
                }
                Vec::new()
            }
            TreeAction::GoToDdl => match node.as_ref() {
                Some(node) => match (node.schema_object_of(), node.table_of()) {
                    (Some(o), _) => {
                        self.db_step(cx, |store, _| store.open_ddl(&o.id, &o.schema, &o.name, Some(o.kind)))
                    }
                    (None, Some(t)) => self.db_step(cx, |store, _| store.open_ddl(&t.id, &t.schema, &t.table, None)),
                    (None, None) => Vec::new(),
                },
                None => Vec::new(),
            },
            TreeAction::DuplicateSource => match self.source_of_node(node.as_ref()) {
                Some(source) => self.db_step(cx, |store, _| store.duplicate_source(&source.id, &random_id())),
                None => Vec::new(),
            },
            TreeAction::Rename => {
                if let Some(NodeKind::Source(source)) = node.as_ref().map(|node| &node.kind)
                    && let Some(slot) = self.database.as_mut()
                {
                    slot.questions.push_back(Question::Rename { id: source.id.clone(), name: source.name.clone() });
                    self.next_db_question(window, cx);
                } else {
                    tracing::info!("Database: renaming a schema object is not available in this window");
                }
                Vec::new()
            }
            TreeAction::Drop => match node.as_ref().map(|node| &node.kind) {
                Some(NodeKind::Source(source)) => {
                    let id = source.id.clone();
                    self.db_step(cx, |store, _| store.remove_source(&id))
                }
                _ => {
                    tracing::info!("Database: dropping a schema object is not available in this window");
                    Vec::new()
                }
            },
            TreeAction::CopyReference => {
                let text = match node.as_ref().map(|node| &node.kind) {
                    Some(NodeKind::Source(source)) => Some(source.name.clone()),
                    Some(NodeKind::Database { name, .. }) => Some(name.clone()),
                    Some(_) => {
                        let driver = self.source_of_node(node.as_ref()).map(|source| source.driver);
                        let object = node.as_ref().and_then(DbNode::object_of);
                        driver.zip(object).map(|(driver, object)| reference(driver, &object))
                    }
                    None => None,
                };
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                Vec::new()
            }
            TreeAction::NewDataSource(vendor) => {
                self.open_source_dialog(None, vendor, window, cx);
                Vec::new()
            }
            other => {
                tracing::info!(action = other.title(), "Database: not available in this window");
                Vec::new()
            }
        };
        self.carry_out_db(commands, window, cx);
    }

    // ---- consoles ---------------------------------------------------------------------------------

    /// Execute SQL Statement / Explain Plan on the active SQL editor: the statement at the caret, or
    /// the selected ones.
    fn run_sql_editor(&mut self, explain: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (path, text, range, language) = {
            let editor = editor.read(cx);
            let path = editor.path().to_string_lossy().into_owned();
            (path, editor.text(), editor.selection(), editor.language().to_string())
        };
        if language != SQL_LANGUAGE || !self.ensure_database(window, cx) {
            return;
        }
        let run = PendingRun { path, text, from: range.start, to: range.end, explain };
        let Some(slot) = self.database.as_mut() else { return };
        if !slot.store.loaded() {
            // The run starts once the data sources are read (`TreeUpdate::Sources`).
            slot.pending_run = Some(run);
            let commands = self.db_step(cx, |store, _| store.load_sources());
            self.carry_out_db(commands, window, cx);
            return;
        }
        self.run_console(run, window, cx);
    }

    fn run_console(&mut self, run: PendingRun, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.database.as_ref() else { return };
        if !run.explain && slot.store.console_source(&run.path).is_none() && slot.store.sources().is_empty() {
            self.open_source_dialog(None, None, window, cx);
            return;
        }
        let commands = self.db_step(cx, |store, _| {
            if run.explain {
                store.explain_console(&run.path, &run.text, run.from, run.to)
            } else {
                store.execute_console(&run.path, &run.text, run.from, run.to)
            }
        });
        self.carry_out_db(commands, window, cx);
    }

    // ---- results ----------------------------------------------------------------------------------

    fn database_results_event(
        &mut self,
        results: &Entity<ResultsView>,
        event: &ResultsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = match event.clone() {
            ResultsEvent::LoadDetails { tab, request } => {
                let weak = results.downgrade();
                let (id, schema, table) = (request.source_id, request.schema, request.table);
                self.db_call(
                    "db_table_details",
                    move |host, label| async move {
                        ide_host::ops::db::table_details(&host, &label, &id, &schema, &table).await
                    },
                    move |_, result, _, cx| {
                        let details = result.map_err(|error| error.message);
                        let _ = weak.update(cx, |results, cx| results.details_loaded(tab, details, cx));
                    },
                    window,
                    cx,
                );
                Vec::new()
            }
            ResultsEvent::Submit { tab, request } => {
                self.db_step(cx, |store, _| store.submit_result_edits(tab, request))
            }
            ResultsEvent::Rerun(id) => self.db_step(cx, |store, _| store.rerun_result(id)),
            ResultsEvent::Page { tab, page, page_size } => {
                self.db_step(cx, |store, _| store.load_result_page(tab, page, page_size, false))
            }
            ResultsEvent::CountRows(id) => self.db_step(cx, |store, _| store.count_result(id, false)),
            ResultsEvent::LastPage(id) => self.db_step(cx, |store, _| store.count_result(id, true)),
            ResultsEvent::ShowPageSizeMenu(position) => {
                let items = results.read(cx).page_size_menu();
                let results = results.clone();
                let run = move |_: &mut Self, size: u64, _: &mut Window, cx: &mut Context<Self>| {
                    results.update(cx, |results, cx| results.set_page_size(size, cx));
                };
                self.show_menu(&items, position, run, window, cx);
                Vec::new()
            }
            ResultsEvent::CloseTab(id) => {
                if let Some(slot) = self.database.as_mut() {
                    slot.store.result_closed(id);
                }
                Vec::new()
            }
        };
        self.carry_out_db(commands, window, cx);
    }

    /// The result grid's copy goes to the clipboard; its other requests stay with the grid.
    fn database_grid_event(&mut self, _: &Entity<GridView>, event: &GridEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let GridEvent::Copy(text) = event {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }
    }

    fn database_panel_event(
        &mut self,
        _: &Entity<DatabasePanel>,
        event: &DatabasePanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let DatabasePanelEvent::TableClosed(id) = event
            && let Some(slot) = self.database.as_mut()
        {
            slot.editor_events.remove(id);
        }
        self.sync_database_context(window, cx);
    }

    // ---- table editors ----------------------------------------------------------------------------

    /// Edit Data: the table's editor in the tool window, opened or brought forward, with the
    /// keyboard.
    fn open_table_editor(&mut self, at: TableAt, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ensure_database(window, cx) {
            return;
        }
        let Some(slot) = self.database.as_mut() else { return };
        let panel = slot.panel.clone();
        let table = TableRef { target: at.id.clone(), schema: at.schema, table: at.table };
        let existing = panel.read(cx).editor_of(&table, cx).cloned();
        let editor = match existing {
            Some(editor) => {
                let id = editor.read(cx).id();
                panel.update(cx, |panel, cx| panel.select(Lower::Table(id), cx));
                editor
            }
            None => {
                let id = slot.next_editor;
                slot.next_editor += 1;
                let source = slot.store.source_of(&at.id);
                let caps = SourceCaps {
                    driver: source.map_or(ide_db::DbDriver::Sqlite, |source| source.driver),
                    read_only: source.is_some_and(|source| source.read_only),
                };
                let editor = cx.new(|cx| TableEditorView::new(id, table, caps, window, cx));
                let focus = editor.focus_handle(cx);
                let events = vec![
                    cx.subscribe_in(&editor, window, Self::table_editor_event),
                    cx.observe_in(&editor, window, |workbench, _, window, cx| {
                        workbench.sync_database_context(window, cx)
                    }),
                    cx.on_focus_in(&focus, window, |workbench, window, cx| workbench.sync_database_context(window, cx)),
                    cx.on_focus_out(&focus, window, |workbench, _, window, cx| {
                        workbench.sync_database_context(window, cx)
                    }),
                ];
                if let Some(slot) = self.database.as_mut() {
                    slot.editor_events.insert(id, events);
                }
                panel.update(cx, |panel, cx| panel.add_editor(editor.clone(), cx));
                editor.update(cx, |editor, cx| editor.load(cx));
                editor
            }
        };
        self.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, false));
        window.focus(&editor.focus_handle(cx), cx);
        self.sync_database_context(window, cx);
    }

    fn table_editor_event(
        &mut self,
        editor: &Entity<TableEditorView>,
        event: &TableEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = editor.read(cx).id();
        let commands = match event.clone() {
            TableEditorEvent::LoadPage(request) => self.db_step(cx, |store, _| store.load_table_page(id, request)),
            TableEditorEvent::Submit { table, edits } => self
                .db_step(cx, |store, _| store.apply_table_edits(id, &table.target, &table.schema, &table.table, edits)),
            TableEditorEvent::Export { path, contents, rows } => {
                self.db_call(
                    "db_export_file",
                    move |_, _| async move {
                        let written = tokio::task::spawn_blocking(move || std::fs::write(&path, contents)).await;
                        match written {
                            Ok(result) => result.map_err(IdeError::from),
                            Err(error) => Err(IdeError::internal(format!("the export did not complete: {error}"))),
                        }
                    },
                    move |workbench, result, _, cx| {
                        let request = match result {
                            Ok(()) => {
                                NotificationRequest::new(NotificationLevel::Info, format!("{rows} rows exported"))
                            }
                            Err(error) => {
                                let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot export");
                                request.body = Some(error.message);
                                request
                            }
                        };
                        workbench.notify(request, cx);
                    },
                    window,
                    cx,
                );
                Vec::new()
            }
            TableEditorEvent::CopyText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                Vec::new()
            }
            TableEditorEvent::Import(_) => {
                tracing::info!("Database: Import Data is not available in this window");
                Vec::new()
            }
            TableEditorEvent::BrowseExport { .. } => {
                tracing::info!("Database: the export's save dialog is not available in this window");
                Vec::new()
            }
        };
        self.carry_out_db(commands, window, cx);
        self.sync_database_context(window, cx);
    }

    /// `dbTableEditorFocus` and `dbTableEditorChanges`: the shown table editor has the keyboard,
    /// and has edits to submit.
    pub(super) fn sync_database_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (focused, dirty) = match self.database_table_editor(cx) {
            Some(editor) => (editor.focus_handle(cx).contains_focused(window, cx), editor.read(cx).is_dirty()),
            None => (false, false),
        };
        set_context(window, cx, TABLE_EDITOR_FOCUS, focused);
        set_context(window, cx, TABLE_EDITOR_CHANGES, focused && dirty);
    }
}

/// The store's answer to a password or SSH secret question.
pub(super) fn secret_answer(values: Vec<String>, save: bool) -> PromptAnswer {
    match values.into_iter().next() {
        Some(value) => PromptAnswer::Secret { value: value.into(), save },
        None => PromptAnswer::Cancel,
    }
}

/// The store's answer to a yes / no question.
fn confirm_answer(answer: &DialogAnswer) -> PromptAnswer {
    if answer.button == OK { PromptAnswer::Confirm } else { PromptAnswer::Cancel }
}

/// A tree update of the store, on the tree.
fn apply_tree_update(tree: &mut DatabaseTree, update: TreeUpdate, cx: &mut Context<DatabaseTree>) {
    match update {
        TreeUpdate::Sources(sources) => tree.set_sources(sources, cx),
        TreeUpdate::Connected(connected) => tree.set_connected(connected, cx),
        TreeUpdate::Databases { id, result } => tree.databases_loaded(&id, result, cx),
        TreeUpdate::Schemas { target, result } => tree.schemas_loaded(&target, result, cx),
        TreeUpdate::Tables { target, schema, result, objects } => {
            tree.tables_loaded(&target, &schema, result, objects, cx)
        }
        TreeUpdate::Details { target, schema, table, result } => {
            tree.details_loaded(&target, &schema, &table, result, cx)
        }
        // The tree keeps what it shows: a refresh reads its levels again and answers them, and a
        // deactivated target keeps its metadata (expanded nodes would otherwise reconnect at once).
        TreeUpdate::Forget(_) => {}
        TreeUpdate::Reveal { expand, key } => {
            for node in &expand {
                tree.set_expanded(node, true, cx);
            }
            tree.reveal(&key, cx);
        }
    }
}

/// A result pane update of the store, on the pane.
fn apply_results_update(results: &mut ResultsView, update: ResultsUpdate, cx: &mut Context<ResultsView>) {
    match update {
        ResultsUpdate::Show(tab) => results.show_result(*tab, cx),
        ResultsUpdate::Update { id, result, page, page_size } => results.update_result(id, result, page, page_size, cx),
        ResultsUpdate::Running { id, running } => results.set_running(id, running, cx),
        ResultsUpdate::Total { id, total } => results.set_total(id, total, cx),
        ResultsUpdate::Log(lines) => results.log(lines, cx),
        ResultsUpdate::Select(id) => results.select_tab(id, cx),
        ResultsUpdate::Close(id) => results.close_tab(id, cx),
        ResultsUpdate::Plan { id, plan, .. } => {
            tracing::info!(id, nodes = plan.len(), "Database: the Query Plan tab is not drawn in this window");
        }
        ResultsUpdate::Submitted { id, result } => results.submit_finished(id, result, cx),
    }
}
