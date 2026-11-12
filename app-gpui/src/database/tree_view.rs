//! The Database tool window: data source → database (a PostgreSQL server) → schema → `tables` /
//! `views` / `materialized views` / `routines` (or `procedures` / `functions`) / `sequences`
//! folders → table → `columns` / `keys` / `foreign keys` / `indexes` / `triggers`, with counts,
//! column types, each data source's icon, colour and connection state. Above the tree, the
//! toolbar (New, Properties, Refresh, Deactivate, Jump to Query Console, Edit Data, DDL) and the
//! quick filter.
//!
//! [`DatabaseTree`] owns what is loaded, expanded and selected. It owns no connection: expanding a
//! node whose level is not loaded asks the window for it ([`TreeEvent::Load`]), and the answer
//! comes back through [`DatabaseTree::databases_loaded`] and its siblings. Everything else the
//! user asks for (open a console, a table's data, a menu) is a [`TreeEvent`] too.

use super::model::{color_token, database_key, shown_names, source_icon, split_target, target_of};
use super::nodes::{DbNode, Folder, Group, InTable, NodeKind, SchemaObjectAt, TableAt};
use super::objects::{ScriptKind, can_drop, can_modify_table, can_rename};
use super::tree::{
    FolderMember, SchemaView, VIEW_OPTIONS_KEY, folder_key, member_key, read_separate_procedures_and_functions,
    view_options_json,
};
use super::vendors::{VENDORS, vendor_by_id};
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_db::{DataSource, Databases, DbDriver, SchemaObjects, TableDetails, TableInfo};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The Database tool window's id.
pub const DATABASE_TOOL_WINDOW: &str = "database";

/// Indentation of one tree level.
const INDENT: f32 = 20.0;

/// A level of the tree as loaded so far.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded<T> {
    Loading,
    Ok(T),
    Error(String),
}

impl<T> Loaded<T> {
    fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(message) => Self::Error(message),
        }
    }

    fn value(&self) -> Option<&T> {
        match self {
            Self::Ok(value) => Some(value),
            Self::Loading | Self::Error(_) => None,
        }
    }
}

/// A schema of a connection target: `(target, schema)`.
type SchemaKey = (String, String);
/// A table of a connection target: `(target, schema, table)`.
type TableKey = (String, String, String);

/// What the tree knows: the data sources and every level loaded below them.
#[derive(Clone, Debug, Default)]
pub struct TreeData {
    pub sources: Vec<DataSource>,
    /// The sources have been read once (the empty state shows only then).
    pub loaded: bool,
    /// Connected targets (`<id>` or `<id>:<database>`).
    pub connected: BTreeSet<String>,
    /// Databases of a server, by data source id.
    pub databases: BTreeMap<String, Loaded<Databases>>,
    /// Schemas, by connection target.
    pub schemas: BTreeMap<String, Loaded<Vec<String>>>,
    pub tables: BTreeMap<SchemaKey, Loaded<Vec<TableInfo>>>,
    /// Routines and sequences; a server refusing their catalog leaves them out.
    pub objects: BTreeMap<SchemaKey, SchemaObjects>,
    pub details: BTreeMap<TableKey, Loaded<TableDetails>>,
}

/// A level the window loads through the backend and hands back to the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeLoad {
    /// Connect the data source, then list its server's databases ([`DatabaseTree::databases_loaded`]).
    Databases { id: String },
    /// Connect the target, then list its schemas ([`DatabaseTree::schemas_loaded`]).
    Schemas { target: String },
    /// The tables and views of a schema, with its routines and sequences ([`DatabaseTree::tables_loaded`]).
    Tables { target: String, schema: String },
    /// A table's columns, keys, indexes and triggers ([`DatabaseTree::details_loaded`]).
    Details { target: String, schema: String, table: String },
}

/// What a toolbar button or a menu row asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeAction {
    OpenConsole,
    Properties,
    Refresh,
    Disconnect,
    JumpToConsole,
    EditData,
    GoToDdl,
    QuickDoc,
    Rename,
    Drop,
    Truncate,
    ModifyTable,
    NewTable,
    NewColumn,
    ImportData,
    CopyReference,
    DuplicateSource,
    DdlToClipboard,
    DdlToConsole,
    RunSqlScript,
    SqlScript(ScriptKind),
    GoToReferencedTable,
    /// The New Data Source dialog, with the preset of this vendor id (`None`: blank).
    NewDataSource(Option<&'static str>),
    DataSourceFromFile,
    DataSourceFromUrl,
    ImportFromClipboard,
    CollapseAll,
    ToggleSeparateProceduresAndFunctions,
}

impl TreeAction {
    /// The row's English title.
    pub fn title(self) -> &'static str {
        match self {
            Self::OpenConsole => "Query Console",
            Self::Properties => "Data Source Properties…",
            Self::Refresh => "Refresh",
            Self::Disconnect => "Deactivate",
            Self::JumpToConsole => "Jump to Query Console…",
            Self::EditData => "Edit Data",
            Self::GoToDdl => "Go to DDL",
            Self::QuickDoc => "Quick Documentation",
            Self::Rename => "Rename…",
            Self::Drop => "Drop…",
            Self::Truncate => "Truncate…",
            Self::ModifyTable => "Modify Table…",
            Self::NewTable => "Table",
            Self::NewColumn => "Column",
            Self::ImportData => "Import Data from File…",
            Self::CopyReference => "Copy Reference",
            Self::DuplicateSource => "Duplicate",
            Self::DdlToClipboard => "Generate DDL to Clipboard",
            Self::DdlToConsole => "Generate DDL to Query Console",
            Self::RunSqlScript => "Run SQL Script…",
            Self::SqlScript(kind) => kind.label(),
            Self::GoToReferencedTable => "Go to Referenced Table",
            Self::NewDataSource(None) => "New Data Source…",
            Self::NewDataSource(Some(vendor)) => vendor_by_id(vendor).map_or("Data Source", |v| v.label),
            Self::DataSourceFromFile => "Data Source from File…",
            Self::DataSourceFromUrl => "Data Source from URL…",
            Self::ImportFromClipboard => "Import from Clipboard",
            Self::CollapseAll => "Collapse All",
            Self::ToggleSeparateProceduresAndFunctions => "Separate Procedures and Functions",
        }
    }

    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::OpenConsole | Self::JumpToConsole | Self::DdlToConsole => "db-console",
            Self::Properties => "db-properties",
            Self::Refresh => "refresh",
            Self::Disconnect => "db-disconnect",
            Self::EditData | Self::NewTable => "db-table",
            Self::GoToDdl => "file-sql",
            Self::QuickDoc => "info",
            Self::Rename | Self::ModifyTable => "edit",
            Self::Drop => "trash",
            Self::NewColumn => "db-column",
            Self::CopyReference | Self::DuplicateSource | Self::DdlToClipboard => "copy",
            Self::RunSqlScript => "run",
            Self::NewDataSource(Some(vendor)) => vendor_by_id(vendor)?.icon,
            Self::NewDataSource(None) => "plus",
            Self::DataSourceFromFile => "folder",
            Self::DataSourceFromUrl => "external-link",
            Self::ImportFromClipboard => "paste",
            Self::CollapseAll => "collapse-all",
            Self::Truncate
            | Self::ImportData
            | Self::SqlScript(_)
            | Self::GoToReferencedTable
            | Self::ToggleSeparateProceduresAndFunctions => return None,
        })
    }
}

/// A menu row's command: the action, on the node with this key (`None`: the tree as a whole).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeCommand {
    pub action: TreeAction,
    pub node: Option<String>,
}

/// What the Database tool window asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeEvent {
    /// Load a level of the tree; the answer goes to the matching `…_loaded` method.
    Load(TreeLoad),
    /// Read the data sources again ([`DatabaseTree::set_sources`]).
    LoadSources,
    /// Open a table's (or view's) data in the table editor.
    OpenTableData(TableAt),
    /// Open the DDL of a routine, trigger or sequence.
    OpenObjectDdl(SchemaObjectAt),
    /// Show [`DatabaseTree::menu_for`] of `node` (`None`: the empty area) here.
    ShowMenu { position: Point<Pixels>, node: Option<String> },
    /// Show [`DatabaseTree::new_menu`] here (the toolbar's New button).
    ShowNewMenu(Point<Pixels>),
    /// Carry out `action` on `node` (`None`: the tree as a whole, e.g. Deactivate every connection).
    Run { action: TreeAction, node: Option<Box<DbNode>> },
}

/// How a row's grey text after the label is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailStyle {
    /// The schema count of a data source, in a small frame.
    Badge,
    /// A count after a database, folder or group.
    Count,
    /// A column's type.
    Type,
    Plain,
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub node: DbNode,
    pub depth: usize,
    /// Index of the parent row.
    pub parent: Option<usize>,
    pub expandable: bool,
    pub expanded: bool,
    /// The placeholder under an expanded node whose children are loading.
    pub loading: bool,
    pub label: String,
    pub detail: Option<String>,
    pub detail_style: DetailStyle,
    pub icon: Option<&'static str>,
    /// A data source with an open connection (a green dot on its icon).
    pub live: bool,
    /// The colour token (without `--`) of the row's data source.
    pub tint: Option<String>,
    /// A data source available in all projects.
    pub global: bool,
    pub error: bool,
}

/// Whether a server data source lists its databases (a PostgreSQL server not limited to one).
pub fn lists_databases(source: &DataSource) -> bool {
    source.driver == DbDriver::Postgres && !source.options.single_database
}

/// A folder's label.
pub fn folder_label(folder: Folder) -> &'static str {
    match folder {
        Folder::Tables => "tables",
        Folder::Views => "views",
        Folder::MaterializedViews => "materialized views",
        Folder::Routines => "routines",
        Folder::Procedures => "procedures",
        Folder::Functions => "functions",
        Folder::Sequences => "sequences",
    }
}

