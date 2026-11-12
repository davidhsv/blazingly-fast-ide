//! TODO items: patterns, the Find-in-Files query, search matches as items, scanning editor text,
//! grouping and filters, the tool window's tabs and options, and the preview pane's hold on
//! documents edited in it.

use crate::everywhere::open::{byte_at_utf16, utf16_len};
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform::paths::ipc_path;
use crate::platform::search::CustomScope;
use crate::project_view::index::local_changed_paths;
use crate::vcs_ops::changelists::LocalChangelist;
use ide_core::IpcPath;
use ide_search::{FileMatches, ProjectFileSet, SearchContext, SearchQuery, SearchScope};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoPattern {
    pub id: String,
    pub keyword: String,
    /// Colour token, e.g. `var(--info)`.
    pub color: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoSettings {
    pub patterns: Vec<TodoPattern>,
    pub case_sensitive: bool,
    /// Only count matches that follow a comment marker on their line.
    pub comments_only: bool,
}

fn pattern(id: &str, keyword: &str, color: &str) -> TodoPattern {
    TodoPattern { id: id.to_owned(), keyword: keyword.to_owned(), color: color.to_owned(), enabled: true }
}

pub fn default_patterns() -> Vec<TodoPattern> {
    vec![
        pattern("todo", "TODO", "var(--info)"),
        pattern("fixme", "FIXME", "var(--danger)"),
        pattern("hack", "HACK", "var(--warning)"),
        pattern("xxx", "XXX", "var(--warning)"),
    ]
}

impl Default for TodoSettings {
    fn default() -> Self {
        Self { patterns: default_patterns(), case_sensitive: true, comments_only: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub path: String,
    /// 0-based line.
    pub line: u32,
    /// UTF-16 column of the keyword in the real line.
    pub col: u32,
    /// Pattern keyword as configured (e.g. `TODO`).
    pub keyword: String,
    /// Text from the keyword to the end of the comment.
    pub text: String,
}

/// The enabled patterns with a keyword.
pub fn active_patterns(s: &TodoSettings) -> impl Iterator<Item = &TodoPattern> {
    s.patterns.iter().filter(|p| p.enabled && !p.keyword.trim().is_empty())
}

/// Escapes what a regular expression treats specially (`.*+?^${}()|[]\`).
fn escape_re(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `\b(TODO|FIXME|HACK|XXX)\b.*`, or `None` when no pattern is enabled.
pub fn todo_regex_source(s: &TodoSettings) -> Option<String> {
    let keywords: Vec<String> = active_patterns(s).map(|p| escape_re(p.keyword.trim())).collect();
    (!keywords.is_empty()).then(|| format!("\\b({})\\b.*", keywords.join("|")))
}

/// The Find in Files query for TODO items in `scope`.
pub fn build_todo_query(s: &TodoSettings, scope: SearchScope) -> Option<SearchQuery> {
    Some(SearchQuery {
        pattern: todo_regex_source(s)?,
        is_regex: true,
        case_sensitive: s.case_sensitive,
        whole_word: false,
        include: Vec::new(),
        exclude: Vec::new(),
        scope,
        max_matches: 0,
        context: SearchContext::Any,
    })
}

// Comment openers that count anywhere on the line, at its start or after whitespace:
// `//` `/*` `<!--` `#` `--` `{-` `(*`. The whitespace rule drops `//` in URLs, `#` in strings,
// `f(*)` and `a * b`. `*` `;` `%` `"""` `'''` are code characters too, so they count only at the
// line start. A TODO counts anywhere inside a comment, so the keyword need not lead it.
static INLINE_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)(?://|/\*|<!--|#|--|\{-|\(\*)").expect("valid regex"));
static LINE_START_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*(?:\*|;|%|"""|''')"#).expect("valid regex"));
// A long line the backend truncated does not start at the real line start, so neither rule can
// apply: any marker counts, a false positive rather than a missed TODO.
static LENIENT_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(//|/\*|<!--|#|--|\{-|\(\*|\*|;|%|"""|''')"#).expect("valid regex"));
static CLOSER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s*(\*/|-->|-\}|\*\)|"""|''')\s*$"#).expect("valid regex"));

/// Whether the text before a keyword makes it part of a comment.
pub fn looks_like_comment(prefix: &str, truncated: bool) -> bool {
    if truncated {
        return LENIENT_MARKER.is_match(prefix);
    }
    INLINE_MARKER.is_match(prefix) || LINE_START_MARKER.is_match(prefix)
}

fn clean_text(text: &str) -> String {
    CLOSER.replacen(text, 1, "").trim_end().to_owned()
}

fn pattern_for<'a>(s: &'a TodoSettings, matched: &str) -> Option<&'a TodoPattern> {
    let lower = matched.to_lowercase();
    active_patterns(s).find(|p| {
        let keyword = p.keyword.trim();
        if s.case_sensitive { matched.starts_with(keyword) } else { lower.starts_with(&keyword.to_lowercase()) }
    })
}

/// One file's search matches as TODO items.
pub fn items_from_matches(fm: &FileMatches, s: &TodoSettings) -> Vec<TodoItem> {
    let path = ipc_path(&fm.path.to_string_lossy());
    let mut out = Vec::new();
    for lm in &fm.matches {
        for &(start, end) in &lm.ranges {
            let (from, to) = (byte_at_utf16(&lm.text, start as usize), byte_at_utf16(&lm.text, end as usize));
            let matched = &lm.text[from..to.max(from)];
            let Some(p) = pattern_for(s, matched) else { continue };
            if s.comments_only && !looks_like_comment(&lm.text[..from], lm.text_offset > 0) {
                continue;
            }
            out.push(TodoItem {
                path: path.clone(),
                line: lm.line,
                col: lm.text_offset + start,
                keyword: p.keyword.trim().to_owned(),
                text: clean_text(matched),
            });
        }
    }
    out
}

/// The local scan's expression: like the search's, with ASCII word boundaries.
fn local_regex(s: &TodoSettings) -> Option<Regex> {
    let keywords: Vec<String> = active_patterns(s).map(|p| regex::escape(p.keyword.trim())).collect();
    if keywords.is_empty() {
        return None;
    }
    let flags = if s.case_sensitive { "" } else { "(?i)" };
    Regex::new(&format!("{flags}(?-u:\\b)({})(?-u:\\b).*", keywords.join("|"))).ok()
}

/// Scans document text locally (open editors) by the same rules as the search.
pub fn scan_text(path: &str, text: &str, s: &TodoSettings) -> Vec<TodoItem> {
    let Some(re) = local_regex(s) else { return Vec::new() };
    let mut out = Vec::new();
    let (mut line, mut line_start) = (0u32, 0usize);
    for m in re.find_iter(text) {
        let idx = m.start();
        let before = &text[line_start..idx];
        if let Some(last) = before.rfind('\n') {
            line += u32::try_from(before.matches('\n').count()).unwrap_or(u32::MAX);
            line_start += last + 1;
        }
        let Some(p) = pattern_for(s, m.as_str()) else { continue };
        if s.comments_only && !looks_like_comment(&text[line_start..idx], false) {
            continue;
        }
        out.push(TodoItem {
            path: path.to_owned(),
            line,
            col: u32::try_from(utf16_len(&text[line_start..idx])).unwrap_or(u32::MAX),
            keyword: p.keyword.trim().to_owned(),
            text: clean_text(m.as_str()),
        });
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoGroup {
    pub path: String,
    pub items: Vec<TodoItem>,
}

/// Which items a tab shows.
#[derive(Clone, Debug, Default)]
pub struct TodoFilter {
    /// Keywords to show; `None` is all.
    pub keywords: Option<HashSet<String>>,
    /// Restrict to one file (Current File tab): `Some(None)` when there is no current file.
    pub path: Option<Option<String>>,
    /// Restrict to these files (Changes tab).
    pub paths: Option<HashSet<String>>,
}

pub fn filter_items(items: &[TodoItem], f: &TodoFilter) -> Vec<TodoItem> {
    items.iter().filter(|i| f.keywords.as_ref().is_none_or(|k| k.contains(&i.keyword))).cloned().collect()
}

/// Items by file.
pub type TodoMap = HashMap<String, Vec<TodoItem>>;

/// Groups by file (sorted by path), items by position; empty groups dropped.
pub fn group_todos(map: &TodoMap, f: &TodoFilter) -> Vec<TodoGroup> {
    let all: Vec<&String> = match &f.path {
        Some(Some(path)) => map.keys().filter(|p| *p == path).collect(),
        Some(None) => Vec::new(),
        None => map.keys().collect(),
    };
    let mut groups: Vec<TodoGroup> = all
        .into_iter()
        .filter(|p| f.paths.as_ref().is_none_or(|paths| paths.contains(*p)))
        .filter_map(|path| {
            let mut items = filter_items(map.get(path).map_or(&[][..], Vec::as_slice), f);
            items.sort_by_key(|i| (i.line, i.col));
            (!items.is_empty()).then(|| TodoGroup { path: path.clone(), items })
        })
        .collect();
    groups.sort_by(|a, b| collate::compare(&a.path, &b.path));
    groups
}

pub fn count_items(groups: &[TodoGroup]) -> usize {
    groups.iter().map(|g| g.items.len()).sum()
}

/// Search results with live editor scans layered on top.
pub fn merge_live(scanned: &TodoMap, live: &TodoMap) -> TodoMap {
    let mut out = scanned.clone();
    out.extend(live.iter().map(|(k, v)| (k.clone(), v.clone())));
    out
}

/// A scope of the Scope Based tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoScope {
    ProjectFiles,
    ProductionFiles,
    TestFiles,
    OpenFiles,
    /// A custom scope, by name.
    Custom(String),
}

impl TodoScope {
    /// The stored value (`custom:<name>` for a custom scope).
    pub fn value(&self) -> String {
        match self {
            Self::ProjectFiles => "projectFiles".to_owned(),
            Self::ProductionFiles => "productionFiles".to_owned(),
            Self::TestFiles => "testFiles".to_owned(),
            Self::OpenFiles => "openFiles".to_owned(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "projectFiles" => Self::ProjectFiles,
            "productionFiles" => Self::ProductionFiles,
            "testFiles" => Self::TestFiles,
            "openFiles" => Self::OpenFiles,
            _ => Self::Custom(value.strip_prefix("custom:")?.to_owned()),
        })
    }
}

/// The built-in scopes with their labels, in menu order.
pub const BUILTIN_TODO_SCOPES: [(&str, &str); 4] = [
    ("projectFiles", "Project Files"),
    ("productionFiles", "Project Production Files"),
    ("testFiles", "Project Test Files"),
    ("openFiles", "Open Files"),
];

/// The search scope to scan, or `None` when there is nothing to scan (no open files, unknown
/// custom scope).
pub fn todo_search_scope(scope: &TodoScope, custom: &[CustomScope], open_paths: &[String]) -> Option<SearchScope> {
    Some(match scope {
        TodoScope::ProjectFiles => SearchScope::Project,
        TodoScope::ProductionFiles => SearchScope::FileSet { set: ProjectFileSet::Production },
        TodoScope::TestFiles => SearchScope::FileSet { set: ProjectFileSet::Test },
        TodoScope::OpenFiles if open_paths.is_empty() => return None,
        TodoScope::OpenFiles => {
            SearchScope::Files { paths: open_paths.iter().map(|p| IpcPath(PathBuf::from(p))).collect() }
        }
        TodoScope::Custom(name) => {
            SearchScope::Pattern { pattern: custom.iter().find(|c| c.name == *name)?.pattern.clone() }
        }
    })
}

/// Files with local changes in change list `list_id` (the active one when unset or gone);
/// unversioned and ignored files belong to no list.
pub fn changed_paths(lists: &[LocalChangelist], list_id: Option<&str>) -> HashSet<String> {
    let id = lists
        .iter()
        .find(|l| Some(l.id.as_str()) == list_id)
        .or_else(|| lists.iter().find(|l| l.active))
        .or_else(|| lists.first())
        .map(|l| l.id.as_str());
    id.map(|id| local_changed_paths(lists, Some(id), true).into_iter().collect()).unwrap_or_default()
}

// --- tool window -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TodoTab {
    #[default]
    Project,
    CurrentFile,
    Scope,
    Changes,
}

/// The tool window's state: tab, hidden keywords, the Scope Based tab's scope and the Changes
/// tab's change list.
#[derive(Clone, Debug)]
pub struct TodoViewState {
    pub tab: TodoTab,
    pub hidden: Vec<String>,
    pub scope: TodoScope,
    /// `None`: the active change list.
    pub changelist: Option<String>,
}

impl Default for TodoViewState {
    fn default() -> Self {
        Self { tab: TodoTab::Project, hidden: Vec::new(), scope: TodoScope::ProjectFiles, changelist: None }
    }
}

/// What a tab groups: the search results and the open editors' scans.
#[derive(Clone, Copy, Debug)]
pub struct TodoSources<'a> {
    pub scanned: &'a TodoMap,
    pub scoped: &'a TodoMap,
    pub live: &'a TodoMap,
    pub patterns: &'a [TodoPattern],
    /// The active editor's file.
    pub current: Option<&'a str>,
    pub changelists: &'a [LocalChangelist],
}

impl TodoViewState {
    pub fn toggle_keyword(&mut self, keyword: &str) {
        match self.hidden.iter().position(|h| h == keyword) {
            Some(at) => {
                self.hidden.remove(at);
            }
            None => self.hidden.push(keyword.to_owned()),
        }
    }

    pub fn show_all(&mut self) {
        self.hidden.clear();
    }

    /// The keywords shown; `None` when none is hidden.
    pub fn keyword_filter(&self, patterns: &[TodoPattern]) -> Option<HashSet<String>> {
        (!self.hidden.is_empty()).then(|| {
            patterns.iter().map(|p| p.keyword.trim().to_owned()).filter(|k| !self.hidden.contains(k)).collect()
        })
    }

    /// The groups of `tab`.
    pub fn groups(&self, tab: TodoTab, src: &TodoSources) -> Vec<TodoGroup> {
        let keywords = self.keyword_filter(src.patterns);
        match tab {
            TodoTab::Scope => {
                // Open editors' unsaved text, for the files the scope scan found.
                let in_scope: TodoMap = src
                    .live
                    .iter()
                    .filter(|(p, _)| src.scoped.contains_key(*p))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                group_todos(&merge_live(src.scoped, &in_scope), &TodoFilter { keywords, ..TodoFilter::default() })
            }
            TodoTab::CurrentFile => {
                let path = Some(src.current.map(str::to_owned));
                group_todos(&merge_live(src.scanned, src.live), &TodoFilter { keywords, path, paths: None })
            }
            TodoTab::Changes => {
                let paths = Some(changed_paths(src.changelists, self.changelist.as_deref()));
                group_todos(&merge_live(src.scanned, src.live), &TodoFilter { keywords, path: None, paths })
            }
            TodoTab::Project => {
                group_todos(&merge_live(src.scanned, src.live), &TodoFilter { keywords, ..TodoFilter::default() })
            }
        }
    }
}

/// `2 items` / `1 item` beside a file.
pub fn count_label(n: usize, i18n: &mut I18n) -> String {
    if n == 1 {
        i18n.tx_built(&["", " item"], &[Param::from(n)])
    } else {
        i18n.tx_built(&["", " items"], &[Param::from(n)])
    }
}

/// `(5, 4)`: the 1-based line and column of an item.
pub fn position_label(item: &TodoItem) -> String {
    format!("({}, {})", item.line + 1, item.col + 1)
}

/// The keyword (emphasised) and the rest of an item's text.
pub fn item_parts(item: &TodoItem) -> (&str, &str) {
    let at = byte_at_utf16(&item.text, utf16_len(&item.keyword));
    item.text.split_at(at)
}

/// The tool window's view options (stored as JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TodoViewOptions {
    pub preview: bool,
    pub autoscroll: bool,
}

