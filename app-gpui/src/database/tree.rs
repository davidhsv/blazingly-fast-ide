//! The Database tree's schema level: the folders a schema shows (with their member counts) and
//! their members, after the Schemas tab's object filter and the toolbar's quick filter; whether a
//! schema shows at all under a quick filter; and the persisted *Separate Procedures and
//! Functions* view option.

use super::model::{passes_object_filter, passes_tree_filter};
use super::nodes::{Folder, routine_folder, table_folder};
use ide_db::{RoutineInfo, SchemaObjects, SequenceInfo, TableInfo};

/// The UI storage key of the tree's view options.
pub const VIEW_OPTIONS_KEY: &str = "openidea.database.viewOptions";

/// The *Separate Procedures and Functions* option as stored (off unless stored on).
pub fn read_separate_procedures_and_functions(stored: Option<&str>) -> bool {
    stored
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get("separateProceduresAndFunctions").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// The stored form of the view options.
pub fn view_options_json(separate: bool) -> String {
    serde_json::json!({ "separateProceduresAndFunctions": separate }).to_string()
}

const FOLDERS: [Folder; 5] =
    [Folder::Tables, Folder::Views, Folder::MaterializedViews, Folder::Routines, Folder::Sequences];
const SEPARATE_FOLDERS: [Folder; 6] = [
    Folder::Tables,
    Folder::Views,
    Folder::MaterializedViews,
    Folder::Procedures,
    Folder::Functions,
    Folder::Sequences,
];

/// A folder's name: its label and the last part of its node key.
pub fn folder_name(folder: Folder) -> &'static str {
    match folder {
        Folder::Tables => "tables",
        Folder::Views => "views",
        Folder::MaterializedViews => "materializedViews",
        Folder::Routines => "routines",
        Folder::Procedures => "procedures",
        Folder::Functions => "functions",
        Folder::Sequences => "sequences",
    }
}

/// A member of a schema folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderMember<'a> {
    Table(&'a TableInfo),
    Routine(&'a RoutineInfo),
    Sequence(&'a SequenceInfo),
}

/// What the tree knows of one schema, and the filters on it.
#[derive(Clone, Copy, Debug)]
pub struct SchemaView<'a> {
    /// The schema's tables and views; `None` until loaded.
    pub tables: Option<&'a [TableInfo]>,
    pub objects: Option<&'a SchemaObjects>,
    /// Schemas tab › Object filter of the data source.
    pub object_filter: &'a str,
    /// The toolbar's quick filter.
    pub tree_filter: &'a str,
    pub separate: bool,
}

impl<'a> SchemaView<'a> {
    fn shown(&self, name: &str) -> bool {
        passes_object_filter(name, Some(self.object_filter)) && passes_tree_filter(name, Some(self.tree_filter))
    }

    pub fn tables(&self) -> Option<Vec<&'a TableInfo>> {
        self.tables.map(|t| t.iter().filter(|t| self.shown(&t.name)).collect())
    }

    pub fn routines(&self) -> Vec<&'a RoutineInfo> {
        self.objects.map(|o| o.routines.iter().filter(|r| self.shown(&r.name)).collect()).unwrap_or_default()
    }

    pub fn sequences(&self) -> Vec<&'a SequenceInfo> {
        self.objects.map(|o| o.sequences.iter().filter(|s| self.shown(&s.name)).collect()).unwrap_or_default()
    }

    /// The members of a folder, after object and quick filtering.
    pub fn members(&self, folder: Folder) -> Vec<FolderMember<'a>> {
        match folder {
            Folder::Routines | Folder::Procedures | Folder::Functions => self
                .routines()
                .into_iter()
                .filter(|r| routine_folder(r.kind, self.separate) == folder)
                .map(FolderMember::Routine)
                .collect(),
            Folder::Sequences => self.sequences().into_iter().map(FolderMember::Sequence).collect(),
            Folder::Tables | Folder::Views | Folder::MaterializedViews => self
                .tables()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| table_folder(t.kind) == folder)
                .map(FolderMember::Table)
                .collect(),
        }
    }

    /// The folders the schema shows, each with its member count; empty folders are left out.
    pub fn folders(&self) -> Vec<(Folder, usize)> {
        let all: &[Folder] = if self.separate { &SEPARATE_FOLDERS } else { &FOLDERS };
        all.iter().map(|&f| (f, self.members(f).len())).filter(|&(_, n)| n > 0).collect()
    }

    /// Under a quick filter a schema shows when its name matches, or its loaded objects do.
    pub fn visible(&self, schema: &str) -> bool {
        passes_tree_filter(schema, Some(self.tree_filter))
            || self.tables().is_some_and(|t| !t.is_empty())
            || !self.routines().is_empty()
            || !self.sequences().is_empty()
    }
}

