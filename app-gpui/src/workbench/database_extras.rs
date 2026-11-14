//! The rest of the Database tools of a project: the query console's toolbar over every SQL editor,
//! Query History, the Query Plan tab, the tree's DDL, navigation and object actions (Modify Table,
//! New Table / Column, Truncate), Import Data from File, Run SQL Script, SQL Script templates, and
//! Go to Column in the table editor.
//!
//! Everything here drives the same connection store as `database.rs` ([`DbStore`]) and runs its
//! backend work off the window's thread: through the store's requests, whose trust check is
//! `run_op`'s, or through `db_call`, which checks the project's trust for the command's IPC name
//! first. SQL a dialog runs comes from the statements `database::objects` writes, which quote every
//! name; imported values are bound by `db_apply_edits`, never written into SQL.

use super::Workbench;
use super::database::DATABASE_VIEW_FOCUS;
use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use crate::database::console_view::{ConsoleEvent, ConsoleView};
use crate::database::history_view::{HistoryDialog, HistoryEvent};
use crate::database::import_dialog::{ImportDialog, ImportEvent};
use crate::database::model::js_number_string;
use crate::database::modify_table_dialog::{ModifyTableDialog, ModifyTableEvent, ModifyTableRequest};
use crate::database::nodes::{DbNode, Folder, NodeKind, table_folder};
use crate::database::objects::{can_modify_table, script_sql};
use crate::database::plan_view::{PlanEvent, PlanView};
use crate::database::store::{Command, DbStore, readable_ddl};
use crate::database::tree_view::TreeAction;
use crate::database::truncate_dialog::{TruncateDialog, TruncateEvent};
use crate::editor::view::Editor;
use crate::platform::actions::{Action, ActionRegistry, manifest};
use gpui_kit::{
    AnyView, App, AsyncWindowContext, ClipboardItem, Context, Entity, EntityId, FocusHandle, Focusable,
    PathPromptOptions, Subscription, Window,
};
use ide_core::{IdeError, IdeResult};
use ide_db::{DbDriver, DbObjectKind, DbRowEdit, DbTableKind, PlanNode};
use ide_host::Host;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The `when` key of the tree's actions.
const PROJECT_OPEN: &str = "projectOpen";
/// The language id of SQL editors.
const SQL_LANGUAGE: &str = "sql";
/// The notification key of Import Data's outcome.
const IMPORT_KEY: &str = "db-import";

/// The tree's actions this module carries out, by id.
const TREE_ACTIONS: [(&str, TreeAction); 11] = [
    ("db.jumpToConsole", TreeAction::JumpToConsole),
    ("db.ddlToClipboard", TreeAction::DdlToClipboard),
    ("db.ddlToConsole", TreeAction::DdlToConsole),
    ("db.goToReferencedTable", TreeAction::GoToReferencedTable),
    ("db.modifyTable", TreeAction::ModifyTable),
    ("db.newTable", TreeAction::NewTable),
    ("db.newColumn", TreeAction::NewColumn),
    ("db.truncate", TreeAction::Truncate),
    ("db.importData", TreeAction::ImportData),
    ("db.runSqlScript", TreeAction::RunSqlScript),
    ("db.separateProceduresAndFunctions", TreeAction::ToggleSeparateProceduresAndFunctions),
];

/// The query console around an SQL editor.
struct ConsoleSlot {
    path: String,
    view: Entity<ConsoleView>,
    _events: Subscription,
}

/// The dialog of this module that is open: one at a time, as the Database tools have one.
enum ExtraDialog {
    History(Entity<HistoryDialog>),
    /// Modify / Create Table on the connection target.
    Modify {
        view: Entity<ModifyTableDialog>,
        target: String,
    },
    Truncate {
        view: Entity<TruncateDialog>,
        target: String,
    },
    Import {
        view: Entity<ImportDialog>,
        target: String,
        schema: String,
        table: String,
    },
}

impl ExtraDialog {
    fn view(&self) -> AnyView {
        match self {
            ExtraDialog::History(view) => view.clone().into(),
            ExtraDialog::Modify { view, .. } => view.clone().into(),
            ExtraDialog::Truncate { view, .. } => view.clone().into(),
            ExtraDialog::Import { view, .. } => view.clone().into(),
        }
    }

    fn focus(&self, cx: &App) -> FocusHandle {
        match self {
            ExtraDialog::History(view) => view.focus_handle(cx),
            ExtraDialog::Modify { view, .. } => view.focus_handle(cx),
            ExtraDialog::Truncate { view, .. } => view.focus_handle(cx),
            ExtraDialog::Import { view, .. } => view.focus_handle(cx),
        }
    }
}

struct DialogSlot {
    dialog: ExtraDialog,
    /// Where the keyboard goes when the dialog closes.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The Query Plan tab.
struct PlanSlot {
    view: Entity<PlanView>,
    _events: Subscription,
}

/// What this module keeps of a project's Database tools; it goes with them.
#[derive(Default)]
pub(super) struct ExtrasSlot {
    /// The consoles, by the entity of their editor.
    consoles: HashMap<EntityId, ConsoleSlot>,
    dialog: Option<DialogSlot>,
    plan: Option<PlanSlot>,
    /// The job of the statements the open dialog ran (`Command::SqlDone`).
    dialog_job: Option<u64>,
    /// Browse Query History asked for before the data sources were read: the console it pastes to.
    history_wanted: Option<Option<String>>,
}

/// Registers the Database actions this module carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    // The tree's actions act on the node the argument names (its key), else on the selection; their
    // shortcuts work only while the tree has the keyboard.
    for (id, action) in TREE_ACTIONS {
        let meta = manifest()
            .get(id)
            .unwrap_or_else(|| panic!("action {id} is not in assets/gen/actions.json"))
            .clone()
            .when(PROJECT_OPEN)
            .key_when(DATABASE_VIEW_FOCUS);
        registry.register(Action::new(meta, move |call, window, cx| {
            let key = call.arg.and_then(serde_json::Value::as_str).map(str::to_owned);
            on_workbench(window, cx, move |workbench, window, cx| workbench.run_tree_action(action, key, window, cx))
        }));
    }
    registry.handle("db.queryHistory", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.browse_query_history(window, cx))
    });
    registry.handle("db.goToColumn", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some(editor) = workbench.database_table_editor(cx) {
                editor.update(cx, |editor, cx| editor.show_columns(window, cx));
            }
        })
    });
}

/// What Generate DDL reads.
enum DdlScope {
    /// A routine, trigger or sequence.
    Object {
        schema: String,
        kind: DbObjectKind,
        name: String,
    },
    Table {
        schema: String,
        table: String,
    },
    /// Every table and view of these schemas (`None`: of all the target's), of one folder only.
    Tables {
        schemas: Option<Vec<String>>,
        folder: Option<Folder>,
    },
}

/// The DDL of `scope` on `target`: tables before views, as views select from them.
async fn generate_ddl(
    host: Arc<Host>,
    label: String,
    target: String,
    scope: DdlScope,
    driver: DbDriver,
) -> IdeResult<String> {
    use ide_host::ops::db;
    let (host, w) = (host.as_ref(), label.as_str());
    match scope {
        DdlScope::Object { schema, kind, name } => db::object_ddl(host, w, &target, &schema, kind, &name).await,
        DdlScope::Table { schema, table } => {
            Ok(readable_ddl(&db::ddl(host, w, &target, &schema, &table).await?, driver))
        }
        DdlScope::Tables { schemas, folder } => {
            let schemas = match schemas {
                Some(schemas) => schemas,
                None => db::schemas(host, w, &target).await?,
            };
            let mut parts = Vec::new();
            for schema in &schemas {
                let mut tables = db::tables(host, w, &target, schema).await?;
                tables.retain(|table| folder.is_none_or(|folder| table_folder(table.kind) == folder));
                tables.sort_by_key(|table| table.kind != DbTableKind::Table);
                for table in &tables {
                    parts.push(readable_ddl(&db::ddl(host, w, &target, schema, &table.name).await?, driver));
                }
            }
            Ok(parts.join("\n"))
        }
    }
}