/// A group's label.
pub fn group_label(group: Group) -> &'static str {
    match group {
        Group::Columns => "columns",
        Group::Keys => "keys",
        Group::ForeignKeys => "foreign keys",
        Group::Indexes => "indexes",
        Group::Triggers => "triggers",
    }
}

/// The last part of a group's node key.
fn group_name(group: Group) -> &'static str {
    match group {
        Group::Columns => "columns",
        Group::Keys => "keys",
        Group::ForeignKeys => "foreignKeys",
        Group::Indexes => "indexes",
        Group::Triggers => "triggers",
    }
}

fn primary_key(d: &TableDetails) -> Vec<String> {
    d.columns.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect()
}

/// The groups under a table; empty ones are left out (columns always show).
fn groups_of(d: &TableDetails) -> Vec<Group> {
    let mut groups = vec![Group::Columns];
    if d.columns.iter().any(|c| c.primary_key) {
        groups.push(Group::Keys);
    }
    if !d.foreign_keys.is_empty() {
        groups.push(Group::ForeignKeys);
    }
    if !d.indexes.is_empty() {
        groups.push(Group::Indexes);
    }
    if !d.triggers.is_empty() {
        groups.push(Group::Triggers);
    }
    groups
}

fn group_size(d: &TableDetails, group: Group) -> usize {
    match group {
        Group::Columns => d.columns.len(),
        Group::Keys => usize::from(d.columns.iter().any(|c| c.primary_key)),
        Group::ForeignKeys => d.foreign_keys.len(),
        Group::Indexes => d.indexes.len(),
        Group::Triggers => d.triggers.len(),
    }
}

fn is_expandable(node: &DbNode) -> bool {
    matches!(
        node.kind,
        NodeKind::Source(_)
            | NodeKind::Database { .. }
            | NodeKind::Schema { .. }
            | NodeKind::Folder { .. }
            | NodeKind::Table { .. }
            | NodeKind::Group { .. }
    )
}

fn message_node(parent: &str, suffix: &str, text: &str, error: bool) -> DbNode {
    DbNode { key: format!("{parent}/{suffix}"), kind: NodeKind::Message { text: text.to_owned(), error } }
}

/// The children of a level: `None` while it is not loaded, an error or "Empty" row otherwise.
fn status<T>(key: &str, loaded: Option<&Loaded<T>>, map: impl FnOnce(&T) -> Vec<DbNode>) -> Option<Vec<DbNode>> {
    match loaded? {
        Loaded::Loading => None,
        Loaded::Error(message) => Some(vec![message_node(key, "err", message, true)]),
        Loaded::Ok(value) => {
            let nodes = map(value);
            Some(if nodes.is_empty() { vec![message_node(key, "empty", "Empty", false)] } else { nodes })
        }
    }
}

/// The tree's rows as the loaded data, the quick filter and the view option shape them.
struct Shape<'a> {
    data: &'a TreeData,
    filter: &'a str,
    separate: bool,
}

