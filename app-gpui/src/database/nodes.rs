//! Nodes of the Database tool window tree and the objects they stand for.

use super::objects::{ObjectKind, ObjectRef};
use ide_db::{
    ColumnInfo, DataSource, DbObjectKind, DbTableKind, ForeignKey, IndexInfo, RoutineInfo, SequenceInfo, TableInfo,
    TriggerInfo,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Columns,
    Keys,
    ForeignKeys,
    Indexes,
    Triggers,
}

/// Schema folders; routines can optionally be separated into procedures and functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Folder {
    Tables,
    Views,
    MaterializedViews,
    Routines,
    Procedures,
    Functions,
    Sequences,
}

/// The folder a table, view or materialized view is listed in.
pub fn table_folder(kind: DbTableKind) -> Folder {
    match kind {
        DbTableKind::Table => Folder::Tables,
        DbTableKind::View => Folder::Views,
        DbTableKind::MaterializedView => Folder::MaterializedViews,
    }
}

/// The *Separate Procedures and Functions* option: the folder a routine is listed in.
pub fn routine_folder(kind: DbObjectKind, separate: bool) -> Folder {
    match (separate, kind) {
        (false, _) => Folder::Routines,
        (true, DbObjectKind::Procedure) => Folder::Procedures,
        (true, _) => Folder::Functions,
    }
}

/// Where a table-level node lives: connection target, schema, table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InTable {
    pub id: String,
    pub schema: String,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Source(Box<DataSource>),
    /// A database of a server; `id` is its connection target.
    Database {
        id: String,
        name: String,
        current: bool,
    },
    Schema {
        id: String,
        schema: String,
    },
    Folder {
        id: String,
        schema: String,
        folder: Folder,
    },
    Table {
        id: String,
        schema: String,
        table: TableInfo,
    },
    Routine {
        id: String,
        schema: String,
        routine: RoutineInfo,
    },
    Sequence {
        id: String,
        schema: String,
        sequence: SequenceInfo,
    },
    Group {
        at: InTable,
        group: Group,
    },
    Column {
        at: InTable,
        column: ColumnInfo,
        foreign_key: bool,
    },
    Key {
        at: InTable,
        columns: Vec<String>,
    },
    ForeignKey {
        at: InTable,
        fk: ForeignKey,
    },
    Index {
        at: InTable,
        index: IndexInfo,
    },
    Trigger {
        at: InTable,
        trigger: TriggerInfo,
    },
    Message {
        text: String,
        error: bool,
    },
}

/// A tree node: its key (`<target>/…`) and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct DbNode {
    pub key: String,
    pub kind: NodeKind,
}

