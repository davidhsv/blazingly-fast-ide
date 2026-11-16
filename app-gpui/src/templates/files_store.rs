//! File and Code Templates in effect: the built-in file templates, includes and code templates with
//! the user's customisations, kept per scheme as IDEA does. `Default` is stored for all projects
//! under [`FILE_TEMPLATES_KEY`]; `Project` is a copy for one project, stored under a key naming its
//! root and seeded from Default the first time it is chosen. Which scheme a project uses is stored
//! under a key of its own. Templates plugins contribute are treated like built-ins: customised,
//! reset and hidden the same way.

use super::files_builtins::{builtin_code_templates, builtin_file_templates, builtin_includes};
use super::files_model::{FileTemplate, TemplateFragment, is_plain_extension};
use super::storage::{StorageWrite, load_object};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};
use std::fmt;
use std::marker::PhantomData;

/// Storage key of the Default scheme, and the start of the per-project keys.
pub const FILE_TEMPLATES_KEY: &str = "openidea.fileTemplates.v1";

/// Where the scheme the project at `root` uses is stored.
pub fn scheme_key(root: &str) -> String {
    format!("{FILE_TEMPLATES_KEY}.scheme:{root}")
}

/// Where the Project scheme of the project at `root` is stored.
pub fn project_key(root: &str) -> String {
    format!("{FILE_TEMPLATES_KEY}.project:{root}")
}

/// The set of customisations a project uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TemplateScheme {
    #[default]
    Default,
    Project,
}

impl TemplateScheme {
    /// The name the scheme is stored under.
    pub fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Project => "project",
        }
    }
}

/// Values by key, in the order they were added: stored as one JSON object, as the other app keeps
/// them, so the user's own entries come back in the order they were made.
#[derive(Debug, Clone, PartialEq)]
pub struct Ordered<T>(Vec<(String, T)>);

impl<T> Default for Ordered<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Ordered<T> {
    pub fn get(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, value)| value)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.0.iter().map(|(_, value)| value)
    }

    /// Puts `value` under `key`; a key already there keeps its place.
    pub fn insert(&mut self, key: &str, value: T) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.0.retain(|(k, _)| k != key);
    }
}

impl<T: Serialize> Serialize for Ordered<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Ordered<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<T>(PhantomData<T>);

        impl<'de, T: DeserializeOwned> Visitor<'de> for Entries<T> {
            type Value = Ordered<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Ordered::default();
                while let Some((key, value)) = map.next_entry::<String, T>()? {
                    out.insert(&key, value);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_map(Entries(PhantomData))
    }
}

/// What the user changed in one scheme.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileTemplateCustomizations {
    /// Edited built-in (or contributed) templates and the user's own, by id, as stored: an edit is
    /// laid over the template it edits, field by field.
    pub templates: Ordered<Value>,
    /// Hidden built-in (or contributed) templates.
    pub deleted: Vec<String>,
    /// Edited built-in includes and the user's own, by name.
    pub includes: Ordered<TemplateFragment>,
    pub deleted_includes: Vec<String>,
    /// Edited code template bodies, by name.
    pub code: Ordered<String>,
}

impl FileTemplateCustomizations {
    /// The customisations stored as `raw`; none when nothing readable is stored. The text is read
    /// as it is, so the entries keep the order they were stored in.
    pub fn parse(raw: Option<&str>) -> Self {
        raw.filter(|raw| !raw.is_empty()).and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_default()
    }
}

/// Whether `name` is a built-in include's.
pub fn is_builtin_include(name: &str) -> bool {
    builtin_includes().iter().any(|include| include.name == name)
}

fn is_builtin_template(id: &str) -> bool {
    builtin_file_templates().iter().any(|template| template.id == id)
}

/// `template` with the stored `edit` laid over it; the template as it is when the edit does not
/// make one.
fn overlay(template: &FileTemplate, edit: &Value) -> FileTemplate {
    let (Ok(Value::Object(mut merged)), Value::Object(edit)) = (serde_json::to_value(template), edit) else {
        return template.clone();
    };
    merged.extend(edit.clone());
    serde_json::from_value(Value::Object(merged)).unwrap_or_else(|_| template.clone())
}