/// The node key of a folder member under `folder_key`; overloads share a name, so a routine's key
/// tells them apart by its arguments.
pub fn member_key(folder_key: &str, member: FolderMember<'_>) -> String {
    match member {
        FolderMember::Table(t) => format!("{folder_key}/{}", t.name),
        FolderMember::Routine(r) => format!("{folder_key}/{}({})", r.name, r.arguments),
        FolderMember::Sequence(s) => format!("{folder_key}/{}", s.name),
    }
}

/// The node key of a folder of schema node `schema_key`.
pub fn folder_key(schema_key: &str, folder: Folder) -> String {
    format!("{schema_key}/{}", folder_name(folder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::DbObjectKind;

    fn objects() -> SchemaObjects {
        let routine = |name: &str, kind, arguments: &str, returns: Option<&str>| RoutineInfo {
            name: name.into(),
            kind,
            arguments: arguments.into(),
            returns: returns.map(str::to_string),
        };
        SchemaObjects {
            routines: vec![
                routine("calculate", DbObjectKind::Function, "integer", Some("integer")),
                routine("calculate", DbObjectKind::Function, "text", Some("text")),
                routine("refresh_data", DbObjectKind::Procedure, "", None),
            ],
            sequences: Vec::new(),
        }
    }

    fn view<'a>(
        objects: &'a SchemaObjects,
        object_filter: &'a str,
        tree_filter: &'a str,
        separate: bool,
    ) -> SchemaView<'a> {
        SchemaView { tables: Some(&[]), objects: Some(objects), object_filter, tree_filter, separate }
    }

    fn names(members: &[FolderMember<'_>]) -> Vec<String> {
        members
            .iter()
            .map(|m| match m {
                FolderMember::Table(t) => t.name.clone(),
                FolderMember::Routine(r) => r.name.clone(),
                FolderMember::Sequence(s) => s.name.clone(),
            })
            .collect()
    }

    #[test]
    fn keeps_an_existing_expanded_routines_folder_and_both_overloads_by_default() {
        let objects = objects();
        let v = view(&objects, "", "", false);
        assert_eq!(v.folders(), [(Folder::Routines, 3)]);
        let members = v.members(Folder::Routines);
        assert_eq!(names(&members), ["calculate", "calculate", "refresh_data"]);
        let folder = folder_key("pg/public", Folder::Routines);
        assert_eq!(member_key(&folder, members[1]), "pg/public/routines/calculate(text)");
        let filtered = view(&objects, "", "refresh", false);
        assert_eq!(names(&filtered.members(Folder::Routines)), ["refresh_data"]);
    }

    #[test]
    fn toggles_through_the_action_persists_the_option_and_restores_the_old_expansion() {
        assert!(!read_separate_procedures_and_functions(None));
        let stored = view_options_json(true);
        assert!(read_separate_procedures_and_functions(Some(&stored)));
        let objects = objects();
        let v = view(&objects, "", "", true);
        assert_eq!(v.folders(), [(Folder::Procedures, 1), (Folder::Functions, 2)]);
        let functions = v.members(Folder::Functions);
        assert_eq!(names(&functions), ["calculate", "calculate"]);
        let folder = folder_key("pg/public", Folder::Functions);
        assert_eq!(member_key(&folder, functions[1]), "pg/public/functions/calculate(text)");
        // Back to one folder: the `routines` key the tree kept expanded is the folder's again.
        assert!(!read_separate_procedures_and_functions(Some(&view_options_json(false))));
        let v = view(&objects, "", "", false);
        assert_eq!(folder_key("pg/public", v.folders()[0].0), "pg/public/routines");
    }

    #[test]
    fn applies_object_and_quick_filters_before_grouping() {
        let objects = objects();
        for separate in [false, true] {
            let v = view(&objects, "calculate", "", separate);
            let folder = if separate { Folder::Functions } else { Folder::Routines };
            assert_eq!(v.folders(), [(folder, 2)], "separate: {separate}");
            assert_eq!(names(&v.members(folder)), ["calculate", "calculate"]);
            let quick = view(&objects, "calculate", "refresh", separate);
            assert!(!quick.visible("public"));
            assert!(quick.folders().is_empty());
        }
    }
}