/// The first path of an open dialog's answer; a dialog that failed is logged.
fn picked_path<E>(picked: Result<anyhow::Result<Option<Vec<PathBuf>>>, E>) -> Option<PathBuf> {
    match picked {
        Ok(Ok(Some(paths))) => paths.into_iter().next(),
        Ok(Ok(None)) | Err(_) => None,
        Ok(Err(error)) => {
            tracing::warn!("database: cannot pick a file: {error:#}");
            None
        }
    }
}

/// The text of `path`, read off the window's thread.
async fn read_text(path: PathBuf, cx: &AsyncWindowContext) -> IdeResult<String> {
    cx.background_executor().spawn(async move { std::fs::read_to_string(&path).map_err(IdeError::from) }).await
}

fn error_notice(title: &str, error: &IdeError) -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Error, title);
    request.body = Some(error.message.clone());
    request.group = Some("tools".to_owned());
    request
}

impl Workbench {
    // ---- reading the store ------------------------------------------------------------------------

    fn database_store(&self) -> Option<&DbStore> {
        self.database.as_ref().map(|slot| &slot.store)
    }

    fn database_extras(&mut self) -> Option<&mut ExtrasSlot> {
        self.database.as_mut().map(|slot| &mut slot.extras)
    }

    fn driver_of_target(&self, target: &str) -> DbDriver {
        self.database_store().and_then(|store| store.source_of(target)).map_or(DbDriver::Sqlite, |source| source.driver)
    }

    /// The dialog of this module over the window, while one is open.
    pub(super) fn database_extras_dialog(&self) -> Option<AnyView> {
        self.database.as_ref()?.extras.dialog.as_ref().map(|slot| slot.dialog.view())
    }

    /// The console of an SQL editor, drawn in its place in the editor area.
    pub(super) fn sql_console_of(&self, editor: &Entity<Editor>) -> Option<Entity<ConsoleView>> {
        self.database.as_ref()?.extras.consoles.get(&editor.entity_id()).map(|slot| slot.view.clone())
    }

    /// Whether `id` is the result id of the Query Plan tab.
    pub(super) fn is_query_plan(&self, id: u64, cx: &App) -> bool {
        let plan = self.database.as_ref().and_then(|slot| slot.extras.plan.as_ref());
        plan.is_some_and(|plan| plan.view.read(cx).id() == id)
    }

    // ---- after every store step ---------------------------------------------------------------

    /// Brings what this module shows up to date with the store: the consoles' toolbars, the Query
    /// History dialog, and a history asked for before the data sources were read.
    pub(super) fn sync_database_extras(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let alive: HashSet<EntityId> = self.editors().iter().map(Entity::entity_id).collect();
        let Some(slot) = self.database.as_mut() else { return };
        // A closed editor's console goes with it.
        slot.extras.consoles.retain(|editor, _| alive.contains(editor));
        let store = &slot.store;
        let bars: Vec<_> = slot
            .extras
            .consoles
            .values()
            .map(|console| {
                let bar = store.console_bar(&console.path);
                let driver = bar.bound.as_deref().and_then(|bound| store.source_of(bound)).map(|source| source.driver);
                (console.view.clone(), bar, driver)
            })
            .collect();
        let history = store.history().cloned();
        let open_history = match slot.extras.dialog.as_ref().map(|slot| &slot.dialog) {
            Some(ExtraDialog::History(view)) => Some(view.clone()),
            _ => None,
        };
        let wanted = if store.loaded() { slot.extras.history_wanted.take() } else { None };
        for (view, bar, driver) in bars {
            if view.read(cx).bar() != &bar {
                view.update(cx, |view, cx| view.set_state(bar, driver, cx));
            }
        }
        match (open_history, history) {
            (Some(view), Some(state)) => view.update(cx, |view, cx| view.set_state(state, cx)),
            // Paste took the history: the dialog closes.
            (Some(_), None) => self.close_extra_dialog(window, cx),
            (None, _) => {}
        }
        if let Some(path) = wanted {
            self.open_query_history(path, window, cx);
        }
    }

    // ---- query consoles -----------------------------------------------------------------------

