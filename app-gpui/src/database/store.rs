//! The Database tools' connection store: data sources and their storage, connection status with
//! the idle auto-disconnect, the introspection cache the tree and the consoles read, query
//! consoles (file → connection target, active schema, running statements, Output log, result
//! tabs, paging and counting) and the query history.
//!
//! The store is a step machine without I/O. Every entry point returns [`Command`]s: backend
//! requests ([`Command::Backend`]) carry a [`Ticket`] the caller answers through
//! [`DbStore::answer`]; questions for the user ([`Command::Ask`]) are answered through
//! [`DbStore::prompt_answered`]; the rest tell the owner what to show (tree, results, files,
//! notifications) or to persist. A newer request for the same thing (the schemas of a target, the
//! data sources, a console run…) supersedes the older one, whose answer is then dropped, as are
//! answers for requests a disconnect or a project change forgot.
//!
//! A request that needs a connection asks for what a failed connection needs (database password,
//! SSH secret, trust of an unknown host key) and runs again once it is given.

use super::model::{
    ConnectRemedy, active_schema, connect_remedy, database_key, effective_ssh, idle_targets, js_number,
    js_number_string, new_source_id, source_label, split_target, target_of,
};
use super::nodes::table_folder;
use super::result_session::{OutputLine, SubmitRequest};
use super::result_view::page_fetch;
use super::results_view::ResultTab;
use super::sql::format::formatted;
use super::sql::params::{SqlParam, bind_params, find_params};
use super::sql::read_only::is_read_only_query;
use super::tree::folder_key;
use super::tree_view::{Loaded, TreeLoad, lists_databases};
use crate::platform::i18n::I18n;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::remote_settings::SshConfiguration;
use ide_db::split::{SqlStatement, statement_at, statements_in};
use ide_db::{
    CompletionModel, DataSource, Databases, DbDriver, DbObjectKind, DbRowEdit, DbTableKind, DbTxMode, HistoryEntry,
    PlanNode, QueryResult, SchemaObjects, SortKey, TableDetails, TableInfo, TablePage,
};
use ide_host::ops::db::StatementOutcome;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

/// Storage key of the console bindings (console path → connection target).
pub const BINDINGS_KEY: &str = "openidea.database.consoles";
/// Storage key of the consoles' active schemas (console path → schema).
pub const CONSOLE_SCHEMAS_KEY: &str = "openidea.database.consoleSchemas";
/// Result tabs kept; older ones close.
pub const MAX_TABS: usize = 12;
/// Output lines kept.
pub const MAX_OUTPUT: usize = 2000;
/// The console toolbar's row limits.
pub const ROW_LIMITS: [u64; 5] = [100, 500, 1000, 5000, 50000];
/// Title of the window's one Query Plan tab.
pub const PLAN_TITLE: &str = "Query Plan";
const DEFAULT_MAX_ROWS: u64 = 500;

/// Statements that only read (they leave a manual transaction clean).
static READS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(select|show|pragma|explain|describe|desc|values|table)\b").expect("the pattern is valid")
});
/// Statements that change the schema: the tree and completion read it again.
static SCHEMA_CHANGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(create|drop|alter|rename)\b").expect("the pattern is valid"));
/// Queries that can be a subquery of `SELECT COUNT(*)`.
static COUNTABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(\(|select\b|with\b|values\b|table\b)").expect("the pattern is valid"));

/// Identifies a request the caller answers.
pub type Ticket = u64;

/// A password or SSH secret on its way to the backend. Its `Debug` output never shows it, so the
/// requests, answers and events that carry one can be printed.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretText(String);

impl SecretText {
    /// The secret itself, for the one call that stores or uses it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for SecretText {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl From<&str> for SecretText {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

/// A backend request; each maps onto an operation of `ide_host::ops::db`.
#[derive(Clone, Debug, PartialEq)]
pub enum DbOp {
    /// `sources` and `connected`, answered together with [`Answer::Sources`].
    LoadSources,
    SaveSources(Vec<DataSource>),
    SetPassword {
        id: String,
        password: Option<SecretText>,
        save: bool,
    },
    SetSshSecret {
        id: String,
        secret: Option<SecretText>,
        save: bool,
    },
    TrustSshHost {
        id: String,
    },
    Connect {
        target: String,
    },
    Disconnect {
        target: String,
    },
    SetManualCommit {
        id: String,
        manual: bool,
    },
    FinishTransaction {
        id: String,
        commit: bool,
    },
    Databases {
        id: String,
    },
    Schemas {
        target: String,
    },
    /// `tables`, then `schema_objects` (its error becomes `None`), answered with [`Answer::Tables`].
    Tables {
        target: String,
        schema: String,
    },
    Details {
        target: String,
        schema: String,
        table: String,
    },
    Ddl {
        target: String,
        schema: String,
        table: String,
    },
    ObjectDdl {
        target: String,
        schema: String,
        kind: DbObjectKind,
        name: String,
    },
    Completion {
        target: String,
    },
    /// Cancellable: the owner allocates a `RequestId` for the ticket ([`Command::Cancel`]).
    Execute {
        target: String,
        statements: Vec<SqlStatement>,
        skip: u64,
        max_rows: u64,
        schema: Option<String>,
    },
    /// Cancellable like [`DbOp::Execute`].
    Explain {
        target: String,
        statement: String,
        schema: Option<String>,
    },
    History {
        target: String,
    },
    RemoveHistory {
        target: String,
        texts: Option<Vec<String>>,
    },
    ConsolePath {
        target: String,
    },
    /// Writes a SQL file next to a console (Go to DDL); answered with [`Answer::Done`].
    WriteFile {
        path: String,
        text: String,
    },
    /// Cancellable like [`DbOp::Execute`]: a page of a table editor.
    TablePage {
        target: String,
        schema: String,
        table: String,
        filter: Option<String>,
        sort: Vec<SortKey>,
        offset: u64,
        limit: u64,
    },
    /// Applies edits in one transaction; answered with [`Answer::Count`].
    ApplyEdits {
        target: String,
        schema: String,
        table: String,
        edits: Vec<DbRowEdit>,
    },
}

impl DbOp {
    /// The target a request connects to: its failure may ask for a password and run again.
    fn connection_target(&self) -> Option<&str> {
        match self {
            DbOp::Connect { target }
            | DbOp::Schemas { target }
            | DbOp::Tables { target, .. }
            | DbOp::Details { target, .. }
            | DbOp::Ddl { target, .. }
            | DbOp::ObjectDdl { target, .. }
            | DbOp::Execute { target, .. }
            | DbOp::Explain { target, .. }
            | DbOp::TablePage { target, .. } => Some(target.as_str()),
            DbOp::Databases { id } => Some(id.as_str()),
            _ => None,
        }
    }
}

/// The backend's answer to a [`Command::Backend`].
pub enum Answer {
    Failed(IdeError),
    Done,
    Sources {
        sources: Vec<DataSource>,
        connected: Vec<String>,
    },
    Databases(Databases),
    Schemas(Vec<String>),
    Tables {
        tables: Vec<TableInfo>,
        objects: Option<SchemaObjects>,
    },
    Details(TableDetails),
    /// DDL, a console path.
    Text(String),
    Completion(CompletionModel),
    Outcomes(Vec<StatementOutcome>),
    Plan(Vec<PlanNode>),
    History(Vec<HistoryEntry>),
    TablePage(Box<TablePage>),
    /// Rows changed.
    Count(f64),
}

impl Answer {
    /// `ok(value)`, or [`Answer::Failed`].
    pub fn of<T>(result: Result<T, IdeError>, ok: impl FnOnce(T) -> Answer) -> Answer {
        match result {
            Ok(value) => ok(value),
            Err(error) => Answer::Failed(error),
        }
    }

    fn cancelled(&self) -> bool {
        matches!(self, Answer::Failed(e) if e.kind == ErrorKind::Cancelled)
    }

    /// The error text of a failed answer, or of an answer of the wrong kind.
    fn message(self) -> String {
        match self {
            Answer::Failed(e) => e.message,
            _ => "unexpected answer".to_owned(),
        }
    }
}

/// What a store step needs from its surroundings.
pub struct Env<'a> {
    /// Milliseconds since the Unix epoch.
    pub now_ms: u64,
    pub i18n: &'a I18n,
}

/// A question for the user; answer with [`DbStore::prompt_answered`].
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// The database password (ask-on-connect, or a rejected one): [`PromptAnswer::Secret`].
    Password { source: Box<DataSource> },
    /// The SSH password or key passphrase; `message` is the backend's reason. The source carries
    /// the SSH settings in effect.
    SshSecret { source: Box<DataSource>, message: String },
    /// An unknown SSH host key: trust it and add it to `~/.ssh/known_hosts`? [`PromptAnswer::Confirm`].
    TrustHost { message: String },
    /// Values of query parameters, `values` the last ones entered: [`PromptAnswer::Params`].
    Params { names: Vec<String>, values: Vec<String> },
    /// Remove data source `name`? [`PromptAnswer::Confirm`].
    RemoveSource { id: String, name: String },
    /// Leaving manual commit with changes pending: [`PromptAnswer::Commit`].
    EndTransaction { id: String, name: String },
}

/// The user's answer to a [`Prompt`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptAnswer {
    Cancel,
    Secret {
        value: SecretText,
        save: bool,
    },
    Confirm,
    Params(HashMap<String, String>),
    /// Commit (`true`) or roll back.
    Commit(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

/// A notification for the workbench (English text; the workbench translates titles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub level: Level,
    pub title: String,
    pub body: Option<String>,
    /// Replaces an earlier notification with the same key.
    pub key: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Info,
    Statement,
    Result,
    Error,
}

/// A line of the consoles' Output log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputEntry {
    pub id: u64,
    pub time_ms: u64,
    pub kind: OutputKind,
    pub text: String,
    /// Before a statement: the schema it ran in (`main>`).
    pub prompt: Option<String>,
}

impl OutputEntry {
    /// The line the result pane shows (an info line shows as a result line, a statement after
    /// its prompt).
    pub fn line(&self) -> OutputLine {
        match self.kind {
            OutputKind::Info | OutputKind::Result => OutputLine::Result(self.text.clone()),
            OutputKind::Error => OutputLine::Error(self.text.clone()),
            OutputKind::Statement => OutputLine::Statement(match &self.prompt {
                Some(prompt) => format!("{prompt} {}", self.text),
                None => self.text.clone(),
            }),
        }
    }
}

/// What the Database tree shows changed; each maps onto a `DatabaseTree` method.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeUpdate {
    /// `set_sources`.
    Sources(Vec<DataSource>),
    /// `set_connected`.
    Connected(BTreeSet<String>),
    /// `databases_loaded`.
    Databases { id: String, result: Result<Databases, String> },
    /// `schemas_loaded`.
    Schemas { target: String, result: Result<Vec<String>, String> },
    /// `tables_loaded`.
    Tables { target: String, schema: String, result: Result<Vec<TableInfo>, String>, objects: Option<SchemaObjects> },
    /// `details_loaded`.
    Details { target: String, schema: String, table: String, result: Result<TableDetails, String> },
    /// Drop the loaded levels of a target (a data source id: of all its databases).
    Forget(String),
    /// Expand these nodes, then select and scroll to `key`.
    Reveal { expand: Vec<String>, key: String },
}

/// What the result pane shows changed; each maps onto a `ResultsView` method.
#[derive(Clone, Debug)]
pub enum ResultsUpdate {
    /// `show_result`.
    Show(Box<ResultTab>),
    /// `update_result`.
    Update { id: u64, result: QueryResult, page: u64, page_size: u64 },
    /// `set_running`.
    Running { id: u64, running: bool },
    /// `set_total`.
    Total { id: u64, total: Option<u64> },
    /// `log`.
    Log(Vec<OutputLine>),
    /// `select_tab` (`None`: the Output tab).
    Select(Option<u64>),
    /// `close_tab`.
    Close(u64),
    /// The Query Plan tab, in place of the one with `id` when it exists.
    Plan { id: u64, title: String, statement: String, plan: Vec<PlanNode> },
    /// `submit_finished`: the answer to [`DbStore::submit_result_edits`].
    Submitted { id: u64, result: Result<f64, String> },
}

/// The outcome of one statement, for the console's gutter mark and timing.
#[derive(Clone, Debug, PartialEq)]
pub struct RunMark {
    /// UTF-16 offsets of the statement in the console text.
    pub from: u32,
    pub to: u32,
    pub error: Option<String>,
    pub elapsed_ms: Option<f64>,
}

/// What the owner does next.
#[derive(Clone, Debug)]
pub enum Command {
    /// Run `op`; answer with [`DbStore::answer`].
    Backend {
        ticket: Ticket,
        op: DbOp,
    },
    /// Cancel the running request of this ticket (`cancel_request` of its `RequestId`).
    Cancel(Ticket),
    /// Ask the user; answer with [`DbStore::prompt_answered`].
    Ask {
        ticket: Ticket,
        prompt: Prompt,
    },
    Tree(TreeUpdate),
    Results(ResultsUpdate),
    Notify(Notice),
    /// Write `value` under `key` in the UI storage.
    Persist {
        key: &'static str,
        value: String,
    },
    /// Open the file in an editor tab, focused.
    OpenFile {
        path: String,
    },
    /// Append `text` to the open console `path` and select it ([`console_append`]).
    AppendToConsole {
        path: String,
        text: String,
    },
    /// Replace the selection of console `path` with `text`; when it is not open, call
    /// [`DbStore::open_console_with`] with `target` instead.
    PasteIntoConsole {
        target: String,
        path: String,
        text: String,
    },
    /// Mark the statements of console `path` that ran; `banner` is the first error.
    MarkOutcomes {
        path: String,
        marks: Vec<RunMark>,
        banner: Option<String>,
    },
    /// Show the result pane (the `dbResults` service view), unfocused.
    ShowResults,
    /// Show the Database tool window, focused.
    ShowToolWindow,
    /// Show the Query History dialog over [`DbStore::history`].
    ShowHistory,
    /// The data source dialog's save went through.
    SourceSaved {
        id: String,
    },
    /// The data source dialog's save failed; the dialog shows the message.
    SourceSaveFailed {
        id: String,
        message: String,
    },
    /// Statements of [`DbStore::run_sql`] finished: all succeeded or not.
    SqlDone {
        job: u64,
        ok: bool,
    },
    /// The answer to [`DbStore::load_table_page`] for table editor `editor`.
    TablePage {
        editor: u64,
        request: u64,
        result: Result<Box<TablePage>, String>,
    },
    /// [`DbStore::apply_table_edits`] went through or not (a notification says which).
    TableEdited {
        editor: u64,
        ok: bool,
    },
}

/// A password or SSH secret to store with a data source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretChange {
    Keep,
    Forget,
    Set(SecretText),
}

impl SecretChange {
    fn value(&self) -> Option<Option<SecretText>> {
        match self {
            SecretChange::Keep => None,
            SecretChange::Forget => Some(None),
            SecretChange::Set(v) => Some(Some(v.clone())),
        }
    }
}

/// A page a table editor asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablePageRequest {
    /// The editor's own counter: an answer for an older request is dropped by the editor too.
    pub request: u64,
    pub target: String,
    pub schema: String,
    pub table: String,
    /// The WHERE clause (empty: every row).
    pub filter: Option<String>,
    pub sort: Vec<SortKey>,
    pub offset: u64,
    pub limit: u64,
}

/// A connection target the console's chooser offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetOption {
    pub value: String,
    pub label: String,
    /// The vendor, `(PostgreSQL)`.
    pub detail: String,
    pub icon: &'static str,
}

/// What a console's toolbar shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConsoleBar {
    /// The connection target the console runs on.
    pub bound: Option<String>,
    pub has_sources: bool,
    pub running: bool,
    pub max_rows: u64,
    /// Tx: Auto / Manual is offered (not ClickHouse).
    pub supports_transactions: bool,
    pub manual: bool,
    /// Manual commit with statements not yet committed.
    pub pending: bool,
    /// The schema chooser is offered.
    pub switchable: bool,
    /// The schema chooser lists databases (SQL Server).
    pub sql_server: bool,
    /// The active schema ("" when unknown).
    pub schema: String,
    pub schema_options: Vec<String>,
    pub targets: Vec<TargetOption>,
}

/// The Query History dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryState {
    pub target: String,
    /// The console Paste goes to.
    pub path: Option<String>,
    /// `None` while loading.
    pub entries: Option<Vec<HistoryEntry>>,
    pub query: String,
    pub active: usize,
}

impl HistoryState {
    /// The entries matching the search, newest first.
    pub fn shown(&self) -> Vec<&HistoryEntry> {
        let q = self.query.trim().to_lowercase();
        self.entries.iter().flatten().filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q)).collect()
    }

    /// The entry the preview and Paste use.
    pub fn current(&self) -> Option<&HistoryEntry> {
        let shown = self.shown();
        let last = shown.len().checked_sub(1)?;
        shown.get(self.active.min(last)).copied()
    }
}

/// Where an append to a console lands: the text inserted at the end, and the selection over the
/// appended text (UTF-16 offsets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleAppend {
    pub insert: String,
    pub anchor: u32,
    pub head: u32,
}