pub const TODO_VIEW_OPTIONS_KEY: &str = "openidea.todo.view.v1";

impl TodoViewOptions {
    /// The defaults (preview on, no autoscroll) with what was stored over them.
    pub fn load(stored: Option<&str>) -> Self {
        let mut options = Self { preview: true, autoscroll: false };
        if let Some(serde_json::Value::Object(o)) = stored.and_then(|s| serde_json::from_str(s).ok()) {
            if let Some(preview) = o.get("preview").and_then(serde_json::Value::as_bool) {
                options.preview = preview;
            }
            if let Some(autoscroll) = o.get("autoscroll").and_then(serde_json::Value::as_bool) {
                options.autoscroll = autoscroll;
            }
        }
        options
    }

    pub fn to_json(self) -> String {
        format!("{{\"preview\":{},\"autoscroll\":{}}}", self.preview, self.autoscroll)
    }
}

// --- preview pane ------------------------------------------------------------------------------

/// What the preview pane does with an attached document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewAttach {
    /// Show the editable editor.
    Edit,
    /// Release the document and show the read-only preview.
    ReleaseReadOnly,
}

/// A read-only document is shown in the read-only preview, and its attachment is released.
pub fn preview_attached(read_only: bool) -> PreviewAttach {
    if read_only { PreviewAttach::ReleaseReadOnly } else { PreviewAttach::Edit }
}