    /// Puts the console toolbar around a newly opened SQL editor of the project: a file bound to a
    /// data source, or one to bind.
    pub(super) fn wrap_sql_console(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let (path, language, read_only) = {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.language().to_string(), editor.is_read_only())
        };
        if language != SQL_LANGUAGE || read_only || !self.ensure_database(window, cx) {
            return;
        }
        let id = editor.entity_id();
        if self.database.as_ref().is_some_and(|slot| slot.extras.consoles.contains_key(&id)) {
            return;
        }
        let view = cx.new(|cx| ConsoleView::new(&path, editor.clone().into(), window, cx));
        let source = editor.downgrade();
        view.update(cx, |view, _| {
            view.set_text_source(Box::new(move |cx: &App| {
                let editor = source.upgrade()?;
                let editor = editor.read(cx);
                Some((editor.text(), editor.selection()))
            }));
        });
        let events = cx.subscribe_in(&view, window, Self::console_event);
        if let Some(extras) = self.database_extras() {
            extras.consoles.insert(id, ConsoleSlot { path: path.clone(), view, _events: events });
        }
        let commands = self.db_step(cx, |store, _| store.ensure_console(&path));
        self.carry_out_db(commands, window, cx);
    }

    fn console_event(
        &mut self,
        _: &Entity<ConsoleView>,
        event: &ConsoleEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = match event.clone() {
            ConsoleEvent::Execute { path, text, from, to, .. } => {
                self.db_step(cx, |store, _| store.execute_console(&path, &text, from, to))
            }
            ConsoleEvent::Explain { path, text, from, to, .. } => {
                self.db_step(cx, |store, _| store.explain_console(&path, &text, from, to))
            }
            ConsoleEvent::Cancel { path } => self.db_step(cx, |store, _| store.cancel_console(&path)),
            ConsoleEvent::History { path } => self.db_step(cx, |store, _| store.open_history(Some(&path), None)),
            ConsoleEvent::SetMaxRows(rows) => self.db_step(cx, |store, _| {
                store.set_max_rows(rows);
                Vec::new()
            }),
            ConsoleEvent::SetManualCommit { target, manual } => {
                self.db_step(cx, |store, _| store.set_manual_commit(&target, manual))
            }
            ConsoleEvent::FinishTransaction { target, commit } => {
                self.db_step(cx, |store, _| store.finish_transaction(&target, commit))
            }
            ConsoleEvent::SetSchema { path, schema } => {
                self.db_step(cx, |store, _| store.set_console_schema(&path, Some(&schema)))
            }
            ConsoleEvent::Bind { path, target } => {
                self.db_step(cx, |store, _| store.bind_console(&path, Some(&target)))
            }
        };
        self.carry_out_db(commands, window, cx);
    }

    /// The error banner of console `path`'s last run (`Command::MarkOutcomes`); `None` hides it.
    pub(super) fn mark_sql_console(&mut self, path: &str, banner: Option<String>, cx: &mut Context<Self>) {
        let views: Vec<Entity<ConsoleView>> = self
            .database
            .as_ref()
            .map(|slot| slot.extras.consoles.values().filter(|c| c.path == path).map(|c| c.view.clone()).collect())
            .unwrap_or_default();
        for view in views {
            let banner = banner.clone();
            view.update(cx, |view, cx| view.set_banner(banner, cx));
        }
    }

    #[cfg(test)]
    pub(super) fn database_console(&self, path: &Path) -> Option<Entity<ConsoleView>> {
        let path = path.to_string_lossy();
        let slot = self.database.as_ref()?;
        slot.extras.consoles.values().find(|console| console.path == path).map(|console| console.view.clone())
    }

    // ---- Query History ------------------------------------------------------------------------

    /// Browse Query History: of the active SQL editor's data source (Paste goes there), else of the
    /// tree's selection or the only data source.
    fn browse_query_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ensure_database(window, cx) {
            return;
        }
        let path = self.editor().and_then(|editor| {
            let editor = editor.read(cx);
            (editor.language().to_string() == SQL_LANGUAGE).then(|| editor.path().to_string_lossy().into_owned())
        });
        let loaded = self.database_store().is_some_and(DbStore::loaded);
        if !loaded {
            if let Some(extras) = self.database_extras() {
                extras.history_wanted = Some(path);
            }
            let commands = self.db_step(cx, |store, _| store.load_sources());
            self.carry_out_db(commands, window, cx);
            return;
        }
        self.open_query_history(path, window, cx);
    }

    fn open_query_history(&mut self, path: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let bound = path.as_deref().and_then(|path| self.database_store().and_then(|store| store.console_source(path)));
        let target = bound.or_else(|| self.database_tree().and_then(|tree| tree.read(cx).selected_target()));
        let commands = self.db_step(cx, |store, _| store.open_history(path.as_deref(), target.as_deref()));
        self.carry_out_db(commands, window, cx);
    }

    /// `Command::ShowHistory`: the dialog over the store's history.
    pub(super) fn show_query_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((state, label)) = self
            .database_store()
            .and_then(|store| store.history().map(|history| (history.clone(), store.target_label(&history.target))))
        else {
            return;
        };
        if let Some(Some(ExtraDialog::History(view))) =
            self.database.as_ref().map(|slot| slot.extras.dialog.as_ref().map(|slot| &slot.dialog))
        {
            view.clone().update(cx, |view, cx| view.set_state(state, cx));
            return;
        }
        let view = cx.new(|cx| HistoryDialog::new(&label, state, window, cx));
        let events = cx.subscribe_in(&view, window, Self::history_event);
        self.open_extra_dialog(ExtraDialog::History(view), events, window, cx);
    }

    fn history_event(
        &mut self,
        _: &Entity<HistoryDialog>,
        event: &HistoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = match event.clone() {
            HistoryEvent::Query(query) => self.db_step(cx, |store, _| {
                store.set_history_query(&query);
                Vec::new()
            }),
            HistoryEvent::Move(delta) => self.db_step(cx, |store, _| {
                store.move_history(delta);
                Vec::new()
            }),
            HistoryEvent::Select(index) => self.db_step(cx, |store, _| {
                store.select_history(index);
                Vec::new()
            }),
            HistoryEvent::Paste => {
                let commands = self.db_step(cx, |store, _| store.paste_history());
                self.close_extra_dialog(window, cx);
                commands
            }
            HistoryEvent::Remove(texts) => self.db_step(cx, |store, _| store.remove_history(Some(texts))),
            HistoryEvent::Clear => self.db_step(cx, |store, _| store.remove_history(None)),
            HistoryEvent::Close => {
                self.db_step(cx, |store, _| {
                    store.close_history();
                    Vec::new()
                });
                self.close_extra_dialog(window, cx);
                Vec::new()
            }
        };
        self.carry_out_db(commands, window, cx);
    }

    #[cfg(test)]
    pub(super) fn database_history_dialog(&self) -> Option<Entity<HistoryDialog>> {
        match &self.database.as_ref()?.extras.dialog.as_ref()?.dialog {
            ExtraDialog::History(view) => Some(view.clone()),
            _ => None,
        }
    }

    // ---- dialogs --------------------------------------------------------------------------------

    fn open_extra_dialog(
        &mut self,
        dialog: ExtraDialog,
        events: Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = dialog.focus(cx);
        let Some(extras) = self.database_extras() else { return };
        let restore = match extras.dialog.take() {
            Some(previous) => previous.restore,
            None => window.focused(cx),
        };
        extras.dialog_job = None;
        extras.dialog = Some(DialogSlot { dialog, restore, _events: events });
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_extra_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extras) = self.database_extras() else { return };
        extras.dialog_job = None;
        let Some(slot) = extras.dialog.take() else { return };
        if let Some(target) = slot.restore {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// `Command::SqlDone`: the statements a dialog ran finished; a success closes it.
    pub(super) fn database_sql_done(&mut self, job: u64, ok: bool, cx: &mut Context<Self>) {
        let Some(extras) = self.database_extras() else { return };
        if extras.dialog_job != Some(job) {
            return;
        }
        extras.dialog_job = None;
        match extras.dialog.as_ref().map(|slot| &slot.dialog) {
            Some(ExtraDialog::Modify { view, .. }) => view.clone().update(cx, |view, cx| view.finished(ok, cx)),
            Some(ExtraDialog::Truncate { view, .. }) => view.clone().update(cx, |view, cx| view.finished(ok, cx)),
            _ => {}
        }
    }

    /// Runs statements of the open dialog on `target` (logged in the Output tab); the dialog hears
    /// how it went through [`Self::database_sql_done`].
    fn run_dialog_sql(&mut self, target: &str, statements: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let mut job = None;
        let commands = self.db_step(cx, |store, _| {
            let (ran, commands) = store.run_sql(target, statements);
            job = Some(ran);
            commands
        });
        if let Some(extras) = self.database_extras() {
            extras.dialog_job = job;
        }
        self.carry_out_db(commands, window, cx);
    }

    // ---- the tree's actions -------------------------------------------------------------------

    /// A tree action `database.rs` leaves to this module, on `node`.
    pub(super) fn run_extra_tree_action(
        &mut self,
        action: TreeAction,
        node: Option<DbNode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Command> {
        let target = node.as_ref().and_then(DbNode::target).map(str::to_owned);
        match action {
            TreeAction::DdlToClipboard => {
                if let Some(node) = node {
                    self.generate_node_ddl(node, false, window, cx);
                }
                Vec::new()
            }
            TreeAction::DdlToConsole => {
                if let Some(node) = node {
                    self.generate_node_ddl(node, true, window, cx);
                }
                Vec::new()
            }
            TreeAction::GoToReferencedTable => match node.map(|node| node.kind) {
                Some(NodeKind::ForeignKey { at, fk }) => self.db_step(cx, |store, _| {
                    store.reveal_in_tree(&at.id, &fk.ref_schema, &fk.ref_table, DbTableKind::Table, None)
                }),
                _ => Vec::new(),
            },
            TreeAction::ModifyTable | TreeAction::NewColumn => {
                let Some(node) = node else { return Vec::new() };
                let Some(at) = node.table_of() else { return Vec::new() };
                let driver = self.driver_of_target(&at.id);
                if at.view || !can_modify_table(driver) {
                    return Vec::new();
                }
                let focus_column = match &node.kind {
                    NodeKind::Column { column, .. } if action == TreeAction::ModifyTable => Some(column.name.clone()),
                    _ => None,
                };
                let request = ModifyTableRequest {
                    target: at.id,
                    schema: at.schema,
                    table: Some(at.table),
                    driver,
                    focus_column,
                    add_column: action == TreeAction::NewColumn,
                };
                self.open_modify_table(request, window, cx);
                Vec::new()
            }
            TreeAction::NewTable => {
                let (Some(node), Some(target)) = (node, target) else { return Vec::new() };
                let driver = self.driver_of_target(&target);
                let schema = match &node.kind {
                    NodeKind::Schema { schema, .. }
                    | NodeKind::Folder { schema, .. }
                    | NodeKind::Table { schema, .. }
                    | NodeKind::Routine { schema, .. }
                    | NodeKind::Sequence { schema, .. } => Some(schema.clone()),
                    _ => node.table_of().map(|at| at.schema),
                };
                let schema = schema.or_else(|| (driver == DbDriver::Sqlite).then(|| "main".to_owned()));
                if let Some(schema) = schema {
                    let request = ModifyTableRequest {
                        target,
                        schema,
                        table: None,
                        driver,
                        focus_column: None,
                        add_column: false,
                    };
                    self.open_modify_table(request, window, cx);
                }
                Vec::new()
            }
            TreeAction::Truncate => {
                if let Some(NodeKind::Table { id, schema, table }) = node.map(|node| node.kind)
                    && table.kind == DbTableKind::Table
                {
                    let driver = self.driver_of_target(&id);
                    let view = cx.new(|cx| TruncateDialog::new(&schema, &table.name, driver, cx));
                    let events = cx.subscribe_in(&view, window, Self::truncate_event);
                    self.open_extra_dialog(ExtraDialog::Truncate { view, target: id }, events, window, cx);
                }
                Vec::new()
            }
            TreeAction::ImportData => {
                if let Some(NodeKind::Table { id, schema, table }) = node.map(|node| node.kind)
                    && table.kind == DbTableKind::Table
                    && self.driver_of_target(&id) != DbDriver::Clickhouse
                {
                    self.open_import_dialog(id, schema, table.name, window, cx);
                }
                Vec::new()
            }
            TreeAction::RunSqlScript => {
                let only = || match self.database_store().map(DbStore::sources) {
                    Some([only]) => Some(only.id.clone()),
                    _ => None,
                };
                if let Some(target) = target.or_else(only) {
                    self.run_sql_script_file(target, window, cx);
                }
                Vec::new()
            }
            TreeAction::SqlScript(kind) => {
                if let Some(at) = node.as_ref().and_then(DbNode::table_of) {
                    let driver = self.driver_of_target(&at.id);
                    let (id, schema, table) = (at.id.clone(), at.schema.clone(), at.table.clone());
                    self.db_call(
                        "db_table_details",
                        move |host, label| async move {
                            ide_host::ops::db::table_details(&host, &label, &id, &schema, &table).await
                        },
                        move |workbench, result, window, cx| {
                            let columns = result.map(|details| details.columns).unwrap_or_default();
                            match script_sql(driver, kind, &at.schema, &at.table, &columns) {
                                Ok(text) => {
                                    let commands =
                                        workbench.db_step(cx, |store, _| store.open_console_with(&at.id, &text));
                                    workbench.carry_out_db(commands, window, cx);
                                }
                                Err(reason) => {
                                    let mut request = NotificationRequest::new(
                                        NotificationLevel::Error,
                                        "Cannot generate the SQL script",
                                    );
                                    request.body = Some(reason.to_owned());
                                    workbench.notify(request, cx);
                                }
                            }
                        },
                        window,
                        cx,
                    );
                }
                Vec::new()
            }
            other => {
                tracing::debug!(action = other.title(), "Database: this tree action has no handler here");
                Vec::new()
            }
        }
    }

    /// Generate DDL to Clipboard / to Query Console of a node: a routine, trigger or sequence; a
    /// table or view; or every table and view of a folder, schema, database or data source.
    fn generate_node_ddl(&mut self, node: DbNode, to_console: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = node.target().map(str::to_owned) else { return };
        let driver = self.driver_of_target(&target);
        let (scope, command) = if let Some(object) = node.schema_object_of() {
            let scope = DdlScope::Object { schema: object.schema, kind: object.kind, name: object.name };
            (scope, "db_object_ddl")
        } else if let Some(at) = node.table_of() {
            (DdlScope::Table { schema: at.schema, table: at.table }, "db_ddl")
        } else {
            let scope = match &node.kind {
                NodeKind::Schema { schema, .. } => {
                    DdlScope::Tables { schemas: Some(vec![schema.clone()]), folder: None }
                }
                NodeKind::Folder { schema, folder, .. } => {
                    DdlScope::Tables { schemas: Some(vec![schema.clone()]), folder: Some(*folder) }
                }
                NodeKind::Source(_) | NodeKind::Database { .. } => DdlScope::Tables { schemas: None, folder: None },
                _ => return,
            };
            (scope, "db_ddl")
        };
        let id = target.clone();
        self.db_call(
            command,
            move |host, label| generate_ddl(host, label, id, scope, driver),
            move |workbench, result, window, cx| match result {
                Ok(text) if to_console => {
                    let commands = workbench.db_step(cx, |store, _| store.open_console_with(&target, &text));
                    workbench.carry_out_db(commands, window, cx);
                }
                Ok(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Err(error) => workbench.notify(error_notice("Cannot generate the DDL", &error), cx),
            },
            window,
            cx,
        );
    }

    // ---- Modify Table / Create Table ----------------------------------------------------------

    fn open_modify_table(&mut self, request: ModifyTableRequest, window: &mut Window, cx: &mut Context<Self>) {
        let target = request.target.clone();
        let load = request.table.clone().map(|table| (request.target.clone(), request.schema.clone(), table));
        let view = cx.new(|cx| ModifyTableDialog::new(request, window, cx));
        let events = cx.subscribe_in(&view, window, Self::modify_table_event);
        self.open_extra_dialog(ExtraDialog::Modify { view: view.clone(), target }, events, window, cx);
        if let Some((id, schema, table)) = load {
            let weak = view.downgrade();
            self.db_call(
                "db_table_details",
                move |host, label| async move {
                    ide_host::ops::db::table_details(&host, &label, &id, &schema, &table).await
                },
                move |_, result, _, cx| {
                    let columns = result.map(|details| details.columns).map_err(|error| error.message);
                    let _ = weak.update(cx, |view, cx| view.set_columns(columns, cx));
                },
                window,
                cx,
            );
        }
    }

    fn modify_table_event(
        &mut self,
        _: &Entity<ModifyTableDialog>,
        event: &ModifyTableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ModifyTableEvent::Apply(statements) => {
                let target = match self.database.as_ref().and_then(|slot| slot.extras.dialog.as_ref()) {
                    Some(DialogSlot { dialog: ExtraDialog::Modify { target, .. }, .. }) => target.clone(),
                    _ => return,
                };
                self.run_dialog_sql(&target, statements, window, cx);
            }
            ModifyTableEvent::Cancelled => self.close_extra_dialog(window, cx),
        }
    }

    #[cfg(test)]
    pub(super) fn database_modify_dialog(&self) -> Option<Entity<ModifyTableDialog>> {
        match &self.database.as_ref()?.extras.dialog.as_ref()?.dialog {
            ExtraDialog::Modify { view, .. } => Some(view.clone()),
            _ => None,
        }
    }

    // ---- Truncate -----------------------------------------------------------------------------

    fn truncate_event(
        &mut self,
        _: &Entity<TruncateDialog>,
        event: &TruncateEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TruncateEvent::Apply(sql) => {
                let target = match self.database.as_ref().and_then(|slot| slot.extras.dialog.as_ref()) {
                    Some(DialogSlot { dialog: ExtraDialog::Truncate { target, .. }, .. }) => target.clone(),
                    _ => return,
                };
                self.run_dialog_sql(&target, std::slice::from_ref(sql), window, cx);
            }
            TruncateEvent::Cancelled => self.close_extra_dialog(window, cx),
        }
    }

    #[cfg(test)]
    pub(super) fn database_truncate_dialog(&self) -> Option<Entity<TruncateDialog>> {
        match &self.database.as_ref()?.extras.dialog.as_ref()?.dialog {
            ExtraDialog::Truncate { view, .. } => Some(view.clone()),
            _ => None,
        }
    }

    // ---- Import Data from File ----------------------------------------------------------------

    /// Import Data from File… into `table` (from the tree or the table editor's toolbar).
    pub(super) fn open_import_dialog(
        &mut self,
        target: String,
        schema: String,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| ImportDialog::new(&schema, &table, cx));
        let events = cx.subscribe_in(&view, window, Self::import_event);
        let dialog = ExtraDialog::Import {
            view: view.clone(),
            target: target.clone(),
            schema: schema.clone(),
            table: table.clone(),
        };
        self.open_extra_dialog(dialog, events, window, cx);
        let weak = view.downgrade();
        self.db_call(
            "db_table_details",
            move |host, label| async move {
                ide_host::ops::db::table_details(&host, &label, &target, &schema, &table).await
            },
            move |_, result, _, cx| {
                let columns = result
                    .map(|details| details.columns.into_iter().map(|column| column.name).collect())
                    .map_err(|error| error.message);
                let _ = weak.update(cx, |view, cx| view.set_columns(columns, cx));
            },
            window,
            cx,
        );
    }

    fn import_event(
        &mut self,
        view: &Entity<ImportDialog>,
        event: &ImportEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            ImportEvent::ChooseFile => {
                let weak = view.downgrade();
                let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
                let picked = cx.prompt_for_paths(options);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(path) = picked_path(picked.await) else { return };
                    let text = read_text(path.clone(), cx).await;
                    let _ = this.update_in(cx, move |workbench, _, cx| match text {
                        Ok(text) => {
                            let path = path.to_string_lossy().into_owned();
                            let _ = weak.update(cx, |view, cx| view.file_read(&path, text, cx));
                        }
                        Err(error) => workbench.notify(error_notice("Cannot read the file", &error), cx),
                    });
                })
                .detach();
            }
            ImportEvent::Apply(edits) => {
                let open = self.database.as_ref().and_then(|slot| slot.extras.dialog.as_ref());
                let Some((target, schema, table)) = (match open {
                    Some(DialogSlot { dialog: ExtraDialog::Import { target, schema, table, .. }, .. }) => {
                        Some((target.clone(), schema.clone(), table.clone()))
                    }
                    _ => None,
                }) else {
                    return;
                };
                self.apply_import(target, schema, table, edits, view, window, cx);
            }
            ImportEvent::Cancelled => self.close_extra_dialog(window, cx),
        }
    }

    /// The imported rows go in, in one transaction; the outcome is notified.
    #[expect(clippy::too_many_arguments, reason = "the table, its rows, the dialog and the window")]
    fn apply_import(
        &mut self,
        target: String,
        schema: String,
        table: String,
        edits: Vec<DbRowEdit>,
        view: &Entity<ImportDialog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = view.downgrade();
        let name = table.clone();
        self.db_call(
            "db_apply_edits",
            move |host, label| async move {
                ide_host::ops::db::apply_edits(&host, &label, &target, &schema, &table, edits).await
            },
            move |workbench, result, _, cx| {
                let (request, ok) = match result {
                    Ok(rows) => {
                        let title = format!("{name}: {} row(s) imported", js_number_string(rows));
                        (NotificationRequest::new(NotificationLevel::Info, title), true)
                    }
                    Err(error) => (error_notice("Nothing was imported", &error), false),
                };
                let mut request = request;
                request.key = Some(IMPORT_KEY.to_owned());
                workbench.notify(request, cx);
                let _ = weak.update(cx, |view, cx| view.finished(ok, cx));
            },
            window,
            cx,
        );
    }

    #[cfg(test)]
    pub(super) fn database_import_dialog(&self) -> Option<Entity<ImportDialog>> {
        match &self.database.as_ref()?.extras.dialog.as_ref()?.dialog {
            ExtraDialog::Import { view, .. } => Some(view.clone()),
            _ => None,
        }
    }

    // ---- Run SQL Script -----------------------------------------------------------------------

    /// Run SQL Script…: every statement of a picked file on `target`, in the Output tab.
    fn run_sql_script_file(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
        let picked = cx.prompt_for_paths(options);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = picked_path(picked.await) else { return };
            let text = read_text(path, cx).await;
            let _ = this.update_in(cx, move |workbench, window, cx| match text {
                Ok(text) => {
                    let commands = workbench.db_step(cx, |store, _| store.run_script(&target, &text).1);
                    workbench.carry_out_db(commands, window, cx);
                }
                Err(error) => workbench.notify(error_notice("Cannot run the script", &error), cx),
            });
        })
        .detach();
    }

    // ---- the table editor's export ------------------------------------------------------------

    /// Export Data › Browse…: a save dialog from the export's file; the chosen path goes back to
    /// the export form of table editor `editor`.
    pub(super) fn browse_table_export(
        &mut self,
        editor: u64,
        path: String,
        extension: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = Path::new(&path);
        let directory = file
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| self.project_root(cx))
            .unwrap_or_default();
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("export.{extension}"));
        let picked = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!("database: cannot choose where to export: {error:#}");
                    return;
                }
            };
            let _ = this.update(cx, move |workbench, cx| {
                let Some(panel) = workbench.database_panel_entity() else { return };
                let form = panel.read(cx).editor(editor, cx).and_then(|view| view.read(cx).export_form().cloned());
                if let Some(form) = form {
                    let path = path.to_string_lossy().into_owned();
                    form.update(cx, |form, cx| form.file_picked(&path, cx));
                }
            });
        })
        .detach();
    }

    // ---- the Query Plan tab -------------------------------------------------------------------

    /// `ResultsUpdate::Plan`: the window's one Query Plan tab shows the plan of `statement`.
    pub(super) fn show_query_plan(
        &mut self,
        id: u64,
        statement: String,
        plan: Vec<PlanNode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self
            .editor()
            .map(|editor| editor.read(cx).path().to_string_lossy().into_owned())
            .and_then(|path| self.database_store().and_then(|store| store.console_source(&path)))
            .and_then(|target| self.database_store().map(|store| store.target_label(&target)))
            .unwrap_or_default();
        let open = self.database.as_ref().and_then(|slot| slot.extras.plan.as_ref()).map(|plan| plan.view.clone());
        let view = match open {
            Some(view) => {
                view.update(cx, |view, cx| view.set_plan(id, &source, &statement, plan, cx));
                view
            }
            None => {
                let view = cx.new(|cx| PlanView::new(id, &source, &statement, plan, cx));
                let events = cx.subscribe_in(&view, window, |workbench, _, event: &PlanEvent, window, cx| {
                    let PlanEvent::Rerun(id) = event;
                    let id = *id;
                    let commands = workbench.db_step(cx, |store, _| store.rerun_result(id));
                    workbench.carry_out_db(commands, window, cx);
                });
                if let Some(extras) = self.database_extras() {
                    extras.plan = Some(PlanSlot { view: view.clone(), _events: events });
                }
                view
            }
        };
        if let Some(panel) = self.database_panel_entity() {
            panel.update(cx, |panel, cx| panel.set_plan(Some(view.into()), cx));
        }
    }

    /// The plan's statement is explained again (`running`), or that stopped without a new plan.
    pub(super) fn query_plan_running(&mut self, running: bool, cx: &mut Context<Self>) {
        if let Some(plan) = self.database.as_ref().and_then(|slot| slot.extras.plan.as_ref()).map(|p| p.view.clone()) {
            plan.update(cx, |plan, cx| plan.set_busy(running, cx));
        }
    }

    /// The store closed the plan's result (older tabs go past their limit).
    pub(super) fn close_query_plan(&mut self, cx: &mut Context<Self>) {
        if let Some(extras) = self.database_extras() {
            extras.plan = None;
        }
        if let Some(panel) = self.database_panel_entity() {
            panel.update(cx, |panel, cx| panel.set_plan(None, cx));
        }
    }

    #[cfg(test)]
    pub(super) fn database_plan(&self) -> Option<Entity<PlanView>> {
        self.database.as_ref()?.extras.plan.as_ref().map(|plan| plan.view.clone())
    }

    fn database_panel_entity(&self) -> Option<Entity<super::database::DatabasePanel>> {
        self.database.as_ref().map(|slot| slot.panel.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        click_on, context_is_on, db_expand, db_query, db_row_key, db_tree, notification_titles, run_action,
        sqlite_source, start, trusted_project, wait_long,
    };
    use super::*;
    use crate::backend::Backend;
    use crate::database::tree_view::{DATABASE_TOOL_WINDOW, TreeCommand};
    use gpui_kit::{TestAppContext, VisualTestContext};

    const REFUSED: &str = "Project is in Restricted Mode. Trust the project to enable execution.";

    fn cell(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    /// Runs `text` on data source `source` through the backend: the rows of its result, or its error.
    fn query_on(cx: &mut VisualTestContext, source: &str, text: &str) -> Result<Vec<Vec<Option<String>>>, String> {
        let to = u32::try_from(text.encode_utf16().count()).expect("a short statement");
        let statement = ide_db::split::SqlStatement { from: 0, to, text: text.to_owned() };
        let source = source.to_owned();
        cx.update(|_, cx| {
            let backend = cx.global::<Backend>();
            let host = backend.host.clone();
            backend.block_on(async move {
                let request = Backend::next_request_id();
                let outcomes =
                    ide_host::ops::db::execute(&host, "w1", request, &source, vec![statement], 0, 1000, None)
                        .await
                        .map_err(|error| error.message)?;
                let outcome = outcomes.into_iter().next().ok_or_else(|| "no outcome".to_owned())?;
                match (outcome.result, outcome.error) {
                    (_, Some(error)) => Err(error),
                    (Some(result), None) => Ok(result.rows),
                    (None, None) => Ok(Vec::new()),
                }
            })
        })
    }

    /// `app` (s1, with `users`) and `mirror` (s2), a second data source on the same file.
    fn two_sources(root: &std::path::Path, cx: &mut VisualTestContext) {
        sqlite_source(root, cx);
        let sources: Vec<ide_db::DataSource> = [("s1", "app"), ("s2", "mirror")]
            .into_iter()
            .map(|(id, name)| ide_db::DataSource {
                id: id.into(),
                name: name.into(),
                driver: DbDriver::Sqlite,
                file: Some(root.join("app.db").into()),
                ..Default::default()
            })
            .collect();
        cx.update(|_, cx| {
            let backend = cx.global::<Backend>();
            let host = backend.host.clone();
            backend.block_on(async move {
                ide_host::ops::db::save_sources(&host, "w1", sources).await.expect("the sources are saved");
            });
        });
    }

    /// The tree with `users` listed, and the key of its row.
    fn users_row(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        run_action(cx, "db.showToolWindow", None);
        let tree = db_tree(workbench, cx);
        for label in ["app", "main", "tables"] {
            db_expand(&tree, label, cx);
        }
        db_row_key(&tree, "users", cx)
    }

    /// Opens `file` of the project and waits for its console toolbar.
    fn open_console_file(
        workbench: &Entity<Workbench>,
        root: &std::path::Path,
        file: &str,
        cx: &mut VisualTestContext,
    ) -> (Entity<Editor>, Entity<ConsoleView>) {
        let path = root.join(file);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        wait_long(cx, "the console", |cx| workbench.read_with(cx, |w, _| w.database_console(&path).is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the editor");
        let console = workbench.read_with(cx, |w, _| w.database_console(&path)).expect("the console");
        (editor, console)
    }

    fn set_text(editor: &Entity<Editor>, text: &str, cx: &mut VisualTestContext) {
        editor.update(cx, |editor, cx| {
            let len = u32::try_from(editor.text().encode_utf16().count()).expect("a short text");
            editor.insert(Some(0..len), text, cx);
            let end = u32::try_from(text.encode_utf16().count()).expect("a short text");
            editor.select(end, end, cx);
        });
        cx.run_until_parked();
    }

    /// A `.sql` file of a project with two data sources is bound in the console's toolbar: its data
    /// source chooser lists both, the pick is kept, and Execute then runs on it, rows shown.
    #[gpui_kit::test]
    async fn the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_and_executes_on_it(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let files = [("query.sql", "select name from users order by id")];
        let (_folder, root, workbench, cx) = trusted_project(cx, &files);
        cx.update(|window, _| window.activate_window());
        two_sources(&root, cx);
        let (editor, console) = open_console_file(&workbench, &root, "query.sql", cx);
        wait_long(cx, "the sources in the chooser", |cx| console.read_with(cx, |c, _| c.bar().targets.len() == 2));
        assert_eq!(console.read_with(cx, |c, _| c.bar().bound.clone()), None, "two sources: none is assumed");
        let enabled = |id: &str, cx: &mut VisualTestContext| {
            console.read_with(cx, |c, _| c.toolbar().iter().any(|b| b.id == id && b.enabled))
        };
        assert!(!enabled("console-execute", cx), "nothing runs before a source is chosen");

        click_on(cx, "console-chooser-DataSource");
        click_on(cx, "console-option-DataSource-0");
        wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.as_deref() == Some("s1")));
        assert!(enabled("console-execute", cx));
        let end = u32::try_from(editor.read_with(cx, |e, _| e.text()).encode_utf16().count()).expect("short");
        editor.update(cx, |editor, cx| editor.select(end, end, cx));
        click_on(cx, "console-execute");
        let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
        wait_long(cx, "the result", |cx| results.read_with(cx, |r, _| r.tabs().len() == 1));
        let rows = results.read_with(cx, |r, _| r.tabs()[0].origin.result.rows.clone());
        assert_eq!(rows, [[cell("ann")], [cell("bob")]]);
        let stored = cx.update(|_, cx| {
            cx.try_global::<crate::platform::storage::UiStorage>()
                .and_then(|s| s.get(crate::database::store::BINDINGS_KEY).map(str::to_owned))
        });
        assert!(stored.is_some_and(|bindings| bindings.contains("query.sql")), "the binding is kept");
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Tx: Manual keeps an insert in the console's open transaction: another connection does not see
    /// it, Roll Back discards it, and Commit makes the next one visible.
    #[gpui_kit::test]
    async fn manual_commit_keeps_changes_pending_until_commit_or_roll_back(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("tx.sql", "")]);
        cx.update(|window, _| window.activate_window());
        two_sources(&root, cx);
        let (editor, console) = open_console_file(&workbench, &root, "tx.sql", cx);
        wait_long(cx, "the sources in the chooser", |cx| console.read_with(cx, |c, _| c.bar().targets.len() == 2));
        click_on(cx, "console-chooser-DataSource");
        click_on(cx, "console-option-DataSource-0");
        wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.is_some()));
        click_on(cx, "console-chooser-Transaction");
        click_on(cx, "console-option-Transaction-1");
        wait_long(cx, "manual commit", |cx| console.read_with(cx, |c, _| c.bar().manual));
        let count = |cx: &mut VisualTestContext| query_on(cx, "s2", "select count(*) from users");

        set_text(&editor, "insert into users (name) values ('cyd')", cx);
        click_on(cx, "console-execute");
        wait_long(cx, "the pending change", |cx| console.read_with(cx, |c, _| c.bar().pending));
        assert_eq!(count(cx), Ok(vec![vec![cell("2")]]), "the other connection sees no uncommitted row");
        click_on(cx, "console-rollback");
        wait_long(cx, "the roll back", |cx| console.read_with(cx, |c, _| !c.bar().pending));
        assert_eq!(query_on(cx, "s1", "select count(*) from users"), Ok(vec![vec![cell("2")]]));

        set_text(&editor, "insert into users (name) values ('dee')", cx);
        click_on(cx, "console-execute");
        wait_long(cx, "the pending change", |cx| console.read_with(cx, |c, _| c.bar().pending));
        click_on(cx, "console-commit");
        wait_long(cx, "the commit", |cx| count(cx) == Ok(vec![vec![cell("3")]]));
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// A failing statement shows its error in the console's banner; the Query Plan tab draws the
    /// plan of Explain Plan, rooted in the statement, Explain Again asks for it once more, and a
    /// later Execute shows the results again.
    #[gpui_kit::test]
    async fn the_console_banner_and_the_query_plan_tab_follow_the_runs(cx: &mut TestAppContext) {
        use super::super::database::Lower;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("plan.sql", "select * from missing")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let (editor, console) = open_console_file(&workbench, &root, "plan.sql", cx);
        wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.as_deref() == Some("s1")));
        click_on(cx, "console-execute");
        let banner = |cx: &mut VisualTestContext| console.read_with(cx, |c, _| c.banner().map(str::to_owned));
        wait_long(cx, "the banner", |cx| banner(cx).is_some_and(|b| b.contains("missing")));
        assert!(console.read_with(cx, |c, _| c.banner().is_some_and(|b| b.starts_with("[app]"))));

        set_text(&editor, "select name from users where id = 1", cx);
        click_on(cx, "console-explain");
        wait_long(cx, "the plan", |cx| workbench.read_with(cx, |w, _| w.database_plan().is_some()));
        let plan = workbench.read_with(cx, |w, _| w.database_plan()).expect("the plan tab");
        let rows = plan.read_with(cx, |p, _| p.rows());
        assert_eq!(rows.first().map(|row| row.operation.as_str()), Some("Select"));
        assert!(rows.len() >= 2, "{rows:?}");
        let panel = workbench.read_with(cx, |w, _| w.database_panel()).expect("the panel");
        assert_eq!(panel.read_with(cx, |p, _| p.lower()), Lower::Plan);
        assert!(console.read_with(cx, |c, _| c.banner().is_none()), "a new run clears the banner");

        click_on(cx, "db-plan-rerun");
        wait_long(cx, "the plan again", |cx| plan.read_with(cx, |p, _| !p.is_busy()));
        assert_eq!(plan.read_with(cx, |p, _| p.statement().to_owned()), "select name from users where id = 1");

        click_on(cx, "console-execute");
        let results = workbench.read_with(cx, |w, _| w.database_results()).expect("the result pane");
        wait_long(cx, "the result", |cx| results.read_with(cx, |r, _| !r.tabs().is_empty()));
        wait_long(cx, "the results shown", |cx| panel.read_with(cx, |p, _| p.lower()) == Lower::Results);
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Browse Query History lists what the data source ran, newest first; Paste puts the chosen
    /// statement into the console and closes the dialog; Delete takes one out of the history.
    #[gpui_kit::test]
    async fn query_history_lists_the_statements_run_and_pastes_one_into_the_console(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("history.sql", "select name from users")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let (editor, console) = open_console_file(&workbench, &root, "history.sql", cx);
        wait_long(cx, "the binding", |cx| console.read_with(cx, |c, _| c.bar().bound.is_some()));
        click_on(cx, "console-execute");
        set_text(&editor, "select id from users", cx);
        click_on(cx, "console-execute");
        let recorded = |cx: &mut VisualTestContext| {
            let history = cx.update(|_, cx| {
                let backend = cx.global::<Backend>();
                let host = backend.host.clone();
                backend.block_on(async move { ide_host::ops::db::history(&host, "w1", "s1").await })
            });
            history.map(|entries| entries.into_iter().map(|entry| entry.text).collect::<Vec<_>>()).unwrap_or_default()
        };
        wait_long(cx, "the history", |cx| recorded(cx).contains(&"select id from users".to_owned()));
        set_text(&editor, "", cx);

        run_action(cx, "db.queryHistory", None);
        let dialog = workbench.read_with(cx, |w, _| w.database_history_dialog()).expect("the dialog");
        wait_long(cx, "the statements", |cx| dialog.read_with(cx, |d, _| d.rows().len() >= 2));
        assert_eq!(dialog.update(cx, |d, cx| d.title(cx)), "app Query History");
        assert_eq!(dialog.read_with(cx, |d, _| d.rows()[0].clone()), "select id from users", "newest first");

        click_on(cx, "db-history-delete");
        wait_long(cx, "the deletion", |cx| dialog.read_with(cx, |d, _| d.rows()[0] == "select name from users"));
        assert!(!dialog.read_with(cx, |d, _| d.rows().contains(&"select id from users".to_owned())));
        click_on(cx, "db-history-paste");
        assert!(workbench.read_with(cx, |w, _| w.database_history_dialog()).is_none(), "Paste closes it");
        wait_long(cx, "the pasted statement", |cx| editor.read_with(cx, |e, _| e.text()) == "select name from users");
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Generate DDL to Clipboard copies the table's CREATE statement, from the action and from
    /// ⌘⌥⇧G in the tree, and not from that key in an editor; Generate DDL to Query Console appends
    /// it to the data source's console; on the data source it covers every table.
    #[gpui_kit::test]
    async fn generate_ddl_copies_a_tables_definition_and_appends_it_to_the_console(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        db_query(cx, "create table orders (id integer primary key)").expect("orders is made");
        let users = users_row(&workbench, cx);
        let copied = |cx: &mut VisualTestContext| {
            cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default().to_lowercase()
        };

        run_action(cx, "db.ddlToClipboard", Some(serde_json::json!(users)));
        wait_long(cx, "the DDL", |cx| copied(cx).contains("create table users"));
        assert!(!copied(cx).contains("orders"));

        let app = db_row_key(&db_tree(&workbench, cx), "app", cx);
        run_action(cx, "db.ddlToClipboard", Some(serde_json::json!(app)));
        wait_long(cx, "the DDL of every table", |cx| copied(cx).contains("create table orders"));
        assert!(copied(cx).contains("create table users"));

        // The shortcut works in the tree only.
        cx.write_to_clipboard(ClipboardItem::new_string("before".to_owned()));
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
        wait_long(cx, "the editor", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-alt-shift-g");
        cx.run_until_parked();
        assert_eq!(copied(cx), "before");
        let tree = db_tree(&workbench, cx);
        tree.update_in(cx, |tree, window, cx| {
            tree.select(&users, cx);
            window.focus(&tree.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        assert!(context_is_on(cx, "databaseViewFocus"));
        cx.simulate_keystrokes("cmd-alt-shift-g");
        wait_long(cx, "the DDL from the key", |cx| copied(cx).contains("create table users"));

        run_action(cx, "db.ddlToConsole", Some(serde_json::json!(users)));
        wait_long(cx, "the console with the DDL", |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|e| {
                    let e = e.read(cx);
                    let sql = e.path().extension().is_some_and(|x| x == "sql");
                    sql && e.text().to_lowercase().contains("create table users")
                })
            })
        });
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// In Restricted Mode Generate DDL reads nothing: the clipboard keeps its text and the notice
    /// gives the backend's reason.
    #[gpui_kit::test]
    async fn generate_ddl_in_restricted_mode_is_refused_with_the_backends_reason(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        run_action(cx, "project.revokeTrust", None);
        wait_long(cx, "Restricted Mode", |cx| workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
        cx.write_to_clipboard(ClipboardItem::new_string("before".to_owned()));
        run_action(cx, "db.ddlToClipboard", Some(serde_json::json!(users)));
        let notice = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| {
                w.notifications().items().iter().find(|n| n.title == "Cannot generate the DDL").map(|n| n.body.clone())
            })
        };
        wait_long(cx, "the refusal", |cx| notice(cx).is_some());
        assert_eq!(notice(cx).flatten().as_deref(), Some(REFUSED));
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("before"));
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Go to Referenced Table selects the table a foreign key points to; Jump to Query Console
    /// opens the data source's console; Separate Procedures and Functions turns the option on.
    #[gpui_kit::test]
    async fn the_trees_navigation_actions_reach_the_referenced_table_and_the_console(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
        sqlite_source(&root, cx);
        let create = "create table orders (id integer primary key, user_id integer references users(id))";
        db_query(cx, create).expect("orders is made");
        let users = users_row(&workbench, cx);
        let tree = db_tree(&workbench, cx);
        for label in ["orders", "foreign keys"] {
            db_expand(&tree, label, cx);
        }
        let find_fk = |cx: &mut VisualTestContext| {
            tree.read_with(cx, |tree, _| {
                tree.rows()
                    .iter()
                    .find(|row| matches!(row.node.kind, NodeKind::ForeignKey { .. }))
                    .map(|row| row.node.key.clone())
            })
        };
        wait_long(cx, "the foreign key", |cx| find_fk(cx).is_some());
        let fk = find_fk(cx).expect("the foreign key row");
        run_action(cx, "db.goToReferencedTable", Some(serde_json::json!(fk)));
        wait_long(cx, "the referenced table", |cx| tree.read_with(cx, |t, _| t.selected() == Some(users.as_str())));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(DATABASE_TOOL_WINDOW)));

        run_action(cx, "db.jumpToConsole", Some(serde_json::json!(users)));
        wait_long(cx, "the console", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path().ends_with("console.sql")))
        });

        assert!(!tree.read_with(cx, |t, _| t.separate()));
        run_action(cx, "db.separateProceduresAndFunctions", None);
        assert!(tree.read_with(cx, |t, _| t.separate()));
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Modify Table adds a column to the SQLite file and the refreshed tree lists it; ⌘F6 opens the
    /// dialog in the tree only. New › Column adds one ready to name; New › Table creates a table.
    #[gpui_kit::test]
    async fn modify_table_new_column_and_new_table_change_the_file_and_the_tree(cx: &mut TestAppContext) {
        use crate::database::modify_table_dialog::ColumnField;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        let tree = db_tree(&workbench, cx);
        let dialog = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.database_modify_dialog());

        // ⌘F6 in an editor is not Modify Table.
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
        wait_long(cx, "the editor", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-f6");
        assert!(dialog(cx).is_none());
        tree.update_in(cx, |tree, window, cx| {
            tree.select(&users, cx);
            window.focus(&tree.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-f6");
        let modify = dialog(cx).expect("⌘F6 in the tree opens Modify Table");
        assert_eq!(modify.read_with(cx, |d, _| d.title()), "Modify Table users");
        wait_long(cx, "the columns", |cx| modify.read_with(cx, |d, _| d.rows().len() == 2));
        click_on(cx, "db-column-add");
        let name = modify.read_with(cx, |d, _| d.column_field(ColumnField::Name).clone());
        cx.update(|window, cx| window.focus(&name.focus_handle(cx), cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("email");
        let sql = modify.read_with(cx, |d, cx| d.statements(cx));
        assert_eq!(sql, ["alter table users add column email INTEGER;".to_owned()]);
        click_on(cx, "db-modify-ok");
        wait_long(cx, "the dialog to close", |cx| dialog(cx).is_none());
        assert_eq!(db_query(cx, "select email from users where id = 1"), Ok(vec![vec![None]]));
        db_expand(&tree, "users", cx);
        db_expand(&tree, "columns", cx);
        db_row_key(&tree, "email", cx);

        run_action(cx, "db.newColumn", Some(serde_json::json!(users)));
        let new_column = dialog(cx).expect("New › Column");
        wait_long(cx, "the columns", |cx| new_column.read_with(cx, |d, _| d.rows().len() == 4));
        assert!(new_column.read_with(cx, |d, _| d.rows()[3].selected));
        click_on(cx, "db-modify-ok");
        wait_long(cx, "the new column", |cx| db_query(cx, "select column_4 from users").is_ok());

        let schema = db_row_key(&tree, "main", cx);
        run_action(cx, "db.newTable", Some(serde_json::json!(schema)));
        let create = dialog(cx).expect("New › Table");
        assert_eq!(create.read_with(cx, |d, _| d.title()), "Create Table");
        let table = create.read_with(cx, |d, _| d.table_field().clone());
        cx.update(|window, cx| window.focus(&table.focus_handle(cx), cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("books");
        click_on(cx, "db-modify-ok");
        wait_long(cx, "the new table", |cx| db_query(cx, "select id from books").is_ok());
        db_row_key(&tree, "books", cx);
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Truncate asks with the statement it runs; OK empties the table.
    #[gpui_kit::test]
    async fn truncate_asks_and_deletes_every_row_of_the_table(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        run_action(cx, "db.truncate", Some(serde_json::json!(users)));
        let dialog = workbench.read_with(cx, |w, _| w.database_truncate_dialog()).expect("the question");
        assert_eq!(dialog.read_with(cx, |d, _| d.sql()), "delete from users;");
        click_on(cx, "db-truncate-ok");
        wait_long(cx, "the rows deleted", |cx| db_query(cx, "select count(*) from users") == Ok(vec![vec![cell("0")]]));
        wait_long(cx, "the dialog to close", |cx| {
            workbench.read_with(cx, |w, _| w.database_truncate_dialog().is_none())
        });
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Import Data from File reads a picked CSV file into the table in one transaction and tells
    /// how many rows went in.
    #[gpui_kit::test]
    async fn import_data_inserts_the_rows_of_a_csv_file_into_the_table(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("people.csv", "name\ncyd\ndee\n")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        run_action(cx, "db.importData", Some(serde_json::json!(users)));
        let dialog = workbench.read_with(cx, |w, _| w.database_import_dialog()).expect("the dialog");
        assert_eq!(dialog.read_with(cx, |d, _| d.title()), "Import Data into main.users");
        click_on(cx, "db-import-choose");
        wait_long(cx, "the file question", |cx| cx.did_prompt_for_paths());
        let file = root.join("people.csv");
        cx.simulate_path_prompt_response(move |_| Some(vec![file]));
        wait_long(cx, "the file read", |cx| dialog.read_with(cx, |d, _| d.can_import()));
        assert_eq!(dialog.read_with(cx, |d, _| d.plan().1), ["name"]);
        click_on(cx, "db-import-ok");
        wait_long(cx, "the import", |cx| db_query(cx, "select count(*) from users") == Ok(vec![vec![cell("4")]]));
        let imported = "users: 2 row(s) imported".to_owned();
        wait_long(cx, "the notice", |cx| notification_titles(&workbench, cx).contains(&imported));
        assert!(workbench.read_with(cx, |w, _| w.database_import_dialog()).is_none(), "the dialog closed");
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Run SQL Script runs every statement of a picked file on the data source; SQL Scripts › Select
    /// all rows opens the console with the template.
    #[gpui_kit::test]
    async fn run_sql_script_runs_a_picked_file_and_a_template_opens_in_the_console(cx: &mut TestAppContext) {
        let _home = start(cx);
        let script = "create table t1 (x integer);\ninsert into t1 values (1);\n";
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("setup.sql", script)]);
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        let app = db_row_key(&db_tree(&workbench, cx), "app", cx);
        run_action(cx, "db.runSqlScript", Some(serde_json::json!(app)));
        wait_long(cx, "the file question", |cx| cx.did_prompt_for_paths());
        let file = root.join("setup.sql");
        cx.simulate_path_prompt_response(move |_| Some(vec![file]));
        wait_long(cx, "the script", |cx| db_query(cx, "select x from t1") == Ok(vec![vec![cell("1")]]));
        wait_long(cx, "the notice", |cx| {
            notification_titles(&workbench, cx).contains(&"Script finished: 2 statements".to_owned())
        });

        let tree = db_tree(&workbench, cx);
        let kind = crate::database::objects::ScriptKind::SelectAll;
        let command = TreeCommand { action: TreeAction::SqlScript(kind), node: Some(users) };
        tree.update(cx, |tree, cx| tree.perform(command, cx));
        wait_long(cx, "the template", |cx| {
            workbench
                .read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text().contains("select *\nfrom users;")))
        });
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// Go to Column lists the table editor's columns while it has the keyboard, from the action and
    /// ⌘F12; ⌘F12 in the tree opens nothing in the table editor.
    #[gpui_kit::test]
    async fn go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
        cx.update(|window, _| window.activate_window());
        sqlite_source(&root, cx);
        let users = users_row(&workbench, cx);
        run_action(cx, "db.editData", Some(serde_json::json!(users)));
        let editor = workbench.read_with(cx, |w, cx| w.database_table_editor(cx)).expect("the table editor");
        wait_long(cx, "the rows", |cx| editor.read_with(cx, |e, _| e.state().page.is_some()));
        let tree = db_tree(&workbench, cx);
        cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-f12");
        assert!(editor.read_with(cx, |e, _| e.columns_query().is_none()), "⌘F12 in the tree is not Go to Column");

        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(context_is_on(cx, "dbTableEditorFocus"));
        cx.simulate_keystrokes("cmd-f12");
        assert!(editor.read_with(cx, |e, _| e.columns_query().is_some()), "⌘F12 in the table editor");
        assert_eq!(editor.read_with(cx, |e, cx| e.shown_columns(cx)), [0, 1]);
        editor.update_in(cx, |e, window, cx| e.pick_column(Some(1), window, cx));
        assert_eq!(editor.read_with(cx, |e, _| e.revealed_column()), Some(1));

        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        run_action(cx, "db.goToColumn", None);
        assert!(editor.read_with(cx, |e, _| e.columns_query().is_some()));
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }
}