impl<'a> Shape<'a> {
    fn source_of(&self, target: &str) -> Option<&'a DataSource> {
        let (id, _) = split_target(target);
        self.data.sources.iter().find(|source| source.id == id)
    }

    fn driver(&self, target: &str) -> DbDriver {
        self.source_of(target).map_or(DbDriver::Sqlite, |source| source.driver)
    }

    /// The Schemas tab's choice of schemas.
    fn shown_schemas(&self, target: &str, names: &[String]) -> Vec<String> {
        match self.source_of(target) {
            Some(s) => shown_names(names, Some(s.introspection.all_schemas), Some(&s.introspection.schemas), None),
            None => names.to_vec(),
        }
    }

    /// The Schemas tab's choice of databases; the connected one always stays.
    fn shown_databases(&self, id: &str, databases: &Databases) -> Vec<String> {
        match self.source_of(id) {
            Some(s) => shown_names(
                &databases.names,
                Some(s.introspection.all_databases),
                Some(&s.introspection.databases),
                Some(&databases.current),
            ),
            None => databases.names.clone(),
        }
    }

    fn schema_view(&self, target: &str, schema: &str) -> SchemaView<'a> {
        let key = (target.to_owned(), schema.to_owned());
        SchemaView {
            tables: self.data.tables.get(&key).and_then(Loaded::value).map(Vec::as_slice),
            objects: self.data.objects.get(&key),
            object_filter: self.source_of(target).map_or("", |source| source.introspection.object_filter.as_str()),
            tree_filter: self.filter,
            separate: self.separate,
        }
    }

    fn details(&self, at: &InTable) -> Option<&'a Loaded<TableDetails>> {
        self.data.details.get(&(at.id.clone(), at.schema.clone(), at.table.clone()))
    }

    fn schemas_of(&self, key: &str, target: &str) -> Option<Vec<DbNode>> {
        status(key, self.data.schemas.get(target), |names| {
            self.shown_schemas(target, names)
                .into_iter()
                .filter(|schema| self.schema_view(target, schema).visible(schema))
                .map(|schema| DbNode {
                    key: format!("{target}/{schema}"),
                    kind: NodeKind::Schema { id: target.to_owned(), schema },
                })
                .collect()
        })
    }

    /// The children of `node`; `None` while they are not loaded.
    fn children(&self, node: &DbNode) -> Option<Vec<DbNode>> {
        match &node.kind {
            NodeKind::Source(source) if lists_databases(source) => {
                let id = source.id.as_str();
                status(&node.key, self.data.databases.get(id), |databases| {
                    self.shown_databases(id, databases)
                        .into_iter()
                        .map(|name| DbNode {
                            key: database_key(id, &name),
                            kind: NodeKind::Database {
                                id: target_of(id, Some(&name), Some(&databases.current)),
                                current: name == databases.current,
                                name,
                            },
                        })
                        .collect()
                })
            }
            NodeKind::Source(source) => self.schemas_of(&node.key, &source.id),
            NodeKind::Database { id, .. } => self.schemas_of(&node.key, id),
            NodeKind::Schema { id, schema } => {
                let view = self.schema_view(id, schema);
                status(&node.key, self.data.tables.get(&(id.clone(), schema.clone())), |_| {
                    view.folders()
                        .into_iter()
                        .map(|(folder, _)| DbNode {
                            key: folder_key(&node.key, folder),
                            kind: NodeKind::Folder { id: id.clone(), schema: schema.clone(), folder },
                        })
                        .collect()
                })
            }
            NodeKind::Folder { id, schema, folder } => Some(
                self.schema_view(id, schema)
                    .members(*folder)
                    .into_iter()
                    .map(|member| {
                        let (id, schema) = (id.clone(), schema.clone());
                        let kind = match member {
                            FolderMember::Table(table) => NodeKind::Table { id, schema, table: table.clone() },
                            FolderMember::Routine(routine) => {
                                NodeKind::Routine { id, schema, routine: routine.clone() }
                            }
                            FolderMember::Sequence(sequence) => {
                                NodeKind::Sequence { id, schema, sequence: sequence.clone() }
                            }
                        };
                        DbNode { key: member_key(&node.key, member), kind }
                    })
                    .collect(),
            ),
            NodeKind::Table { id, schema, table } => {
                let at = InTable { id: id.clone(), schema: schema.clone(), table: table.name.clone() };
                match self.details(&at)? {
                    Loaded::Loading => None,
                    Loaded::Error(message) => Some(vec![message_node(&node.key, "err", message, true)]),
                    Loaded::Ok(details) => Some(
                        groups_of(details)
                            .into_iter()
                            .map(|group| DbNode {
                                key: format!("{}/{}", node.key, group_name(group)),
                                kind: NodeKind::Group { at: at.clone(), group },
                            })
                            .collect(),
                    ),
                }
            }
            NodeKind::Group { at, group } => {
                let details = self.details(at)?.value()?;
                let key = |name: &str| format!("{}/{name}", node.key);
                Some(match group {
                    Group::Columns => {
                        let foreign: BTreeSet<&str> =
                            details.foreign_keys.iter().flat_map(|k| k.columns.iter().map(String::as_str)).collect();
                        details
                            .columns
                            .iter()
                            .map(|column| DbNode {
                                key: key(&column.name),
                                kind: NodeKind::Column {
                                    at: at.clone(),
                                    column: column.clone(),
                                    foreign_key: foreign.contains(column.name.as_str()),
                                },
                            })
                            .collect()
                    }
                    Group::Keys => {
                        let kind = NodeKind::Key { at: at.clone(), columns: primary_key(details) };
                        vec![DbNode { key: key("pk"), kind }]
                    }
                    Group::ForeignKeys => details
                        .foreign_keys
                        .iter()
                        .enumerate()
                        .map(|(index, fk)| DbNode {
                            key: key(&index.to_string()),
                            kind: NodeKind::ForeignKey { at: at.clone(), fk: fk.clone() },
                        })
                        .collect(),
                    Group::Indexes => details
                        .indexes
                        .iter()
                        .map(|index| DbNode {
                            key: key(&index.name),
                            kind: NodeKind::Index { at: at.clone(), index: index.clone() },
                        })
                        .collect(),
                    Group::Triggers => details
                        .triggers
                        .iter()
                        .map(|trigger| DbNode {
                            key: key(&trigger.name),
                            kind: NodeKind::Trigger { at: at.clone(), trigger: trigger.clone() },
                        })
                        .collect(),
                })
            }
            _ => None,
        }
    }

    fn label(&self, node: &DbNode) -> String {
        match &node.kind {
            NodeKind::Source(source) => match &source.plugin_owner {
                Some(owner) => format!("{} ({owner})", source.name),
                None => source.name.clone(),
            },
            NodeKind::Database { name, .. } => name.clone(),
            NodeKind::Schema { schema, .. } => schema.clone(),
            NodeKind::Folder { folder, .. } => folder_label(*folder).to_owned(),
            NodeKind::Table { table, .. } => table.name.clone(),
            NodeKind::Routine { routine, .. } => routine.name.clone(),
            NodeKind::Sequence { sequence, .. } => sequence.name.clone(),
            NodeKind::Group { group, .. } => group_label(*group).to_owned(),
            NodeKind::Column { column, .. } => column.name.clone(),
            // Unnamed (SQLite) keys are numbered: `key #1`, `foreign-key #1`.
            NodeKind::Key { .. } => "key #1".to_owned(),
            NodeKind::ForeignKey { .. } => {
                let index = node.key.rsplit('/').next().and_then(|i| i.parse::<usize>().ok()).unwrap_or(0);
                format!("foreign-key #{}", index + 1)
            }
            NodeKind::Index { index, .. } => index.name.clone(),
            NodeKind::Trigger { trigger, .. } => trigger.name.clone(),
            NodeKind::Message { text, .. } => text.clone(),
        }
    }

    /// The grey text after the label: counts, column types, key columns.
    fn detail(&self, node: &DbNode) -> Option<String> {
        let schema_count = |target: &str| {
            self.data.schemas.get(target).and_then(Loaded::value).map(|names| self.shown_schemas(target, names).len())
        };
        let count = match &node.kind {
            NodeKind::Source(source) if lists_databases(source) => self
                .data
                .databases
                .get(&source.id)
                .and_then(Loaded::value)
                .map(|databases| self.shown_databases(&source.id, databases).len()),
            NodeKind::Source(source) => schema_count(&source.id),
            NodeKind::Database { id, .. } => schema_count(id),
            NodeKind::Folder { id, schema, folder } => Some(self.schema_view(id, schema).members(*folder).len()),
            NodeKind::Group { at, group } => {
                self.details(at).and_then(Loaded::value).map(|details| group_size(details, *group))
            }
            NodeKind::Routine { routine, .. } => {
                let returns = routine.returns.as_ref().map(|r| format!(": {r}")).unwrap_or_default();
                return Some(format!("({}){returns}", routine.arguments));
            }
            NodeKind::Sequence { sequence, .. } => return Some(sequence.data_type.clone()),
            NodeKind::Trigger { trigger, .. } => {
                return Some(format!("{} {}", trigger.timing, trigger.events).trim().to_owned());
            }
            NodeKind::Column { column, .. } => return Some(column.data_type.clone()),
            NodeKind::Key { columns, .. } => return Some(format!("({})", columns.join(", "))),
            NodeKind::ForeignKey { fk, .. } => {
                return Some(format!("({}) → {} ({})", fk.columns.join(", "), fk.ref_table, fk.ref_columns.join(", ")));
            }
            NodeKind::Index { index, .. } => {
                let unique = if index.unique { "unique " } else { "" };
                return Some(format!("{unique}({})", index.columns.join(", ")));
            }
            NodeKind::Schema { .. } | NodeKind::Table { .. } | NodeKind::Message { .. } => None,
        };
        count.map(|n| n.to_string())
    }

    fn icon(&self, node: &DbNode) -> Option<&'static str> {
        Some(match &node.kind {
            NodeKind::Source(source) => source_icon(&source.vendor, source.driver),
            NodeKind::Database { .. } => "database",
            NodeKind::Schema { .. } => "db-schema",
            NodeKind::Folder { .. } | NodeKind::Group { .. } => "db-folder",
            NodeKind::Table { table, .. } => match table.kind {
                ide_db::DbTableKind::Table => "db-table",
                ide_db::DbTableKind::View => "db-view",
                ide_db::DbTableKind::MaterializedView => "db-matview",
            },
            NodeKind::Routine { .. } => "db-routine",
            NodeKind::Sequence { .. } => "db-sequence",
            NodeKind::Trigger { .. } => "db-trigger",
            NodeKind::Column { column, foreign_key, .. } => {
                if column.primary_key {
                    "db-column-key"
                } else if *foreign_key {
                    "db-column-fk"
                } else {
                    "db-column"
                }
            }
            NodeKind::Key { .. } => "db-key",
            NodeKind::ForeignKey { .. } => "db-foreign-key",
            NodeKind::Index { .. } => "db-index",
            NodeKind::Message { error: true, .. } => "error",
            NodeKind::Message { error: false, .. } => return None,
        })
    }

    fn row(&self, node: DbNode, depth: usize, parent: Option<usize>, expanded: bool) -> TreeRow {
        let detail_style = match node.kind {
            NodeKind::Source(_) => DetailStyle::Badge,
            NodeKind::Database { .. } | NodeKind::Folder { .. } | NodeKind::Group { .. } => DetailStyle::Count,
            NodeKind::Column { .. } => DetailStyle::Type,
            _ => DetailStyle::Plain,
        };
        let (live, global) = match &node.kind {
            NodeKind::Source(source) => {
                (self.data.connected.iter().any(|target| split_target(target).0 == source.id), source.global)
            }
            _ => (false, false),
        };
        let tint = node
            .target()
            .and_then(|target| self.source_of(target))
            .and_then(|source| color_token(source.color))
            .map(|token| token.trim_start_matches("--").to_owned());
        TreeRow {
            depth,
            parent,
            expandable: is_expandable(&node),
            expanded,
            loading: false,
            label: self.label(&node),
            detail: self.detail(&node),
            detail_style,
            icon: self.icon(&node),
            live,
            tint,
            global,
            error: matches!(node.kind, NodeKind::Message { error: true, .. }),
            node,
        }
    }

    /// The visible rows, top to bottom.
    fn rows(&self, expanded: &BTreeSet<String>) -> Vec<TreeRow> {
        let roots: Vec<DbNode> = self
            .data
            .sources
            .iter()
            .map(|source| DbNode { key: source.id.clone(), kind: NodeKind::Source(Box::new(source.clone())) })
            .collect();
        let mut out = Vec::new();
        self.walk(&roots, 0, None, expanded, &mut out);
        out
    }

    fn walk(
        &self,
        nodes: &[DbNode],
        depth: usize,
        parent: Option<usize>,
        expanded: &BTreeSet<String>,
        out: &mut Vec<TreeRow>,
    ) {
        for node in nodes {
            let open = is_expandable(node) && expanded.contains(&node.key);
            let index = out.len();
            let row = self.row(node.clone(), depth, parent, open);
            let tint = row.tint.clone();
            out.push(row);
            if !open {
                continue;
            }
            match self.children(node) {
                Some(children) => self.walk(&children, depth + 1, Some(index), expanded, out),
                None => {
                    let placeholder = message_node(&node.key, "loading", "Loading…", false);
                    out.push(TreeRow { loading: true, tint, ..self.row(placeholder, depth + 1, Some(index), false) });
                }
            }
        }
    }

    /// The level an expanded node needs that was never asked for.
    fn load_for(&self, node: &DbNode) -> Option<TreeLoad> {
        match &node.kind {
            NodeKind::Source(source) if lists_databases(source) => {
                (!self.data.databases.contains_key(&source.id)).then(|| TreeLoad::Databases { id: source.id.clone() })
            }
            NodeKind::Source(source) => {
                (!self.data.schemas.contains_key(&source.id)).then(|| TreeLoad::Schemas { target: source.id.clone() })
            }
            NodeKind::Database { id, .. } => {
                (!self.data.schemas.contains_key(id)).then(|| TreeLoad::Schemas { target: id.clone() })
            }
            NodeKind::Schema { id, schema } => (!self.data.tables.contains_key(&(id.clone(), schema.clone())))
                .then(|| TreeLoad::Tables { target: id.clone(), schema: schema.clone() }),
            NodeKind::Table { id, schema, table } => {
                let key = (id.clone(), schema.clone(), table.name.clone());
                (!self.data.details.contains_key(&key)).then(|| TreeLoad::Details {
                    target: id.clone(),
                    schema: schema.clone(),
                    table: table.name.clone(),
                })
            }
            _ => None,
        }
    }

    /// While a quick filter is on, the schemas of expanded sources load their objects, so tables
    /// and routines matching it show up without expanding every schema.
    fn filter_loads(&self, expanded: &BTreeSet<String>) -> Vec<TreeLoad> {
        if self.filter.trim().is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (target, list) in &self.data.schemas {
            let Loaded::Ok(list) = list else { continue };
            let (id, database) = split_target(target);
            let key = database.map_or_else(|| id.to_owned(), |database| database_key(id, database));
            if !expanded.contains(&key) {
                continue;
            }
            for schema in list {
                if !self.data.tables.contains_key(&(target.clone(), schema.clone())) {
                    out.push(TreeLoad::Tables { target: target.clone(), schema: schema.clone() });
                }
            }
        }
        out
    }
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn sep() -> MenuItem<TreeCommand> {
    MenuItem::Separator
}

/// A menu row for `action` on the node `node`.
fn action_row(action: TreeAction, node: Option<&str>) -> MenuItem<TreeCommand> {
    let item = MenuItem::item(action.title(), TreeCommand { action, node: node.map(str::to_owned) });
    match action.icon() {
        Some(icon) => item.icon(icon),
        None => item,
    }
}

/// New ▸ Data Source ▸ (every type the backend speaks, A–Z), from a file, from a URL, from the
/// clipboard.
fn data_source_items() -> Vec<MenuItem<TreeCommand>> {
    let vendors = VENDORS.iter().map(|vendor| action_row(TreeAction::NewDataSource(Some(vendor.id)), None)).collect();
    vec![
        MenuItem::submenu("Data Source", vendors).icon("database"),
        action_row(TreeAction::DataSourceFromFile, None),
        action_row(TreeAction::DataSourceFromUrl, None),
        action_row(TreeAction::ImportFromClipboard, None),
    ]
}

/// The Database tool window's tree with its toolbar.
pub struct DatabaseTree {
    focus: FocusHandle,
    data: TreeData,
    expanded: BTreeSet<String>,
    selected: Option<String>,
    filter: String,
    separate: bool,
    rows: Rc<Vec<TreeRow>>,
    /// What was typed for speed search.
    query: String,
    /// A node to select and scroll to once its row exists.
    reveal: Option<String>,
    scroll: UniformListScrollHandle,
    filter_field: Entity<TextField>,
    _filter_events: Subscription,
}