fn len16(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// Appends `text` to console text `doc` after a blank line, selecting the appended text
/// without its trailing whitespace.
pub fn console_append(doc: &str, text: &str) -> ConsoleAppend {
    let sep = if doc.is_empty() || doc.ends_with("\n\n") {
        ""
    } else if doc.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let from = len16(doc) + len16(sep);
    ConsoleAppend { insert: format!("{sep}{text}"), anchor: from, head: from + len16(text.trim_end()) }
}

/// DDL laid out for reading: SQLite returns the statements as they were typed.
pub fn readable_ddl(text: &str, driver: DbDriver) -> String {
    if driver == DbDriver::Sqlite { formatted(text, driver) } else { text.to_owned() }
}

/// `200 ms` / `1.5 s`.
fn elapsed(ms: f64) -> String {
    if ms < 1000.0 { format!("{} ms", ms.round()) } else { format!("{:.1} s", ms / 1000.0) }
}

/// The summary line of an executed statement.
pub fn outcome_summary(outcome: &StatementOutcome) -> String {
    let Some(r) = &outcome.result else { return outcome.error.clone().unwrap_or_default() };
    let (n, one) = if r.columns.is_empty() {
        let a = r.affected.unwrap_or(0.0);
        (js_number_string(a), a == 1.0)
    } else {
        (r.rows.len().to_string(), r.rows.len() == 1)
    };
    let rows = format!("{n} row{}", if one { "" } else { "s" });
    if r.columns.is_empty() {
        return format!("{rows} affected in {}", elapsed(r.elapsed_ms));
    }
    let total = r.elapsed_ms;
    let execution = total.min(r.execution_ms);
    let split = format!(" (execution: {}, fetching: {})", elapsed(execution), elapsed(total - execution));
    let limit = if r.has_more { " (limit reached)" } else { "" };
    format!("{rows} retrieved starting from 1 in {}{split}{limit}", elapsed(total))
}

/// `SELECT COUNT(*)` over a query, for queries that can be a subquery. The `)` goes on its own
/// line: a trailing line comment would swallow it.
pub fn count_query(text: &str) -> Option<String> {
    COUNTABLE.is_match(text).then(|| format!("select count(*) from (\n{}\n) q", text.trim()))
}

/// A file name part: runs of characters other than ASCII letters, digits, `_`, `.` and `-`
/// become one `_`.
fn safe_name(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            out.push(c);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

/// `path` with its file name replaced.
fn sibling(path: &str, name: &str) -> String {
    let dir = path.rfind(['/', '\\']).map_or("", |i| &path[..=i]);
    format!("{dir}{name}")
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Sources,
    Databases(String),
    Schemas(String),
    Tables(String, String),
    Details(String, String, String),
    Completion(String),
    History(String),
    TablePage(u64),
}

impl Slot {
    /// The target of a loader slot (dropped by forget).
    fn target(&self) -> Option<&str> {
        match self {
            Slot::Databases(t)
            | Slot::Schemas(t)
            | Slot::Tables(t, _)
            | Slot::Details(t, _, _)
            | Slot::Completion(t) => Some(t.as_str()),
            Slot::Sources | Slot::History(_) | Slot::TablePage(_) => None,
        }
    }
}

/// A multi-stage job a loader reports back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    Refresh(u64),
    Reveal(u64),
}

/// How a data source save reports its outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveReport {
    Dialog,
    Duplicate,
    Rename,
    Remove,
}

#[derive(Clone, Debug)]
struct ConsoleRun {
    path: String,
    target: String,
    statements: Vec<SqlStatement>,
}

#[derive(Clone, Debug)]
enum RunKind {
    Console { path: String, page_size: u64 },
    Page { tab: u64, page: u64, page_size: u64, rerun: bool },
    Count { tab: u64, last: bool },
    Sql { job: u64, script: Option<usize> },
}

#[derive(Clone, Debug)]
enum Then {
    Sources,
    Save {
        id: String,
        rest: Vec<DbOp>,
        report: SaveReport,
        forget: bool,
    },
    Connect {
        load: bool,
    },
    Disconnect {
        group: u64,
    },
    Load {
        after: Option<After>,
    },
    /// The model for completion; a console run waits for its default schema.
    Completion {
        run: Option<ConsoleRun>,
    },
    Run {
        kind: RunKind,
        target: String,
        schema: String,
        was_connected: bool,
    },
    Explain {
        path: Option<String>,
        tab: Option<u64>,
        target: String,
        statement: SqlStatement,
        schema: String,
    },
    /// A password / secret / host key went in: run the parked request again.
    Unlock {
        ticket: Ticket,
        pending: Box<Pending>,
    },
    ManualCommit {
        id: String,
        manual: bool,
    },
    FinishTx {
        id: String,
        commit: bool,
        then_auto: bool,
    },
    History,
    RemoveHistory {
        texts: Option<Vec<String>>,
    },
    Ddl {
        target: String,
        schema: String,
        name: String,
        routine: bool,
    },
    DdlPath {
        target: String,
        name: String,
        file: String,
        text: String,
        routine: bool,
    },
    Open {
        target: String,
        path: String,
        name: String,
        routine: bool,
    },
    ConsolePath {
        target: String,
        append: Option<String>,
    },
    TablePage {
        editor: u64,
        request: u64,
    },
    TableEdits {
        editor: u64,
        table: String,
    },
    ResultEdits {
        tab: u64,
    },
}

#[derive(Clone, Debug)]
struct Pending {
    op: DbOp,
    slot: Option<Slot>,
    then: Then,
}

enum Parked {
    Retry { ticket: Ticket, pending: Box<Pending>, error: IdeError, remedy: ConnectRemedy, id: String },
    Params { run: ConsoleRun, found: Vec<Vec<SqlParam>>, explain: bool },
    Remove { id: String },
    EndTx { id: String },
}

/// A result tab as the store runs it again.
#[derive(Clone, Debug, PartialEq)]
struct ResultMeta {
    id: u64,
    path: String,
    target: String,
    schema: String,
    statement: SqlStatement,
    plan: bool,
    busy: bool,
    page: u64,
    page_size: u64,
    total: Option<u64>,
}

struct RefreshPlan {
    schemas: Vec<String>,
    tables: Vec<(String, String)>,
    details: Vec<(String, String, String)>,
    stage: u8,
    outstanding: usize,
}

struct RevealPlan {
    target: String,
    schema: String,
    table: String,
    kind: DbTableKind,
    column: Option<String>,
    stage: u8,
    outstanding: usize,
}

struct DisconnectGroup {
    id: String,
    targets: Vec<String>,
    remaining: usize,
    idle: bool,
    remove: bool,
}

/// The Database tools' state; see the module documentation.
pub struct DbStore {
    sources: Vec<DataSource>,
    loaded: bool,
    connected: BTreeSet<String>,
    databases: BTreeMap<String, Loaded<Databases>>,
    schemas: BTreeMap<String, Loaded<Vec<String>>>,
    tables: BTreeMap<(String, String), Loaded<Vec<TableInfo>>>,
    objects: BTreeMap<(String, String), SchemaObjects>,
    details: BTreeMap<(String, String, String), Loaded<TableDetails>>,
    completion: BTreeMap<String, CompletionModel>,
    consoles: BTreeMap<String, String>,
    console_schemas: BTreeMap<String, String>,
    /// Console path → the ticket of its run.
    running: BTreeMap<String, Ticket>,
    max_rows: u64,
    results: Vec<ResultMeta>,
    output: Vec<OutputEntry>,
    param_values: HashMap<String, String>,
    manual_commit: BTreeSet<String>,
    pending_tx: BTreeSet<String>,
    last_used: HashMap<String, u64>,
    ssh_configs: Vec<SshConfiguration>,
    history: Option<HistoryState>,
    pending: HashMap<Ticket, Pending>,
    slots: HashMap<Slot, Ticket>,
    parked: HashMap<Ticket, Parked>,
    refreshes: HashMap<u64, RefreshPlan>,
    reveals: HashMap<u64, RevealPlan>,
    disconnects: HashMap<u64, DisconnectGroup>,
    next_ticket: Ticket,
    next_tab: u64,
    next_line: u64,
    next_job: u64,
}

fn read_map(raw: Option<&str>) -> BTreeMap<String, String> {
    raw.and_then(|r| serde_json::from_str(r).ok()).unwrap_or_default()
}

fn write_map(map: &BTreeMap<String, String>) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".to_owned())
}

fn loaded_ok<T>(l: Option<&Loaded<T>>) -> Option<&T> {
    match l {
        Some(Loaded::Ok(v)) => Some(v),
        _ => None,
    }
}

fn loaded_of<T>(result: Result<T, String>) -> Loaded<T> {
    match result {
        Ok(v) => Loaded::Ok(v),
        Err(m) => Loaded::Error(m),
    }
}

impl DbStore {
    /// A store starting from the stored console bindings and active schemas ([`BINDINGS_KEY`],
    /// [`CONSOLE_SCHEMAS_KEY`]); malformed values start empty.
    pub fn new(bindings: Option<&str>, console_schemas: Option<&str>) -> Self {
        Self {
            sources: Vec::new(),
            loaded: false,
            connected: BTreeSet::new(),
            databases: BTreeMap::new(),
            schemas: BTreeMap::new(),
            tables: BTreeMap::new(),
            objects: BTreeMap::new(),
            details: BTreeMap::new(),
            completion: BTreeMap::new(),
            consoles: read_map(bindings),
            console_schemas: read_map(console_schemas),
            running: BTreeMap::new(),
            max_rows: DEFAULT_MAX_ROWS,
            results: Vec::new(),
            output: Vec::new(),
            param_values: HashMap::new(),
            manual_commit: BTreeSet::new(),
            pending_tx: BTreeSet::new(),
            last_used: HashMap::new(),
            ssh_configs: Vec::new(),
            history: None,
            pending: HashMap::new(),
            slots: HashMap::new(),
            parked: HashMap::new(),
            refreshes: HashMap::new(),
            reveals: HashMap::new(),
            disconnects: HashMap::new(),
            next_ticket: 1,
            next_tab: 1,
            next_line: 1,
            next_job: 1,
        }
    }

    // ---- reading -------------------------------------------------------------------------------

    pub fn sources(&self) -> &[DataSource] {
        &self.sources
    }

