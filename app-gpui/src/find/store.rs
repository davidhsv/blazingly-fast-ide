//! Find in Files popup state: the options the user sets, the scope they resolve to, the query sent to the
//! backend, and the file masks and recent strings the popup offers.
//!
//! Nothing here reads or writes storage: what is kept between sessions is parsed from its stored JSON, and a
//! change comes back as the [`StorageWrite`] the caller saves.

use super::ipc::{ExtensionCount, ProjectFileSet, SearchContext, SearchQuery, SearchScope};
use crate::platform::context::{ContextKeys, ContextValue};
use crate::platform::search::{CustomScope, find_custom_scope};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub use crate::platform::search::{
    HistoryKind, SEARCH_CONTEXTS, parse_history as find_history, remember_search as remember_find_string,
};

/// A value to keep between sessions: the JSON to store under a key.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct StorageWrite {
    pub key: &'static str,
    pub value: String,
}

/// Which of the popup's scope tabs is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScopeTab {
    #[default]
    Project,
    Module,
    Directory,
    Scope,
}

/// The named scopes every project has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinScope {
    ProjectFiles,
    ProductionFiles,
    TestFiles,
    OpenFiles,
    RecentFiles,
}

/// A built-in named scope or one the user defined.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NamedScope {
    Builtin(BuiltinScope),
    /// A custom scope, by name.
    Custom(String),
}

impl Default for NamedScope {
    fn default() -> Self {
        Self::Builtin(BuiltinScope::ProjectFiles)
    }
}

impl NamedScope {
    /// The value the scope is selected by: the built-in scope's id, or `custom:<name>`.
    pub fn value(&self) -> String {
        match self {
            Self::Builtin(BuiltinScope::ProjectFiles) => "projectFiles".to_owned(),
            Self::Builtin(BuiltinScope::ProductionFiles) => "productionFiles".to_owned(),
            Self::Builtin(BuiltinScope::TestFiles) => "testFiles".to_owned(),
            Self::Builtin(BuiltinScope::OpenFiles) => "openFiles".to_owned(),
            Self::Builtin(BuiltinScope::RecentFiles) => "recentFiles".to_owned(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }
}

pub fn custom_scope_value(name: &str) -> NamedScope {
    NamedScope::Custom(name.to_owned())
}

/// The name of a custom scope; `None` for a built-in one.
pub fn custom_scope_name(scope: &NamedScope) -> Option<&str> {
    match scope {
        NamedScope::Custom(name) => Some(name),
        NamedScope::Builtin(_) => None,
    }
}

/// The root-based named scopes, which the backend searches over the project's source and test roots.
pub const FILE_SET_SCOPES: [(BuiltinScope, ProjectFileSet); 3] = [
    (BuiltinScope::ProjectFiles, ProjectFileSet::All),
    (BuiltinScope::ProductionFiles, ProjectFileSet::Production),
    (BuiltinScope::TestFiles, ProjectFileSet::Test),
];

fn file_set_label(set: ProjectFileSet) -> &'static str {
    match set {
        ProjectFileSet::All => "Project Files",
        ProjectFileSet::Production => "Project Production Files",
        ProjectFileSet::Test => "Project Test Files",
    }
}

/// What the user set in the popup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub pattern: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub is_regex: bool,
    /// Replace: keep the case of each found occurrence ("Preserve case").
    pub preserve_case: bool,
    pub mask_enabled: bool,
    pub mask: String,
    pub scope_tab: ScopeTab,
    /// Module tab: relative module folder or absolute workspace member (`""`: plain project root).
    pub module: String,
    pub directory: String,
    pub named_scope: NamedScope,
    /// The search context filter (funnel menu): anywhere, only or except comments and string literals.
    pub context: SearchContext,
}

/// Storage key of the popup's pin.
pub const PINNED_KEY: &str = "openidea.search.findPinned.v1";
/// Storage key of "Open results in new tab".
pub const NEW_TAB_KEY: &str = "openidea.search.findNewTab.v1";

/// What Find in Files or Replace in Files is opened with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindArg {
    pub directory: Option<String>,
    pub query: Option<String>,
}

/// The argument of the Find actions: a directory (`directory` or `path`, else the focused directory of the
/// context) and a query to start with.
pub fn parse_find_arg(arg: Option<&Value>, ctx: &ContextKeys) -> FindArg {
    let text = |key: &str| arg.and_then(|arg| arg.get(key)).and_then(Value::as_str).map(str::to_owned);
    let focused = || match ctx.get("focusedDirectory") {
        Some(ContextValue::Text(directory)) => Some(directory.to_string()),
        _ => None,
    };
    FindArg { directory: text("directory").or_else(|| text("path")).or_else(focused), query: text("query") }
}