/// The file templates of `custom` over `base` (the built-in and contributed ones): the hidden ones
/// left out, the edited ones edited, then the user's own in the order they were added.
pub fn merge_file_templates(base: &[FileTemplate], custom: &FileTemplateCustomizations) -> Vec<FileTemplate> {
    let deleted = |id: &str| custom.deleted.iter().any(|d| d == id);
    let mut out: Vec<FileTemplate> = base
        .iter()
        .filter(|template| !deleted(&template.id))
        .map(|template| match custom.templates.get(&template.id) {
            Some(edit) => overlay(template, edit),
            None => template.clone(),
        })
        .collect();
    for (id, edit) in &custom.templates.0 {
        if base.iter().any(|template| template.id == *id) || deleted(id) {
            continue;
        }
        // A stored template that does not read as one (a field of the wrong type) is left out.
        if let Ok(template) = serde_json::from_value::<FileTemplate>(edit.clone()) {
            out.push(template);
        }
    }
    out
}

/// The includes: the built-in ones not hidden, edited where edited, then the user's own.
pub fn merge_includes(custom: &FileTemplateCustomizations) -> Vec<TemplateFragment> {
    let deleted = |name: &str| custom.deleted_includes.iter().any(|d| d == name);
    let builtins = builtin_includes().iter().filter(|include| !deleted(&include.name));
    let mut out: Vec<TemplateFragment> =
        builtins.map(|include| custom.includes.get(&include.name).unwrap_or(include).clone()).collect();
    let own = custom.includes.values().filter(|include| !is_builtin_include(&include.name) && !deleted(&include.name));
    out.extend(own.cloned());
    out
}

/// The code templates, their bodies edited where edited.
pub fn merge_code_templates(custom: &FileTemplateCustomizations) -> Vec<TemplateFragment> {
    builtin_code_templates()
        .iter()
        .map(|fragment| match custom.code.get(&fragment.name) {
            Some(body) => TemplateFragment { body: body.clone(), ..fragment.clone() },
            None => fragment.clone(),
        })
        .collect()
}

/// The scheme stored for the project at `root`; Default without a project or a stored choice.
fn read_scheme(root: Option<&str>, read: &dyn Fn(&str) -> Option<String>) -> TemplateScheme {
    let Some(root) = root else { return TemplateScheme::Default };
    let stored = load_object(read(&scheme_key(root)).as_deref(), Map::new());
    if stored.get("scheme").and_then(Value::as_str) == Some("project") {
        TemplateScheme::Project
    } else {
        TemplateScheme::Default
    }
}

/// The file templates, includes and code templates of the scheme a project uses.
#[derive(Debug, Clone, PartialEq)]
pub struct FileTemplates {
    scheme: TemplateScheme,
    root: Option<String>,
    custom: FileTemplateCustomizations,
    contributed: Vec<FileTemplate>,
    templates: Vec<FileTemplate>,
    includes: Vec<TemplateFragment>,
    code: Vec<TemplateFragment>,
}

impl FileTemplates {
    /// The templates of the project at `root` (none: Default), reading the storage with `read`.
    pub fn load(root: Option<&str>, read: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut out = Self {
            scheme: TemplateScheme::Default,
            root: root.map(str::to_owned),
            custom: FileTemplateCustomizations::default(),
            contributed: Vec::new(),
            templates: Vec::new(),
            includes: Vec::new(),
            code: Vec::new(),
        };
        out.reload(read);
        out
    }

    /// Reads the scheme and its customisations again (changed by another window); contributed
    /// templates stay.
    pub fn reload(&mut self, read: &dyn Fn(&str) -> Option<String>) {
        self.scheme = read_scheme(self.root.as_deref(), read);
        self.custom = FileTemplateCustomizations::parse(read(&self.storage_key()).as_deref());
        self.refresh();
    }

    fn base(&self) -> Vec<FileTemplate> {
        builtin_file_templates().iter().chain(&self.contributed).cloned().collect()
    }

    fn refresh(&mut self) {
        self.templates = merge_file_templates(&self.base(), &self.custom);
        self.includes = merge_includes(&self.custom);
        self.code = merge_code_templates(&self.custom);
    }

    fn commit(&mut self, custom: FileTemplateCustomizations) -> StorageWrite {
        self.custom = custom;
        self.refresh();
        StorageWrite::json(self.storage_key(), &self.custom)
    }

    /// The key the scheme in use is stored under.
    pub fn storage_key(&self) -> String {
        match (self.scheme, &self.root) {
            (TemplateScheme::Project, Some(root)) => project_key(root),
            _ => FILE_TEMPLATES_KEY.to_owned(),
        }
    }