impl EventEmitter<TreeEvent> for DatabaseTree {}

impl DatabaseTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Filter objects");
        let filter_field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let filter_events =
            cx.subscribe(&filter_field, |tree: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    tree.set_filter(&text, cx);
                }
                TextFieldEvent::Cancelled => tree.set_filter("", cx),
                TextFieldEvent::Submitted => {}
            });
        let separate = read_separate_procedures_and_functions(
            cx.try_global::<UiStorage>().and_then(|storage| storage.get(VIEW_OPTIONS_KEY)),
        );
        Self {
            focus: cx.focus_handle(),
            data: TreeData::default(),
            expanded: BTreeSet::new(),
            selected: None,
            filter: String::new(),
            separate,
            rows: Rc::default(),
            query: String::new(),
            reveal: None,
            scroll: UniformListScrollHandle::new(),
            filter_field,
            _filter_events: filter_events,
        }
    }

    fn shape(&self) -> Shape<'_> {
        Shape { data: &self.data, filter: &self.filter, separate: self.separate }
    }

    /// Rebuilds the rows, asks for the levels expanded nodes need and serves a pending reveal.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let (rows, loads) = {
            let shape = self.shape();
            let rows = shape.rows(&self.expanded);
            let mut loads: Vec<TreeLoad> = Vec::new();
            let wanted = rows.iter().filter(|row| row.expanded).filter_map(|row| shape.load_for(&row.node));
            for load in wanted.chain(shape.filter_loads(&self.expanded)) {
                if !loads.contains(&load) {
                    loads.push(load);
                }
            }
            (rows, loads)
        };
        for load in loads {
            self.mark_loading(&load);
            cx.emit(TreeEvent::Load(load));
        }
        self.rows = Rc::new(rows);
        if let Some(key) = self.reveal.clone()
            && let Some(index) = self.index_of(&key)
        {
            self.selected = Some(key);
            self.reveal = None;
            self.scroll.scroll_to_item(index, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn mark_loading(&mut self, load: &TreeLoad) {
        match load.clone() {
            TreeLoad::Databases { id } => {
                self.data.databases.insert(id, Loaded::Loading);
            }
            TreeLoad::Schemas { target } => {
                self.data.schemas.insert(target, Loaded::Loading);
            }
            TreeLoad::Tables { target, schema } => {
                self.data.tables.insert((target, schema), Loaded::Loading);
            }
            TreeLoad::Details { target, schema, table } => {
                self.data.details.insert((target, schema, table), Loaded::Loading);
            }
        }
    }

    /// The data sources as read from the project; the tree shows the empty state once they are known.
    pub fn set_sources(&mut self, sources: Vec<DataSource>, cx: &mut Context<Self>) {
        self.data.sources = sources;
        self.data.loaded = true;
        self.sync(cx);
    }

    /// The connected targets (a green dot on their data sources).
    pub fn set_connected(&mut self, connected: BTreeSet<String>, cx: &mut Context<Self>) {
        self.data.connected = connected;
        self.sync(cx);
    }

    /// The answer to [`TreeLoad::Databases`].
    pub fn databases_loaded(&mut self, id: &str, result: Result<Databases, String>, cx: &mut Context<Self>) {
        self.data.databases.insert(id.to_owned(), Loaded::from_result(result));
        self.sync(cx);
    }

    /// The answer to [`TreeLoad::Schemas`].
    pub fn schemas_loaded(&mut self, target: &str, result: Result<Vec<String>, String>, cx: &mut Context<Self>) {
        self.data.schemas.insert(target.to_owned(), Loaded::from_result(result));
        self.sync(cx);
    }

    /// The answer to [`TreeLoad::Tables`]: the tables, and the routines and sequences when the
    /// server lists them.
    pub fn tables_loaded(
        &mut self,
        target: &str,
        schema: &str,
        result: Result<Vec<TableInfo>, String>,
        objects: Option<SchemaObjects>,
        cx: &mut Context<Self>,
    ) {
        let key = (target.to_owned(), schema.to_owned());
        if let Some(objects) = objects {
            self.data.objects.insert(key.clone(), objects);
        }
        self.data.tables.insert(key, Loaded::from_result(result));
        self.sync(cx);
    }

    /// The answer to [`TreeLoad::Details`].
    pub fn details_loaded(
        &mut self,
        target: &str,
        schema: &str,
        table: &str,
        result: Result<TableDetails, String>,
        cx: &mut Context<Self>,
    ) {
        let key = (target.to_owned(), schema.to_owned(), table.to_owned());
        self.data.details.insert(key, Loaded::from_result(result));
        self.sync(cx);
    }

    pub fn data(&self) -> &TreeData {
        &self.data
    }

    /// The visible rows, top to bottom.
    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    /// The rows as text, indented by depth: `v ` before an expanded node, `> ` before a collapsed
    /// one, the grey detail after the label.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let marker = match (row.expandable, row.expanded) {
                    (true, true) => "v ",
                    (true, false) => "> ",
                    (false, _) => "",
                };
                let detail = row.detail.as_ref().map(|d| format!(" {d}")).unwrap_or_default();
                format!("{}{marker}{}{detail}", "  ".repeat(row.depth), row.label)
            })
            .collect()
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|row| row.node.key == key && !row.loading)
    }

    /// The visible node with this key.
    pub fn node(&self, key: &str) -> Option<&DbNode> {
        self.index_of(key).map(|index| &self.rows[index].node)
    }

    /// Key of the selected node.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn selected_node(&self) -> Option<&DbNode> {
        self.node(self.selected.as_deref()?)
    }

    /// The connection target of the selection; the only data source when nothing is selected.
    pub fn selected_target(&self) -> Option<String> {
        match self.selected_node() {
            Some(node) => node.target().map(str::to_owned),
            None => match self.data.sources.as_slice() {
                [only] => Some(only.id.clone()),
                _ => None,
            },
        }
    }

    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected = Some(key.to_owned());
        cx.notify();
    }

    /// Selects `key` and scrolls to it once its row exists (Go to Declaration from a console).
    pub fn reveal(&mut self, key: &str, cx: &mut Context<Self>) {
        self.reveal = Some(key.to_owned());
        self.sync(cx);
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    /// Expands or collapses the node `key`; expanding asks for its level when it is not loaded.
    pub fn set_expanded(&mut self, key: &str, open: bool, cx: &mut Context<Self>) {
        let changed = if open { self.expanded.insert(key.to_owned()) } else { self.expanded.remove(key) };
        if changed {
            self.sync(cx);
        }
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let open = !self.expanded.contains(key);
        self.set_expanded(key, open, cx);
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.sync(cx);
    }

    /// The toolbar's quick filter.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn filter_field(&self) -> &Entity<TextField> {
        &self.filter_field
    }

    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.filter_field.read(cx).text() != text {
            self.filter_field.update(cx, |field, cx| field.set_text(text, cx));
        }
        if self.filter != text {
            self.filter = text.to_owned();
            self.sync(cx);
        }
    }

    /// The *Separate Procedures and Functions* option.
    pub fn separate(&self) -> bool {
        self.separate
    }

    /// Sets the option and stores it for every Database tree from now on.
    pub fn set_separate(&mut self, separate: bool, cx: &mut Context<Self>) {
        self.separate = separate;
        if cx.has_global::<UiStorage>() {
            cx.global_mut::<UiStorage>().set(VIEW_OPTIONS_KEY, view_options_json(separate));
        }
        self.sync(cx);
    }

    /// What the tree says instead of rows while the project has no data source.
    pub fn empty_text(&self, cx: &mut App) -> Option<String> {
        (self.data.loaded && self.data.sources.is_empty()).then(|| tx(cx, "No data sources"))
    }

    /// Enter or a double click: a table opens its data, a routine, trigger or sequence its DDL,
    /// any other node with children expands or collapses.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(row) = self.index_of(key).map(|index| self.rows[index].clone()) else { return };
        let table = matches!(row.node.kind, NodeKind::Table { .. });
        if row.expandable && !table {
            self.toggle(key, cx);
        } else if let Some(at) = row.node.table_of().filter(|_| table) {
            cx.emit(TreeEvent::OpenTableData(at));
        } else if let Some(object) = row.node.schema_object_of() {
            cx.emit(TreeEvent::OpenObjectDdl(object));
        }
    }

    /// Carries out a toolbar or menu command: Refresh, Collapse All and the view option here,
    /// anything else as [`TreeEvent::Run`]. A command on a node that is no longer shown is dropped.
    pub fn perform(&mut self, command: TreeCommand, cx: &mut Context<Self>) {
        let node = match &command.node {
            Some(key) => match self.node(key) {
                Some(node) => Some(node.clone()),
                None => return,
            },
            None => None,
        };
        match command.action {
            TreeAction::Refresh => self.refresh(node.as_ref(), cx),
            TreeAction::CollapseAll => self.collapse_all(cx),
            TreeAction::ToggleSeparateProceduresAndFunctions => self.set_separate(!self.separate, cx),
            action => cx.emit(TreeEvent::Run { action, node: node.map(Box::new) }),
        }
    }

    /// `action` on the selected node (the toolbar's buttons).
    fn perform_on_selection(&mut self, action: TreeAction, cx: &mut Context<Self>) {
        let node = self.selected_node().map(|node| node.key.clone());
        self.perform(TreeCommand { action, node }, cx);
    }

    /// Refresh: the data sources and every connection (no node), a schema's objects, a table's
    /// details, or every loaded level of the node's target.
    fn refresh(&mut self, node: Option<&DbNode>, cx: &mut Context<Self>) {
        let Some(node) = node else {
            cx.emit(TreeEvent::LoadSources);
            for target in self.data.connected.clone() {
                self.refresh_target(&target, cx);
            }
            return;
        };
        match &node.kind {
            NodeKind::Schema { id, schema } | NodeKind::Folder { id, schema, .. } => {
                cx.emit(TreeEvent::Load(TreeLoad::Tables { target: id.clone(), schema: schema.clone() }));
            }
            _ => match node.table_of() {
                Some(t) => {
                    cx.emit(TreeEvent::Load(TreeLoad::Details { target: t.id, schema: t.schema, table: t.table }));
                }
                None => {
                    if let Some(target) = node.target().map(str::to_owned) {
                        self.refresh_target(&target, cx);
                    }
                }
            },
        }
    }

    /// Reloads the levels loaded for `target`; a data source id covers all its databases. What is
    /// shown stays until the answers arrive.
    fn refresh_target(&mut self, target: &str, cx: &mut Context<Self>) {
        let whole = split_target(target).1.is_none();
        let covers = |other: &str| other == target || (whole && split_target(other).0 == target);
        let mut loads = Vec::new();
        if self.data.databases.contains_key(target) {
            loads.push(TreeLoad::Databases { id: target.to_owned() });
        }
        loads.extend(self.data.schemas.keys().filter(|t| covers(t)).map(|t| TreeLoad::Schemas { target: t.clone() }));
        loads.extend(
            self.data
                .tables
                .keys()
                .filter(|(t, _)| covers(t))
                .map(|(t, schema)| TreeLoad::Tables { target: t.clone(), schema: schema.clone() }),
        );
        loads.extend(self.data.details.keys().filter(|(t, _, _)| covers(t)).map(|(t, schema, table)| {
            TreeLoad::Details { target: t.clone(), schema: schema.clone(), table: table.clone() }
        }));
        for load in loads {
            cx.emit(TreeEvent::Load(load));
        }
    }

    /// The toolbar's New menu: Query Console, then the data source types.
    pub fn new_menu(&self) -> Vec<MenuItem<TreeCommand>> {
        let mut items = vec![action_row(TreeAction::OpenConsole, None), sep()];
        items.extend(data_source_items());
        items
    }

    /// The tool window header's menu: the *Separate Procedures and Functions* option (ticked while
    /// on) and Collapse All.
    pub fn header_menu(&self) -> Vec<MenuItem<TreeCommand>> {
        let toggle = action_row(TreeAction::ToggleSeparateProceduresAndFunctions, None);
        vec![if self.separate { toggle.icon("check") } else { toggle }, action_row(TreeAction::CollapseAll, None)]
    }

    /// The context menu of the node `key`, or of the empty area (`None`).
    pub fn menu_for(&self, key: Option<&str>) -> Vec<MenuItem<TreeCommand>> {
        let Some(key) = key else {
            return vec![MenuItem::submenu("New", self.new_menu()).icon("plus")];
        };
        let Some(node) = self.node(key) else { return Vec::new() };
        let shape = self.shape();
        let id = node.target().unwrap_or_default();
        let driver = shape.driver(id);
        let object = node.object_of();
        let table = node.table_of();
        let at = Some(node.key.as_str());
        let act = |action: TreeAction| action_row(action, at);
        let new_console = act(TreeAction::OpenConsole);
        let refresh = act(TreeAction::Refresh);
        let quick_doc = act(TreeAction::QuickDoc);
        let renamable =
            matches!(node.kind, NodeKind::Source(_)) || object.as_ref().is_some_and(|o| can_rename(driver, o));
        let rename = act(TreeAction::Rename).disabled(!renamable);
        let drop_row = act(TreeAction::Drop).disabled(!object.as_ref().is_some_and(|o| can_drop(driver, o)));
        let copy_paste = |mut extra: Vec<MenuItem<TreeCommand>>| {
            extra.insert(0, act(TreeAction::CopyReference));
            MenuItem::submenu("Copy/Paste", extra)
        };
        let navigation = |mut extra: Vec<MenuItem<TreeCommand>>| {
            extra.push(act(TreeAction::JumpToConsole));
            MenuItem::submenu("Navigation", extra)
        };
        let sql_scripts = |of_table: bool| {
            let mut items = vec![act(TreeAction::DdlToClipboard), act(TreeAction::DdlToConsole), sep()];
            if of_table {
                let kinds = [
                    ScriptKind::SelectAll,
                    ScriptKind::SelectFirst,
                    ScriptKind::Insert,
                    ScriptKind::Update,
                    ScriptKind::Delete,
                ];
                items.extend(kinds.into_iter().map(|kind| act(TreeAction::SqlScript(kind))));
            } else {
                items.push(act(TreeAction::RunSqlScript));
            }
            MenuItem::submenu("SQL Scripts", items)
        };
        match &node.kind {
            NodeKind::Source(source) => {
                let connected = self.data.connected.iter().any(|target| split_target(target).0 == source.id);
                let mut new = vec![new_console, sep()];
                new.extend(data_source_items());
                vec![
                    act(TreeAction::Properties).labelled("Properties"),
                    sep(),
                    MenuItem::submenu("New", new).icon("plus"),
                    copy_paste(vec![act(TreeAction::DuplicateSource)]),
                    refresh,
                    act(TreeAction::Disconnect).disabled(!connected),
                    sep(),
                    rename,
                    sep(),
                    quick_doc,
                    navigation(Vec::new()),
                    sep(),
                    sql_scripts(false),
                    sep(),
                    act(TreeAction::Drop).labelled("Remove Data Source…"),
                ]
            }
            NodeKind::Database { current, .. } => vec![
                MenuItem::submenu("New", vec![new_console]).icon("plus"),
                copy_paste(Vec::new()),
                refresh,
                act(TreeAction::Disconnect).disabled(!self.data.connected.contains(id) || *current),
                sep(),
                navigation(Vec::new()),
                sql_scripts(false),
            ],
            NodeKind::Schema { .. } | NodeKind::Folder { .. } => {
                let schema = matches!(node.kind, NodeKind::Schema { .. });
                let mut items =
                    vec![MenuItem::submenu("New", vec![new_console, sep(), act(TreeAction::NewTable)]).icon("plus")];
                if schema {
                    items.push(copy_paste(Vec::new()));
                }
                items.extend([refresh, sep()]);
                if schema {
                    items.push(quick_doc);
                }
                items.extend([navigation(Vec::new()), sep(), sql_scripts(false)]);
                if schema {
                    items.extend([sep(), drop_row]);
                }
                items
            }
            NodeKind::Table { .. } | NodeKind::Column { .. } => {
                let view = table.as_ref().is_some_and(|t| t.view);
                let modifiable = !view && can_modify_table(driver);
                let column = matches!(node.kind, NodeKind::Column { .. });
                let new = vec![
                    new_console,
                    sep(),
                    act(TreeAction::NewColumn).disabled(!modifiable),
                    act(TreeAction::NewTable),
                ];
                let mut items = vec![
                    MenuItem::submenu("New", new).icon("plus"),
                    act(TreeAction::ModifyTable)
                        .labelled(if column { "Modify Column…" } else { "Modify Table…" })
                        .disabled(!modifiable),
                    copy_paste(Vec::new()),
                    act(TreeAction::EditData),
                    refresh,
                    sep(),
                    rename,
                    sep(),
                    quick_doc,
                    navigation(vec![act(TreeAction::GoToDdl)]),
                    sep(),
                    sql_scripts(true),
                ];
                if !column && !view {
                    let import = act(TreeAction::ImportData).disabled(driver == DbDriver::Clickhouse);
                    items.push(MenuItem::submenu("Import/Export", vec![import]));
                    items.push(MenuItem::submenu("Object Actions", vec![act(TreeAction::Truncate)]));
                }
                items.extend([sep(), drop_row]);
                items
            }
            NodeKind::Index { .. } => vec![
                copy_paste(Vec::new()),
                refresh,
                sep(),
                rename,
                sep(),
                navigation(vec![act(TreeAction::GoToDdl)]),
                sep(),
                drop_row,
            ],
            NodeKind::ForeignKey { .. } => vec![
                refresh,
                sep(),
                MenuItem::submenu("Navigation", vec![act(TreeAction::GoToReferencedTable), act(TreeAction::GoToDdl)]),
            ],
            NodeKind::Routine { .. } | NodeKind::Sequence { .. } | NodeKind::Trigger { .. } => vec![
                copy_paste(Vec::new()),
                refresh,
                sep(),
                quick_doc,
                navigation(vec![act(TreeAction::GoToDdl)]),
                MenuItem::submenu("SQL Scripts", vec![act(TreeAction::DdlToClipboard), act(TreeAction::DdlToConsole)]),
                sep(),
                drop_row,
            ],
            NodeKind::Group { .. } | NodeKind::Key { .. } => {
                let mut items = Vec::new();
                if matches!(node.kind, NodeKind::Group { group: Group::Columns, .. }) {
                    let column = act(TreeAction::NewColumn).disabled(!can_modify_table(driver));
                    items.push(MenuItem::submenu("New", vec![column]).icon("plus"));
                }
                items.extend([refresh, sep(), MenuItem::submenu("Navigation", vec![act(TreeAction::GoToDdl)])]);
                items
            }
            NodeKind::Message { .. } => Vec::new(),
        }
    }

    fn select_index(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index).filter(|row| !row.loading) {
            self.selected = Some(row.node.key.clone());
        }
    }

    /// The next row from `from` in direction `forward` that is not a loading placeholder.
    fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let selectable = |&index: &usize| !self.rows[index].loading;
        match (from, forward) {
            (None, _) => (0..self.rows.len()).find(selectable),
            (Some(at), true) => (at + 1..self.rows.len()).find(selectable).or(Some(at)),
            (Some(at), false) => (0..at).rev().find(selectable).or(Some(at)),
        }
    }

    fn speed_match(&self, index: usize, query: &str) -> bool {
        let row = &self.rows[index];
        !row.loading && row.label.to_lowercase().contains(&query.to_lowercase())
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let (modifiers, key) = (keystroke.modifiers, keystroke.key.as_str());
        let lead = self.selected.as_deref().and_then(|key| self.index_of(key));
        // Jump to Source (F4 / ⌘↓) activates like Enter.
        if key == "f4" || (key == "down" && modifiers.platform && !modifiers.shift && !modifiers.alt) {
            if let Some(index) = lead {
                self.query.clear();
                let key = self.rows[index].node.key.clone();
                self.activate(&key, cx);
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let typed = keystroke
            .key_char
            .as_deref()
            .filter(|c| c.chars().count() == 1 && !c.chars().any(char::is_control))
            .filter(|c| *c != " " || !self.query.is_empty());
        let count = self.rows.len();
        if let Some(c) = typed {
            let query = format!("{}{c}", self.query);
            let start = lead.unwrap_or(0);
            if let Some(hit) = (0..count).map(|k| (start + k) % count).find(|&i| self.speed_match(i, &query)) {
                self.select_index(hit);
            }
            self.query = query;
        } else {
            match key {
                "escape" if !self.query.is_empty() => self.query.clear(),
                "backspace" if !self.query.is_empty() => {
                    self.query.pop();
                }
                "down" | "up" => {
                    let forward = key == "down";
                    let next = if self.query.is_empty() {
                        self.step(lead, forward)
                    } else {
                        let query = self.query.clone();
                        let at = lead.unwrap_or(0);
                        if forward {
                            (at + 1..count).find(|&i| self.speed_match(i, &query))
                        } else {
                            (0..at).rev().find(|&i| self.speed_match(i, &query))
                        }
                    };
                    if let Some(index) = next {
                        self.select_index(index);
                    }
                }
                "right" => {
                    self.query.clear();
                    let Some(index) = lead else { return };
                    let row = &self.rows[index];
                    if row.expandable && !row.expanded {
                        let key = row.node.key.clone();
                        self.set_expanded(&key, true, cx);
                    } else if row.expanded
                        && self.rows.get(index + 1).is_some_and(|next| next.parent == Some(index) && !next.loading)
                    {
                        self.select_index(index + 1);
                    }
                }
                "left" => {
                    self.query.clear();
                    let Some(index) = lead else { return };
                    let row = &self.rows[index];
                    if row.expanded {
                        let key = row.node.key.clone();
                        self.set_expanded(&key, false, cx);
                    } else if let Some(parent) = row.parent {
                        self.select_index(parent);
                    }
                }
                "home" | "end" => {
                    self.query.clear();
                    let index = if key == "home" {
                        self.step(None, true)
                    } else {
                        (0..count).rev().find(|&i| !self.rows[i].loading)
                    };
                    if let Some(index) = index {
                        self.select_index(index);
                    }
                }
                "pagedown" | "pageup" => {
                    self.query.clear();
                    let row_height = f32::from(theme::length(cx, "h-row")).max(1.0);
                    let page = ((f32::from(window.viewport_size().height) / row_height).floor() as usize).max(2) - 1;
                    let at = lead.unwrap_or(0);
                    let last = count.saturating_sub(1);
                    let index = if key == "pagedown" { (at + page).min(last) } else { at.saturating_sub(page) };
                    self.select_index(index);
                }
                "enter" => {
                    self.query.clear();
                    let Some(index) = lead else { return };
                    let key = self.rows[index].node.key.clone();
                    self.activate(&key, cx);
                }
                _ => return,
            }
        }
        if let Some(index) = self.selected.as_deref().and_then(|key| self.index_of(key)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn tool_button(
        &self,
        id: &'static str,
        icon: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |tree, event: &ClickEvent, _, cx| on_click(tree, event, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let has_sources = !self.data.sources.is_empty();
        let node = self.selected_node();
        let target = self.selected_target();
        let active = target.as_deref().is_some_and(|target| {
            let id = split_target(target).0;
            self.data.connected.iter().any(|connected| split_target(connected).0 == id)
        });
        let table = node.is_some_and(|n| {
            matches!(n.kind, NodeKind::Table { .. } | NodeKind::Column { .. } | NodeKind::Group { .. })
        });
        let can_disconnect = if node.is_some() { active } else { !self.data.connected.is_empty() };
        let border = theme::color(cx, "border");
        let separator = || div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(border);
        let ddl = tx(cx, "DDL");
        let ddl_button = div()
            .id("db-tree-ddl")
            .debug_selector(|| "db-tree-ddl".to_owned())
            .h(px(20.0))
            .px(px(3.0))
            .flex_none()
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .text_size(theme::length(cx, "fs-small"))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .text_color(theme::color(cx, "fg-strong"))
            .when(!table, |el| el.opacity(0.4))
            .when(table, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover"))).on_click(
                    cx.listener(|tree, _: &ClickEvent, _, cx| tree.perform_on_selection(TreeAction::GoToDdl, cx)),
                )
            })
            .child(SharedString::from(ddl));
        let on_selection = |action: TreeAction| {
            move |tree: &mut Self, _: &ClickEvent, cx: &mut Context<Self>| {
                tree.perform_on_selection(action, cx);
            }
        };
        div()
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .child(self.tool_button("db-tree-new", "db-add", true, cx, |_, event, cx| {
                cx.emit(TreeEvent::ShowNewMenu(event.position()));
            }))
            .child(self.tool_button(
                "db-tree-properties",
                "db-properties",
                has_sources,
                cx,
                on_selection(TreeAction::Properties),
            ))
            .child(separator())
            .child(self.tool_button("db-tree-refresh", "refresh", has_sources, cx, on_selection(TreeAction::Refresh)))
            .child(self.tool_button(
                "db-tree-disconnect",
                "db-disconnect",
                can_disconnect,
                cx,
                on_selection(TreeAction::Disconnect),
            ))
            .child(separator())
            .child(self.tool_button(
                "db-tree-console",
                "db-console",
                target.is_some(),
                cx,
                on_selection(TreeAction::JumpToConsole),
            ))
            .child(self.tool_button("db-tree-edit-data", "db-table", table, cx, on_selection(TreeAction::EditData)))
            .child(ddl_button)
            .child(div().flex_1().min_w(px(80.0)).max_w(px(280.0)).ml(px(4.0)).child(self.filter_field.clone()))
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let title = self.empty_text(cx).unwrap_or_default();
        let button = tx(cx, "New Data Source…");
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .p(px(24.0))
            .child(
                div()
                    .debug_selector(|| "db-tree-empty".to_owned())
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .id("db-tree-new-source")
                    .debug_selector(|| "db-tree-new-source".to_owned())
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(theme::color(cx, "fg-strong"))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(|tree, _: &ClickEvent, _, cx| {
                        tree.perform(TreeCommand { action: TreeAction::NewDataSource(None), node: None }, cx);
                    }))
                    .child(SharedString::from(button)),
            )
    }

    fn render_tree(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = Rc::clone(&self.rows);
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, muted, secondary, danger) = (
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "fg-secondary"),
            theme::color(cx, "danger"),
        );
        let (chevron_color, border) = (theme::color(cx, "fg-icon"), theme::color(cx, "border"));
        let (dot, ring) = (theme::color(cx, "db-connected"), theme::color(cx, "db-connected-ring"));
        let (small, radius) = (theme::length(cx, "fs-small"), theme::length(cx, "radius-small"));
        let global = tx(cx, "global");
        let selected = self.selected.clone();
        let list = uniform_list(
            "database-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let key = row.node.key.clone();
                        let chevron = if row.expandable {
                            let (toggled, selector) = (key.clone(), key.clone());
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            div()
                                .id(SharedString::from(format!("toggle:{key}")))
                                .debug_selector(move || format!("db-tree-toggle-{selector}"))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron_color))
                                .on_click(cx.listener(move |tree, _: &ClickEvent, window, cx| {
                                    window.focus(&tree.focus, cx);
                                    tree.query.clear();
                                    tree.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let color = if row.error {
                            danger
                        } else if row.loading {
                            muted
                        } else if matches!(row.node.kind, NodeKind::Key { .. } | NodeKind::ForeignKey { .. }) {
                            secondary
                        } else {
                            fg
                        };
                        let icon = row.icon.map(|icon| {
                            div().relative().flex_none().child(theme::icon(cx, icon, 16.0, color)).when(
                                row.live,
                                |el| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .right(px(-2.0))
                                            .bottom(px(-1.0))
                                            .size(px(5.0))
                                            .rounded_full()
                                            .bg(dot)
                                            .border_1()
                                            .border_color(ring),
                                    )
                                },
                            )
                        });
                        let detail = row.detail.clone().map(|text| {
                            let el = div().flex_none().whitespace_nowrap().text_color(muted);
                            match row.detail_style {
                                DetailStyle::Badge => el
                                    .px(px(4.0))
                                    .text_size(small)
                                    .border_1()
                                    .border_color(border)
                                    .rounded(radius)
                                    .child(SharedString::from(text)),
                                DetailStyle::Count | DetailStyle::Type => {
                                    el.text_size(small).child(SharedString::from(text))
                                }
                                DetailStyle::Plain => {
                                    el.flex_shrink(1.0).min_w_0().overflow_hidden().child(SharedString::from(text))
                                }
                            }
                        });
                        let tint = row.tint.as_deref().map(|token| theme::color(cx, token).opacity(0.22));
                        let is_selected = selected.as_deref() == Some(key.as_str());
                        let (clicked, menu_key, selector) = (key.clone(), key.clone(), key.clone());
                        div()
                            .id(SharedString::from(format!("row:{key}")))
                            .debug_selector(move || format!("db-tree-row-{selector}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + INDENT * row.depth as f32))
                            .pr(px(8.0))
                            .text_color(color)
                            .when_some(tint, |el, tint| el.bg(tint))
                            .when(is_selected, |el| el.bg(selected_bg))
                            .child(chevron)
                            .children(icon)
                            .child(div().flex_none().whitespace_nowrap().child(SharedString::from(row.label.clone())))
                            .when(row.global, |el| {
                                el.child(div().flex_none().text_color(muted).child(SharedString::from(global.clone())))
                            })
                            .children(detail)
                            .when(!row.loading, |el| {
                                el.on_click(cx.listener(move |tree, event: &ClickEvent, window, cx| {
                                    window.focus(&tree.focus, cx);
                                    tree.query.clear();
                                    tree.select(&clicked, cx);
                                    if event.click_count() >= 2 {
                                        tree.activate(&clicked, cx);
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |tree, event: &MouseDownEvent, window, cx| {
                                        window.focus(&tree.focus, cx);
                                        tree.select(&menu_key, cx);
                                        cx.emit(TreeEvent::ShowMenu {
                                            position: event.position,
                                            node: Some(menu_key.clone()),
                                        });
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
            .id("database-tree-host")
            .debug_selector(|| "db-tree".to_owned())
            .relative()
            .flex_1()
            .min_h_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|tree, event: &MouseDownEvent, window, cx| {
                    window.focus(&tree.focus, cx);
                    cx.emit(TreeEvent::ShowMenu { position: event.position, node: None });
                }),
            )
            .child(list)
            .when(!self.query.is_empty(), |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(0.0))
                        .left(px(8.0))
                        .px(px(6.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .bg(theme::color(cx, "bg-island"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .child(SharedString::from(self.query.clone())),
                )
            })
    }
}

impl Focusable for DatabaseTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DatabaseTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let body = if self.data.loaded && self.data.sources.is_empty() {
            self.render_empty(cx).into_any_element()
        } else {
            self.render_tree(window, cx).into_any_element()
        };
        div().size_full().flex().flex_col().text_size(theme::length(cx, "fs-ui")).child(toolbar).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Bounds, Modifiers, TestAppContext, VisualTestContext};
    use ide_db::{ColumnInfo, DbColor, DbIntrospection, DbObjectKind, DbOptions, DbTableKind, ForeignKey, RoutineInfo};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<TreeEvent>>>;

    /// The tree in a window of its own, and what it has asked of the window so far.
    fn open(cx: &mut TestAppContext) -> (Entity<DatabaseTree>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
        });
        let (tree, cx) = cx.add_window_view(|_, cx| DatabaseTree::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&tree, move |_, event: &TreeEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (tree, events, cx)
    }

    fn pg(single_database: bool) -> DataSource {
        DataSource {
            id: "pg".into(),
            name: "PostgreSQL".into(),
            driver: DbDriver::Postgres,
            vendor: "postgres".into(),
            options: DbOptions { single_database, ..DbOptions::default() },
            ..DataSource::default()
        }
    }

    fn routine(name: &str, kind: DbObjectKind, arguments: &str, returns: Option<&str>) -> RoutineInfo {
        RoutineInfo { name: name.into(), kind, arguments: arguments.into(), returns: returns.map(str::to_owned) }
    }

    fn routines() -> SchemaObjects {
        SchemaObjects {
            routines: vec![
                routine("calculate", DbObjectKind::Function, "integer", Some("integer")),
                routine("calculate", DbObjectKind::Function, "text", Some("text")),
                routine("refresh_data", DbObjectKind::Procedure, "", None),
            ],
            sequences: Vec::new(),
        }
    }

    /// One PostgreSQL data source with schema `public`, its routines loaded, and `expanded` open.
    fn fill(tree: &Entity<DatabaseTree>, source: DataSource, expanded: &[&str], cx: &mut VisualTestContext) {
        tree.update(cx, |tree, cx| {
            tree.set_sources(vec![source], cx);
            tree.schemas_loaded("pg", Ok(vec!["public".into()]), cx);
            tree.tables_loaded("pg", "public", Ok(Vec::new()), Some(routines()), cx);
            for key in expanded {
                tree.set_expanded(key, true, cx);
            }
        });
        cx.run_until_parked();
    }

    fn lines(tree: &Entity<DatabaseTree>, cx: &mut VisualTestContext) -> Vec<String> {
        tree.read_with(cx, |tree, _| tree.lines())
    }

    fn labels(tree: &Entity<DatabaseTree>, cx: &mut VisualTestContext) -> Vec<String> {
        tree.read_with(cx, |tree, _| tree.rows().iter().map(|row| row.label.clone()).collect())
    }

    fn count_of(tree: &Entity<DatabaseTree>, label: &str, cx: &mut VisualTestContext) -> usize {
        labels(tree, cx).iter().filter(|l| *l == label).count()
    }

    fn row_of(tree: &Entity<DatabaseTree>, label: &str, cx: &mut VisualTestContext) -> TreeRow {
        tree.read_with(cx, |tree, _| tree.rows().iter().find(|row| row.label == label).cloned())
            .unwrap_or_else(|| panic!("a row `{label}`"))
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
    }

    fn selected(tree: &Entity<DatabaseTree>, cx: &mut VisualTestContext) -> Option<String> {
        tree.read_with(cx, |tree, _| tree.selected().map(str::to_owned))
    }

    fn menu_labels(items: &[MenuItem<TreeCommand>]) -> Vec<String> {
        items.iter().map(|item| item.label().unwrap_or("-").to_owned()).collect()
    }

    #[gpui_kit::test]
    async fn keeps_an_existing_expanded_routines_folder_and_both_overloads_by_default(cx: &mut TestAppContext) {
        let (tree, _, cx) = open(cx);
        fill(&tree, pg(true), &["pg", "pg/public", "pg/public/routines"], cx);
        assert_eq!(
            lines(&tree, cx),
            [
                "v PostgreSQL 1",
                "  v public",
                "    v routines 3",
                "      calculate (integer): integer",
                "      calculate (text): text",
                "      refresh_data ()",
            ]
        );
        assert_eq!(count_of(&tree, "procedures", cx) + count_of(&tree, "functions", cx), 0);
        click(cx, "db-tree-row-pg/public/routines/calculate(text)");
        assert_eq!(selected(&tree, cx).as_deref(), Some("pg/public/routines/calculate(text)"));
        tree.update(cx, |tree, cx| tree.set_filter("refresh", cx));
        assert_eq!(count_of(&tree, "calculate", cx), 0);
        assert_eq!(count_of(&tree, "refresh_data", cx), 1);
    }

    #[gpui_kit::test]
    async fn toggles_through_the_action_persists_the_option_and_restores_the_old_expansion(cx: &mut TestAppContext) {
        let (tree, _, cx) = open(cx);
        assert!(!tree.read_with(cx, |tree, _| tree.separate()));
        fill(&tree, pg(true), &["pg", "pg/public", "pg/public/routines"], cx);
        let toggle = TreeCommand { action: TreeAction::ToggleSeparateProceduresAndFunctions, node: None };
        tree.update(cx, |tree, cx| tree.perform(toggle.clone(), cx));
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(VIEW_OPTIONS_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some(r#"{"separateProceduresAndFunctions":true}"#));
        assert_eq!(count_of(&tree, "routines", cx), 0);
        assert_eq!(row_of(&tree, "procedures", cx).detail.as_deref(), Some("1"));
        assert_eq!(row_of(&tree, "functions", cx).detail.as_deref(), Some("2"));
        click(cx, "db-tree-toggle-pg/public/functions");
        assert_eq!(count_of(&tree, "calculate", cx), 2);
        click(cx, "db-tree-row-pg/public/functions/calculate(text)");
        assert_eq!(selected(&tree, cx).as_deref(), Some("pg/public/functions/calculate(text)"));
        tree.update(cx, |tree, cx| tree.perform(toggle, cx));
        assert!(row_of(&tree, "routines", cx).expanded);
        assert_eq!(count_of(&tree, "calculate", cx), 2);
        assert_eq!(count_of(&tree, "refresh_data", cx), 1);
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(VIEW_OPTIONS_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some(r#"{"separateProceduresAndFunctions":false}"#));
    }

    #[gpui_kit::test]
    async fn applies_object_and_quick_filters_before_grouping(cx: &mut TestAppContext) {
        for separate in [false, true] {
            let (tree, _, cx) = open(cx);
            tree.update(cx, |tree, cx| tree.set_separate(separate, cx));
            let source = DataSource {
                introspection: DbIntrospection { object_filter: "calculate".into(), ..DbIntrospection::default() },
                ..pg(true)
            };
            fill(&tree, source, &["pg", "pg/public", "pg/public/routines", "pg/public/functions"], cx);
            assert_eq!(count_of(&tree, "procedures", cx), 0, "separate: {separate}");
            assert_eq!(count_of(&tree, "refresh_data", cx), 0);
            assert_eq!(count_of(&tree, "calculate", cx), 2);
            let folder = row_of(&tree, if separate { "functions" } else { "routines" }, cx);
            assert_eq!(folder.detail.as_deref(), Some("2"));
            tree.update(cx, |tree, cx| tree.set_filter("refresh", cx));
            assert_eq!(count_of(&tree, "public", cx), 0);
            assert_eq!(count_of(&tree, "routines", cx) + count_of(&tree, "functions", cx), 0);
        }
    }

    #[gpui_kit::test]
    async fn expanding_a_server_asks_for_its_databases_and_lists_them_with_counts(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        tree.update(cx, |tree, cx| tree.set_sources(vec![pg(false)], cx));
        assert_eq!(lines(&tree, cx), ["> PostgreSQL"]);
        click(cx, "db-tree-toggle-pg");
        assert_eq!(*events.borrow(), [TreeEvent::Load(TreeLoad::Databases { id: "pg".into() })]);
        // While it loads, a placeholder; expanding again does not ask twice.
        assert_eq!(lines(&tree, cx), ["v PostgreSQL", "  Loading…"]);
        tree.update(cx, |tree, cx| {
            tree.set_expanded("pg", false, cx);
            tree.set_expanded("pg", true, cx);
        });
        assert_eq!(events.borrow().len(), 1);
        let databases = Databases { current: "app".into(), names: vec!["app".into(), "other".into()] };
        tree.update(cx, |tree, cx| tree.databases_loaded("pg", Ok(databases), cx));
        assert_eq!(lines(&tree, cx), ["v PostgreSQL 2", "  > app", "  > other"]);
        // The connected database is the data source itself; another one is `<id>:<name>`.
        click(cx, "db-tree-toggle-pg/@other");
        assert_eq!(events.borrow().last(), Some(&TreeEvent::Load(TreeLoad::Schemas { target: "pg:other".into() })));
        tree.update(cx, |tree, cx| tree.schemas_loaded("pg:other", Err("permission denied".into()), cx));
        let error = row_of(&tree, "permission denied", cx);
        assert!(error.error && error.icon == Some("error"));
        assert_eq!(error.node.key, "pg/@other/err");
        assert_eq!(row_of(&tree, "other", cx).detail, None);
    }

    fn details() -> TableDetails {
        let column = |name: &str, data_type: &str, primary_key: bool| ColumnInfo {
            name: name.into(),
            data_type: data_type.into(),
            nullable: !primary_key,
            default: None,
            primary_key,
        };
        TableDetails {
            columns: vec![column("id", "integer", true), column("owner_id", "integer", false)],
            indexes: Vec::new(),
            foreign_keys: vec![ForeignKey {
                schema: "public".into(),
                table: "pets".into(),
                columns: vec!["owner_id".into()],
                ref_schema: "public".into(),
                ref_table: "owners".into(),
                ref_columns: vec!["id".into()],
            }],
            triggers: Vec::new(),
        }
    }

    /// A SQLite data source with table `pets` under `main`, expanded down to its tables.
    fn sqlite_with_pets(tree: &Entity<DatabaseTree>, cx: &mut VisualTestContext) {
        let source = DataSource { id: "lite".into(), name: "app.db".into(), ..DataSource::default() };
        let pets = TableInfo { name: "pets".into(), kind: DbTableKind::Table };
        tree.update(cx, |tree, cx| {
            tree.set_sources(vec![source], cx);
            tree.schemas_loaded("lite", Ok(vec!["main".into()]), cx);
            tree.tables_loaded("lite", "main", Ok(vec![pets]), None, cx);
            for key in ["lite", "lite/main", "lite/main/tables"] {
                tree.set_expanded(key, true, cx);
            }
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn navigates_with_the_keyboard_and_opens_a_table_s_data_on_enter(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        sqlite_with_pets(&tree, cx);
        cx.update(|window, cx| {
            let focus = tree.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down down down down");
        assert_eq!(selected(&tree, cx).as_deref(), Some("lite/main/tables/pets"));
        // Right on a table asks for its details; the answer brings its groups.
        cx.simulate_keystrokes("right");
        let details_load =
            TreeEvent::Load(TreeLoad::Details { target: "lite".into(), schema: "main".into(), table: "pets".into() });
        assert_eq!(events.borrow().last(), Some(&details_load));
        tree.update(cx, |tree, cx| tree.details_loaded("lite", "main", "pets", Ok(details()), cx));
        assert_eq!(
            lines(&tree, cx)[3..],
            ["      v pets", "        > columns 2", "        > keys 1", "        > foreign keys 1"]
        );
        // Right steps onto the first group, Right opens it; Left closes it, Left goes up.
        cx.simulate_keystrokes("right right");
        assert_eq!(count_of(&tree, "owner_id", cx), 1);
        assert_eq!(row_of(&tree, "owner_id", cx).icon, Some("db-column-fk"));
        assert_eq!(row_of(&tree, "id", cx).icon, Some("db-column-key"));
        cx.simulate_keystrokes("left left");
        assert_eq!(selected(&tree, cx).as_deref(), Some("lite/main/tables/pets"));
        events.borrow_mut().clear();
        cx.simulate_keystrokes("enter");
        let opened = TableAt {
            id: "lite".into(),
            schema: "main".into(),
            table: "pets".into(),
            view: false,
            kind: DbTableKind::Table,
        };
        assert_eq!(*events.borrow(), [TreeEvent::OpenTableData(opened)]);
        // Speed search: typing selects the first row whose label contains it; Escape ends it.
        cx.simulate_keystrokes("home m a");
        assert_eq!(selected(&tree, cx).as_deref(), Some("lite/main"));
        cx.simulate_keystrokes("escape");
        assert_eq!(tree.read_with(cx, |tree, _| tree.query.clone()), "");
    }

    #[gpui_kit::test]
    async fn offers_the_context_menu_of_the_clicked_node_and_refreshes_through_it(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        sqlite_with_pets(&tree, cx);
        let at = bounds_of(cx, "db-tree-row-lite/main/tables/pets").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        let shown = events.borrow().last().cloned();
        let Some(TreeEvent::ShowMenu { node, .. }) = shown else { panic!("a menu is asked for, not {shown:?}") };
        assert_eq!(node.as_deref(), Some("lite/main/tables/pets"));
        assert_eq!(selected(&tree, cx), node);
        let menu = tree.read_with(cx, |tree, _| tree.menu_for(node.as_deref()));
        assert_eq!(
            menu_labels(&menu),
            [
                "New",
                "Modify Table…",
                "Copy/Paste",
                "Edit Data",
                "Refresh",
                "-",
                "Rename…",
                "-",
                "Quick Documentation",
                "Navigation",
                "-",
                "SQL Scripts",
                "Import/Export",
                "Object Actions",
                "-",
                "Drop…",
            ]
        );
        let refresh = menu.iter().find_map(|item| item.command().filter(|c| c.action == TreeAction::Refresh)).cloned();
        tree.update(cx, |tree, cx| tree.perform(refresh.expect("a Refresh row"), cx));
        assert_eq!(
            events.borrow().last(),
            Some(&TreeEvent::Load(TreeLoad::Details {
                target: "lite".into(),
                schema: "main".into(),
                table: "pets".into()
            }))
        );
        // Anything else goes to the window, with the node it was asked on.
        let rename = TreeCommand { action: TreeAction::Rename, node: node.clone() };
        tree.update(cx, |tree, cx| tree.perform(rename, cx));
        let Some(TreeEvent::Run { action, node: Some(on) }) = events.borrow().last().cloned() else {
            panic!("Rename runs in the window")
        };
        assert_eq!((action, on.key.as_str()), (TreeAction::Rename, "lite/main/tables/pets"));
        // The empty area offers New only.
        let empty = tree.read_with(cx, |tree, _| tree.menu_for(None));
        assert_eq!(menu_labels(&empty), ["New"]);
    }

    #[gpui_kit::test]
    async fn shows_the_empty_state_with_a_new_data_source_button(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("db-tree-empty").is_none(), "nothing is said before the sources are read");
        tree.update(cx, |tree, cx| tree.set_sources(Vec::new(), cx));
        assert_eq!(tree.update(cx, |tree, cx| tree.empty_text(cx)).as_deref(), Some("No data sources"));
        click(cx, "db-tree-new-source");
        assert_eq!(*events.borrow(), [TreeEvent::Run { action: TreeAction::NewDataSource(None), node: None }]);
    }

    #[gpui_kit::test]
    async fn marks_connected_sources_tints_coloured_ones_and_enables_the_toolbar(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        let source = DataSource { color: Some(DbColor::Red), global: true, ..pg(true) };
        fill(&tree, source, &["pg"], cx);
        let row = row_of(&tree, "PostgreSQL", cx);
        assert!(!row.live && row.global);
        assert_eq!(row.tint.as_deref(), Some("db-color-red"));
        // Every row of the data source carries its colour.
        assert_eq!(row_of(&tree, "public", cx).tint.as_deref(), Some("db-color-red"));
        // Deactivate is off until something is connected.
        click(cx, "db-tree-disconnect");
        assert!(events.borrow().is_empty());
        tree.update(cx, |tree, cx| tree.set_connected(BTreeSet::from(["pg".to_owned()]), cx));
        assert!(row_of(&tree, "PostgreSQL", cx).live);
        click(cx, "db-tree-disconnect");
        assert_eq!(*events.borrow(), [TreeEvent::Run { action: TreeAction::Disconnect, node: None }]);
        // Edit Data and DDL need a table; Jump to Query Console falls back on the only source.
        events.borrow_mut().clear();
        click(cx, "db-tree-edit-data");
        click(cx, "db-tree-ddl");
        assert!(events.borrow().is_empty());
        click(cx, "db-tree-console");
        assert_eq!(*events.borrow(), [TreeEvent::Run { action: TreeAction::JumpToConsole, node: None }]);
    }

    #[gpui_kit::test]
    async fn a_quick_filter_loads_the_tables_of_the_schemas_of_expanded_sources(cx: &mut TestAppContext) {
        let (tree, events, cx) = open(cx);
        tree.update(cx, |tree, cx| {
            tree.set_sources(vec![pg(true)], cx);
            tree.schemas_loaded("pg", Ok(vec!["public".into(), "audit".into()]), cx);
            tree.tables_loaded("pg", "public", Ok(Vec::new()), None, cx);
            tree.set_expanded("pg", true, cx);
        });
        assert!(events.borrow().is_empty());
        let focus = tree.read_with(cx, |tree, cx| tree.filter_field().focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.run_until_parked();
        cx.simulate_input("log");
        assert_eq!(tree.read_with(cx, |tree, _| tree.filter().to_owned()), "log");
        // Only the schema never loaded is asked for, and only once.
        assert_eq!(
            *events.borrow(),
            [TreeEvent::Load(TreeLoad::Tables { target: "pg".into(), schema: "audit".into() })]
        );
        let log = TableInfo { name: "log".into(), kind: DbTableKind::Table };
        tree.update(cx, |tree, cx| tree.tables_loaded("pg", "audit", Ok(vec![log]), None, cx));
        assert_eq!(lines(&tree, cx), ["v PostgreSQL 2", "  > audit"]);
    }
}