/// Keys the popup handles that are also global shortcuts (next / previous occurrence, jump to source, recent
/// searches): the actions bound to them forward here while the popup is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindPopupCommand {
    NextResult,
    PreviousResult,
    OpenResult,
    RecentSearches,
}

/// The popup: its options and whether and how it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindPopupState {
    pub options: FindOptions,
    pub open: bool,
    pub replace: bool,
    /// Counts the times the popup was shown, so that a view can tell a new showing from the current one.
    pub session: u32,
    /// Title-bar pin: the popup stays open when a result is opened or the user clicks outside.
    pub pinned: bool,
    /// "Open results in new tab": Open in Find Window keeps the active Find tab.
    pub new_tab: bool,
}

/// A stored flag: set only by the JSON value `true`.
fn stored_flag(stored: Option<&str>) -> bool {
    stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) == Some(Value::Bool(true))
}

impl FindPopupState {
    /// The closed popup with the flags stored under [`PINNED_KEY`] and [`NEW_TAB_KEY`].
    pub fn restored(pinned: Option<&str>, new_tab: Option<&str>) -> Self {
        Self { pinned: stored_flag(pinned), new_tab: stored_flag(new_tab), ..Self::default() }
    }

    pub fn set_pinned(&mut self, pinned: bool) -> StorageWrite {
        self.pinned = pinned;
        StorageWrite { key: PINNED_KEY, value: json!(pinned).to_string() }
    }

    pub fn set_new_tab(&mut self, new_tab: bool) -> StorageWrite {
        self.new_tab = new_tab;
        StorageWrite { key: NEW_TAB_KEY, value: json!(new_tab).to_string() }
    }

    /// Shows the popup for Find or Replace. The pattern starts as the argument's query, else the editor's
    /// `selection`; a directory in the argument selects the Directory tab. A popup that was closed on an
    /// empty Directory tab reopens on the Project tab.
    pub fn show(&mut self, replace: bool, arg: &FindArg, selection: Option<&str>) {
        let was_open = std::mem::replace(&mut self.open, true);
        self.replace = replace;
        self.session += 1;
        let options = &mut self.options;
        if let Some(prefill) = arg.query.as_deref().or(selection).filter(|prefill| !prefill.is_empty()) {
            options.pattern = prefill.to_owned();
        }
        if let Some(directory) = arg.directory.as_deref().filter(|directory| !directory.is_empty()) {
            options.scope_tab = ScopeTab::Directory;
            options.directory = directory.to_owned();
        } else if !was_open && options.scope_tab == ScopeTab::Directory && options.directory.is_empty() {
            options.scope_tab = ScopeTab::Project;
        }
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Whether a [`FindPopupCommand`] goes to the popup: it is open and its view listens.
    pub fn takes_commands(&self, handler_installed: bool) -> bool {
        self.open && handler_installed
    }
}

/// Storage key of the popup's size.
pub const SIZE_KEY: &str = "openidea.search.findPopupSize.v1";
/// Storage key of the popup's results / preview split.
pub const SPLIT_KEY: &str = "openidea.search.findPopupSplit.v1";

/// The popup's size, remembered between sessions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopupSize {
    pub w: f64,
    pub h: f64,
}

/// The stored size; `None` unless both sides are numbers.
pub fn parse_popup_size(stored: &str) -> Option<PopupSize> {
    let value: Value = serde_json::from_str(stored).ok()?;
    Some(PopupSize { w: value.get("w")?.as_f64()?, h: value.get("h")?.as_f64()? })
}

pub fn save_popup_size(size: PopupSize) -> StorageWrite {
    StorageWrite { key: SIZE_KEY, value: json!({ "w": size.w, "h": size.h }).to_string() }
}

/// The stored share of each pane of the popup's split, by pane id; `None` unless every value is a number.
pub fn parse_popup_split(stored: &str) -> Option<BTreeMap<String, f64>> {
    let Value::Object(panes) = serde_json::from_str::<Value>(stored).ok()? else { return None };
    panes.into_iter().map(|(pane, share)| Some((pane, share.as_f64()?))).collect()
}

pub fn save_popup_split(layout: &BTreeMap<String, f64>) -> StorageWrite {
    StorageWrite { key: SPLIT_KEY, value: json!(layout).to_string() }
}