/// Where the preview pane's text comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewText {
    /// A document the preview edited and still holds.
    Held,
    /// The open editor's buffer.
    Editor,
    /// The file on disk.
    Disk,
}

/// Documents the preview pane edited and keeps attached after it moved to another file or
/// closed, so that the edits are not lost before they are saved.
#[derive(Clone, Debug, Default)]
pub struct HeldPreviews {
    held: HashSet<String>,
}

impl HeldPreviews {
    /// The preview of `path` is gone: a dirty document is held, a clean one released (`true`).
    pub fn unmounted(&mut self, path: &str, dirty: bool) -> bool {
        if dirty {
            self.held.insert(path.to_owned());
            return false;
        }
        true
    }

    /// Whether `path` is still held, and whether to release it now: a held document that is clean
    /// again is released.
    pub fn check(&mut self, path: &str, dirty: bool) -> (bool, bool) {
        if !self.held.contains(path) {
            return (false, false);
        }
        if dirty {
            return (true, false);
        }
        self.held.remove(path);
        (false, true)
    }

    /// Where the text of `path` comes from: a held document, else an open editor, else the disk.
    pub fn text_source(&self, path: &str, open_in_editor: bool) -> PreviewText {
        if self.held.contains(path) {
            PreviewText::Held
        } else if open_in_editor {
            PreviewText::Editor
        } else {
            PreviewText::Disk
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_ops::changelists::LocalChange;
    use ide_search::LineMatch;

    fn settings() -> TodoSettings {
        TodoSettings::default()
    }

    fn prefix(line: &str) -> &str {
        &line[..line.find("TODO").expect("a TODO")]
    }

    fn item(path: &str, line: u32, keyword: &str) -> TodoItem {
        TodoItem { path: path.to_owned(), line, col: 0, keyword: keyword.to_owned(), text: keyword.to_owned() }
    }

    fn todo_item(path: &str, line: u32, col: u32, keyword: &str, text: &str) -> TodoItem {
        TodoItem { path: path.to_owned(), line, col, keyword: keyword.to_owned(), text: text.to_owned() }
    }

    fn map() -> TodoMap {
        TodoMap::from([
            ("/p/b.ts".to_owned(), vec![item("/p/b.ts", 5, "TODO"), item("/p/b.ts", 1, "FIXME")]),
            ("/p/a.ts".to_owned(), vec![item("/p/a.ts", 2, "HACK")]),
            ("/p/c.ts".to_owned(), Vec::new()),
        ])
    }

    fn keywords(list: &[&str]) -> Option<HashSet<String>> {
        Some(list.iter().map(|k| (*k).to_owned()).collect())
    }

    fn paths(groups: &[TodoGroup]) -> Vec<&str> {
        groups.iter().map(|g| g.path.as_str()).collect()
    }

    fn lists() -> Vec<LocalChangelist> {
        let change = |path: &str, deleted: bool| LocalChange { path: path.to_owned(), deleted };
        vec![
            LocalChangelist {
                id: "default".to_owned(),
                name: "Changes".to_owned(),
                active: true,
                files: vec![change("/p/a.ts", false), change("/p/new.ts", false), change("/p/gone.ts", true)],
            },
            LocalChangelist {
                id: "ui".to_owned(),
                name: "UI".to_owned(),
                active: false,
                files: vec![change("/p/b.ts", false)],
            },
        ]
    }

    fn sorted(set: HashSet<String>) -> Vec<String> {
        let mut list: Vec<String> = set.into_iter().collect();
        list.sort();
        list
    }

    #[test]
    fn keeps_comment_markers() {
        for line in [
            "// TODO",
            "/* TODO */",
            " * TODO",
            "/** TODO: document this */",
            "# TODO",
            "foo() # TODO",
            "SELECT 1 -- TODO",
            "<!-- TODO -->",
            "  ; TODO",
            "  % TODO",
            "\"\"\" TODO",
            "''' TODO",
            "x = 1; // TODO",
            "x (* TODO *)",
            "x {- TODO -}",
            "/** 业务类别：WORK_NOTICE 工作通知，TODO 待办。 */",
            "// fixed the TODO bug",
            "/* see the TODO list */",
        ] {
            assert!(looks_like_comment(prefix(line), false), "{line}");
        }
    }

    #[test]
    fn drops_code_that_only_looks_like_a_comment_marker() {
        for line in [
            "foo(); TODO",
            "if (a % 2) TODO",
            "i--; TODO",
            "let s = \"a#b\"; TODO",
            "x = a--b; TODO",
            "see http://example.com TODO",
            "\"//\"; TODO",
            "f(*) TODO",
            "a * b; TODO",
        ] {
            assert!(!looks_like_comment(prefix(line), false), "{line}");
        }
    }

    #[test]
    fn falls_back_to_the_lenient_rule_on_truncated_lines() {
        assert!(looks_like_comment("very long code # remainder ", true));
        assert!(looks_like_comment("very long code % remainder ", true));
        assert!(looks_like_comment("i-- ", true));
    }

    #[test]
    fn builds_the_keyword_regex_from_enabled_patterns() {
        assert_eq!(todo_regex_source(&settings()).as_deref(), Some("\\b(TODO|FIXME|HACK|XXX)\\b.*"));
        let scope = SearchScope::Files { paths: vec![IpcPath(PathBuf::from("/a"))] };
        let q = build_todo_query(&TodoSettings { case_sensitive: false, ..settings() }, scope).expect("a query");
        assert!(q.is_regex && !q.case_sensitive);
        assert!(matches!(&q.scope, SearchScope::Files { paths } if paths.len() == 1));
        let mut none = settings();
        for p in &mut none.patterns {
            p.enabled = false;
        }
        assert!(build_todo_query(&none, SearchScope::Project).is_none());
    }

    fn file_matches() -> FileMatches {
        let lm = |line: u32, text: &str, range: (u32, u32), text_offset: u32| LineMatch {
            line,
            text: text.to_owned(),
            ranges: vec![range],
            text_offset,
        };
        FileMatches {
            path: IpcPath(PathBuf::from("/p/src/a.ts")),
            rel_path: "src/a.ts".to_owned(),
            matches: vec![
                lm(3, "  // TODO: fix this", (5, 19), 0),
                lm(9, "const TODO_LIST = 1; /* FIXME later */", (24, 38), 0),
                lm(12, "const s = \"TODO nope\";", (11, 21), 0),
                lm(20, "…x # HACK around", (5, 16), 100),
            ],
        }
    }

    #[test]
    fn keeps_comment_matches_with_keyword_position_and_clean_text() {
        assert_eq!(
            items_from_matches(&file_matches(), &settings()),
            [
                todo_item("/p/src/a.ts", 3, 5, "TODO", "TODO: fix this"),
                todo_item("/p/src/a.ts", 9, 24, "FIXME", "FIXME later"),
                todo_item("/p/src/a.ts", 20, 105, "HACK", "HACK around"),
            ]
        );
    }

    #[test]
    fn includes_non_comment_matches_when_configured() {
        assert_eq!(items_from_matches(&file_matches(), &TodoSettings { comments_only: false, ..settings() }).len(), 4);
    }

    #[test]
    fn finds_items_with_0_based_lines_and_columns() {
        let text = "fn a() {}\n// todo lower\n    // TODO: one\n/* XXX two */\nlet TODO = 1;";
        assert_eq!(
            scan_text("/f.rs", text, &settings()),
            [todo_item("/f.rs", 2, 7, "TODO", "TODO: one"), todo_item("/f.rs", 3, 3, "XXX", "XXX two")]
        );
        let ci: Vec<(u32, String)> = scan_text("/f.rs", text, &TodoSettings { case_sensitive: false, ..settings() })
            .into_iter()
            .map(|i| (i.line, i.keyword))
            .collect();
        assert_eq!(ci, [(1, "TODO".to_owned()), (2, "TODO".to_owned()), (3, "XXX".to_owned())]);
    }

    #[test]
    fn groups_by_file_sorted_by_path_items_by_line() {
        let groups = group_todos(&map(), &TodoFilter::default());
        assert_eq!(paths(&groups), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(groups[1].items.iter().map(|i| i.line).collect::<Vec<_>>(), [1, 5]);
        assert_eq!(count_items(&groups), 3);
    }

    #[test]
    fn filters_by_keyword_and_current_file() {
        let map = map();
        let todo = TodoFilter { keywords: keywords(&["TODO"]), ..TodoFilter::default() };
        assert_eq!(count_items(&group_todos(&map, &todo)), 1);
        let current = |path: Option<&str>, kw: Option<HashSet<String>>| {
            group_todos(&map, &TodoFilter { keywords: kw, path: Some(path.map(str::to_owned)), paths: None })
        };
        assert_eq!(paths(&current(Some("/p/b.ts"), None)), ["/p/b.ts"]);
        assert!(current(None, None).is_empty());
        assert!(current(Some("/p/b.ts"), keywords(&["HACK"])).is_empty());
    }

    #[test]
    fn layers_live_editor_results_over_the_scan() {
        let merged = merge_live(&map(), &TodoMap::from([("/p/a.ts".to_owned(), Vec::new())]));
        assert_eq!(count_items(&group_todos(&merged, &TodoFilter::default())), 2);
    }

    #[test]
    fn maps_scopes_to_backend_scopes() {
        let custom = [CustomScope { name: "Web".to_owned(), pattern: "file:web//*".to_owned() }];
        assert!(matches!(todo_search_scope(&TodoScope::ProjectFiles, &custom, &[]), Some(SearchScope::Project)));
        assert!(matches!(
            todo_search_scope(&TodoScope::TestFiles, &custom, &[]),
            Some(SearchScope::FileSet { set: ProjectFileSet::Test })
        ));
        assert!(todo_search_scope(&TodoScope::OpenFiles, &custom, &[]).is_none());
        let open = todo_search_scope(&TodoScope::OpenFiles, &custom, &["/p/a.ts".to_owned()]);
        assert!(matches!(open, Some(SearchScope::Files { paths }) if paths == [IpcPath(PathBuf::from("/p/a.ts"))]));
        let web = todo_search_scope(&TodoScope::Custom("Web".to_owned()), &custom, &[]);
        assert!(matches!(web, Some(SearchScope::Pattern { pattern }) if pattern == "file:web//*"));
        assert!(todo_search_scope(&TodoScope::Custom("Gone".to_owned()), &custom, &[]).is_none());
    }

    #[test]
    fn keeps_only_files_of_the_chosen_change_list_the_active_one_by_default() {
        let lists = lists();
        let changed = changed_paths(&lists, None);
        assert_eq!(sorted(changed.clone()), ["/p/a.ts", "/p/new.ts"]);
        let map: TodoMap = ["/p/a.ts", "/p/b.ts"]
            .into_iter()
            .map(|p| (p.to_owned(), vec![todo_item(p, 0, 0, "TODO", "TODO")]))
            .collect();
        let groups = group_todos(&map, &TodoFilter { keywords: None, path: None, paths: Some(changed) });
        assert_eq!(paths(&groups), ["/p/a.ts"]);
        assert_eq!(sorted(changed_paths(&lists, Some("ui"))), ["/p/b.ts"]);
        assert_eq!(sorted(changed_paths(&lists, Some("gone"))), ["/p/a.ts", "/p/new.ts"]);
        assert!(changed_paths(&[], None).is_empty());
    }

    fn sources<'a>(scanned: &'a TodoMap, empty: &'a TodoMap, lists: &'a [LocalChangelist]) -> TodoSources<'a> {
        TodoSources { scanned, scoped: empty, live: empty, patterns: &[], current: None, changelists: lists }
    }

    #[test]
    fn renders_grouped_items_with_the_keyword_emphasised_and_filters_by_keyword() {
        let scanned = TodoMap::from([(
            "/p/src/a.ts".to_owned(),
            vec![
                todo_item("/p/src/a.ts", 4, 3, "TODO", "TODO: wire it"),
                todo_item("/p/src/a.ts", 9, 3, "FIXME", "FIXME broken"),
            ],
        )]);
        let empty = TodoMap::new();
        let patterns = default_patterns();
        let src = TodoSources { patterns: &patterns, ..sources(&scanned, &empty, &[]) };
        let mut view = TodoViewState::default();
        let mut i18n = I18n::default();
        let groups = view.groups(TodoTab::Project, &src);
        assert_eq!(count_label(count_items(&groups), &mut i18n), "2 items");
        let first = &groups[0].items[0];
        assert_eq!(position_label(first), "(5, 4)");
        assert_eq!(item_parts(first), ("TODO", ": wire it"));
        view.toggle_keyword("FIXME");
        assert_eq!(count_label(count_items(&view.groups(TodoTab::Project, &src)), &mut i18n), "1 item");
        view.show_all();
        assert!(view.hidden.is_empty());
    }

    #[test]
    fn lists_only_changed_files_on_the_changes_tab_by_change_list() {
        let lists = vec![
            LocalChangelist {
                id: "default".to_owned(),
                name: "Changes".to_owned(),
                active: true,
                files: vec![LocalChange { path: "/p/src/b.ts".to_owned(), deleted: false }],
            },
            LocalChangelist {
                id: "ui".to_owned(),
                name: "UI work".to_owned(),
                active: false,
                files: vec![LocalChange { path: "/p/src/a.ts".to_owned(), deleted: false }],
            },
        ];
        let scanned: TodoMap = ["/p/src/a.ts", "/p/src/b.ts"]
            .into_iter()
            .map(|p| (p.to_owned(), vec![todo_item(p, 0, 3, "TODO", "TODO x")]))
            .collect();
        let empty = TodoMap::new();
        let src = sources(&scanned, &empty, &lists);
        let mut view = TodoViewState { tab: TodoTab::Changes, ..TodoViewState::default() };
        assert_eq!(paths(&view.groups(TodoTab::Changes, &src)), ["/p/src/b.ts"]);
        view.changelist = Some("ui".to_owned());
        assert_eq!(paths(&view.groups(TodoTab::Changes, &src)), ["/p/src/a.ts"]);
    }

    #[test]
    fn shows_the_preview_pane_and_does_not_autoscroll_by_default() {
        assert_eq!(TodoViewOptions::load(None), TodoViewOptions { preview: true, autoscroll: false });
        let off = TodoViewOptions { preview: false, autoscroll: false };
        assert_eq!(TodoViewOptions::load(Some(&off.to_json())), off);
        assert_eq!(TodoViewOptions::load(Some("{bad")), TodoViewOptions { preview: true, autoscroll: false });
    }

    #[test]
    fn falls_back_to_the_read_only_preview_for_read_only_documents() {
        assert_eq!(preview_attached(true), PreviewAttach::ReleaseReadOnly);
        assert_eq!(preview_attached(false), PreviewAttach::Edit);
    }

    #[test]
    fn releases_a_clean_document_on_unmount() {
        let mut held = HeldPreviews::default();
        assert!(held.unmounted("/p/clean.ts", false));
        assert_eq!(held.check("/p/clean.ts", false), (false, false));
    }

    #[test]
    fn holds_a_dirty_document_instead_of_releasing_and_preview_text_reads_its_text() {
        let mut held = HeldPreviews::default();
        assert!(!held.unmounted("/p/dirty.ts", true));
        assert_eq!(held.check("/p/dirty.ts", true), (true, false));
        assert_eq!(held.text_source("/p/dirty.ts", false), PreviewText::Held);
        // Once saved (clean), the held document is released on the next lookup.
        assert_eq!(held.check("/p/dirty.ts", false), (false, true));
        assert_eq!(held.text_source("/p/dirty.ts", false), PreviewText::Disk);
    }

    #[test]
    fn falls_back_to_disk_text_when_nothing_holds_the_document() {
        let held = HeldPreviews::default();
        assert_eq!(held.text_source("/p/gone.ts", false), PreviewText::Disk);
        assert_eq!(held.text_source("/p/open.ts", true), PreviewText::Editor);
    }
}