    pub fn scheme(&self) -> TemplateScheme {
        self.scheme
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn templates(&self) -> &[FileTemplate] {
        &self.templates
    }

    pub fn includes(&self) -> &[TemplateFragment] {
        &self.includes
    }

    pub fn code_templates(&self) -> &[TemplateFragment] {
        &self.code
    }

    pub fn custom(&self) -> &FileTemplateCustomizations {
        &self.custom
    }

    /// Replaces the templates plugins contribute.
    pub fn set_contributed(&mut self, templates: Vec<FileTemplate>) {
        self.contributed = templates;
        self.refresh();
    }

    /// Whether `id` is a built-in or contributed template's: it can be reset, and is hidden when
    /// removed.
    pub fn is_base_template(&self, id: &str) -> bool {
        is_builtin_template(id) || self.contributed.iter().any(|template| template.id == id)
    }

    /// Saves a user template or an edit of a built-in or contributed one. Nothing changes, and
    /// `None` is returned, for a template whose extension or a kind's is not a plain extension: New
    /// would append it to the file's name.
    pub fn save_template(&mut self, template: &FileTemplate) -> Option<StorageWrite> {
        let kinds = template.kinds.iter().filter_map(|kind| kind.extension.as_deref());
        if !std::iter::once(template.extension.as_str()).chain(kinds).all(is_plain_extension) {
            return None;
        }
        let mut custom = self.custom.clone();
        custom.deleted.retain(|id| *id != template.id);
        let stored = serde_json::to_value(template).unwrap_or_else(|_| json!({}));
        custom.templates.insert(&template.id, stored);
        Some(self.commit(custom))
    }

    /// Removes a user template, or hides a built-in or contributed one.
    pub fn remove_template(&mut self, id: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.templates.remove(id);
        if self.is_base_template(id) && !custom.deleted.iter().any(|d| d == id) {
            custom.deleted.push(id.to_owned());
        }
        self.commit(custom)
    }

    /// Drops the user's edit of a template, or shows a hidden one again.
    pub fn reset_template(&mut self, id: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.templates.remove(id);
        custom.deleted.retain(|d| d != id);
        self.commit(custom)
    }

    pub fn is_modified_template(&self, id: &str) -> bool {
        self.is_base_template(id) && self.custom.templates.contains(id)
    }

    /// Saves an include; `previous` is its name before a rename (a renamed built-in is hidden under
    /// its old name).
    pub fn save_include(&mut self, include: &TemplateFragment, previous: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.deleted_includes.retain(|d| *d != include.name);
        if previous != include.name {
            custom.includes.remove(previous);
            if is_builtin_include(previous) && !custom.deleted_includes.iter().any(|d| d == previous) {
                custom.deleted_includes.push(previous.to_owned());
            }
        }
        custom.includes.insert(&include.name, include.clone());
        self.commit(custom)
    }

    pub fn remove_include(&mut self, name: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.includes.remove(name);
        if is_builtin_include(name) && !custom.deleted_includes.iter().any(|d| d == name) {
            custom.deleted_includes.push(name.to_owned());
        }
        self.commit(custom)
    }

    pub fn reset_include(&mut self, name: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.includes.remove(name);
        custom.deleted_includes.retain(|d| d != name);
        self.commit(custom)
    }

    pub fn is_modified_include(&self, name: &str) -> bool {
        is_builtin_include(name) && self.custom.includes.contains(name)
    }

    pub fn save_code_template(&mut self, name: &str, body: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.code.insert(name, body.to_owned());
        self.commit(custom)
    }

    pub fn reset_code_template(&mut self, name: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.code.remove(name);
        self.commit(custom)
    }

    pub fn is_modified_code_template(&self, name: &str) -> bool {
        self.custom.code.contains(name)
    }

    /// Switches the project between the Default and Project schemes and returns what to store. A
    /// Project scheme chosen for the first time starts as a copy of Default. Nothing without a
    /// project.
    pub fn set_scheme(&mut self, scheme: TemplateScheme, read: &dyn Fn(&str) -> Option<String>) -> Vec<StorageWrite> {
        let Some(root) = self.root.clone() else { return Vec::new() };
        let mut writes = Vec::new();
        if scheme == TemplateScheme::Project && read(&project_key(&root)).is_none() {
            // The copy is Default's text as stored (an object), so its entries keep their order.
            let default = read(FILE_TEMPLATES_KEY)
                .filter(|raw| matches!(serde_json::from_str::<Value>(raw), Ok(Value::Object(_))))
                .unwrap_or_else(|| "{}".to_owned());
            writes.push(StorageWrite { key: project_key(&root), value: default });
        }
        writes.push(StorageWrite::json(scheme_key(&root), &json!({ "scheme": scheme.id() })));
        let written = writes.clone();
        let after = move |key: &str| {
            written.iter().find(|write| write.key == key).map(|write| write.value.clone()).or_else(|| read(key))
        };
        self.reload(&after);
        writes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A storage in memory.
    #[derive(Default)]
    struct Storage(RefCell<BTreeMap<String, String>>);

    impl Storage {
        fn read(&self, key: &str) -> Option<String> {
            self.0.borrow().get(key).cloned()
        }

        fn save(&self, write: StorageWrite) {
            self.0.borrow_mut().insert(write.key, write.value);
        }
    }

    fn header(store: &FileTemplates) -> String {
        let header = store.includes().iter().find(|include| include.name == "File Header");
        header.map(|include| include.body.clone()).unwrap_or_default()
    }

    // src/features/templates/__tests__/views.test.tsx › settings pages › keeps a project scheme seeded
    // from Default (the store's half; the page's half is in the page's tests)
    #[test]
    fn keeps_a_project_scheme_seeded_from_default() {
        let storage = Storage::default();
        let read = |key: &str| storage.read(key);
        let mut store = FileTemplates::load(Some("/proj"), &read);
        let include =
            |body: &str| TemplateFragment { name: "File Header".to_owned(), body: body.to_owned(), description: None };
        storage.save(store.save_include(&include("// default"), "File Header"));
        for write in store.set_scheme(TemplateScheme::Project, &read) {
            storage.save(write);
        }
        assert_eq!(store.scheme(), TemplateScheme::Project);
        assert_eq!(header(&store), "// default");
        storage.save(store.save_include(&include("// project"), "File Header"));
        assert_eq!(store.storage_key(), project_key("/proj"));
        for write in store.set_scheme(TemplateScheme::Default, &read) {
            storage.save(write);
        }
        assert_eq!(header(&store), "// default");
        for write in store.set_scheme(TemplateScheme::Project, &read) {
            storage.save(write);
        }
        assert_eq!(header(&store), "// project");
        assert_eq!(storage.read(&scheme_key("/proj")).as_deref(), Some(r#"{"scheme":"project"}"#));
        assert_eq!(FileTemplates::load(Some("/proj"), &read).scheme(), TemplateScheme::Project);
        assert_eq!(FileTemplates::load(None, &read).storage_key(), FILE_TEMPLATES_KEY);
        let mut alone = FileTemplates::load(None, &read);
        assert!(alone.set_scheme(TemplateScheme::Project, &read).is_empty(), "no scheme without a project");
    }

    // Not from a spec: an edit is laid over its built-in field by field and resetting drops it; a user
    // template comes after the built-ins in the order it was added; removing a built-in hides it.
    #[test]
    fn templates_are_edited_added_hidden_and_reset() {
        let mut store = FileTemplates::load(None, &|_| None);
        let kotlin = store.templates().iter().find(|t| t.id == "kotlin").cloned().expect("a built-in");
        let partial = json!({ "templates": { "kotlin": { "name": "Kotlin!" } }, "deleted": [] }).to_string();
        let edited = FileTemplates::load(None, &|key| (key == FILE_TEMPLATES_KEY).then(|| partial.clone()));
        let shown = edited.templates().iter().find(|t| t.id == "kotlin").expect("listed");
        assert_eq!((shown.name.as_str(), shown.kinds.len()), ("Kotlin!", kotlin.kinds.len()));
        assert!(edited.is_modified_template("kotlin"));

        let user = FileTemplate {
            id: "user.a".to_owned(),
            name: "Mine".to_owned(),
            icon: "file".to_owned(),
            ..FileTemplate::default()
        };
        let write = store.save_template(&user).expect("a template with a plain extension is saved");
        assert_eq!(write.key, FILE_TEMPLATES_KEY);
        assert_eq!(store.templates().last(), Some(&user));
        assert!(!store.is_modified_template("user.a"));
        let write = store.remove_template("kotlin");
        assert!(!store.templates().iter().any(|t| t.id == "kotlin"));
        let stored: Value = serde_json::from_str(&write.value).expect("JSON");
        assert_eq!(stored["deleted"], json!(["kotlin"]));
        assert_eq!(stored["deletedIncludes"], json!([]));
        assert_eq!(FileTemplates::load(None, &|_| Some(write.value.clone())).templates(), store.templates());
        let _ = store.reset_template("kotlin");
        assert!(store.templates().iter().any(|t| t.id == "kotlin"));
        let _ = store.remove_template("user.a");
        assert_eq!(store.custom(), &FileTemplateCustomizations::default());
    }

    // Not from a spec: a renamed built-in include is hidden under its old name, a removed one too;
    // code templates keep their edited body until reset; unreadable storage gives the defaults.
    #[test]
    fn includes_and_code_templates_are_renamed_removed_and_reset() {
        let mut store = FileTemplates::load(None, &|_| None);
        let renamed = TemplateFragment { name: "Header".to_owned(), body: "// h".to_owned(), description: None };
        let _ = store.save_include(&renamed, "File Header");
        assert_eq!(store.includes(), std::slice::from_ref(&renamed));
        assert_eq!(store.custom().deleted_includes, ["File Header"]);
        assert!(!store.is_modified_include("Header"));
        let _ = store.remove_include("Header");
        assert!(store.includes().is_empty());
        let _ = store.reset_include("File Header");
        assert_eq!(store.includes(), builtin_includes());
        let name = builtin_code_templates()[0].name.clone();
        let _ = store.save_code_template(&name, "@Test\nvoid t() {}\n");
        assert_eq!(store.code_templates()[0].body, "@Test\nvoid t() {}\n");
        assert!(store.is_modified_code_template(&name));
        let _ = store.reset_code_template(&name);
        assert_eq!(store.code_templates(), builtin_code_templates());
        assert_eq!(FileTemplateCustomizations::parse(Some("{ not json")), FileTemplateCustomizations::default());
        assert_eq!(FileTemplateCustomizations::parse(Some("[1, 2]")), FileTemplateCustomizations::default());
    }

    // Not from a spec: the user's own includes and templates come back in the order they were added,
    // after a reload and in a Project scheme seeded from Default.
    #[test]
    fn own_entries_keep_the_order_they_were_added_in() {
        let storage = Storage::default();
        let read = |key: &str| storage.read(key);
        let mut store = FileTemplates::load(Some("/proj"), &read);
        for name in ["b", "a"] {
            let include = TemplateFragment { name: name.to_owned(), ..TemplateFragment::default() };
            storage.save(store.save_include(&include, name));
            let template = FileTemplate {
                id: format!("user.{name}"),
                name: name.to_owned(),
                icon: "file".to_owned(),
                ..FileTemplate::default()
            };
            storage.save(store.save_template(&template).expect("saved"));
        }
        let names = |store: &FileTemplates| -> Vec<String> {
            let own = store.includes().iter().filter(|include| !is_builtin_include(&include.name));
            own.map(|include| include.name.clone()).collect()
        };
        let ids = |store: &FileTemplates| -> Vec<String> {
            let own = store.templates().iter().filter(|template| template.id.starts_with("user."));
            own.map(|template| template.id.clone()).collect()
        };
        let reloaded = FileTemplates::load(Some("/proj"), &read);
        assert_eq!(names(&reloaded), ["b", "a"]);
        assert_eq!(ids(&reloaded), ["user.b", "user.a"]);
        for write in store.set_scheme(TemplateScheme::Project, &read) {
            storage.save(write);
        }
        assert_eq!(names(&store), ["b", "a"]);
        assert_eq!(storage.read(&project_key("/proj")), storage.read(FILE_TEMPLATES_KEY));
    }

    // Not from a spec: a template whose extension, or one of its kinds', names another folder or holds
    // what no file name can is not stored, and the templates in effect stay as they were.
    #[test]
    fn a_template_whose_extension_is_not_a_plain_extension_is_not_stored() {
        let mut store = FileTemplates::load(None, &|_| None);
        let before = store.clone();
        let kotlin = store.templates().iter().find(|t| t.id == "kotlin").cloned().expect("a built-in");
        for extension in ["../x", "/../../x", "a\\b", ".kt", "kt "] {
            let edited = FileTemplate { extension: extension.to_owned(), ..kotlin.clone() };
            assert_eq!(store.save_template(&edited), None, "{extension:?}");
            let mut kinds = kotlin.kinds.clone();
            kinds[0].extension = Some(extension.to_owned());
            assert_eq!(store.save_template(&FileTemplate { kinds, ..kotlin.clone() }), None, "kind {extension:?}");
        }
        assert_eq!(store, before);
        let plain = FileTemplate { extension: "kts".to_owned(), ..kotlin };
        assert!(store.save_template(&plain).is_some());
    }
}