/// Storage key of the recently used file masks.
pub const MASKS_KEY: &str = "openidea.search.fileMasks.v1";
/// How many masks the project's file types contribute.
const PROJECT_MASKS: usize = 5;
/// Most recent masks kept.
const MAX_RECENT_MASKS: usize = 10;

/// `*.ext` masks of the project's most common language file types. `counts` are the indexed files per
/// extension, most common first; `language_extensions` the extensions of every known language.
pub fn project_masks<'a>(
    counts: &[ExtensionCount],
    language_extensions: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let known: HashSet<String> = language_extensions.into_iter().map(str::to_lowercase).collect();
    let common = counts.iter().filter(|count| known.contains(&count.extension));
    common.take(PROJECT_MASKS).map(|count| format!("*.{}", count.extension)).collect()
}

/// The recently used masks from their stored JSON, newest first.
pub fn parse_masks(stored: &str) -> Vec<String> {
    find_history(stored)
}

/// Masks to offer: the ones used recently, then the project's file types.
pub fn recent_masks(recent: &[String], project: &[String]) -> Vec<String> {
    recent.iter().chain(project.iter().filter(|mask| !recent.contains(mask))).cloned().collect()
}

/// Puts `mask` at the front of the recent masks. A blank mask is not remembered; otherwise returns what to
/// store.
pub fn remember_mask(recent: &mut Vec<String>, mask: &str) -> Option<StorageWrite> {
    let mask = mask.trim();
    if mask.is_empty() {
        return None;
    }
    recent.retain(|entry| entry != mask);
    recent.insert(0, mask.to_owned());
    recent.truncate(MAX_RECENT_MASKS);
    Some(StorageWrite { key: MASKS_KEY, value: json!(recent).to_string() })
}

/// The query the options ask for in `scope`.
pub fn build_query(options: &FindOptions, scope: SearchScope, max_matches: u32) -> SearchQuery {
    let mask = options.mask.trim();
    SearchQuery {
        pattern: options.pattern.clone(),
        is_regex: options.is_regex,
        case_sensitive: options.case_sensitive,
        whole_word: options.whole_word,
        include: if options.mask_enabled && !mask.is_empty() { vec![mask.to_owned()] } else { Vec::new() },
        exclude: Vec::new(),
        scope,
        max_matches,
        context: options.context,
    }
}

/// What the scope tabs resolve against.
#[derive(Clone, Copy, Debug)]
pub struct ScopeInputs<'a> {
    pub root: Option<&'a str>,
    /// Files of the open editors (Open Files).
    pub open_paths: &'a [String],
    /// Recently viewed files, once loaded.
    pub recent_paths: Option<&'a [String]>,
    /// The project's custom scopes.
    pub custom_scopes: &'a [CustomScope],
}