    /// The sources have been read once.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn connected(&self) -> &BTreeSet<String> {
        &self.connected
    }

    pub fn databases(&self, id: &str) -> Option<&Loaded<Databases>> {
        self.databases.get(id)
    }

    pub fn schemas(&self, target: &str) -> Option<&Loaded<Vec<String>>> {
        self.schemas.get(target)
    }

    pub fn tables(&self, target: &str, schema: &str) -> Option<&Loaded<Vec<TableInfo>>> {
        self.tables.get(&(target.to_owned(), schema.to_owned()))
    }

    pub fn details(&self, target: &str, schema: &str, table: &str) -> Option<&Loaded<TableDetails>> {
        self.details.get(&(target.to_owned(), schema.to_owned(), table.to_owned()))
    }

    pub fn objects(&self, target: &str, schema: &str) -> Option<&SchemaObjects> {
        self.objects.get(&(target.to_owned(), schema.to_owned()))
    }

    /// The schema model for completion, once loaded.
    pub fn completion(&self, target: &str) -> Option<&CompletionModel> {
        self.completion.get(target)
    }

    pub fn output(&self) -> &[OutputEntry] {
        &self.output
    }

    pub fn max_rows(&self) -> u64 {
        self.max_rows
    }

    pub fn history(&self) -> Option<&HistoryState> {
        self.history.as_ref()
    }

    pub fn manual_commit(&self, id: &str) -> bool {
        self.manual_commit.contains(id)
    }

    pub fn pending_transaction(&self, id: &str) -> bool {
        self.pending_tx.contains(id)
    }

    /// A statement of console `path` is running.
    pub fn is_running(&self, path: &str) -> bool {
        self.running.contains_key(path)
    }

    /// The data source of a target (`<id>` or `<id>:<database>`).
    pub fn source_of(&self, target: &str) -> Option<&DataSource> {
        let id = split_target(target).0;
        self.sources.iter().find(|s| s.id == id)
    }

    fn driver_of(&self, target: &str) -> DbDriver {
        self.source_of(target).map_or(DbDriver::Sqlite, |s| s.driver)
    }

    /// `name` or `name: database` for messages and pickers.
    pub fn target_label(&self, target: &str) -> String {
        let name = self.source_of(target).map_or(target, |s| s.name.as_str());
        match split_target(target).1 {
            Some(db) => format!("{name}: {db}"),
            None => name.to_owned(),
        }
    }

    /// Targets of data source `id` that are connected.
    pub fn connected_targets(&self, id: &str) -> Vec<String> {
        self.connected.iter().filter(|t| split_target(t).0 == id).cloned().collect()
    }

    /// The target console `path` runs on: its binding, else the only data source.
    pub fn console_source(&self, path: &str) -> Option<String> {
        if let Some(bound) = self.consoles.get(path)
            && self.source_of(bound).is_some()
        {
            return Some(bound.clone());
        }
        match self.sources.as_slice() {
            [only] => Some(only.id.clone()),
            _ => None,
        }
    }

    /// The schema console `path`'s statements run in: its pick, else the connection's default.
    pub fn console_schema(&self, path: &str) -> String {
        let target = self.console_source(path);
        let default = target.and_then(|t| self.completion.get(&t)).map(|m| m.default_schema.as_str());
        active_schema(self.console_schemas.get(path).map(String::as_str), default)
    }

    /// Whether result tab `id` can show other pages: running its query again only reads.
    pub fn pageable(&self, id: u64) -> bool {
        self.results.iter().find(|r| r.id == id).is_some_and(|r| self.meta_pageable(r))
    }

    fn meta_pageable(&self, r: &ResultMeta) -> bool {
        !r.plan && is_read_only_query(&r.statement.text, self.driver_of(&r.target))
    }

    /// Whether the rows of result tab `id` can be counted.
    pub fn countable(&self, id: u64) -> bool {
        self.results
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| self.meta_pageable(r) && count_query(&r.statement.text).is_some())
    }

    /// What console `path`'s toolbar shows.
    pub fn console_bar(&self, path: &str) -> ConsoleBar {
        let bound = self.console_source(path);
        let source = bound.as_deref().and_then(|b| self.source_of(b));
        let driver = source.map(|s| s.driver);
        let id = bound.as_deref().map(|b| split_target(b).0.to_owned());
        let sql_server = driver == Some(DbDriver::SqlServer);
        let switchable = super::model::schema_switchable(driver);
        let default = bound.as_deref().and_then(|b| self.completion.get(b)).map(|m| m.default_schema.clone());
        let schema = active_schema(self.console_schemas.get(path).map(String::as_str), default.as_deref());
        let mut schema_options: Vec<String> = if !switchable {
            Vec::new()
        } else if sql_server {
            id.as_deref().and_then(|i| loaded_ok(self.databases.get(i))).map(|d| d.names.clone()).unwrap_or_default()
        } else {
            bound.as_deref().and_then(|b| loaded_ok(self.schemas.get(b))).cloned().unwrap_or_default()
        };
        // The connection's default is offered before its schema list loads.
        if switchable
            && let Some(d) = default.filter(|d| !d.is_empty())
            && !schema_options.contains(&d)
        {
            schema_options.insert(0, d);
        }
        let mut targets = Vec::new();
        for s in &self.sources {
            let label = source_label(&s.name, s.driver, &s.vendor);
            let detail = label.get(s.name.len() + 1..).unwrap_or("").to_owned();
            match loaded_ok(self.databases.get(&s.id)).filter(|_| lists_databases(s)) {
                Some(dbs) => {
                    for name in &dbs.names {
                        targets.push(TargetOption {
                            value: target_of(&s.id, Some(name), Some(&dbs.current)),
                            label: format!("{}: {name}", s.name),
                            detail: detail.clone(),
                            icon: "database",
                        });
                    }
                }
                None => {
                    targets.push(TargetOption { value: s.id.clone(), label: s.name.clone(), detail, icon: "db-schema" })
                }
            }
        }
        // A console bound to a database whose list is not loaded yet still shows it.
        if let Some(b) = &bound
            && !targets.iter().any(|t| &t.value == b)
        {
            targets.push(TargetOption {
                value: b.clone(),
                label: self.target_label(b),
                detail: String::new(),
                icon: "database",
            });
        }
        ConsoleBar {
            running: self.running.contains_key(path),
            max_rows: self.max_rows,
            supports_transactions: driver != Some(DbDriver::Clickhouse),
            manual: bound.as_deref().is_some_and(|b| self.manual_commit.contains(b)),
            pending: bound.as_deref().is_some_and(|b| self.pending_tx.contains(b)),
            has_sources: !self.sources.is_empty(),
            bound,
            switchable,
            sql_server,
            schema,
            schema_options,
            targets,
        }
    }

    // ---- plumbing -----------------------------------------------------------------------------

    fn ticket(&mut self) -> Ticket {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }

    fn issue(&mut self, op: DbOp, slot: Option<Slot>, then: Then, out: &mut Vec<Command>) -> Ticket {
        let ticket = self.ticket();
        if let Some(slot) = &slot
            && let Some(old) = self.slots.insert(slot.clone(), ticket)
        {
            self.drop_request(old, out);
        }
        self.pending.insert(ticket, Pending { op: op.clone(), slot, then });
        out.push(Command::Backend { ticket, op });
        ticket
    }

    /// Forgets a request: its answer, or the answer to its prompt, will be dropped.
    fn drop_request(&mut self, ticket: Ticket, out: &mut Vec<Command>) {
        let then = match self.pending.remove(&ticket) {
            Some(p) => Some(p.then),
            None => {
                let parked = self.parked.iter().find_map(|(k, p)| match p {
                    Parked::Retry { ticket: t, .. } if *t == ticket => Some(*k),
                    _ => None,
                });
                parked.and_then(|k| match self.parked.remove(&k) {
                    Some(Parked::Retry { pending, .. }) => Some(pending.then),
                    _ => None,
                })
            }
        };
        if let Some(Then::Load { after: Some(after) }) = then {
            self.step(after, out);
        }
    }

    fn release_slot(&mut self, slot: Option<&Slot>, ticket: Ticket) {
        if let Some(slot) = slot
            && self.slots.get(slot) == Some(&ticket)
        {
            self.slots.remove(slot);
        }
    }

    fn notify(out: &mut Vec<Command>, level: Level, title: String, body: Option<String>, key: Option<&'static str>) {
        out.push(Command::Notify(Notice { level, title, body, key }));
    }

    /// Adds a line to the Output log.
    pub fn log(&mut self, kind: OutputKind, text: String, prompt: Option<String>, env: &Env<'_>) -> Vec<Command> {
        let mut out = Vec::new();
        self.push_log(kind, text, prompt, env, &mut out);
        out
    }

    fn push_log(
        &mut self,
        kind: OutputKind,
        text: String,
        prompt: Option<String>,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) {
        let entry = OutputEntry { id: self.next_line, time_ms: env.now_ms, kind, text, prompt };
        self.next_line += 1;
        out.push(Command::Results(ResultsUpdate::Log(vec![entry.line()])));
        self.output.push(entry);
        if self.output.len() > MAX_OUTPUT {
            let extra = self.output.len() - MAX_OUTPUT;
            self.output.drain(..extra);
        }
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }

    pub fn set_max_rows(&mut self, rows: u64) {
        self.max_rows = rows.max(1);
    }

    /// Settings › SSH Configurations, which tunnels may name.
    pub fn set_ssh_configs(&mut self, configs: Vec<SshConfiguration>) {
        self.ssh_configs = configs;
    }

    /// A reopened project starts with a fresh backend: everything but the console bindings, their
    /// schemas, the row limit and the parameter values is forgotten, and pending answers dropped.
    pub fn reset(&mut self) {
        let keep = (
            std::mem::take(&mut self.consoles),
            std::mem::take(&mut self.console_schemas),
            self.max_rows,
            std::mem::take(&mut self.param_values),
            std::mem::take(&mut self.ssh_configs),
            self.next_ticket,
            self.next_tab,
            self.next_line,
            self.next_job,
        );
        *self = Self::new(None, None);
        (
            self.consoles,
            self.console_schemas,
            self.max_rows,
            self.param_values,
            self.ssh_configs,
            self.next_ticket,
            self.next_tab,
            self.next_line,
            self.next_job,
        ) = keep;
    }

    // ---- answers ------------------------------------------------------------------------------

    /// The backend's answer to request `ticket`; an answer to a superseded or forgotten request
    /// is dropped.
    pub fn answer(&mut self, ticket: Ticket, answer: Answer, env: &Env<'_>) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(pending) = self.pending.remove(&ticket) else { return out };
        let remedy = match (&answer, pending.op.connection_target()) {
            (Answer::Failed(error), Some(target)) => self.remedy(target, error),
            _ => None,
        };
        if let (Some((remedy, source)), Answer::Failed(error)) = (remedy, &answer) {
            let prompt = match remedy {
                ConnectRemedy::Password => Prompt::Password { source: Box::new(source.clone()) },
                ConnectRemedy::SshSecret => {
                    Prompt::SshSecret { source: Box::new(source.clone()), message: error.message.clone() }
                }
                ConnectRemedy::TrustHostKey => Prompt::TrustHost {
                    message: format!(
                        "{}. The authenticity of the host cannot be established. Trust it and add the key to \
                         ~/.ssh/known_hosts?",
                        error.message
                    ),
                },
            };
            let ask = self.ticket();
            self.parked.insert(
                ask,
                Parked::Retry { ticket, pending: Box::new(pending), error: error.clone(), remedy, id: source.id },
            );
            out.push(Command::Ask { ticket: ask, prompt });
            return out;
        }
        self.release_slot(pending.slot.as_ref(), ticket);
        if !matches!(answer, Answer::Failed(_))
            && let Some(target) = pending.op.connection_target()
        {
            self.used(target.to_owned(), env, &mut out);
        }
        self.finish(pending, answer, env, &mut out);
        out
    }

    /// What a failed connection of `target` needs, with the source as the prompt shows it.
    fn remedy(&self, target: &str, error: &IdeError) -> Option<(ConnectRemedy, DataSource)> {
        let source = self.source_of(target)?;
        let mut resolved = source.clone();
        resolved.ssh = effective_ssh(source.ssh.as_ref(), &self.ssh_configs);
        connect_remedy(Some(error.kind), &error.message, resolved.ssh.as_ref()).map(|r| (r, resolved))
    }

    /// A request of `target` succeeded: it is connected and was just used.
    fn used(&mut self, target: String, env: &Env<'_>, out: &mut Vec<Command>) {
        self.last_used.insert(target.clone(), env.now_ms);
        if self.connected.insert(target.clone()) {
            out.push(Command::Tree(TreeUpdate::Connected(self.connected.clone())));
            // Options › Transaction control: a manual data source's consoles start in manual commit.
            let manual = self
                .source_of(&target)
                .is_some_and(|s| s.driver != DbDriver::Clickhouse && s.options.transaction == DbTxMode::Manual);
            if manual && !self.manual_commit.contains(&target) {
                self.issue(
                    DbOp::SetManualCommit { id: target.clone(), manual: true },
                    None,
                    Then::ManualCommit { id: target, manual: true },
                    out,
                );
            }
        }
    }

    /// The user's answer to prompt `ticket`.
    pub fn prompt_answered(&mut self, ticket: Ticket, answer: PromptAnswer, env: &Env<'_>) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(parked) = self.parked.remove(&ticket) else { return out };
        match parked {
            Parked::Retry { ticket, pending, error, remedy, id } => {
                let op = match (remedy, answer) {
                    (ConnectRemedy::Password, PromptAnswer::Secret { value, save }) => {
                        Some(DbOp::SetPassword { id, password: Some(value), save })
                    }
                    (ConnectRemedy::SshSecret, PromptAnswer::Secret { value, save }) => {
                        Some(DbOp::SetSshSecret { id, secret: Some(value), save })
                    }
                    (ConnectRemedy::TrustHostKey, PromptAnswer::Confirm) => Some(DbOp::TrustSshHost { id }),
                    _ => None,
                };
                match op {
                    Some(op) => {
                        self.issue(op, None, Then::Unlock { ticket, pending }, &mut out);
                    }
                    None => self.give_up(ticket, *pending, error, env, &mut out),
                }
            }
            Parked::Params { run, found, explain } => {
                if let PromptAnswer::Params(values) = answer {
                    self.param_values.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
                    let statements = run
                        .statements
                        .iter()
                        .zip(&found)
                        .map(|(st, params)| SqlStatement { text: bind_params(&st.text, params, &values), ..st.clone() })
                        .collect();
                    let run = ConsoleRun { statements, ..run };
                    if explain { self.start_explain(run, &mut out) } else { self.start_run(run, &mut out) }
                }
            }
            Parked::Remove { id } => {
                if answer == PromptAnswer::Confirm {
                    if self.connected_targets(&id).is_empty() {
                        self.delete_source(&id, &mut out);
                    } else {
                        self.disconnect_group(&id, false, true, &mut out);
                    }
                }
            }
            Parked::EndTx { id } => {
                if let PromptAnswer::Commit(commit) = answer {
                    self.issue(
                        DbOp::FinishTransaction { id: id.clone(), commit },
                        None,
                        Then::FinishTx { id, commit, then_auto: true },
                        &mut out,
                    );
                }
            }
        }
        out
    }

    /// A parked request ends with the error that parked it.
    fn give_up(&mut self, ticket: Ticket, pending: Pending, error: IdeError, env: &Env<'_>, out: &mut Vec<Command>) {
        self.release_slot(pending.slot.as_ref(), ticket);
        self.finish(pending, Answer::Failed(error), env, out);
    }

    /// Runs a parked request again, unless a newer one took its place.
    fn reissue(&mut self, ticket: Ticket, pending: Pending, out: &mut Vec<Command>) {
        if let Some(slot) = &pending.slot
            && self.slots.get(slot) != Some(&ticket)
        {
            if let Then::Load { after: Some(after) } = pending.then {
                self.step(after, out);
            }
            return;
        }
        let Pending { op, slot, then } = pending;
        let new = self.issue(op, slot, then, out);
        // Running consoles follow their request to the new ticket.
        for t in self.running.values_mut() {
            if *t == ticket {
                *t = new;
            }
        }
    }

    fn finish(&mut self, pending: Pending, answer: Answer, env: &Env<'_>, out: &mut Vec<Command>) {
        match pending.then {
            Then::Sources => self.sources_loaded(answer, out),
            Then::Save { id, rest, report, forget } => self.save_step(id, rest, report, forget, answer, out),
            Then::Connect { load } => match answer {
                Answer::Failed(e) if e.kind == ErrorKind::Cancelled => {}
                Answer::Failed(e) => {
                    if let DbOp::Connect { target } = &pending.op {
                        let title = format!("Cannot connect to {}", self.target_label(target));
                        Self::notify(out, Level::Error, title, Some(e.message), None);
                    }
                }
                _ => {
                    if load && let DbOp::Connect { target } = pending.op {
                        let lists = self.source_of(&target).is_some_and(lists_databases);
                        if lists {
                            self.load_databases(&target, None, out)
                        } else {
                            self.load_schemas(&target, None, out)
                        }
                    }
                }
            },
            Then::Disconnect { group } => self.disconnected(group, env, out),
            Then::Load { after } => {
                self.loaded_level(pending.op, answer, out);
                if let Some(after) = after {
                    self.step(after, out);
                }
            }
            Then::Completion { run } => {
                let target = match &pending.op {
                    DbOp::Completion { target } => target.clone(),
                    _ => String::new(),
                };
                if let Answer::Completion(model) = answer {
                    self.completion.insert(target.clone(), model);
                }
                if let Some(run) = run {
                    let schema = self.completion.get(&target).map(|m| m.default_schema.clone()).unwrap_or_default();
                    self.execute_run(run, schema, out);
                }
            }
            Then::Run { kind, target, schema, was_connected } => {
                self.run_finished(kind, target, schema, was_connected, answer, env, out)
            }
            Then::Explain { path, tab, target, statement, schema } => {
                self.explained(path, tab, target, statement, schema, answer, env, out)
            }
            Then::Unlock { ticket, pending } => match answer {
                Answer::Failed(e) => self.give_up(ticket, *pending, e, env, out),
                _ => self.reissue(ticket, *pending, out),
            },
            Then::ManualCommit { id, manual } => match answer {
                Answer::Failed(e) => {
                    Self::notify(out, Level::Error, "Cannot switch the transaction mode".into(), Some(e.message), None)
                }
                _ => {
                    if manual {
                        self.manual_commit.insert(id.clone());
                    } else {
                        self.manual_commit.remove(&id);
                    }
                    self.pending_tx.remove(&id);
                }
            },
            Then::FinishTx { id, commit, then_auto } => match answer {
                Answer::Failed(e) => {
                    let label = self.target_label(&id);
                    self.push_log(OutputKind::Error, format!("[{label}] {}", e.message), None, env, out);
                    let title = if commit { "Cannot commit" } else { "Cannot roll back" };
                    Self::notify(out, Level::Error, title.into(), Some(e.message), None);
                }
                _ => {
                    let prompt = format!("{}>", self.prompt_of(&id));
                    let word = if commit { "commit" } else { "rollback" };
                    self.push_log(OutputKind::Statement, word.into(), Some(prompt), env, out);
                    let done = if commit { "Transaction committed" } else { "Transaction rolled back" };
                    self.push_log(OutputKind::Result, done.into(), None, env, out);
                    self.pending_tx.remove(&id);
                    if then_auto {
                        self.issue(
                            DbOp::SetManualCommit { id: id.clone(), manual: false },
                            None,
                            Then::ManualCommit { id, manual: false },
                            out,
                        );
                    }
                }
            },
            Then::History => {
                if let Some(h) = &mut self.history {
                    h.entries = Some(match answer {
                        Answer::History(entries) => entries,
                        _ => Vec::new(),
                    });
                }
            }
            Then::RemoveHistory { texts } => match answer {
                Answer::Failed(e) => {
                    let title = "Cannot remove from the query history".to_owned();
                    Self::notify(out, Level::Error, title, Some(e.message), None)
                }
                _ => {
                    if let Some(h) = &mut self.history
                        && let Some(entries) = &mut h.entries
                    {
                        match &texts {
                            Some(texts) => entries.retain(|e| !texts.contains(&e.text)),
                            None => entries.clear(),
                        }
                    }
                }
            },
            Then::Ddl { target, schema, name, routine } => match answer {
                Answer::Text(text) => {
                    let text = if routine { text } else { readable_ddl(&text, self.driver_of(&target)) };
                    let file = format!("{}.{}.sql", safe_name(&schema), safe_name(&name));
                    self.issue(
                        DbOp::ConsolePath { target: target.clone() },
                        None,
                        Then::DdlPath { target, name, file, text, routine },
                        out,
                    );
                }
                other => Self::ddl_failed(&name, routine, other.message(), out),
            },
            Then::DdlPath { target, name, file, text, routine } => match answer {
                Answer::Text(console) => {
                    let path = sibling(&console, &file);
                    self.issue(
                        DbOp::WriteFile { path: path.clone(), text },
                        None,
                        Then::Open { target, path, name, routine },
                        out,
                    );
                }
                other => Self::ddl_failed(&name, routine, other.message(), out),
            },
            Then::Open { target, path, name, routine } => match answer {
                Answer::Failed(e) => Self::ddl_failed(&name, routine, e.message, out),
                _ => {
                    self.bind(&path, Some(&target), out);
                    out.push(Command::OpenFile { path });
                }
            },
            Then::TablePage { editor, request } => match answer {
                Answer::TablePage(page) => out.push(Command::TablePage { editor, request, result: Ok(page) }),
                other if other.cancelled() => {}
                other => out.push(Command::TablePage { editor, request, result: Err(other.message()) }),
            },
            Then::TableEdits { editor, table } => match answer {
                Answer::Count(n) => {
                    let title = format!("{table}: {} row(s) changed", js_number_string(n));
                    Self::notify(out, Level::Info, title, None, Some("db-submit"));
                    out.push(Command::TableEdited { editor, ok: true });
                }
                other => {
                    let title = "Changes were not submitted".to_owned();
                    Self::notify(out, Level::Error, title, Some(other.message()), Some("db-submit"));
                    out.push(Command::TableEdited { editor, ok: false });
                }
            },
            Then::ResultEdits { tab } => {
                let result = match answer {
                    Answer::Count(n) => Ok(n),
                    other => Err(other.message()),
                };
                out.push(Command::Results(ResultsUpdate::Submitted { id: tab, result }));
            }
            Then::ConsolePath { target, append } => match answer {
                Answer::Text(path) => {
                    self.bind(&path, Some(&target), out);
                    out.push(Command::OpenFile { path: path.clone() });
                    if let Some(text) = append {
                        out.push(Command::AppendToConsole { path, text });
                    }
                }
                other => {
                    let title = "Cannot open the query console".to_owned();
                    Self::notify(out, Level::Error, title, Some(other.message()), None)
                }
            },
        }
    }

    fn ddl_failed(name: &str, routine: bool, message: String, out: &mut Vec<Command>) {
        let title = if routine {
            format!("Cannot get the definition of {name}")
        } else {
            format!("Cannot get the DDL of {name}")
        };
        Self::notify(out, Level::Error, title, Some(message), None);
    }

    // ---- data sources -------------------------------------------------------------------------

    /// Reads the data sources and the connected targets (superseding an earlier read).
    pub fn load_sources(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        self.issue(DbOp::LoadSources, Some(Slot::Sources), Then::Sources, &mut out);
        out
    }

    fn sources_loaded(&mut self, answer: Answer, out: &mut Vec<Command>) {
        match answer {
            Answer::Sources { sources, connected } => {
                // Changed or removed sources drop their cached metadata.
                let stale: Vec<String> = self
                    .sources
                    .iter()
                    .filter(|old| sources.iter().find(|s| s.id == old.id) != Some(*old))
                    .map(|old| old.id.clone())
                    .collect();
                for id in stale {
                    self.forget(&id, out);
                }
                self.sources = sources;
                self.connected = connected.into_iter().collect();
                self.loaded = true;
                out.push(Command::Tree(TreeUpdate::Sources(self.sources.clone())));
                out.push(Command::Tree(TreeUpdate::Connected(self.connected.clone())));
            }
            other => {
                self.loaded = true;
                out.push(Command::Tree(TreeUpdate::Sources(self.sources.clone())));
                let title = "Cannot read data sources".to_owned();
                Self::notify(out, Level::Error, title, Some(other.message()), Some("db-sources"));
            }
        }
    }

    fn save_chain(
        &mut self,
        next: Vec<DataSource>,
        id: &str,
        rest: Vec<DbOp>,
        report: SaveReport,
        forget: bool,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        self.issue(DbOp::SaveSources(next), None, Then::Save { id: id.to_owned(), rest, report, forget }, &mut out);
        out
    }

    fn save_step(
        &mut self,
        id: String,
        mut rest: Vec<DbOp>,
        report: SaveReport,
        forget: bool,
        answer: Answer,
        out: &mut Vec<Command>,
    ) {
        if let Answer::Failed(e) = answer {
            match report {
                SaveReport::Dialog | SaveReport::Rename => {
                    out.push(Command::SourceSaveFailed { id, message: e.message })
                }
                SaveReport::Duplicate => {
                    Self::notify(out, Level::Error, "Cannot duplicate the data source".into(), Some(e.message), None)
                }
                SaveReport::Remove => {
                    Self::notify(out, Level::Error, "Cannot remove the data source".into(), Some(e.message), None)
                }
            }
            return;
        }
        if !rest.is_empty() {
            let op = rest.remove(0);
            self.issue(op, None, Then::Save { id, rest, report, forget }, out);
            return;
        }
        if forget {
            self.forget(&id, out);
        }
        self.issue(DbOp::LoadSources, Some(Slot::Sources), Then::Sources, out);
        if matches!(report, SaveReport::Dialog | SaveReport::Rename) {
            out.push(Command::SourceSaved { id });
        }
    }

    /// Saves `source` (added or replacing the one with its id) and its password and SSH secret,
    /// then reads the sources again. Answers [`Command::SourceSaved`] or [`Command::SourceSaveFailed`].
    pub fn save_source(
        &mut self,
        source: DataSource,
        password: SecretChange,
        ssh_secret: SecretChange,
    ) -> Vec<Command> {
        let id = source.id.clone();
        let mut rest = Vec::new();
        if let Some(password) = password.value() {
            rest.push(DbOp::SetPassword { id: id.clone(), password, save: source.save_password });
        }
        if let Some(secret) = ssh_secret.value() {
            let save = source.ssh.as_ref().is_some_and(|s| s.save_secret);
            rest.push(DbOp::SetSshSecret { id: id.clone(), secret, save });
        }
        let mut next = self.sources.clone();
        match next.iter_mut().find(|s| s.id == id) {
            Some(slot) => *slot = source,
            None => next.push(source),
        }
        self.save_chain(next, &id, rest, SaveReport::Dialog, true)
    }

    /// Copy/Paste › Duplicate: a copy named `<name> [2]` (its password is not copied); `random`
    /// makes the new id.
    pub fn duplicate_source(&mut self, id: &str, random: &str) -> Vec<Command> {
        let Some(src) = self.source_of(id).cloned() else { return Vec::new() };
        let names: BTreeSet<&str> = self.sources.iter().map(|s| s.name.as_str()).collect();
        let mut n = 2;
        while names.contains(format!("{} [{n}]", src.name).as_str()) {
            n += 1;
        }
        let copy = DataSource {
            id: new_source_id(random),
            name: format!("{} [{n}]", src.name),
            plugin_owner: None,
            plugin_source_id: None,
            ..src
        };
        let new_id = copy.id.clone();
        let mut next = self.sources.clone();
        next.push(copy);
        self.save_chain(next, &new_id, Vec::new(), SaveReport::Duplicate, true)
    }

    /// Renames data source `id`; an empty or unchanged name does nothing.
    pub fn rename_source(&mut self, id: &str, name: &str) -> Vec<Command> {
        let name = name.trim();
        let Some(src) = self.source_of(id) else { return Vec::new() };
        if name.is_empty() || name == src.name {
            return Vec::new();
        }
        let src_id = src.id.clone();
        let next = self
            .sources
            .iter()
            .map(|s| if s.id == src_id { DataSource { name: name.to_owned(), ..s.clone() } } else { s.clone() })
            .collect();
        self.save_chain(next, &src_id, Vec::new(), SaveReport::Rename, false)
    }

    /// Removes data source `id` after asking; connected targets disconnect first.
    pub fn remove_source(&mut self, id: &str) -> Vec<Command> {
        let Some((id, name)) = self.source_of(id).map(|src| (src.id.clone(), src.name.clone())) else {
            return Vec::new();
        };
        let ticket = self.ticket();
        self.parked.insert(ticket, Parked::Remove { id: id.clone() });
        let prompt = Prompt::RemoveSource { id, name };
        vec![Command::Ask { ticket, prompt }]
    }

    fn delete_source(&mut self, id: &str, out: &mut Vec<Command>) {
        let next = self.sources.iter().filter(|s| s.id != id).cloned().collect();
        out.extend(self.save_chain(next, id, Vec::new(), SaveReport::Remove, true));
    }

    // ---- connections --------------------------------------------------------------------------

    /// Connects `target` explicitly; failures notify.
    pub fn connect(&mut self, target: &str) -> Vec<Command> {
        let mut out = Vec::new();
        self.issue(DbOp::Connect { target: target.to_owned() }, None, Then::Connect { load: false }, &mut out);
        out
    }

    /// Connect Plugin Data Source: shows the tree, reads the sources, connects and lists the
    /// first level below the source.
    pub fn plugin_connect(&mut self, id: &str) -> Vec<Command> {
        let mut out = vec![Command::ShowToolWindow];
        out.extend(self.load_sources());
        self.issue(DbOp::Connect { target: id.to_owned() }, None, Then::Connect { load: true }, &mut out);
        out
    }

    /// Disconnects target `id`; a data source id disconnects all its databases.
    pub fn disconnect(&mut self, id: &str) -> Vec<Command> {
        let mut out = Vec::new();
        self.disconnect_group(id, false, false, &mut out);
        out
    }

    /// Deactivate on the tree: no node disconnects every connected target; a database node its own
    /// target; any other node its data source.
    pub fn disconnect_node(&mut self, target: Option<&str>, database_node: bool) -> Vec<Command> {
        match target {
            None => {
                let all: Vec<String> = self.connected.iter().cloned().collect();
                all.iter().flat_map(|t| self.disconnect(t)).collect()
            }
            Some(t) if database_node => self.disconnect(t),
            Some(t) => {
                let id = split_target(t).0.to_owned();
                self.disconnect(&id)
            }
        }
    }

    fn disconnect_group(&mut self, id: &str, idle: bool, remove: bool, out: &mut Vec<Command>) {
        let mut targets = if split_target(id).1.is_some() { vec![id.to_owned()] } else { self.connected_targets(id) };
        if !targets.iter().any(|t| t == id) {
            targets.push(id.to_owned());
        }
        let group = self.next_job;
        self.next_job += 1;
        self.disconnects.insert(
            group,
            DisconnectGroup { id: id.to_owned(), targets: targets.clone(), remaining: targets.len(), idle, remove },
        );
        for target in targets {
            self.issue(DbOp::Disconnect { target }, None, Then::Disconnect { group }, out);
        }
    }

    fn disconnected(&mut self, group: u64, env: &Env<'_>, out: &mut Vec<Command>) {
        let Some(g) = self.disconnects.get_mut(&group) else { return };
        g.remaining = g.remaining.saturating_sub(1);
        if g.remaining > 0 {
            return;
        }
        let Some(g) = self.disconnects.remove(&group) else { return };
        self.forget(&g.id, out);
        for t in &g.targets {
            self.connected.remove(t);
        }
        out.push(Command::Tree(TreeUpdate::Connected(self.connected.clone())));
        if g.idle {
            let text = format!("{}: disconnected after being idle", self.target_label(&g.id));
            self.push_log(OutputKind::Info, text, None, env, out);
        }
        if g.remove {
            self.delete_source(&g.id, out);
        }
    }

    /// Disconnects targets idle past their data source's Auto-disconnect; call it every few
    /// seconds.
    pub fn tick(&mut self, env: &Env<'_>) -> Vec<Command> {
        let mut out = Vec::new();
        let running: BTreeSet<String> = self.running.keys().filter_map(|p| self.consoles.get(p).cloned()).collect();
        let busy = |t: &str| running.contains(t) || self.pending_tx.contains(t);
        let connected = self.connected.iter().map(String::as_str);
        let idle = idle_targets(connected, &self.sources, &self.last_used, busy, env.now_ms);
        for t in idle {
            self.last_used.remove(&t);
            self.disconnect_group(&t, true, false, &mut out);
        }
        out
    }

    /// Drops the cached metadata of target `id` (a data source id: of all its databases) and the
    /// loads in flight for it.
    fn forget(&mut self, id: &str, out: &mut Vec<Command>) {
        let whole = split_target(id).1.is_none();
        let of = |t: &str| t == id || (whole && split_target(t).0 == id);
        if whole {
            self.databases.remove(id);
        }
        self.schemas.retain(|k, _| !of(k.as_str()));
        self.completion.retain(|k, _| !of(k.as_str()));
        self.tables.retain(|k, _| !of(k.0.as_str()));
        self.details.retain(|k, _| !of(k.0.as_str()));
        self.objects.retain(|k, _| !of(k.0.as_str()));
        let dropped: Vec<Ticket> =
            self.slots.iter().filter(|(slot, _)| slot.target().is_some_and(of)).map(|(_, &t)| t).collect();
        self.slots.retain(|_, t| !dropped.contains(t));
        for t in dropped {
            self.drop_request(t, out);
        }
        out.push(Command::Tree(TreeUpdate::Forget(id.to_owned())));
    }

    // ---- loaders ------------------------------------------------------------------------------

    /// Answers a tree's load request.
    pub fn tree_load(&mut self, load: &TreeLoad) -> Vec<Command> {
        let mut out = Vec::new();
        match load {
            TreeLoad::Databases { id } => self.load_databases(id, None, &mut out),
            TreeLoad::Schemas { target } => self.load_schemas(target, None, &mut out),
            TreeLoad::Tables { target, schema } => self.load_tables(target, schema, None, &mut out),
            TreeLoad::Details { target, schema, table } => self.load_details(target, schema, table, None, &mut out),
        }
        out
    }

    fn load_databases(&mut self, id: &str, after: Option<After>, out: &mut Vec<Command>) {
        self.databases.insert(id.to_owned(), Loaded::Loading);
        let slot = Some(Slot::Databases(id.to_owned()));
        self.issue(DbOp::Databases { id: id.to_owned() }, slot, Then::Load { after }, out);
    }

    fn load_schemas(&mut self, target: &str, after: Option<After>, out: &mut Vec<Command>) {
        self.schemas.insert(target.to_owned(), Loaded::Loading);
        let op = DbOp::Schemas { target: target.to_owned() };
        self.issue(op, Some(Slot::Schemas(target.to_owned())), Then::Load { after }, out);
    }

    fn load_tables(&mut self, target: &str, schema: &str, after: Option<After>, out: &mut Vec<Command>) {
        let (t, s) = (target.to_owned(), schema.to_owned());
        self.tables.insert((t.clone(), s.clone()), Loaded::Loading);
        let op = DbOp::Tables { target: t.clone(), schema: s.clone() };
        self.issue(op, Some(Slot::Tables(t, s)), Then::Load { after }, out);
    }

    fn load_details(&mut self, target: &str, schema: &str, table: &str, after: Option<After>, out: &mut Vec<Command>) {
        let (t, s, n) = (target.to_owned(), schema.to_owned(), table.to_owned());
        self.details.insert((t.clone(), s.clone(), n.clone()), Loaded::Loading);
        let op = DbOp::Details { target: t.clone(), schema: s.clone(), table: n.clone() };
        self.issue(op, Some(Slot::Details(t, s, n)), Then::Load { after }, out);
    }

    /// Loads the schema model for completion of `target` once (cached; one load at a time).
    pub fn load_completion(&mut self, target: &str) -> Vec<Command> {
        let mut out = Vec::new();
        let slot = Slot::Completion(target.to_owned());
        if !self.completion.contains_key(target) && !self.slots.contains_key(&slot) {
            let op = DbOp::Completion { target: target.to_owned() };
            self.issue(op, Some(slot), Then::Completion { run: None }, &mut out);
        }
        out
    }

    fn loaded_level(&mut self, op: DbOp, answer: Answer, out: &mut Vec<Command>) {
        match op {
            DbOp::Databases { id } => {
                let result = match answer {
                    Answer::Databases(d) => Ok(d),
                    other => Err(other.message()),
                };
                self.databases.insert(id.clone(), loaded_of(result.clone()));
                out.push(Command::Tree(TreeUpdate::Databases { id, result }));
            }
            DbOp::Schemas { target } => {
                let result = match answer {
                    Answer::Schemas(s) => Ok(s),
                    other => Err(other.message()),
                };
                self.schemas.insert(target.clone(), loaded_of(result.clone()));
                out.push(Command::Tree(TreeUpdate::Schemas { target, result }));
            }
            DbOp::Tables { target, schema } => {
                let (result, objects) = match answer {
                    Answer::Tables { tables, objects } => (Ok(tables), objects),
                    other => (Err(other.message()), None),
                };
                let key = (target.clone(), schema.clone());
                if let Some(o) = &objects {
                    self.objects.insert(key.clone(), o.clone());
                }
                self.tables.insert(key, loaded_of(result.clone()));
                out.push(Command::Tree(TreeUpdate::Tables { target, schema, result, objects }));
            }
            DbOp::Details { target, schema, table } => {
                let result = match answer {
                    Answer::Details(d) => Ok(d),
                    other => Err(other.message()),
                };
                self.details.insert((target.clone(), schema.clone(), table.clone()), loaded_of(result.clone()));
                out.push(Command::Tree(TreeUpdate::Details { target, schema, table, result }));
            }
            _ => {}
        }
    }

    fn step(&mut self, after: After, out: &mut Vec<Command>) {
        match after {
            After::Refresh(r) => self.refresh_step(r, out),
            After::Reveal(r) => self.reveal_step(r, out),
        }
    }

    /// Re-reads the metadata of target `id` (a data source id: all its databases), reloading the
    /// levels that were loaded so the tree keeps its expanded nodes.
    pub fn refresh(&mut self, id: &str) -> Vec<Command> {
        let mut out = Vec::new();
        self.start_refresh(id, &mut out);
        out
    }

    /// Refresh on the tree: no node reads the sources and refreshes every connected target; a
    /// schema or folder its tables; a table its details; anything else its target.
    pub fn refresh_node(&mut self, target: Option<&str>, schema: Option<&str>, table: Option<&str>) -> Vec<Command> {
        let mut out = Vec::new();
        match (target, schema, table) {
            (None, _, _) => {
                out.extend(self.load_sources());
                let all: Vec<String> = self.connected.iter().cloned().collect();
                for t in all {
                    self.start_refresh(&t, &mut out);
                }
            }
            (Some(t), Some(s), Some(n)) => self.load_details(t, s, n, None, &mut out),
            (Some(t), Some(s), None) => self.load_tables(t, s, None, &mut out),
            (Some(t), None, _) => self.start_refresh(t, &mut out),
        }
        out
    }

    fn start_refresh(&mut self, id: &str, out: &mut Vec<Command>) {
        let whole = split_target(id).1.is_none();
        let of = |t: &str| t == id || (whole && split_target(t).0 == id);
        let mut schemas: Vec<String> = self
            .schemas
            .iter()
            .filter(|(k, l)| of(k.as_str()) && matches!(l, Loaded::Ok(_)))
            .map(|(k, _)| k.clone())
            .collect();
        let tables = self
            .tables
            .iter()
            .filter(|(k, l)| of(k.0.as_str()) && matches!(l, Loaded::Ok(_)))
            .map(|(k, _)| k.clone())
            .collect();
        let details = self
            .details
            .iter()
            .filter(|(k, l)| of(k.0.as_str()) && matches!(l, Loaded::Ok(_)))
            .map(|(k, _)| k.clone())
            .collect();
        self.forget(id, out);
        let lists = whole && self.source_of(id).is_some_and(lists_databases);
        if !lists && !schemas.iter().any(|s| s == id) {
            schemas.push(id.to_owned());
        }
        let r = self.next_job;
        self.next_job += 1;
        self.refreshes.insert(r, RefreshPlan { schemas, tables, details, stage: 0, outstanding: 0 });
        if lists {
            if let Some(plan) = self.refreshes.get_mut(&r) {
                plan.outstanding = 1;
            }
            self.load_databases(id, Some(After::Refresh(r)), out);
        } else {
            self.advance_refresh(r, out);
        }
    }

    fn refresh_step(&mut self, r: u64, out: &mut Vec<Command>) {
        let Some(plan) = self.refreshes.get_mut(&r) else { return };
        plan.outstanding = plan.outstanding.saturating_sub(1);
        if plan.outstanding == 0 {
            self.advance_refresh(r, out);
        }
    }

    /// Issues the next stage of refresh `r`: schemas, then tables, then the details of tables
    /// that still exist.
    fn advance_refresh(&mut self, r: u64, out: &mut Vec<Command>) {
        loop {
            let Some(plan) = self.refreshes.get_mut(&r) else { return };
            plan.stage += 1;
            let after = Some(After::Refresh(r));
            match plan.stage {
                1 => {
                    let schemas = std::mem::take(&mut plan.schemas);
                    plan.outstanding = schemas.len();
                    for t in &schemas {
                        self.load_schemas(t, after, out);
                    }
                    if !schemas.is_empty() {
                        return;
                    }
                }
                2 => {
                    let tables = std::mem::take(&mut plan.tables);
                    plan.outstanding = tables.len();
                    for (t, s) in &tables {
                        self.load_tables(t, s, after, out);
                    }
                    if !tables.is_empty() {
                        return;
                    }
                }
                3 => {
                    let details: Vec<(String, String, String)> = std::mem::take(&mut plan.details);
                    let exists: Vec<(String, String, String)> = details
                        .into_iter()
                        .filter(|(t, s, n)| {
                            let tables = loaded_ok(self.tables.get(&(t.clone(), s.clone())));
                            tables.is_some_and(|l| l.iter().any(|x| &x.name == n))
                        })
                        .collect();
                    if let Some(plan) = self.refreshes.get_mut(&r) {
                        plan.outstanding = exists.len();
                    }
                    for (t, s, n) in &exists {
                        self.load_details(t, s, n, after, out);
                    }
                    if !exists.is_empty() {
                        return;
                    }
                }
                _ => {
                    self.refreshes.remove(&r);
                    return;
                }
            }
        }
    }

    /// Selects a table (or one of its columns) in the Database tool window, loading and expanding
    /// the nodes that lead to it.
    pub fn reveal_in_tree(
        &mut self,
        target: &str,
        schema: &str,
        table: &str,
        kind: DbTableKind,
        column: Option<&str>,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        let r = self.next_job;
        self.next_job += 1;
        self.reveals.insert(
            r,
            RevealPlan {
                target: target.to_owned(),
                schema: schema.to_owned(),
                table: table.to_owned(),
                kind,
                column: column.map(str::to_owned),
                stage: 0,
                outstanding: 0,
            },
        );
        self.advance_reveal(r, &mut out);
        out
    }

    fn reveal_step(&mut self, r: u64, out: &mut Vec<Command>) {
        let Some(plan) = self.reveals.get_mut(&r) else { return };
        plan.outstanding = plan.outstanding.saturating_sub(1);
        if plan.outstanding == 0 {
            self.advance_reveal(r, out);
        }
    }

    fn advance_reveal(&mut self, r: u64, out: &mut Vec<Command>) {
        loop {
            let Some(plan) = self.reveals.get_mut(&r) else { return };
            plan.stage += 1;
            let (target, schema, table, column) =
                (plan.target.clone(), plan.schema.clone(), plan.table.clone(), plan.column.clone());
            let source_id = split_target(&target).0.to_owned();
            let after = Some(After::Reveal(r));
            let need = match plan.stage {
                1 => {
                    let lists = self.source_of(&target).is_some_and(lists_databases);
                    lists && loaded_ok(self.databases.get(&source_id)).is_none()
                }
                2 => loaded_ok(self.schemas.get(&target)).is_none(),
                3 => loaded_ok(self.tables.get(&(target.clone(), schema.clone()))).is_none(),
                4 => {
                    let key = (target.clone(), schema.clone(), table.clone());
                    column.is_some() && loaded_ok(self.details.get(&key)).is_none()
                }
                _ => {
                    let Some(plan) = self.reveals.remove(&r) else { return };
                    self.finish_reveal(plan, out);
                    return;
                }
            };
            if !need {
                continue;
            }
            if let Some(plan) = self.reveals.get_mut(&r) {
                plan.outstanding = 1;
            }
            match self.reveals.get(&r).map(|p| p.stage) {
                Some(1) => self.load_databases(&source_id, after, out),
                Some(2) => self.load_schemas(&target, after, out),
                Some(3) => self.load_tables(&target, &schema, after, out),
                _ => self.load_details(&target, &schema, &table, after, out),
            }
            return;
        }
    }

    fn finish_reveal(&mut self, plan: RevealPlan, out: &mut Vec<Command>) {
        let (source_id, database) = split_target(&plan.target);
        let mut expand = vec![source_id.to_owned()];
        if self.source_of(&plan.target).is_some_and(lists_databases) {
            let current = loaded_ok(self.databases.get(source_id)).map(|d| d.current.clone());
            if let Some(name) = database.map(str::to_owned).or(current) {
                expand.push(database_key(source_id, &name));
            }
        }
        let schema_key = format!("{}/{}", plan.target, plan.schema);
        let folder = folder_key(&schema_key, table_folder(plan.kind));
        let table_key = format!("{folder}/{}", plan.table);
        expand.push(schema_key);
        expand.push(folder);
        let key = match &plan.column {
            Some(column) => {
                expand.push(table_key.clone());
                expand.push(format!("{table_key}/columns"));
                format!("{table_key}/columns/{column}")
            }
            None => table_key,
        };
        out.push(Command::Tree(TreeUpdate::Reveal { expand, key }));
        out.push(Command::ShowToolWindow);
    }

    // ---- consoles -----------------------------------------------------------------------------

    fn bind(&mut self, path: &str, target: Option<&str>, out: &mut Vec<Command>) {
        let before = self.consoles.get(path).cloned();
        match target {
            Some(t) => self.consoles.insert(path.to_owned(), t.to_owned()),
            None => self.consoles.remove(path),
        };
        out.push(Command::Persist { key: BINDINGS_KEY, value: write_map(&self.consoles) });
        // Re-bound to another target: the console starts from its default schema again.
        if before.as_deref() != target && self.console_schemas.remove(path).is_some() {
            out.push(Command::Persist { key: CONSOLE_SCHEMAS_KEY, value: write_map(&self.console_schemas) });
        }
    }

    /// Binds console `path` to `target` (`None`: unbinds it).
    pub fn bind_console(&mut self, path: &str, target: Option<&str>) -> Vec<Command> {
        let mut out = Vec::new();
        self.bind(path, target, &mut out);
        out.extend(self.ensure_console(path));
        out
    }

    /// Sets (or clears, for `None` or "") console `path`'s active schema.
    pub fn set_console_schema(&mut self, path: &str, schema: Option<&str>) -> Vec<Command> {
        match schema.filter(|s| !s.is_empty()) {
            Some(s) => self.console_schemas.insert(path.to_owned(), s.to_owned()),
            None => self.console_schemas.remove(path),
        };
        vec![Command::Persist { key: CONSOLE_SCHEMAS_KEY, value: write_map(&self.console_schemas) }]
    }

    /// What console `path`'s toolbar needs loaded: the sources, the bound target's completion
    /// model, its databases (servers listing them, SQL Server) and its schemas.
    pub fn ensure_console(&mut self, path: &str) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.loaded && !self.slots.contains_key(&Slot::Sources) {
            out.extend(self.load_sources());
        }
        let Some(bound) = self.console_source(path) else { return out };
        out.extend(self.load_completion(&bound));
        let id = split_target(&bound).0.to_owned();
        let driver = self.source_of(&bound).map(|s| s.driver);
        let sql_server = driver == Some(DbDriver::SqlServer);
        let lists = self.source_of(&bound).is_some_and(lists_databases);
        if (sql_server || lists) && !self.databases.contains_key(&id) {
            self.load_databases(&id, None, &mut out);
        }
        if super::model::schema_switchable(driver) && !sql_server && !self.schemas.contains_key(&bound) {
            self.load_schemas(&bound, None, &mut out);
        }
        out
    }

    /// Opens (creating it on first use) the query console of `target`.
    pub fn open_console(&mut self, target: &str) -> Vec<Command> {
        let mut out = Vec::new();
        let then = Then::ConsolePath { target: target.to_owned(), append: None };
        self.issue(DbOp::ConsolePath { target: target.to_owned() }, None, then, &mut out);
        out
    }

    /// Opens the query console of `target` with `text` appended and selected (SQL scripts, DDL).
    pub fn open_console_with(&mut self, target: &str, text: &str) -> Vec<Command> {
        let mut out = Vec::new();
        let then = Then::ConsolePath { target: target.to_owned(), append: Some(text.to_owned()) };
        self.issue(DbOp::ConsolePath { target: target.to_owned() }, None, then, &mut out);
        out
    }

    /// Go to DDL: the definition of a table or view (`kind` `None`) or of a routine, trigger or
    /// sequence, in a SQL file next to the target's console.
    pub fn open_ddl(&mut self, target: &str, schema: &str, name: &str, kind: Option<DbObjectKind>) -> Vec<Command> {
        let mut out = Vec::new();
        let (t, s, n) = (target.to_owned(), schema.to_owned(), name.to_owned());
        let op = match kind {
            Some(kind) => DbOp::ObjectDdl { target: t.clone(), schema: s.clone(), kind, name: n.clone() },
            None => DbOp::Ddl { target: t.clone(), schema: s.clone(), table: n.clone() },
        };
        self.issue(op, None, Then::Ddl { target: t, schema: s, name: n, routine: kind.is_some() }, &mut out);
        out
    }

    fn prompt_of(&self, target: &str) -> String {
        if let Some(m) = self.completion.get(target).filter(|m| !m.default_schema.is_empty()) {
            return m.default_schema.clone();
        }
        match self.source_of(target) {
            Some(s) if !s.name.is_empty() => s.name.clone(),
            _ => target.to_owned(),
        }
    }

    /// The statements of a console run: the selection's (`from < to`), else the one at `from`.
    pub fn console_statements(text: &str, from: u32, to: u32, driver: DbDriver) -> Vec<SqlStatement> {
        if from < to {
            statements_in(text, from, to, driver)
        } else {
            statement_at(text, from, driver).into_iter().collect()
        }
    }

    /// Runs the statement at `from` (or the selection `from..to`, UTF-16 offsets) of console
    /// `path` on its target, after asking for query parameters. Result sets open as tabs, every
    /// statement is logged, and the console gets its marks ([`Command::MarkOutcomes`]).
    pub fn execute_console(&mut self, path: &str, text: &str, from: u32, to: u32) -> Vec<Command> {
        self.console_request(path, text, from, to, false)
    }

    /// Explain Plan of the statement at `from` (the first selected one) of console `path`.
    pub fn explain_console(&mut self, path: &str, text: &str, from: u32, to: u32) -> Vec<Command> {
        self.console_request(path, text, from, to, true)
    }

    fn console_request(&mut self, path: &str, text: &str, from: u32, to: u32, explain: bool) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(target) = self.console_source(path) else {
            if !explain {
                let (title, body) = if self.sources.is_empty() {
                    ("No data sources", "Add one in the Database tool window.")
                } else {
                    ("Choose a data source for this console", "Pick one in the toolbar above the editor.")
                };
                Self::notify(&mut out, Level::Info, title.into(), Some(body.into()), Some("db-execute"));
            }
            return out;
        };
        let driver = self.driver_of(&target);
        let statements = Self::console_statements(text, from, to, driver);
        if statements.is_empty() {
            return out;
        }
        let run = ConsoleRun { path: path.to_owned(), target, statements };
        // Query parameters: bare `?` are numbered across the statements.
        let mut numbered = 0;
        let found: Vec<Vec<SqlParam>> = run
            .statements
            .iter()
            .map(|st| {
                let params = find_params(&st.text, driver, numbered);
                numbered += params.iter().filter(|p| p.positional).count();
                params
            })
            .collect();
        let mut names: Vec<String> = Vec::new();
        for p in found.iter().flatten() {
            if !names.contains(&p.name) {
                names.push(p.name.clone());
            }
        }
        if names.is_empty() {
            if explain {
                self.start_explain(run, &mut out)
            } else {
                self.start_run(run, &mut out)
            }
            return out;
        }
        let values = names.iter().map(|n| self.param_values.get(n).cloned().unwrap_or_default()).collect();
        let ticket = self.ticket();
        self.parked.insert(ticket, Parked::Params { run, found, explain });
        out.push(Command::Ask { ticket, prompt: Prompt::Params { names, values } });
        out
    }

    fn start_run(&mut self, run: ConsoleRun, out: &mut Vec<Command>) {
        if let Some(&prev) = self.running.get(&run.path) {
            out.push(Command::Cancel(prev));
        }
        let schema = self.console_schema(&run.path);
        if schema.is_empty() && !self.completion.contains_key(&run.target) {
            // The connection's default schema comes with the completion model.
            let (path, target) = (run.path.clone(), run.target.clone());
            let ticket = self.issue(DbOp::Completion { target }, None, Then::Completion { run: Some(run) }, out);
            self.running.insert(path, ticket);
            return;
        }
        self.execute_run(run, schema, out);
    }

    fn execute_run(&mut self, run: ConsoleRun, schema: String, out: &mut Vec<Command>) {
        let page_size = self.max_rows;
        let was_connected = self.connected.contains(&run.target);
        let op = DbOp::Execute {
            target: run.target.clone(),
            statements: run.statements,
            skip: 0,
            max_rows: page_size,
            schema: (!schema.is_empty()).then(|| schema.clone()),
        };
        let kind = RunKind::Console { path: run.path.clone(), page_size };
        let ticket = self.issue(op, None, Then::Run { kind, target: run.target, schema, was_connected }, out);
        self.running.insert(run.path, ticket);
    }

    fn start_explain(&mut self, run: ConsoleRun, out: &mut Vec<Command>) {
        let Some(statement) = run.statements.into_iter().next() else { return };
        let schema = self.console_schemas.get(&run.path).cloned().unwrap_or_default();
        self.explain(Some(run.path), None, run.target, statement, schema, out);
    }

    fn explain(
        &mut self,
        path: Option<String>,
        tab: Option<u64>,
        target: String,
        statement: SqlStatement,
        schema: String,
        out: &mut Vec<Command>,
    ) {
        let op = DbOp::Explain {
            target: target.clone(),
            statement: statement.text.clone(),
            schema: (!schema.is_empty()).then(|| schema.clone()),
        };
        self.issue(op, None, Then::Explain { path, tab, target, statement, schema }, out);
    }

    /// Cancels the running statement of console `path`.
    pub fn cancel_console(&self, path: &str) -> Vec<Command> {
        self.running.get(path).map(|&t| Command::Cancel(t)).into_iter().collect()
    }

    /// Runs statements the tree's object actions generated (Drop, Rename, Modify Table…) on
    /// `target`, logging them; [`Command::SqlDone`] with the returned job tells how it went.
    pub fn run_sql(&mut self, target: &str, texts: &[String]) -> (u64, Vec<Command>) {
        let statements = texts
            .iter()
            .map(|t| {
                let t = t.trim();
                let text = t.strip_suffix(';').unwrap_or(t).to_owned();
                SqlStatement { from: 0, to: len16(&text), text }
            })
            .collect();
        self.run_statements(target, statements, None)
    }

    /// Run SQL Script…: every statement of `text` (a file's contents) on `target`, the Output tab
    /// shown; success notifies with the count.
    pub fn run_script(&mut self, target: &str, text: &str) -> (u64, Vec<Command>) {
        let statements = statements_in(text, 0, len16(text), self.driver_of(target));
        let count = statements.len();
        let texts: Vec<SqlStatement> = statements
            .into_iter()
            .map(|s| {
                let t = s.text.trim();
                let text = t.strip_suffix(';').unwrap_or(t).to_owned();
                SqlStatement { from: 0, to: len16(&text), text }
            })
            .collect();
        let (job, mut out) = self.run_statements(target, texts, Some(count));
        out.insert(0, Command::ShowResults);
        out.insert(0, Command::Results(ResultsUpdate::Select(None)));
        (job, out)
    }

    fn run_statements(
        &mut self,
        target: &str,
        statements: Vec<SqlStatement>,
        script: Option<usize>,
    ) -> (u64, Vec<Command>) {
        let mut out = Vec::new();
        let job = self.next_job;
        self.next_job += 1;
        let was_connected = self.connected.contains(target);
        let max_rows = self.max_rows;
        let op = DbOp::Execute { target: target.to_owned(), statements, skip: 0, max_rows, schema: None };
        let kind = RunKind::Sql { job, script };
        let then = Then::Run { kind, target: target.to_owned(), schema: String::new(), was_connected };
        self.issue(op, None, then, &mut out);
        (job, out)
    }

    /// Logging and bookkeeping after statements ran.
    fn after_run(
        &mut self,
        target: &str,
        outcomes: &[StatementOutcome],
        schema: &str,
        was_connected: bool,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) {
        if !was_connected {
            self.push_log(OutputKind::Info, "Connected".into(), None, env, out);
        }
        let prompt = if schema.is_empty() { self.prompt_of(target) } else { schema.to_owned() };
        let label = self.target_label(target);
        for o in outcomes {
            self.push_log(OutputKind::Statement, o.statement.text.clone(), Some(format!("{prompt}>")), env, out);
            match &o.error {
                Some(e) => self.push_log(OutputKind::Error, format!("[{label}] {e}"), None, env, out),
                None => self.push_log(OutputKind::Result, outcome_summary(o), None, env, out),
            }
        }
        if self.manual_commit.contains(target)
            && outcomes.iter().any(|o| o.error.is_none() && !READS.is_match(&o.statement.text))
        {
            self.pending_tx.insert(target.to_owned());
        }
        if outcomes.iter().any(|o| o.error.is_none() && SCHEMA_CHANGE.is_match(&o.statement.text)) {
            self.start_refresh(target, out);
        }
    }

    #[expect(clippy::too_many_arguments, reason = "the continuation's fields and the answer")]
    fn run_finished(
        &mut self,
        kind: RunKind,
        target: String,
        schema: String,
        was_connected: bool,
        answer: Answer,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) {
        let cancelled = answer.cancelled();
        let outcomes = match answer {
            Answer::Outcomes(outcomes) => Ok(outcomes),
            other => Err(other.message()),
        };
        if let Ok(outcomes) = &outcomes {
            self.after_run(&target, outcomes, &schema, was_connected, env, out);
        }
        match kind {
            RunKind::Console { path, page_size } => {
                self.running.remove(&path);
                match outcomes {
                    Ok(outcomes) => self.console_finished(&path, &target, &schema, page_size, outcomes, env, out),
                    Err(_) if cancelled => {
                        self.push_log(OutputKind::Info, "Cancelled".into(), None, env, out);
                        // The backend dropped the connection, and its transaction with it.
                        if self.pending_tx.remove(&target) {
                            self.push_log(OutputKind::Info, "Transaction rolled back".into(), None, env, out);
                        }
                    }
                    Err(message) => {
                        self.push_log(OutputKind::Error, message.clone(), None, env, out);
                        Self::notify(out, Level::Error, "Cannot execute".into(), Some(message), Some("db-execute"));
                    }
                }
            }
            RunKind::Page { tab, page, page_size, rerun } => {
                let Some(meta) = self.results.iter_mut().find(|r| r.id == tab) else { return };
                meta.busy = false;
                match outcomes {
                    Ok(outcomes) => {
                        let Some(outcome) = outcomes.into_iter().next() else {
                            out.push(Command::Results(ResultsUpdate::Running { id: tab, running: false }));
                            return;
                        };
                        match outcome.result {
                            Some(result) => {
                                let reset_total = rerun || page_size != meta.page_size;
                                (meta.page, meta.page_size) = (page, page_size);
                                out.push(Command::Results(ResultsUpdate::Update { id: tab, result, page, page_size }));
                                if reset_total {
                                    meta.total = None;
                                    out.push(Command::Results(ResultsUpdate::Total { id: tab, total: None }));
                                }
                            }
                            None => out.push(Command::Results(ResultsUpdate::Running { id: tab, running: false })),
                        }
                        if outcome.error.is_some() {
                            out.push(Command::Results(ResultsUpdate::Select(None)));
                        }
                    }
                    Err(message) => {
                        out.push(Command::Results(ResultsUpdate::Running { id: tab, running: false }));
                        if !cancelled {
                            self.push_log(OutputKind::Error, message, None, env, out);
                        }
                    }
                }
            }
            RunKind::Count { tab, last } => {
                let Some(meta) = self.results.iter_mut().find(|r| r.id == tab) else { return };
                meta.busy = false;
                let known = match outcomes {
                    Ok(outcomes) => outcomes.into_iter().next().and_then(|o| {
                        if o.error.is_some() {
                            return None;
                        }
                        let cell = o.result?.rows.into_iter().next()?.into_iter().next()??;
                        js_number(&cell).filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64)
                    }),
                    Err(message) => {
                        if !cancelled {
                            self.push_log(OutputKind::Error, message, None, env, out);
                        }
                        None
                    }
                };
                let Some(meta) = self.results.iter_mut().find(|r| r.id == tab) else { return };
                match known {
                    Some(total) => {
                        meta.total = Some(total);
                        let page_size = meta.page_size.max(1);
                        out.push(Command::Results(ResultsUpdate::Total { id: tab, total: Some(total) }));
                        if last {
                            let last_page = total.div_ceil(page_size).saturating_sub(1);
                            out.extend(self.load_result_page(tab, last_page, page_size, false));
                        }
                    }
                    None => out.push(Command::Results(ResultsUpdate::Running { id: tab, running: false })),
                }
            }
            RunKind::Sql { job, script } => {
                let ok = match outcomes {
                    Ok(outcomes) => match outcomes.iter().find(|o| o.error.is_some()) {
                        Some(failed) => {
                            let title = format!("[{}] {}", self.target_label(&target), failed.statement.text);
                            Self::notify(out, Level::Error, title, failed.error.clone(), None);
                            false
                        }
                        None => true,
                    },
                    Err(message) => {
                        if !cancelled {
                            let title = match script {
                                Some(_) => "Cannot run the script".to_owned(),
                                None => format!("Cannot execute on {}", self.target_label(&target)),
                            };
                            Self::notify(out, Level::Error, title, Some(message), None);
                        }
                        false
                    }
                };
                if ok && let Some(n) = script {
                    let title = format!("Script finished: {n} statement{}", if n == 1 { "" } else { "s" });
                    Self::notify(out, Level::Info, title, None, None);
                }
                out.push(Command::SqlDone { job, ok });
            }
        }
    }

    #[expect(clippy::too_many_arguments, reason = "the run's context and its outcomes")]
    fn console_finished(
        &mut self,
        path: &str,
        target: &str,
        schema: &str,
        page_size: u64,
        outcomes: Vec<StatementOutcome>,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) {
        let failed = outcomes.iter().any(|o| o.error.is_some());
        let marks = outcomes
            .iter()
            .map(|o| RunMark {
                from: o.statement.from,
                to: o.statement.to,
                error: o.error.clone(),
                elapsed_ms: o.result.as_ref().map(|r| r.elapsed_ms),
            })
            .collect();
        let banner = outcomes.iter().find_map(|o| o.error.as_ref()).map(|e| {
            let name = self.source_of(target).map_or("", |s| s.name.as_str());
            format!("[{name}] {e}")
        });
        let source = self.source_of(target).cloned();
        let driver = source.as_ref().map_or(DbDriver::Sqlite, |s| s.driver);
        let mut shown = 0;
        let mut last = None;
        for o in outcomes {
            let Some(result) = o.result.filter(|r| !r.columns.is_empty()) else { continue };
            let n = self.next_tab;
            self.next_tab += 1;
            let title = env.i18n.tx_template(&format!("Result {n}"));
            let mut tab = ResultTab::new(n, &title, target, driver, &o.statement.text, result);
            tab.source_label = self.target_label(target);
            tab.origin.read_only_source = source.as_ref().is_some_and(|s| s.read_only);
            tab.origin.schema = Some(schema.to_owned());
            tab.origin.model = self.completion.get(target).cloned();
            tab.page_size = page_size;
            tab.pageable = is_read_only_query(&o.statement.text, driver);
            self.results.push(ResultMeta {
                id: n,
                path: path.to_owned(),
                target: target.to_owned(),
                schema: schema.to_owned(),
                statement: o.statement,
                plan: false,
                busy: false,
                page: 0,
                page_size,
                total: None,
            });
            out.push(Command::Results(ResultsUpdate::Show(Box::new(tab))));
            shown += 1;
            last = Some(n);
        }
        self.trim_results(out);
        if failed || shown == 0 {
            out.push(Command::Results(ResultsUpdate::Select(None)));
        } else {
            out.push(Command::Results(ResultsUpdate::Select(last)));
        }
        out.push(Command::ShowResults);
        out.push(Command::MarkOutcomes { path: path.to_owned(), marks, banner });
    }

    fn trim_results(&mut self, out: &mut Vec<Command>) {
        while self.results.len() > MAX_TABS {
            let old = self.results.remove(0);
            out.push(Command::Results(ResultsUpdate::Close(old.id)));
        }
    }

    #[expect(clippy::too_many_arguments, reason = "the continuation's fields and the answer")]
    fn explained(
        &mut self,
        path: Option<String>,
        tab: Option<u64>,
        target: String,
        statement: SqlStatement,
        schema: String,
        answer: Answer,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) {
        if let Some(id) = tab
            && let Some(meta) = self.results.iter_mut().find(|r| r.id == id)
        {
            meta.busy = false;
        }
        match answer {
            Answer::Plan(plan) => {
                let prompt = format!("{}>", if schema.is_empty() { self.prompt_of(&target) } else { schema.clone() });
                self.push_log(OutputKind::Statement, format!("explain {}", statement.text), Some(prompt), env, out);
                let n = plan.len();
                let nodes = format!("{n} plan node{}", if n == 1 { "" } else { "s" });
                self.push_log(OutputKind::Result, nodes, None, env, out);
                // One Query Plan tab per window.
                let existing = self.results.iter().find(|r| r.plan).map(|r| r.id);
                let id = existing.unwrap_or_else(|| {
                    let n = self.next_tab;
                    self.next_tab += 1;
                    n
                });
                let meta = ResultMeta {
                    id,
                    path: path.unwrap_or_default(),
                    target,
                    schema,
                    statement: statement.clone(),
                    plan: true,
                    busy: false,
                    page: 0,
                    page_size: self.max_rows,
                    total: None,
                };
                match self.results.iter_mut().find(|r| r.id == id) {
                    Some(old) => {
                        // A re-run keeps its console and schema.
                        old.statement = meta.statement;
                    }
                    None => self.results.push(meta),
                }
                self.trim_results(out);
                let title = env.i18n.tx(PLAN_TITLE);
                out.push(Command::Results(ResultsUpdate::Plan { id, title, statement: statement.text, plan }));
                out.push(Command::Results(ResultsUpdate::Select(Some(id))));
                out.push(Command::ShowResults);
            }
            other if other.cancelled() => {
                if let Some(id) = tab {
                    out.push(Command::Results(ResultsUpdate::Running { id, running: false }));
                }
            }
            other => {
                let message = other.message();
                let label = self.target_label(&target);
                self.push_log(OutputKind::Error, format!("[{label}] {message}"), None, env, out);
                match tab {
                    Some(id) => out.push(Command::Results(ResultsUpdate::Running { id, running: false })),
                    None => Self::notify(
                        out,
                        Level::Error,
                        "Cannot explain the statement".into(),
                        Some(message),
                        Some("db-explain"),
                    ),
                }
            }
        }
    }

    // ---- result tabs --------------------------------------------------------------------------

    /// Re-runs the statement of result tab `id` in the schema it ran in, replacing its rows (a plan
    /// tab explains it again).
    pub fn rerun_result(&mut self, id: u64) -> Vec<Command> {
        let Some(meta) = self.results.iter().find(|r| r.id == id).cloned() else { return Vec::new() };
        if meta.busy {
            return Vec::new();
        }
        if meta.plan {
            let mut out = vec![Command::Results(ResultsUpdate::Running { id, running: true })];
            if let Some(m) = self.results.iter_mut().find(|r| r.id == id) {
                m.busy = true;
            }
            let schema = if meta.schema.is_empty() { self.console_schema(&meta.path) } else { meta.schema };
            self.explain(None, Some(id), meta.target, meta.statement, schema, &mut out);
            return out;
        }
        self.load_result_page(id, meta.page, meta.page_size, true)
    }

    /// Shows page `page` of `page_size` rows of result tab `id`: its query runs again and the
    /// backend returns only that page. Pages past the first need a query that only reads;
    /// `rerun` also forgets the counted total.
    pub fn load_result_page(&mut self, id: u64, page: u64, page_size: u64, rerun: bool) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(meta) = self.results.iter().find(|r| r.id == id).cloned() else { return out };
        if meta.busy || meta.plan || (page > 0 && !self.meta_pageable(&meta)) {
            return out;
        }
        let page_size = page_size.max(1);
        if let Some(m) = self.results.iter_mut().find(|r| r.id == id) {
            m.busy = true;
        }
        out.push(Command::Results(ResultsUpdate::Running { id, running: true }));
        let fetch = page_fetch(page, page_size);
        let schema = if meta.schema.is_empty() { self.console_schema(&meta.path) } else { meta.schema };
        let was_connected = self.connected.contains(&meta.target);
        let op = DbOp::Execute {
            target: meta.target.clone(),
            statements: vec![meta.statement],
            skip: fetch.skip,
            max_rows: fetch.max_rows,
            schema: (!schema.is_empty()).then(|| schema.clone()),
        };
        let kind = RunKind::Page { tab: id, page, page_size, rerun };
        let then = Then::Run { kind, target: meta.target, schema, was_connected };
        self.issue(op, None, then, &mut out);
        out
    }

    /// Counts the rows of result tab `id`'s whole query (the pager's "of 501+"); `last` then shows
    /// the last page.
    pub fn count_result(&mut self, id: u64, last: bool) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(meta) = self.results.iter().find(|r| r.id == id).cloned() else { return out };
        if last && let Some(total) = meta.total {
            let last_page = total.div_ceil(meta.page_size.max(1)).saturating_sub(1);
            return self.load_result_page(id, last_page, meta.page_size, false);
        }
        if meta.busy || !self.countable(id) {
            return out;
        }
        let Some(text) = count_query(&meta.statement.text) else { return out };
        if let Some(m) = self.results.iter_mut().find(|r| r.id == id) {
            m.busy = true;
        }
        out.push(Command::Results(ResultsUpdate::Running { id, running: true }));
        let schema = if meta.schema.is_empty() { self.console_schema(&meta.path) } else { meta.schema };
        let was_connected = self.connected.contains(&meta.target);
        let op = DbOp::Execute {
            target: meta.target.clone(),
            statements: vec![SqlStatement { from: 0, to: len16(&text), text }],
            skip: 0,
            max_rows: 1,
            schema: (!schema.is_empty()).then(|| schema.clone()),
        };
        let then = Then::Run { kind: RunKind::Count { tab: id, last }, target: meta.target, schema, was_connected };
        self.issue(op, None, then, &mut out);
        out
    }

    /// The result pane closed tab `id`.
    pub fn result_closed(&mut self, id: u64) {
        self.results.retain(|r| r.id != id);
    }

    // ---- table editors and result edits --------------------------------------------------------

    /// A page of table editor `editor`; an earlier page still loading is cancelled and its answer
    /// dropped. Answers [`Command::TablePage`].
    pub fn load_table_page(&mut self, editor: u64, page: TablePageRequest) -> Vec<Command> {
        let mut out = Vec::new();
        let slot = Slot::TablePage(editor);
        if let Some(&old) = self.slots.get(&slot) {
            out.push(Command::Cancel(old));
        }
        let op = DbOp::TablePage {
            target: page.target,
            schema: page.schema,
            table: page.table,
            filter: page.filter.filter(|f| !f.trim().is_empty()),
            sort: page.sort,
            offset: page.offset,
            limit: page.limit.max(1),
        };
        self.issue(op, Some(slot), Then::TablePage { editor, request: page.request }, &mut out);
        out
    }

    /// Submit of table editor `editor`: the edits in one transaction; answers
    /// [`Command::TableEdited`] and notifies.
    pub fn apply_table_edits(
        &mut self,
        editor: u64,
        target: &str,
        schema: &str,
        table: &str,
        edits: Vec<DbRowEdit>,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        let op =
            DbOp::ApplyEdits { target: target.to_owned(), schema: schema.to_owned(), table: table.to_owned(), edits };
        self.issue(op, None, Then::TableEdits { editor, table: table.to_owned() }, &mut out);
        out
    }

    /// The result pane's Submit of tab `tab` (`ResultsEvent::Submit`); answers
    /// [`ResultsUpdate::Submitted`].
    pub fn submit_result_edits(&mut self, tab: u64, request: SubmitRequest) -> Vec<Command> {
        let mut out = Vec::new();
        let op = DbOp::ApplyEdits {
            target: request.source_id,
            schema: request.schema,
            table: request.table,
            edits: request.edits,
        };
        self.issue(op, None, Then::ResultEdits { tab }, &mut out);
        out
    }

    // ---- transactions -------------------------------------------------------------------------

    /// Tx: Auto / Manual. Leaving manual mode with changes pending asks whether to commit or roll
    /// them back first.
    pub fn set_manual_commit(&mut self, id: &str, manual: bool) -> Vec<Command> {
        let mut out = Vec::new();
        let source = self.source_of(id).cloned();
        if (manual && source.as_ref().is_some_and(|s| s.driver == DbDriver::Clickhouse))
            || manual == self.manual_commit.contains(id)
        {
            return out;
        }
        if !manual && self.pending_tx.contains(id) {
            let Some(source) = source else {
                self.issue(
                    DbOp::FinishTransaction { id: id.to_owned(), commit: false },
                    None,
                    Then::FinishTx { id: id.to_owned(), commit: false, then_auto: true },
                    &mut out,
                );
                return out;
            };
            let ticket = self.ticket();
            self.parked.insert(ticket, Parked::EndTx { id: id.to_owned() });
            out.push(Command::Ask { ticket, prompt: Prompt::EndTransaction { id: id.to_owned(), name: source.name } });
            return out;
        }
        self.issue(
            DbOp::SetManualCommit { id: id.to_owned(), manual },
            None,
            Then::ManualCommit { id: id.to_owned(), manual },
            &mut out,
        );
        out
    }

    /// Commits or rolls back the open transaction of a manual-commit data source.
    pub fn finish_transaction(&mut self, id: &str, commit: bool) -> Vec<Command> {
        let mut out = Vec::new();
        if self.driver_of(id) == DbDriver::Clickhouse {
            return out;
        }
        self.issue(
            DbOp::FinishTransaction { id: id.to_owned(), commit },
            None,
            Then::FinishTx { id: id.to_owned(), commit, then_auto: false },
            &mut out,
        );
        out
    }

    // ---- query history ------------------------------------------------------------------------

    /// Browse Query History of `target` (Paste goes to console `path`); no target notifies.
    pub fn open_history(&mut self, path: Option<&str>, target: Option<&str>) -> Vec<Command> {
        let mut out = Vec::new();
        let target = target.map(str::to_owned).or_else(|| path.and_then(|p| self.console_source(p)));
        let Some(target) = target else {
            let title = "Choose a data source for this console".to_owned();
            Self::notify(&mut out, Level::Info, title, None, Some("db-history"));
            return out;
        };
        self.history = Some(HistoryState {
            target: target.clone(),
            path: path.map(str::to_owned),
            entries: None,
            query: String::new(),
            active: 0,
        });
        out.push(Command::ShowHistory);
        self.issue(DbOp::History { target: target.clone() }, Some(Slot::History(target)), Then::History, &mut out);
        out
    }

    /// The history search changed.
    pub fn set_history_query(&mut self, query: &str) {
        if let Some(h) = &mut self.history {
            h.query = query.to_owned();
            h.active = 0;
        }
    }

    /// Moves the history selection by `delta` rows.
    pub fn move_history(&mut self, delta: isize) {
        if let Some(h) = &mut self.history {
            let last = h.shown().len().saturating_sub(1);
            h.active = h.active.min(last).saturating_add_signed(delta).min(last);
        }
    }

    pub fn select_history(&mut self, index: usize) {
        if let Some(h) = &mut self.history {
            h.active = index;
        }
    }

    /// Delete (the current entry) or Clear History (`None`).
    pub fn remove_history(&mut self, texts: Option<Vec<String>>) -> Vec<Command> {
        let mut out = Vec::new();
        let Some(target) = self.history.as_ref().map(|h| h.target.clone()) else { return out };
        let op = DbOp::RemoveHistory { target, texts: texts.clone() };
        self.issue(op, None, Then::RemoveHistory { texts }, &mut out);
        out
    }

    /// Paste: the current statement over the console's selection, else appended to the target's
    /// console; the dialog closes.
    pub fn paste_history(&mut self) -> Vec<Command> {
        let Some(h) = self.history.take() else { return Vec::new() };
        let Some(text) = h.current().map(|e| e.text.clone()) else { return Vec::new() };
        match h.path {
            Some(path) => vec![Command::PasteIntoConsole { target: h.target, path, text }],
            None => self.open_console_with(&h.target, &text),
        }
    }

    pub fn close_history(&mut self) {
        self.history = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::{CompletionTable, DbOptions, ResultColumn};

    fn env() -> I18n {
        I18n::default()
    }

    fn at(i18n: &I18n, now_ms: u64) -> Env<'_> {
        Env { now_ms, i18n }
    }

    fn source(id: &str, driver: DbDriver) -> DataSource {
        DataSource { id: id.into(), name: "Local".into(), driver, ..DataSource::default() }
    }

    fn model(default_schema: &str) -> CompletionModel {
        CompletionModel {
            default_schema: default_schema.into(),
            tables: vec![CompletionTable {
                schema: "public".into(),
                name: "users".into(),
                kind: DbTableKind::Table,
                columns: Vec::new(),
            }],
            foreign_keys: Vec::new(),
        }
    }

    fn result(rows: &[&[&str]]) -> QueryResult {
        QueryResult {
            statement: "select".into(),
            columns: ["id", "name"]
                .iter()
                .map(|n| ResultColumn { name: (*n).into(), type_name: "text".into() })
                .collect(),
            rows: rows.iter().map(|r| r.iter().map(|c| Some((*c).to_owned())).collect()).collect(),
            affected: None,
            has_more: false,
            elapsed_ms: 1.0,
            execution_ms: 1.0,
        }
    }

    fn outcome(text: &str, result: Option<QueryResult>, error: Option<&str>) -> StatementOutcome {
        StatementOutcome {
            statement: SqlStatement { from: 0, to: len16(text), text: text.into() },
            result,
            error: error.map(str::to_owned),
        }
    }

    /// A store with the sources loaded.
    fn store_with(sources: Vec<DataSource>) -> DbStore {
        let i18n = env();
        let mut store = DbStore::new(None, None);
        let cmds = store.load_sources();
        let ticket = backend(&cmds)[0].0;
        store.answer(ticket, Answer::Sources { sources, connected: Vec::new() }, &at(&i18n, 0));
        store
    }

    fn backend(cmds: &[Command]) -> Vec<(Ticket, DbOp)> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::Backend { ticket, op } => Some((*ticket, op.clone())),
                _ => None,
            })
            .collect()
    }

    fn asks(cmds: &[Command]) -> Vec<(Ticket, Prompt)> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::Ask { ticket, prompt } => Some((*ticket, prompt.clone())),
                _ => None,
            })
            .collect()
    }

    fn shown_tabs(cmds: &[Command]) -> Vec<ResultTab> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::Results(ResultsUpdate::Show(tab)) => Some((**tab).clone()),
                _ => None,
            })
            .collect()
    }

    fn execute_schema(op: &DbOp) -> Option<String> {
        match op {
            DbOp::Execute { schema, .. } => schema.clone(),
            other => panic!("not an execute: {other:?}"),
        }
    }

    fn texts(store: &DbStore) -> Vec<String> {
        store.output().iter().map(|l| l.text.clone()).collect()
    }

    #[test]
    fn records_the_selected_schema_or_connection_default() {
        for schema in ["s2", ""] {
            let i18n = env();
            let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
            store.completion.insert("pg".into(), model("public"));
            store.bind_console("console.sql", Some("pg"));
            store.set_console_schema("console.sql", Some(schema));
            let cmds = store.execute_console("console.sql", "SELECT * FROM users", 0, 0);
            let (ticket, op) = backend(&cmds).pop().expect("the statement runs");
            let expected = if schema.is_empty() { "public" } else { schema };
            assert_eq!(execute_schema(&op).as_deref(), Some(expected));
            let answer = Answer::Outcomes(vec![outcome("SELECT * FROM users", Some(result(&[&["1", "Ann"]])), None)]);
            let cmds = store.answer(ticket, answer, &at(&i18n, 1));
            let tabs = shown_tabs(&cmds);
            assert_eq!(tabs.len(), 1);
            assert_eq!(tabs[0].origin.schema.as_deref(), Some(expected));
        }
    }

    #[test]
    fn records_the_connection_default_once_the_completion_model_loads() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        store.bind_console("console.sql", Some("pg"));
        let cmds = store.execute_console("console.sql", "SELECT * FROM users", 0, 0);
        let (ticket, op) = backend(&cmds).pop().expect("the model loads first");
        assert_eq!(op, DbOp::Completion { target: "pg".into() });
        assert!(store.is_running("console.sql"));
        let cmds = store.answer(ticket, Answer::Completion(model("public")), &at(&i18n, 1));
        let (_, op) = backend(&cmds).pop().expect("then the statement runs");
        assert_eq!(execute_schema(&op).as_deref(), Some("public"));
    }

    #[test]
    fn uses_the_execution_schema_even_after_the_console_selection_changes() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        store.completion.insert("pg".into(), model("public"));
        store.bind_console("console.sql", Some("pg"));
        store.set_console_schema("console.sql", Some("s2"));
        let cmds = store.execute_console("console.sql", "SELECT * FROM users", 0, 0);
        let (ticket, _) = backend(&cmds)[0].clone();
        let answer = Answer::Outcomes(vec![outcome("SELECT * FROM users", Some(result(&[&["1", "Ann"]])), None)]);
        let tab = shown_tabs(&store.answer(ticket, answer, &at(&i18n, 1)))[0].id;
        store.set_console_schema("console.sql", Some("s1"));
        let cmds = store.rerun_result(tab);
        let (ticket, op) = backend(&cmds)[0].clone();
        assert_eq!(execute_schema(&op).as_deref(), Some("s2"));
        let answer = Answer::Outcomes(vec![outcome("SELECT * FROM users", Some(result(&[&["1", "Bob"]])), None)]);
        let cmds = store.answer(ticket, answer, &at(&i18n, 2));
        // The refreshed rows replace the same tab.
        assert!(cmds.iter().any(|c| matches!(c, Command::Results(ResultsUpdate::Update { id, .. }) if *id == tab)));
    }

    #[test]
    fn drops_the_answer_of_a_superseded_schema_load() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let first = backend(&store.tree_load(&TreeLoad::Schemas { target: "pg".into() }))[0].0;
        let second = backend(&store.tree_load(&TreeLoad::Schemas { target: "pg".into() }))[0].0;
        assert!(store.answer(first, Answer::Schemas(vec!["old".into()]), &at(&i18n, 1)).is_empty());
        let cmds = store.answer(second, Answer::Schemas(vec!["public".into()]), &at(&i18n, 2));
        assert!(cmds.iter().any(|c| matches!(
            c,
            Command::Tree(TreeUpdate::Schemas { result: Ok(names), .. }) if names == &["public".to_owned()]
        )));
        assert_eq!(store.schemas("pg"), Some(&Loaded::Ok(vec!["public".to_owned()])));
        assert!(store.connected().contains("pg"));
    }

    #[test]
    fn drops_the_answer_of_an_older_source_read() {
        let i18n = env();
        let mut store = DbStore::new(None, None);
        let first = backend(&store.load_sources())[0].0;
        let second = backend(&store.load_sources())[0].0;
        let old = Answer::Sources { sources: vec![source("old", DbDriver::Sqlite)], connected: Vec::new() };
        assert!(store.answer(first, old, &at(&i18n, 1)).is_empty());
        assert!(!store.loaded());
        let new = Answer::Sources { sources: vec![source("pg", DbDriver::Postgres)], connected: vec!["pg".into()] };
        store.answer(second, new, &at(&i18n, 2));
        assert_eq!(store.sources()[0].id, "pg");
        assert!(store.connected().contains("pg"));
    }

    #[test]
    fn drops_loads_a_disconnect_forgot() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let load = backend(&store.tree_load(&TreeLoad::Tables { target: "pg".into(), schema: "public".into() }))[0].0;
        let cmds = store.disconnect("pg");
        let (ticket, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::Disconnect { target: "pg".into() });
        store.answer(ticket, Answer::Done, &at(&i18n, 1));
        let answer = Answer::Tables { tables: Vec::new(), objects: None };
        assert!(store.answer(load, answer, &at(&i18n, 2)).is_empty());
        assert_eq!(store.tables("pg", "public"), None);
    }

    #[test]
    fn asks_for_the_password_and_runs_again() {
        let i18n = env();
        let mut store = store_with(vec![DataSource { save_password: true, ..source("pg", DbDriver::Postgres) }]);
        store.completion.insert("pg".into(), model("public"));
        let cmds = store.execute_console("console.sql", "select 1", 0, 0);
        let (ticket, _) = backend(&cmds)[0].clone();
        let denied = IdeError::new(ErrorKind::PermissionDenied, "password authentication failed");
        let cmds = store.answer(ticket, Answer::Failed(denied), &at(&i18n, 1));
        let (ask, prompt) = asks(&cmds).pop().expect("the password is asked for");
        assert!(matches!(prompt, Prompt::Password { .. }));
        let cmds =
            store.prompt_answered(ask, PromptAnswer::Secret { value: "secret".into(), save: true }, &at(&i18n, 2));
        let (set, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::SetPassword { id: "pg".into(), password: Some("secret".into()), save: true });
        let cmds = store.answer(set, Answer::Done, &at(&i18n, 3));
        let (retry, op) = backend(&cmds)[0].clone();
        assert!(matches!(op, DbOp::Execute { .. }));
        assert!(store.is_running("console.sql"));
        let answer = Answer::Outcomes(vec![outcome("select 1", Some(result(&[&["1", "x"]])), None)]);
        let cmds = store.answer(retry, answer, &at(&i18n, 4));
        assert_eq!(shown_tabs(&cmds).len(), 1);
        assert!(!store.is_running("console.sql"));
        assert_eq!(texts(&store)[0], "Connected");
    }

    #[test]
    fn gives_up_when_the_password_prompt_is_cancelled() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let ticket = backend(&store.connect("pg"))[0].0;
        let denied = IdeError::new(ErrorKind::PermissionDenied, "password required");
        let ask = asks(&store.answer(ticket, Answer::Failed(denied), &at(&i18n, 1)))[0].0;
        let cmds = store.prompt_answered(ask, PromptAnswer::Cancel, &at(&i18n, 2));
        assert!(cmds.iter().any(|c| matches!(c, Command::Notify(n) if n.title == "Cannot connect to Local")));
        assert!(!store.connected().contains("pg"));
    }

    #[test]
    fn trusts_an_unknown_host_key_and_connects_again() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let ticket = backend(&store.connect("pg"))[0].0;
        let unknown = IdeError::new(ErrorKind::PermissionDenied, "Unknown SSH host key for db.example");
        let (ask, prompt) = asks(&store.answer(ticket, Answer::Failed(unknown), &at(&i18n, 1)))[0].clone();
        assert!(matches!(prompt, Prompt::TrustHost { .. }));
        let (trust, op) = backend(&store.prompt_answered(ask, PromptAnswer::Confirm, &at(&i18n, 2)))[0].clone();
        assert_eq!(op, DbOp::TrustSshHost { id: "pg".into() });
        let (again, op) = backend(&store.answer(trust, Answer::Done, &at(&i18n, 3)))[0].clone();
        assert_eq!(op, DbOp::Connect { target: "pg".into() });
        store.answer(again, Answer::Done, &at(&i18n, 4));
        assert!(store.connected().contains("pg"));
    }

    #[test]
    fn asks_for_query_parameters_and_binds_them() {
        let i18n = env();
        let mut store = store_with(vec![source("lite", DbDriver::Sqlite)]);
        store.completion.insert("lite".into(), model("main"));
        let cmds = store.execute_console("console.sql", "select * from t where id = :id", 0, 0);
        let (ask, prompt) = asks(&cmds).pop().expect("the parameters are asked for");
        assert_eq!(prompt, Prompt::Params { names: vec![":id".into()], values: vec![String::new()] });
        let values = HashMap::from([(":id".to_owned(), "7".to_owned())]);
        let cmds = store.prompt_answered(ask, PromptAnswer::Params(values), &at(&i18n, 1));
        match &backend(&cmds)[0].1 {
            DbOp::Execute { statements, .. } => assert_eq!(statements[0].text, "select * from t where id = 7"),
            other => panic!("not an execute: {other:?}"),
        }
        // The value is offered next time.
        let cmds = store.execute_console("console.sql", "select :id", 0, 0);
        assert_eq!(asks(&cmds)[0].1, Prompt::Params { names: vec![":id".into()], values: vec!["7".into()] });
    }

    #[test]
    fn cancels_the_running_statement_and_logs_it() {
        let i18n = env();
        let mut store = store_with(vec![source("lite", DbDriver::Sqlite)]);
        store.completion.insert("lite".into(), model("main"));
        let ticket = backend(&store.execute_console("console.sql", "select 1", 0, 0))[0].0;
        let cmds = store.cancel_console("console.sql");
        assert!(matches!(cmds.as_slice(), [Command::Cancel(t)] if *t == ticket));
        store.answer(ticket, Answer::Failed(IdeError::cancelled()), &at(&i18n, 1));
        assert_eq!(texts(&store), vec!["Cancelled".to_owned()]);
        assert!(!store.is_running("console.sql"));
    }

    #[test]
    fn cancels_the_previous_run_of_the_console() {
        let mut store = store_with(vec![source("lite", DbDriver::Sqlite)]);
        store.completion.insert("lite".into(), model("main"));
        let first = backend(&store.execute_console("console.sql", "select 1", 0, 0))[0].0;
        let cmds = store.execute_console("console.sql", "select 2", 0, 0);
        assert!(cmds.iter().any(|c| matches!(c, Command::Cancel(t) if *t == first)));
    }

    #[test]
    fn disconnects_connections_idle_past_their_auto_disconnect() {
        let i18n = env();
        let options = DbOptions { auto_disconnect_secs: Some(60), ..DbOptions::default() };
        let mut store = store_with(vec![DataSource { options, ..source("pg", DbDriver::Postgres) }]);
        let ticket = backend(&store.connect("pg"))[0].0;
        store.answer(ticket, Answer::Done, &at(&i18n, 1_000));
        assert!(store.tick(&at(&i18n, 30_000)).is_empty());
        let cmds = store.tick(&at(&i18n, 61_000));
        let (ticket, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::Disconnect { target: "pg".into() });
        store.answer(ticket, Answer::Done, &at(&i18n, 61_100));
        assert!(!store.connected().contains("pg"));
        assert_eq!(texts(&store), vec!["Local: disconnected after being idle".to_owned()]);
    }

    #[test]
    fn keeps_a_running_console_connected() {
        let i18n = env();
        let options = DbOptions { auto_disconnect_secs: Some(60), ..DbOptions::default() };
        let mut store = store_with(vec![DataSource { options, ..source("pg", DbDriver::Postgres) }]);
        store.completion.insert("pg".into(), model("public"));
        store.bind_console("console.sql", Some("pg"));
        let ticket = backend(&store.connect("pg"))[0].0;
        store.answer(ticket, Answer::Done, &at(&i18n, 0));
        store.execute_console("console.sql", "select pg_sleep(100)", 0, 0);
        assert!(store.tick(&at(&i18n, 120_000)).is_empty());
    }

    #[test]
    fn pages_only_queries_that_read() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        store.completion.insert("pg".into(), model("public"));
        let text = "insert into t values (1) returning id";
        let ticket = backend(&store.execute_console("console.sql", text, 0, 0))[0].0;
        let answer = Answer::Outcomes(vec![outcome(text, Some(result(&[&["1", "x"]])), None)]);
        let tab = shown_tabs(&store.answer(ticket, answer, &at(&i18n, 1)))[0].id;
        assert!(!store.pageable(tab));
        assert!(store.load_result_page(tab, 1, 500, false).is_empty());
    }

    #[test]
    fn counts_the_rows_then_shows_the_last_page() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        store.completion.insert("pg".into(), model("public"));
        let text = "select * from users";
        let ticket = backend(&store.execute_console("console.sql", text, 0, 0))[0].0;
        let answer = Answer::Outcomes(vec![outcome(text, Some(result(&[&["1", "x"]])), None)]);
        let tab = shown_tabs(&store.answer(ticket, answer, &at(&i18n, 1)))[0].id;
        let (count, op) = backend(&store.count_result(tab, true))[0].clone();
        match op {
            DbOp::Execute { statements, max_rows, .. } => {
                assert_eq!(statements[0].text, "select count(*) from (\nselect * from users\n) q");
                assert_eq!(max_rows, 1);
            }
            other => panic!("not an execute: {other:?}"),
        }
        let total = QueryResult { rows: vec![vec![Some("1201".into())]], ..result(&[]) };
        let cmds = store.answer(count, Answer::Outcomes(vec![outcome("count", Some(total), None)]), &at(&i18n, 2));
        assert!(cmds.iter().any(|c| matches!(c, Command::Results(ResultsUpdate::Total { total: Some(1201), .. }))));
        match &backend(&cmds)[0].1 {
            DbOp::Execute { skip, max_rows, .. } => assert_eq!((*skip, *max_rows), (1000, 500)),
            other => panic!("not an execute: {other:?}"),
        }
    }

    #[test]
    fn marks_a_manual_transaction_pending_and_asks_before_auto_commit() {
        let i18n = env();
        let options = DbOptions { transaction: DbTxMode::Manual, ..DbOptions::default() };
        let mut store = store_with(vec![DataSource { options, ..source("pg", DbDriver::Postgres) }]);
        store.completion.insert("pg".into(), model("public"));
        let ticket = backend(&store.execute_console("console.sql", "update t set a = 1", 0, 0))[0].0;
        let answer = Answer::Outcomes(vec![outcome("update t set a = 1", None, None)]);
        let cmds = store.answer(ticket, answer, &at(&i18n, 1));
        // Connecting applies the data source's manual commit.
        let (manual, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::SetManualCommit { id: "pg".into(), manual: true });
        store.answer(manual, Answer::Done, &at(&i18n, 2));
        assert!(store.manual_commit("pg"));
        let ticket = backend(&store.execute_console("console.sql", "update t set a = 2", 0, 0))[0].0;
        store.answer(ticket, Answer::Outcomes(vec![outcome("update t set a = 2", None, None)]), &at(&i18n, 3));
        assert!(store.pending_transaction("pg"));
        let (ask, prompt) = asks(&store.set_manual_commit("pg", false))[0].clone();
        assert_eq!(prompt, Prompt::EndTransaction { id: "pg".into(), name: "Local".into() });
        let (finish, op) = backend(&store.prompt_answered(ask, PromptAnswer::Commit(true), &at(&i18n, 4)))[0].clone();
        assert_eq!(op, DbOp::FinishTransaction { id: "pg".into(), commit: true });
        let (auto, op) = backend(&store.answer(finish, Answer::Done, &at(&i18n, 5)))[0].clone();
        assert_eq!(op, DbOp::SetManualCommit { id: "pg".into(), manual: false });
        store.answer(auto, Answer::Done, &at(&i18n, 6));
        assert!(!store.manual_commit("pg") && !store.pending_transaction("pg"));
        assert!(texts(&store).contains(&"Transaction committed".to_owned()));
    }

    #[test]
    fn saves_a_source_with_its_password_then_reads_the_sources() {
        let i18n = env();
        let mut store = store_with(Vec::new());
        let new = DataSource { save_password: false, ..source("pg", DbDriver::Postgres) };
        let cmds = store.save_source(new.clone(), SecretChange::Set("pw".into()), SecretChange::Keep);
        let (save, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::SaveSources(vec![new.clone()]));
        let (password, op) = backend(&store.answer(save, Answer::Done, &at(&i18n, 1)))[0].clone();
        assert_eq!(op, DbOp::SetPassword { id: "pg".into(), password: Some("pw".into()), save: false });
        let cmds = store.answer(password, Answer::Done, &at(&i18n, 2));
        assert_eq!(backend(&cmds)[0].1, DbOp::LoadSources);
        assert!(cmds.iter().any(|c| matches!(c, Command::SourceSaved { id } if id == "pg")));
    }

    /// The requests and answers that carry a password or SSH secret print it redacted.
    #[test]
    fn requests_and_answers_that_carry_a_secret_never_print_it() {
        const PASSWORD: &str = "Zq7#hunter";
        const SSH: &str = "tunnel-Kx9";
        let i18n = env();
        let mut store = store_with(Vec::new());
        let ssh = ide_db::DbSsh { enabled: true, save_secret: true, ..Default::default() };
        let new = DataSource { ssh: Some(ssh), ..source("pg", DbDriver::Postgres) };
        let mut printed = Vec::new();
        let cmds = store.save_source(new, SecretChange::Set(PASSWORD.into()), SecretChange::Set(SSH.into()));
        printed.push(format!("{cmds:?}"));
        let cmds = store.answer(backend(&cmds)[0].0, Answer::Done, &at(&i18n, 1));
        printed.push(format!("{cmds:?}"));
        let ops: Vec<DbOp> = backend(&cmds).into_iter().map(|(_, op)| op).collect();
        assert!(
            ops.iter().any(|op| matches!(op, DbOp::SetPassword { password: Some(p), .. } if p.expose() == PASSWORD))
        );
        for (ticket, _) in backend(&cmds) {
            let cmds = store.answer(ticket, Answer::Done, &at(&i18n, 2));
            printed.push(format!("{cmds:?}"));
            let more: Vec<DbOp> = backend(&cmds).into_iter().map(|(_, op)| op).collect();
            printed.push(format!("{more:?}"));
        }
        printed.push(format!("{:?}", PromptAnswer::Secret { value: PASSWORD.into(), save: true }));
        printed.push(format!("{:?}", SecretChange::Set(SSH.into())));
        printed.push(format!("{:?}", DbOp::SetSshSecret { id: "pg".into(), secret: Some(SSH.into()), save: false }));
        let all = printed.join("\n");
        assert!(all.contains("SetPassword") && all.contains("<redacted>"), "{all}");
        assert!(!all.contains(PASSWORD) && !all.contains(SSH), "{all}");
    }

    #[test]
    fn reports_a_failed_save_to_the_dialog() {
        let i18n = env();
        let mut store = store_with(Vec::new());
        let cmds = store.save_source(source("pg", DbDriver::Postgres), SecretChange::Keep, SecretChange::Keep);
        let cmds = store.answer(backend(&cmds)[0].0, Answer::Failed(IdeError::invalid("read-only")), &at(&i18n, 1));
        assert!(cmds.iter().any(|c| matches!(c, Command::SourceSaveFailed { message, .. } if message == "read-only")));
    }

    #[test]
    fn duplicates_a_source_under_the_next_free_name() {
        let mut store = store_with(vec![
            source("pg", DbDriver::Postgres),
            DataSource { name: "Local [2]".into(), ..source("pg2", DbDriver::Postgres) },
        ]);
        let cmds = store.duplicate_source("pg", "abc-123");
        match &backend(&cmds)[0].1 {
            DbOp::SaveSources(next) => {
                let copy = next.last().expect("the copy is added");
                assert_eq!((copy.id.as_str(), copy.name.as_str()), ("abc-123", "Local [3]"));
            }
            other => panic!("not a save: {other:?}"),
        }
    }

    #[test]
    fn removes_a_connected_source_after_disconnecting_it() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let ticket = backend(&store.connect("pg"))[0].0;
        store.answer(ticket, Answer::Done, &at(&i18n, 0));
        let (ask, _) = asks(&store.remove_source("pg"))[0].clone();
        let (disconnect, op) = backend(&store.prompt_answered(ask, PromptAnswer::Confirm, &at(&i18n, 1)))[0].clone();
        assert_eq!(op, DbOp::Disconnect { target: "pg".into() });
        let cmds = store.answer(disconnect, Answer::Done, &at(&i18n, 2));
        assert_eq!(backend(&cmds)[0].1, DbOp::SaveSources(Vec::new()));
    }

    #[test]
    fn refreshes_the_loaded_levels_after_a_schema_change() {
        let i18n = env();
        let mut store = store_with(vec![source("lite", DbDriver::Sqlite)]);
        store.completion.insert("lite".into(), model("main"));
        let schemas = backend(&store.tree_load(&TreeLoad::Schemas { target: "lite".into() }))[0].0;
        store.answer(schemas, Answer::Schemas(vec!["main".into()]), &at(&i18n, 0));
        let tables = backend(&store.tree_load(&TreeLoad::Tables { target: "lite".into(), schema: "main".into() }))[0].0;
        store.answer(tables, Answer::Tables { tables: Vec::new(), objects: None }, &at(&i18n, 0));
        let ticket = backend(&store.execute_console("console.sql", "create table t (id int)", 0, 0))[0].0;
        let answer = Answer::Outcomes(vec![outcome("create table t (id int)", None, None)]);
        let cmds = store.answer(ticket, answer, &at(&i18n, 1));
        assert!(cmds.iter().any(|c| matches!(c, Command::Tree(TreeUpdate::Forget(t)) if t == "lite")));
        let (schemas, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::Schemas { target: "lite".into() });
        let cmds = store.answer(schemas, Answer::Schemas(vec!["main".into()]), &at(&i18n, 2));
        assert_eq!(backend(&cmds)[0].1, DbOp::Tables { target: "lite".into(), schema: "main".into() });
    }

    #[test]
    fn reveals_a_column_after_loading_the_levels_leading_to_it() {
        let i18n = env();
        let mut store = store_with(vec![source("lite", DbDriver::Sqlite)]);
        let cmds = store.reveal_in_tree("lite", "main", "users", DbTableKind::Table, Some("id"));
        let (t, op) = backend(&cmds)[0].clone();
        assert_eq!(op, DbOp::Schemas { target: "lite".into() });
        let cmds = store.answer(t, Answer::Schemas(vec!["main".into()]), &at(&i18n, 0));
        let (t, _) = backend(&cmds)[0].clone();
        let tables = vec![TableInfo { name: "users".into(), kind: DbTableKind::Table }];
        let cmds = store.answer(t, Answer::Tables { tables, objects: None }, &at(&i18n, 0));
        let (t, op) = backend(&cmds)[0].clone();
        assert!(matches!(op, DbOp::Details { .. }));
        let details =
            TableDetails { columns: Vec::new(), indexes: Vec::new(), foreign_keys: Vec::new(), triggers: Vec::new() };
        let cmds = store.answer(t, Answer::Details(details), &at(&i18n, 0));
        let reveal = cmds.iter().find_map(|c| match c {
            Command::Tree(TreeUpdate::Reveal { expand, key }) => Some((expand.clone(), key.clone())),
            _ => None,
        });
        let (expand, key) = reveal.expect("the column is revealed");
        assert_eq!(expand[0], "lite");
        assert_eq!(expand[1], "lite/main");
        assert_eq!(key, format!("{}/users/columns/id", folder_key("lite/main", table_folder(DbTableKind::Table))));
    }

    #[test]
    fn opens_the_ddl_in_a_file_next_to_the_console() {
        let i18n = env();
        let mut store = store_with(vec![source("lite", DbDriver::Postgres)]);
        let (t, _) = backend(&store.open_ddl("lite", "public", "my table", None))[0].clone();
        let (t, op) = backend(&store.answer(t, Answer::Text("create table x ()".into()), &at(&i18n, 0)))[0].clone();
        assert_eq!(op, DbOp::ConsolePath { target: "lite".into() });
        let cmds = store.answer(t, Answer::Text("/data/consoles/db/lite/console.sql".into()), &at(&i18n, 0));
        let (t, op) = backend(&cmds)[0].clone();
        let path = "/data/consoles/db/lite/public.my_table.sql".to_owned();
        assert_eq!(op, DbOp::WriteFile { path: path.clone(), text: "create table x ()".into() });
        let cmds = store.answer(t, Answer::Done, &at(&i18n, 0));
        assert!(cmds.iter().any(|c| matches!(c, Command::OpenFile { path: p } if *p == path)));
        assert_eq!(store.console_source(&path).as_deref(), Some("lite"));
    }

    #[test]
    fn persists_bindings_and_forgets_the_schema_of_a_rebound_console() {
        let mut store = store_with(vec![source("a", DbDriver::Postgres), source("b", DbDriver::Postgres)]);
        store.bind_console("c.sql", Some("a"));
        store.set_console_schema("c.sql", Some("s1"));
        let cmds = store.bind_console("c.sql", Some("b"));
        let persisted = |k: &str| {
            cmds.iter().rev().find_map(|c| match c {
                Command::Persist { key, value } if *key == k => Some(value.clone()),
                _ => None,
            })
        };
        assert_eq!(persisted(BINDINGS_KEY).as_deref(), Some(r#"{"c.sql":"b"}"#));
        assert_eq!(persisted(CONSOLE_SCHEMAS_KEY).as_deref(), Some("{}"));
        let reopened = DbStore::new(Some(r#"{"c.sql":"b"}"#), Some("not json"));
        assert_eq!(reopened.consoles.get("c.sql").map(String::as_str), Some("b"));
        assert!(reopened.console_schemas.is_empty());
    }

    #[test]
    fn summarises_outcomes() {
        let rows = QueryResult { has_more: true, elapsed_ms: 1500.0, execution_ms: 1200.0, ..result(&[&["1", "a"]]) };
        let query = outcome("select", Some(rows), None);
        assert_eq!(
            outcome_summary(&query),
            "1 row retrieved starting from 1 in 1.5 s (execution: 1.2 s, fetching: 300 ms) (limit reached)"
        );
        let changed = QueryResult { columns: Vec::new(), affected: Some(3.0), ..result(&[]) };
        let dml = outcome("update", Some(changed), None);
        assert_eq!(outcome_summary(&dml), "3 rows affected in 1 ms");
        assert_eq!(outcome_summary(&outcome("x", None, Some("boom"))), "boom");
    }

    #[test]
    fn appends_to_a_console_after_a_blank_line() {
        assert_eq!(console_append("", "select 1\n"), ConsoleAppend { insert: "select 1\n".into(), anchor: 0, head: 8 });
        assert_eq!(console_append("a;", "b"), ConsoleAppend { insert: "\n\nb".into(), anchor: 4, head: 5 });
        assert_eq!(console_append("a;\n", "b"), ConsoleAppend { insert: "\nb".into(), anchor: 4, head: 5 });
        assert_eq!(console_append("a;\n\n", "b"), ConsoleAppend { insert: "b".into(), anchor: 4, head: 5 });
    }

    #[test]
    fn offers_the_console_toolbar_its_targets_and_schemas() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        store.completion.insert("pg".into(), model("public"));
        let cmds = store.ensure_console("c.sql");
        let ops: Vec<DbOp> = backend(&cmds).into_iter().map(|(_, op)| op).collect();
        assert!(ops.contains(&DbOp::Databases { id: "pg".into() }));
        assert!(ops.contains(&DbOp::Schemas { target: "pg".into() }));
        let databases = backend(&cmds).into_iter().find(|(_, op)| matches!(op, DbOp::Databases { .. })).map(|(t, _)| t);
        let answer = Answer::Databases(Databases { current: "app".into(), names: vec!["app".into(), "logs".into()] });
        store.answer(databases.expect("the databases load"), answer, &at(&i18n, 0));
        let bar = store.console_bar("c.sql");
        assert_eq!(bar.bound.as_deref(), Some("pg"));
        assert_eq!(bar.schema, "public");
        assert_eq!(bar.schema_options, vec!["public".to_owned()]);
        let values: Vec<&str> = bar.targets.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(values, vec!["pg", "pg:logs"]);
        assert!(bar.supports_transactions && bar.switchable && !bar.sql_server);
    }

    #[test]
    fn cancels_and_drops_an_older_table_page() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let request = |n: u64| TablePageRequest {
            request: n,
            target: "pg".into(),
            schema: "public".into(),
            table: "users".into(),
            filter: Some(" ".into()),
            sort: Vec::new(),
            offset: 0,
            limit: 500,
        };
        let (first, op) = backend(&store.load_table_page(7, request(1)))[0].clone();
        assert!(matches!(op, DbOp::TablePage { filter: None, .. }), "a blank filter is no filter");
        let cmds = store.load_table_page(7, request(2));
        assert!(cmds.iter().any(|c| matches!(c, Command::Cancel(t) if *t == first)));
        let second = backend(&cmds)[0].0;
        let page = TablePage {
            result: result(&[&["1", "Ann"]]),
            details: TableDetails {
                columns: Vec::new(),
                indexes: Vec::new(),
                foreign_keys: Vec::new(),
                triggers: Vec::new(),
            },
            offset: 0.0,
        };
        assert!(store.answer(first, Answer::TablePage(Box::new(page.clone())), &at(&i18n, 0)).is_empty());
        let cmds = store.answer(second, Answer::TablePage(Box::new(page)), &at(&i18n, 0));
        assert!(cmds.iter().any(|c| matches!(c, Command::TablePage { editor: 7, request: 2, result: Ok(_) })));
        let cmds = store.apply_table_edits(7, "pg", "public", "users", Vec::new());
        let cmds = store.answer(backend(&cmds)[0].0, Answer::Count(2.0), &at(&i18n, 0));
        assert!(cmds.iter().any(|c| matches!(c, Command::Notify(n) if n.title == "users: 2 row(s) changed")));
        assert!(cmds.iter().any(|c| matches!(c, Command::TableEdited { editor: 7, ok: true })));
    }

    #[test]
    fn filters_and_pastes_the_query_history() {
        let i18n = env();
        let mut store = store_with(vec![source("pg", DbDriver::Postgres)]);
        let cmds = store.open_history(Some("c.sql"), None);
        assert!(cmds.iter().any(|c| matches!(c, Command::ShowHistory)));
        let ticket = backend(&cmds)[0].0;
        let entries = vec![
            HistoryEntry { text: "select * from users".into(), time: 2.0 },
            HistoryEntry { text: "delete from logs".into(), time: 1.0 },
        ];
        store.answer(ticket, Answer::History(entries), &at(&i18n, 0));
        store.set_history_query("LOGS");
        assert_eq!(store.history().map(|h| h.shown().len()), Some(1));
        let cmds = store.paste_history();
        assert!(matches!(
            cmds.as_slice(),
            [Command::PasteIntoConsole { path, text, .. }] if path == "c.sql" && text == "delete from logs"
        ));
        assert!(store.history().is_none());
    }
}