impl DbNode {
    /// The connection target the node belongs to.
    pub fn target(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Source(s) => Some(s.id.as_str()),
            NodeKind::Database { id, .. }
            | NodeKind::Schema { id, .. }
            | NodeKind::Folder { id, .. }
            | NodeKind::Table { id, .. }
            | NodeKind::Routine { id, .. }
            | NodeKind::Sequence { id, .. } => Some(id.as_str()),
            NodeKind::Group { at, .. }
            | NodeKind::Column { at, .. }
            | NodeKind::Key { at, .. }
            | NodeKind::ForeignKey { at, .. }
            | NodeKind::Index { at, .. }
            | NodeKind::Trigger { at, .. } => Some(at.id.as_str()),
            NodeKind::Message { .. } => self.key.split('/').next(),
        }
    }

    fn in_table(&self) -> Option<&InTable> {
        match &self.kind {
            NodeKind::Group { at, .. }
            | NodeKind::Column { at, .. }
            | NodeKind::Key { at, .. }
            | NodeKind::ForeignKey { at, .. }
            | NodeKind::Index { at, .. }
            | NodeKind::Trigger { at, .. } => Some(at),
            _ => None,
        }
    }

    /// The table (or view) the node is, or is inside of.
    pub fn table_of(&self) -> Option<TableAt> {
        if let NodeKind::Table { id, schema, table } = &self.kind {
            return Some(TableAt {
                id: id.clone(),
                schema: schema.clone(),
                table: table.name.clone(),
                view: table.kind != DbTableKind::Table,
                kind: table.kind,
            });
        }
        self.in_table().map(|at| TableAt {
            id: at.id.clone(),
            schema: at.schema.clone(),
            table: at.table.clone(),
            view: false,
            kind: DbTableKind::Table,
        })
    }

    /// A routine, trigger or sequence node: what Go to DDL / Quick Documentation read.
    pub fn schema_object_of(&self) -> Option<SchemaObjectAt> {
        let (id, schema, kind, name) = match &self.kind {
            NodeKind::Routine { id, schema, routine } => (id, schema, routine.kind, &routine.name),
            NodeKind::Sequence { id, schema, sequence } => (id, schema, DbObjectKind::Sequence, &sequence.name),
            NodeKind::Trigger { at, trigger } => (&at.id, &at.schema, DbObjectKind::Trigger, &trigger.name),
            _ => return None,
        };
        Some(SchemaObjectAt { id: id.clone(), schema: schema.clone(), kind, name: name.clone() })
    }

    /// The schema object the node stands for (Rename, Drop, Copy Reference).
    pub fn object_of(&self) -> Option<ObjectRef> {
        let o = match &self.kind {
            NodeKind::Schema { schema, .. } => ObjectRef::new(ObjectKind::Schema, schema, schema),
            NodeKind::Table { schema, table, .. } => ObjectRef::new(
                match table.kind {
                    DbTableKind::Table => ObjectKind::Table,
                    DbTableKind::View => ObjectKind::View,
                    DbTableKind::MaterializedView => ObjectKind::MaterializedView,
                },
                schema,
                &table.name,
            ),
            NodeKind::Column { at, column, .. } => ObjectRef {
                column: Some(column.name.clone()),
                ..ObjectRef::new(ObjectKind::Column, &at.schema, &at.table)
            },
            NodeKind::Index { at, index } => ObjectRef {
                table: Some(at.table.clone()),
                ..ObjectRef::new(ObjectKind::Index, &at.schema, &index.name)
            },
            NodeKind::Routine { schema, routine, .. } => ObjectRef {
                arguments: Some(routine.arguments.clone()),
                ..ObjectRef::new(
                    if routine.kind == DbObjectKind::Procedure { ObjectKind::Procedure } else { ObjectKind::Function },
                    schema,
                    &routine.name,
                )
            },
            NodeKind::Sequence { schema, sequence, .. } => ObjectRef::new(ObjectKind::Sequence, schema, &sequence.name),
            NodeKind::Trigger { at, trigger } => ObjectRef {
                table: Some(at.table.clone()),
                ..ObjectRef::new(ObjectKind::Trigger, &at.schema, &trigger.name)
            },
            _ => return None,
        };
        Some(o)
    }
}

/// A table a node is or is inside of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableAt {
    pub id: String,
    pub schema: String,
    pub table: String,
    pub view: bool,
    pub kind: DbTableKind,
}

/// A routine, trigger or sequence of a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaObjectAt {
    pub id: String,
    pub schema: String,
    pub kind: DbObjectKind,
    pub name: String,
}

/// The table editor's Go to Column list: every column for an empty query, else prefix matches
/// first, then other matches, ignoring case.
pub fn filter_columns(names: &[&str], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..names.len()).collect();
    }
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let starts = (0..names.len()).filter(|&i| lower[i].starts_with(&q));
    let contains = (0..names.len()).filter(|&i| !lower[i].starts_with(&q) && lower[i].contains(&q));
    starts.chain(contains).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_routines_by_default_and_separates_them_on_request() {
        assert_eq!(routine_folder(DbObjectKind::Procedure, false), Folder::Routines);
        assert_eq!(routine_folder(DbObjectKind::Function, false), Folder::Routines);
        assert_eq!(routine_folder(DbObjectKind::Procedure, true), Folder::Procedures);
        assert_eq!(routine_folder(DbObjectKind::Function, true), Folder::Functions);
    }

    #[test]
    fn keeps_tables_views_and_materialized_views_apart() {
        assert_eq!(table_folder(DbTableKind::Table), Folder::Tables);
        assert_eq!(table_folder(DbTableKind::View), Folder::Views);
        assert_eq!(table_folder(DbTableKind::MaterializedView), Folder::MaterializedViews);
    }

    const COLUMNS: [&str; 5] = ["id", "user_id", "name", "created_at", "username"];

    #[test]
    fn lists_every_column_for_an_empty_query() {
        assert_eq!(filter_columns(&COLUMNS, " "), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn puts_prefix_matches_first_then_other_matches_ignoring_case() {
        assert_eq!(filter_columns(&COLUMNS, "US"), [1, 4]);
        assert_eq!(filter_columns(&COLUMNS, "name"), [2, 4]);
        assert_eq!(filter_columns(&COLUMNS, "_id"), [1]);
    }
}