/// Whether `path` starts with a drive (`C:\` or `C:/`).
fn starts_with_drive(path: &str) -> bool {
    matches!(path.as_bytes(), [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic())
}

/// The directory typed in the Directory tab as an absolute path: a relative one lies below the project.
pub fn resolve_directory(directory: &str, root: Option<&str>) -> String {
    let directory = directory.trim().trim_end_matches('/');
    match root.filter(|root| !root.is_empty()) {
        Some(root) if !directory.is_empty() && !directory.starts_with('/') && !starts_with_drive(directory) => {
            format!("{root}/{directory}")
        }
        _ => directory.to_owned(),
    }
}

/// The backend scope of the popup's scope tabs.
pub fn resolve_find_scope(options: &FindOptions, inputs: &ScopeInputs<'_>) -> SearchScope {
    let root = inputs.root.filter(|root| !root.is_empty());
    match options.scope_tab {
        ScopeTab::Directory if !options.directory.trim().is_empty() => {
            SearchScope::Directory { path: resolve_directory(&options.directory, root) }
        }
        ScopeTab::Module => match root {
            Some(_) if options.module.starts_with('/') => SearchScope::Directory { path: options.module.clone() },
            Some(root) if !options.module.is_empty() => {
                SearchScope::Directory { path: format!("{root}/{}", options.module) }
            }
            _ => SearchScope::Project,
        },
        ScopeTab::Scope => match &options.named_scope {
            NamedScope::Custom(name) => match find_custom_scope(inputs.custom_scopes, name) {
                Some(scope) => SearchScope::Pattern { pattern: scope.pattern.clone() },
                None => SearchScope::Project,
            },
            NamedScope::Builtin(builtin) => match FILE_SET_SCOPES.iter().find(|(scope, _)| scope == builtin) {
                Some(&(_, set)) => SearchScope::FileSet { set },
                None if *builtin == BuiltinScope::OpenFiles => SearchScope::Files { paths: inputs.open_paths.to_vec() },
                None => SearchScope::Files { paths: inputs.recent_paths.unwrap_or_default().to_vec() },
            },
        },
        ScopeTab::Project | ScopeTab::Directory => SearchScope::Project,
    }
}

/// A one-line description of a search for the tab of its results: `'x' in Project Test Files`.
pub fn describe_query(query: &SearchQuery, root: Option<&str>, custom_scopes: &[CustomScope]) -> String {
    let relative = |path: &str| -> String {
        let below = root.filter(|root| !root.is_empty()).and_then(|root| path.strip_prefix(root));
        below.and_then(|rest| rest.strip_prefix('/')).unwrap_or(path).to_owned()
    };
    let place = match &query.scope {
        SearchScope::Project => "Project".to_owned(),
        SearchScope::Directory { path } => format!("Directory {}", relative(path)),
        SearchScope::FileSet { set } => file_set_label(*set).to_owned(),
        SearchScope::Files { paths } => format!("{} files", paths.len()),
        SearchScope::Pattern { pattern } => match custom_scopes.iter().find(|scope| scope.pattern == *pattern) {
            Some(named) => format!("Scope '{}'", named.name),
            None => format!("Scope {pattern}"),
        },
    };
    let masks = if query.include.is_empty() { String::new() } else { format!(" ({})", query.include.join(", ")) };
    let context = SEARCH_CONTEXTS
        .iter()
        .find(|(context, _)| *context == query.context && query.context != SearchContext::Any)
        .map(|(_, label)| format!(", {}", label.to_lowercase()))
        .unwrap_or_default();
    format!("'{}' in {place}{masks}{context}", query.pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(name: &str, pattern: &str) -> CustomScope {
        CustomScope { name: name.to_owned(), pattern: pattern.to_owned() }
    }

    fn inputs<'a>(open_paths: &'a [String], custom_scopes: &'a [CustomScope]) -> ScopeInputs<'a> {
        ScopeInputs { root: Some("/p"), open_paths, recent_paths: None, custom_scopes }
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files named scopes › defaults to IDEA's Project
    // Files and maps root-based scopes to backend file sets
    #[test]
    fn defaults_to_project_files_and_maps_root_based_scopes_to_backend_file_sets() {
        let state = FindPopupState::default();
        assert_eq!(state.options.named_scope, NamedScope::Builtin(BuiltinScope::ProjectFiles));
        assert_eq!(
            FILE_SET_SCOPES,
            [
                (BuiltinScope::ProjectFiles, ProjectFileSet::All),
                (BuiltinScope::ProductionFiles, ProjectFileSet::Production),
                (BuiltinScope::TestFiles, ProjectFileSet::Test),
            ]
        );
        let query = build_query(&state.options, SearchScope::FileSet { set: ProjectFileSet::Test }, 0);
        assert_eq!(query.scope, SearchScope::FileSet { set: ProjectFileSet::Test });
        let query = SearchQuery { pattern: "x".to_owned(), ..query };
        assert_eq!(describe_query(&query, Some("/p"), &[]), "'x' in Project Test Files");
        let production = SearchQuery { scope: SearchScope::FileSet { set: ProjectFileSet::Production }, ..query };
        assert_eq!(describe_query(&production, Some("/p"), &[]), "'x' in Project Production Files");
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files module and custom scopes › a module
    // searches its folder, the root module the project
    #[test]
    fn a_module_searches_its_folder_the_root_module_the_project() {
        let open = ["/p/a.ts".to_owned()];
        let inputs = inputs(&open, &[]);
        let module = |module: &str| FindOptions {
            scope_tab: ScopeTab::Module,
            module: module.to_owned(),
            ..FindOptions::default()
        };
        assert_eq!(
            resolve_find_scope(&module("/member"), &inputs),
            SearchScope::Directory { path: "/member".to_owned() }
        );
        assert_eq!(resolve_find_scope(&module("web"), &inputs), SearchScope::Directory { path: "/p/web".to_owned() });
        assert_eq!(resolve_find_scope(&module(""), &inputs), SearchScope::Project);
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files module and custom scopes › named scopes
    // resolve to file sets, files and custom patterns
    #[test]
    fn named_scopes_resolve_to_file_sets_files_and_custom_patterns() {
        let open = ["/p/a.ts".to_owned()];
        let scopes = [scope("No tests", "!file:*test*")];
        let inputs = inputs(&open, &scopes);
        let resolve = |named_scope: NamedScope| {
            let options = FindOptions { scope_tab: ScopeTab::Scope, named_scope, ..FindOptions::default() };
            resolve_find_scope(&options, &inputs)
        };
        assert_eq!(
            resolve(NamedScope::Builtin(BuiltinScope::ProductionFiles)),
            SearchScope::FileSet { set: ProjectFileSet::Production }
        );
        assert_eq!(
            resolve(NamedScope::Builtin(BuiltinScope::OpenFiles)),
            SearchScope::Files { paths: vec!["/p/a.ts".to_owned()] }
        );
        let custom = resolve(custom_scope_value("No tests"));
        assert_eq!(custom, SearchScope::Pattern { pattern: "!file:*test*".to_owned() });
        assert_eq!(resolve(custom_scope_value("gone")), SearchScope::Project);
        let options = FindOptions { pattern: "x".to_owned(), ..FindOptions::default() };
        let query = build_query(&options, custom, 0);
        assert_eq!(describe_query(&query, Some("/p"), &scopes), "'x' in Scope 'No tests'");
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files search context and history › sends the
    // context filter only when it narrows the search
    #[test]
    fn sends_the_context_filter_only_when_it_narrows_the_search() {
        let base = FindOptions::default();
        assert_eq!(build_query(&base, SearchScope::Project, 0).context, SearchContext::Any);
        let options = FindOptions { pattern: "x".to_owned(), context: SearchContext::ExceptComments, ..base };
        let query = build_query(&options, SearchScope::Project, 0);
        assert_eq!(query.context, SearchContext::ExceptComments);
        assert_eq!(describe_query(&query, Some("/p"), &[]), "'x' in Project, except comments");
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files search context and history › remembers
    // recent strings per field, newest first and unique
    #[test]
    fn remembers_recent_strings_per_field_newest_first_and_unique() {
        let (mut find, mut replace) = (Vec::new(), Vec::new());
        remember_find_string(&mut find, "a");
        remember_find_string(&mut find, "b");
        remember_find_string(&mut find, "a");
        remember_find_string(&mut replace, "r");
        remember_find_string(&mut find, "  ");
        assert_eq!(find, ["a", "b"]);
        assert_eq!(replace, ["r"]);
        assert_ne!(HistoryKind::Find.storage_key(), HistoryKind::Replace.storage_key());
        assert_eq!(find_history(&json!(find).to_string()), find);
    }

    // src/features/search/__tests__/fileMasks.test.ts › file mask suggestions › are the project's most common
    // language file types, recent masks first
    #[test]
    fn masks_are_the_projects_most_common_language_file_types_recent_masks_first() {
        let counts: Vec<ExtensionCount> =
            [("rs", 40), ("md", 12), ("lock", 9), ("toml", 6), ("png", 5), ("json", 3), ("yml", 2), ("sh", 1)]
                .into_iter()
                .map(|(extension, files)| ExtensionCount { extension: extension.to_owned(), files })
                .collect();
        let languages: [&[&str]; 6] = [&["rs"], &["md", "markdown"], &["toml"], &["json"], &["yml", "yaml"], &["sh"]];
        let project = project_masks(&counts, languages.into_iter().flatten().copied());
        assert_eq!(project, ["*.rs", "*.md", "*.toml", "*.json", "*.yml"]);
        let mut recent = Vec::new();
        assert!(remember_mask(&mut recent, "*.toml").is_some());
        let stored = remember_mask(&mut recent, "!*Test*").expect("the mask is remembered");
        assert_eq!(stored.key, MASKS_KEY);
        assert_eq!(parse_masks(&stored.value), recent);
        assert_eq!(recent_masks(&recent, &project), ["!*Test*", "*.toml", "*.rs", "*.md", "*.json", "*.yml"]);
    }

    // Not from a spec: showing the popup prefills the pattern, selects the Directory tab for a directory and
    // leaves an empty Directory tab when reopened.
    #[test]
    fn showing_the_popup_prefills_the_pattern_and_picks_the_scope_tab() {
        let mut popup = FindPopupState::default();
        popup.show(false, &FindArg::default(), Some("selected"));
        assert!(popup.open && !popup.replace);
        assert_eq!((popup.session, popup.options.pattern.as_str()), (1, "selected"));
        let arg = FindArg { directory: Some("/p/src".to_owned()), query: Some("typed".to_owned()) };
        popup.show(true, &arg, Some("selected"));
        assert_eq!((popup.session, popup.replace, popup.options.pattern.as_str()), (2, true, "typed"));
        assert_eq!((popup.options.scope_tab, popup.options.directory.as_str()), (ScopeTab::Directory, "/p/src"));
        assert!(popup.takes_commands(true) && !popup.takes_commands(false));

        popup.options.directory.clear();
        popup.show(false, &FindArg::default(), None);
        assert_eq!(popup.options.scope_tab, ScopeTab::Directory);
        popup.hide();
        assert!(!popup.takes_commands(true));
        popup.show(false, &FindArg::default(), None);
        assert_eq!(popup.options.scope_tab, ScopeTab::Project);
        assert_eq!(popup.options.pattern, "typed");
    }

    // Not from a spec: the action argument names the directory and the query, and the context's focused
    // directory stands in for a missing one.
    #[test]
    fn the_find_argument_falls_back_to_the_focused_directory() {
        let mut ctx = ContextKeys::new();
        assert_eq!(parse_find_arg(None, &ctx), FindArg::default());
        ctx.set("focusedDirectory", "/p/focused");
        assert_eq!(parse_find_arg(None, &ctx).directory.as_deref(), Some("/p/focused"));
        let arg = json!({ "path": "/p/lib", "query": "needle", "directory": 7 });
        let parsed = parse_find_arg(Some(&arg), &ctx);
        assert_eq!((parsed.directory.as_deref(), parsed.query.as_deref()), (Some("/p/lib"), Some("needle")));
    }

    // Not from a spec: stored flags, sizes and splits are read leniently and written as JSON.
    #[test]
    fn stored_popup_values_round_trip_and_bad_ones_are_dropped() {
        let mut popup = FindPopupState::restored(Some("true"), Some("1"));
        assert!(popup.pinned && !popup.new_tab);
        let write = popup.set_new_tab(true);
        assert_eq!((write.key, write.value.as_str()), (NEW_TAB_KEY, "true"));
        assert_eq!(popup.set_pinned(false), StorageWrite { key: PINNED_KEY, value: "false".to_owned() });
        assert!(!FindPopupState::restored(None, Some("not json")).new_tab);

        let size = PopupSize { w: 640.0, h: 480.5 };
        assert_eq!(parse_popup_size(&save_popup_size(size).value), Some(size));
        assert_eq!(parse_popup_size(r#"{"w":"640","h":480}"#), None);
        assert_eq!(parse_popup_size("null"), None);
        let split = BTreeMap::from([("results".to_owned(), 60.0), ("preview".to_owned(), 40.0)]);
        assert_eq!(parse_popup_split(&save_popup_split(&split).value), Some(split));
        assert_eq!(parse_popup_split(r#"{"results":"60"}"#), None);
        assert_eq!(parse_popup_split("[1,2]"), None);
    }

    // Not from a spec: a typed directory is made absolute, and a directory scope is described relative to the
    // project with the file mask.
    #[test]
    fn directories_resolve_below_the_project_and_are_described_relative_to_it() {
        assert_eq!(resolve_directory(" src/main/ ", Some("/p")), "/p/src/main");
        assert_eq!(resolve_directory("/abs//", Some("/p")), "/abs");
        assert_eq!(resolve_directory("C:\\work", Some("/p")), "C:\\work");
        assert_eq!(resolve_directory("src", None), "src");
        let options = FindOptions {
            pattern: "x".to_owned(),
            mask_enabled: true,
            mask: " *.ts ".to_owned(),
            scope_tab: ScopeTab::Directory,
            directory: "src".to_owned(),
            ..FindOptions::default()
        };
        let scope = resolve_find_scope(&options, &inputs(&[], &[]));
        assert_eq!(scope, SearchScope::Directory { path: "/p/src".to_owned() });
        let query = build_query(&options, scope, 100);
        assert_eq!(query.include, ["*.ts"]);
        assert_eq!(describe_query(&query, Some("/p"), &[]), "'x' in Directory src (*.ts)");
        assert_eq!(custom_scope_name(&custom_scope_value("Mine")), Some("Mine"));
        assert_eq!(custom_scope_value("Mine").value(), "custom:Mine");
        assert_eq!(NamedScope::default().value(), "projectFiles");
    }
}
