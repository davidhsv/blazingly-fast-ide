//! Recent Files (⌘E), the Switcher (⌃⇥) and Recent Locations (⇧⌘E): the rows they list, how they filter
//! them and what a location shows about itself.
//!
//! Highlight indices are byte offsets into the text they belong to, as [`super::rank`] reports them.

use super::locations::RecentLocation;
use super::open::{byte_at_utf16, rel_dir, rel_to_root, symbol_display_name, utf16_len};
use super::rank::{MatchOptions, match_text};
use crate::editor::tabs::EditorInput;
use crate::platform::virtual_docs::{basename, is_virtual_uri, virtual_container};
use crate::workbench::tool_windows::{Anchor, ToolWindow};
use ide_core::SymbolKind;
use std::collections::{HashMap, HashSet};

/// A row of Recent Files or of the Switcher.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentFileItem {
    /// The file; for an editor that is not a file, its input key.
    pub path: String,
    pub rel: String,
    pub name: String,
    pub dir: String,
    /// Matched characters of `name`.
    pub indices: Vec<usize>,
    /// Switcher: an open editor that is not a file (agent session, diff…).
    pub editor: Option<SwitcherEditor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitcherEditor {
    pub input: EditorInput,
    pub icon: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecentFilesOptions<'a> {
    pub root: Option<&'a str>,
    /// Files edited in this session.
    pub edited: Option<&'a HashSet<String>>,
    /// List only the edited files (⌘E again).
    pub edited_only: bool,
}

/// The recent files to list, each once, in the order given: those whose name matches `query`, then
/// (without highlights) those whose project-relative path does.
pub fn filter_recent_files<P: AsRef<str>>(
    paths: &[P],
    query: &str,
    opts: &RecentFilesOptions<'_>,
) -> Vec<RecentFileItem> {
    let searching = !query.trim().is_empty();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let path = path.as_ref();
        if !seen.insert(path) {
            continue;
        }
        if opts.edited_only && opts.edited.is_none_or(|edited| !edited.contains(path)) {
            continue;
        }
        let rel = rel_to_root(opts.root, path);
        let name = basename(rel);
        let mut indices = Vec::new();
        if searching {
            match match_text(&name, query, MatchOptions::default()) {
                Some(hit) => indices = hit.indices,
                None if match_text(rel, query, MatchOptions::default()).is_none() => continue,
                None => {}
            }
        }
        let dir = if is_virtual_uri(path) { virtual_container(path) } else { rel_dir(rel).to_owned() };
        out.push(RecentFileItem {
            path: path.to_owned(),
            rel: rel.to_owned(),
            name: name.into_owned(),
            dir,
            indices,
            editor: None,
        });
    }
    out
}

/// The row selected when the popup opens. IDEA preselects the previous file, so ⌘E ↵ toggles between the
/// last two.
pub fn initial_file_index(items: &[RecentFileItem], active_path: Option<&str>, query: &str) -> Option<usize> {
    let first = items.first()?;
    let previous = query.is_empty() && items.len() > 1 && active_path == Some(first.path.as_str());
    Some(usize::from(previous))
}

/// Switcher (⌃⇥) files: only the open ones, most recently opened first, then the open files that are
/// not among the recent ones.
pub fn switcher_paths<R: AsRef<str>, O: AsRef<str>>(recent: &[R], open: &[O]) -> Vec<String> {
    let is_open: HashSet<&str> = open.iter().map(|path| path.as_ref()).collect();
    let recent_open = recent.iter().map(|path| path.as_ref()).filter(|path| is_open.contains(path));
    let mut listed = HashSet::new();
    let mut out = Vec::new();
    for path in recent_open.chain(open.iter().map(|path| path.as_ref())) {
        if listed.insert(path) {
            out.push(path.to_owned());
        }
    }
    out
}

/// An open editor tab, as the Switcher lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenEditorTab {
    pub input: EditorInput,
    pub title: String,
    pub icon: String,
    /// Activation order this session (higher is more recent); 0 if never active.
    pub last_active: u64,
}

/// The file a tab shows, when it is a text or a virtual document.
fn file_path(tab: &OpenEditorTab) -> Option<&str> {
    let path = tab.input.path.as_deref().filter(|path| !path.is_empty())?;
    matches!(tab.input.kind.as_str(), "file" | "virtual").then_some(path)
}

/// Switcher (⌃⇥) rows: every open editor, like IDEA (whose editors are all files). Tabs active this
/// session come first, in the order given; then the other open files in recent-files order; then the
/// other editors in tab order.
pub fn switcher_items<R: AsRef<str>>(
    recent: &[R],
    tabs: &[OpenEditorTab],
    query: &str,
    root: Option<&str>,
) -> Vec<RecentFileItem> {
    let mut file_paths: Vec<&str> = Vec::new();
    let mut file_tabs: HashMap<&str, &OpenEditorTab> = HashMap::new();
    for tab in tabs {
        if let Some(path) = file_path(tab)
            && file_tabs.insert(path, tab).is_none()
        {
            file_paths.push(path);
        }
    }
    let open_files = switcher_paths(recent, &file_paths);
    let active = tabs.iter().filter(|tab| tab.last_active > 0);
    let idle_files = open_files.iter().filter_map(|path| file_tabs.get(path.as_str()).copied());
    let idle_files = idle_files.filter(|tab| tab.last_active == 0);
    let idle_editors = tabs.iter().filter(|tab| tab.last_active == 0 && file_path(tab).is_none());

    let searching = !query.trim().is_empty();
    let file_opts = RecentFilesOptions { root, ..RecentFilesOptions::default() };
    let mut out = Vec::new();
    for tab in active.chain(idle_files).chain(idle_editors) {
        if let Some(path) = file_path(tab) {
            out.extend(filter_recent_files(&[path], query, &file_opts));
            continue;
        }
        let mut indices = Vec::new();
        if searching {
            let Some(hit) = match_text(&tab.title, query, MatchOptions::default()) else { continue };
            indices = hit.indices;
        }
        let rel = tab.input.path.as_deref().filter(|path| !path.is_empty()).map(|path| rel_to_root(root, path));
        out.push(RecentFileItem {
            path: tab.input.key.clone(),
            rel: rel.unwrap_or(&tab.title).to_owned(),
            name: tab.title.clone(),
            dir: rel.map_or("", rel_dir).to_owned(),
            indices,
            editor: Some(SwitcherEditor { input: tab.input.clone(), icon: tab.icon.clone() }),
        });
    }
    out
}

/// The column of the Switcher a row is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherColumn {
    Files,
    Tools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitcherSelection {
    pub column: SwitcherColumn,
    pub index: usize,
}

/// Switcher selection `step` rows away from file row `base`: moving past the end of one column continues
/// in the other (files, then tool windows), like IDEA. `None` when there are no rows.
pub fn switcher_selection(base: usize, step: i64, files: usize, tools: usize) -> Option<SwitcherSelection> {
    let total = i64::try_from(files + tools).ok().filter(|&total| total > 0)?;
    let at = i64::try_from(base).ok()?.saturating_add(step).rem_euclid(total);
    let at = usize::try_from(at).ok()?;
    Some(if at < files {
        SwitcherSelection { column: SwitcherColumn::Files, index: at }
    } else {
        SwitcherSelection { column: SwitcherColumn::Tools, index: at - files }
    })
}

/// A tool window listed in the Switcher, with the matched characters of its title.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolWindowMatch<'a> {
    pub window: &'a ToolWindow,
    pub indices: Vec<usize>,
}

const fn anchor_rank(anchor: Anchor) -> u8 {
    match anchor {
        Anchor::Left => 0,
        Anchor::Bottom => 1,
        Anchor::Right => 2,
    }
}

/// The Switcher's tool windows by stripe: left, bottom, right; within one, the primary area first, then
/// by order. Without a project only the windows that need none are listed.
pub fn switcher_tool_windows<'a>(
    windows: impl IntoIterator<Item = &'a ToolWindow>,
    project_open: bool,
    query: &str,
) -> Vec<ToolWindowMatch<'a>> {
    let mut list: Vec<&ToolWindow> =
        windows.into_iter().filter(|window| project_open || !window.requires_project).collect();
    list.sort_by(|a, b| {
        let stripe = anchor_rank(a.anchor).cmp(&anchor_rank(b.anchor)).then(a.secondary.cmp(&b.secondary));
        stripe.then(a.order.unwrap_or(0.0).total_cmp(&b.order.unwrap_or(0.0)))
    });
    if query.trim().is_empty() {
        return list.into_iter().map(|window| ToolWindowMatch { window, indices: Vec::new() }).collect();
    }
    list.into_iter()
        .filter_map(|window| {
            let hit = match_text(&window.title, query, MatchOptions::default())?;
            Some(ToolWindowMatch { window, indices: hit.indices })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocationFilter<'a> {
    /// List only the locations recorded because of an edit (⇧⌘E again).
    pub changed_only: bool,
    pub root: Option<&'a str>,
}

/// A recent location that passed the filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredLocation {
    pub location: RecentLocation,
    /// Its path relative to the project.
    pub rel: String,
    /// Matched characters of the file name.
    pub name_indices: Vec<usize>,
}

/// The recent locations matching `query` by file name, else by a snippet line or the relative path
/// containing it.
pub fn filter_locations(list: &[RecentLocation], query: &str, opts: &LocationFilter<'_>) -> Vec<FilteredLocation> {
    let wanted = query.trim().to_lowercase();
    let mut out = Vec::new();
    for location in list {
        if opts.changed_only && !location.changed {
            continue;
        }
        let rel = rel_to_root(opts.root, &location.path);
        let mut name_indices = Vec::new();
        if !wanted.is_empty() {
            match match_text(&basename(rel), &wanted, MatchOptions::default()) {
                Some(hit) => name_indices = hit.indices,
                None => {
                    let in_snippet = location.snippet.iter().any(|line| line.to_lowercase().contains(&wanted));
                    if !in_snippet && !rel.to_lowercase().contains(&wanted) {
                        continue;
                    }
                }
            }
        }
        out.push(FilteredLocation { location: location.clone(), rel: rel.to_owned(), name_indices });
    }
    out
}

/// IDEA's Recent Locations time: "Just now", "14 minutes ago", then what `format` makes of the visit
/// (`ms`) seen from `now`, both Unix milliseconds: the pretty timestamp ("Yesterday 22:52"). Empty for
/// a place whose time is unknown.
pub fn location_time(ms: u64, now: u64, format: impl FnOnce(u64, u64) -> String) -> String {
    if ms == 0 {
        return String::new();
    }
    match now.checked_sub(ms).map(|elapsed| elapsed / 60_000) {
        Some(0) => "Just now".to_owned(),
        Some(1) => "1 minute ago".to_owned(),
        Some(minutes) if minutes < 60 => format!("{minutes} minutes ago"),
        _ => format(ms, now),
    }
}

/// Breadcrumb of the symbols around a location, outermost first ("OrderController > list()"), from
/// their names and kinds.
pub fn location_breadcrumb<'a>(path: impl IntoIterator<Item = (&'a str, SymbolKind)>) -> String {
    let names: Vec<String> = path.into_iter().map(|(name, kind)| symbol_display_name(name, kind)).collect();
    names.join(" > ")
}

/// A highlighted range of a text: `from..to` in UTF-16 code units, of the kind the highlighter gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeToken<K> {
    pub from: usize,
    pub to: usize,
    pub kind: K,
}

/// A run of a line with one highlight; `kind` is `None` for plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSegment<K> {
    pub text: String,
    pub kind: Option<K>,
}

/// Splits the highlight tokens of `lines` joined by line breaks (sorted, not overlapping) into the
/// segments of each line. A token spanning a line break colours its part of every line it covers.
pub fn line_segments<L: AsRef<str>, K: Clone>(lines: &[L], tokens: &[CodeToken<K>]) -> Vec<Vec<CodeSegment<K>>> {
    let mut out = Vec::with_capacity(lines.len());
    // UTF-16 offset of the current line in the joined text, and the first token that may reach into it.
    let mut start = 0;
    let mut first = 0;
    for line in lines {
        let line = line.as_ref();
        let end = start + utf16_len(line);
        let part = |from: usize, to: usize| {
            line[byte_at_utf16(line, from - start)..byte_at_utf16(line, to - start)].to_owned()
        };
        let mut segments = Vec::new();
        let mut pos = start;
        while tokens.get(first).is_some_and(|token| token.to <= start) {
            first += 1;
        }
        for token in tokens[first..].iter().take_while(|token| token.from < end) {
            let from = token.from.max(start);
            let to = token.to.min(end);
            if from > pos {
                segments.push(CodeSegment { text: part(pos, from), kind: None });
            }
            if to > from {
                segments.push(CodeSegment { text: part(from, to), kind: Some(token.kind.clone()) });
            }
            pos = pos.max(to);
        }
        if pos < end {
            segments.push(CodeSegment { text: part(pos, end), kind: None });
        }
        out.push(segments);
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000_000;

    fn pretty(_: u64, _: u64) -> String {
        "Yesterday 22:52".to_owned()
    }

    fn loc(path: &str, line: u32) -> RecentLocation {
        RecentLocation {
            path: path.to_owned(),
            line,
            col: 0,
            snippet: vec![format!("line {line}")],
            snippet_start: line,
            changed: false,
            time: u64::from(line),
        }
    }

    fn token(from: usize, to: usize, kind: &'static str) -> CodeToken<&'static str> {
        CodeToken { from, to, kind }
    }

    fn segment(text: &str, kind: &'static str) -> CodeSegment<&'static str> {
        CodeSegment { text: text.to_owned(), kind: (!kind.is_empty()).then_some(kind) }
    }

    const PATHS: [&str; 4] = ["/p/src/Main.kt", "/p/src/Order.kt", "/p/README.md", "/p/src/Main.kt"];

    fn in_project() -> RecentFilesOptions<'static> {
        RecentFilesOptions { root: Some("/p"), ..RecentFilesOptions::default() }
    }

    fn names(items: &[RecentFileItem]) -> Vec<&str> {
        items.iter().map(|item| item.name.as_str()).collect()
    }

    #[test]
    fn formats_times_like_idea() {
        assert_eq!(location_time(0, NOW, pretty), "");
        assert_eq!(location_time(NOW - 20_000, NOW, pretty), "Just now");
        assert_eq!(location_time(NOW - 60_000, NOW, pretty), "1 minute ago");
        assert_eq!(location_time(NOW - 14 * 60_000, NOW, pretty), "14 minutes ago");
        assert_eq!(location_time(NOW - 26 * 3_600_000, NOW, pretty), "Yesterday 22:52");
    }

    #[test]
    fn builds_the_symbol_breadcrumb() {
        let path = [("Foo", SymbolKind::Class), ("bar", SymbolKind::Method)];
        assert_eq!(location_breadcrumb(path), "Foo > bar()");
        assert_eq!(location_breadcrumb([]), "");
    }

    #[test]
    fn splits_highlight_tokens_per_line_also_across_line_breaks() {
        let lines = ["let a = 1;", "/* x", "y */ b"];
        let tokens = [token(0, 3, "tok-keyword"), token(8, 9, "tok-number"), token(11, 20, "tok-comment")];
        let segments = line_segments(&lines, &tokens);
        let (keyword, number) = (segment("let", "tok-keyword"), segment("1", "tok-number"));
        assert_eq!(segments[0], [keyword, segment(" a = ", ""), number, segment(";", "")]);
        assert_eq!(segments[1], [segment("/* x", "tok-comment")]);
        assert_eq!(segments[2], [segment("y */", "tok-comment"), segment(" b", "")]);
        let no_tokens: [CodeToken<&'static str>; 0] = [];
        let plain = line_segments(&["", "x"], &no_tokens);
        assert_eq!(plain, [Vec::new(), vec![segment("x", "")]]);
    }

    #[test]
    fn filters_by_name_snippet_and_changed_state() {
        let list = [
            RecentLocation { snippet: vec!["val total".to_owned()], ..loc("/p/Order.kt", 1) },
            RecentLocation { changed: true, ..loc("/p/Main.kt", 2) },
        ];
        let rels = |query: &str, changed_only: bool| -> Vec<String> {
            let opts = LocationFilter { changed_only, root: Some("/p") };
            filter_locations(&list, query, &opts).into_iter().map(|found| found.rel).collect()
        };
        assert_eq!(rels("ord", false), ["Order.kt"]);
        assert_eq!(rels("total", false), ["Order.kt"]);
        assert_eq!(rels("", true), ["Main.kt"]);
    }

    #[test]
    fn dedupes_filters_and_supports_edited_only() {
        let all = filter_recent_files(&PATHS, "", &in_project());
        let rels: Vec<&str> = all.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(rels, ["src/Main.kt", "src/Order.kt", "README.md"]);
        let hit = filter_recent_files(&PATHS, "ord", &in_project());
        assert_eq!(names(&hit), ["Order.kt"]);
        assert_eq!(hit[0].indices, [0, 1, 2]);
        assert_eq!(names(&filter_recent_files(&PATHS, "src", &in_project())), ["Main.kt", "Order.kt"]);
        let edited = HashSet::from(["/p/README.md".to_owned()]);
        let opts = RecentFilesOptions { edited: Some(&edited), edited_only: true, ..in_project() };
        assert_eq!(names(&filter_recent_files(&PATHS, "", &opts)), ["README.md"]);
    }

    #[test]
    fn preselects_the_previous_file_when_the_current_one_is_first() {
        let items = filter_recent_files(&PATHS, "", &in_project());
        assert_eq!(initial_file_index(&items, Some("/p/src/Main.kt"), ""), Some(1));
        assert_eq!(initial_file_index(&items, Some("/p/other"), ""), Some(0));
        assert_eq!(initial_file_index(&items, Some("/p/src/Main.kt"), "m"), Some(0));
    }

    #[test]
    fn lists_only_open_files_in_the_switcher_most_recent_first() {
        assert_eq!(switcher_paths(&["/c", "/x", "/a", "/c"], &["/a", "/b", "/c"]), ["/c", "/a", "/b"]);
    }

    #[test]
    fn lists_every_open_editor_in_the_switcher_not_just_files() {
        let tab = |kind: &str, key: &str, title: &str, last_active: u64, path: Option<&str>| {
            let path = path.map(str::to_owned);
            let icon = if kind == "file" { "file" } else { "toolwindow-agents" };
            OpenEditorTab {
                input: EditorInput { kind: kind.to_owned(), key: key.to_owned(), path, data: None },
                title: title.to_owned(),
                icon: icon.to_owned(),
                last_active,
            }
        };
        let tabs = [
            tab("agent-session", "agent:1", "Fix the parser", 3, None),
            tab("file", "/p/src/b.rs", "b.rs", 2, Some("/p/src/b.rs")),
            tab("file", "/p/src/a.rs", "a.rs", 0, Some("/p/src/a.rs")),
            tab("diff", "diff:x", "c.rs (main ↔ x)", 0, Some("/p/src/c.rs")),
        ];
        let items = switcher_items(&["/p/src/a.rs"], &tabs, "", Some("/p"));
        assert_eq!(names(&items), ["Fix the parser", "b.rs", "a.rs", "c.rs (main ↔ x)"]);
        assert_eq!((items[0].path.as_str(), items[0].dir.as_str()), ("agent:1", ""));
        assert_eq!(items[0].editor.as_ref().map(|editor| editor.icon.as_str()), Some("toolwindow-agents"));
        assert_eq!(items[1].editor, None);
        assert_eq!((items[3].path.as_str(), items[3].dir.as_str()), ("diff:x", "src"));
        assert_eq!(names(&switcher_items::<&str>(&[], &tabs, "parser", Some("/p"))), ["Fix the parser"]);
    }

    #[test]
    fn moves_the_switcher_selection_past_the_end_of_the_files_into_the_tool_windows() {
        let at = |column: SwitcherColumn, index: usize| Some(SwitcherSelection { column, index });
        assert_eq!(switcher_selection(1, 0, 3, 2), at(SwitcherColumn::Files, 1));
        assert_eq!(switcher_selection(1, 2, 3, 2), at(SwitcherColumn::Tools, 0));
        assert_eq!(switcher_selection(1, 4, 3, 2), at(SwitcherColumn::Files, 0));
        assert_eq!(switcher_selection(0, -1, 3, 2), at(SwitcherColumn::Tools, 1));
        assert_eq!(switcher_selection(3, -1, 3, 2), at(SwitcherColumn::Files, 2));
        assert_eq!(switcher_selection(0, 0, 0, 0), None);
    }

    #[test]
    fn orders_tool_windows_by_stripe() {
        let tw = |id: &str, anchor: Anchor, order: f64| ToolWindow {
            order: Some(order),
            ..ToolWindow::new(id, id, "folder", anchor)
        };
        let list = [
            tw("terminal", Anchor::Bottom, 10.0),
            tw("project", Anchor::Left, 0.0),
            ToolWindow { secondary: true, ..tw("bookmarks", Anchor::Left, 30.0) },
            tw("structure", Anchor::Left, 20.0),
        ];
        let ids = |project_open: bool, query: &str| -> Vec<String> {
            let found = switcher_tool_windows(&list, project_open, query);
            found.into_iter().map(|found| found.window.id.clone()).collect()
        };
        assert_eq!(ids(true, ""), ["project", "structure", "bookmarks", "terminal"]);
        assert_eq!(ids(true, "ter"), ["terminal"]);
        assert!(ids(false, "").is_empty());
    }

    // Not from a spec: tokens are measured in UTF-16 code units and segments are cut on characters.
    #[test]
    fn segments_of_lines_with_characters_outside_the_bmp() {
        let segments = line_segments(&["a😀b", "é"], &[token(1, 3, "tok-string"), token(5, 6, "tok-name")]);
        assert_eq!(segments[0], [segment("a", ""), segment("😀", "tok-string"), segment("b", "")]);
        assert_eq!(segments[1], [segment("é", "tok-name")]);
    }

    // Not from a spec: a place visited after `now` (clocks differ) and one an hour old show the formatted
    // time; a window that needs no project is listed without one.
    #[test]
    fn times_outside_the_last_hour_and_windows_without_a_project() {
        assert_eq!(location_time(NOW + 5_000, NOW, pretty), "Yesterday 22:52");
        assert_eq!(location_time(NOW - 60 * 60_000, NOW, pretty), "Yesterday 22:52");
        assert_eq!(location_time(NOW - 59 * 60_000, NOW, pretty), "59 minutes ago");
        let project = ToolWindow::new("project", "Project", "folder", Anchor::Left);
        let welcome = ToolWindow::new("welcome", "Welcome", "home", Anchor::Left);
        let welcome = ToolWindow { requires_project: false, ..welcome };
        let found = switcher_tool_windows([&project, &welcome], false, "");
        assert_eq!(found.iter().map(|found| found.window.id.as_str()).collect::<Vec<_>>(), ["welcome"]);
        assert_eq!(initial_file_index(&[], None, ""), None);
    }

    // Not from a spec: a virtual document shows its container as the directory and its name decoded.
    #[test]
    fn virtual_documents_show_their_container() {
        let uri = "jdt://contents/spring-boot-4.1.1.jar/org.springframework.boot/SpringApplication.class";
        let items = filter_recent_files(&[uri], "", &in_project());
        assert_eq!(items[0].name, "SpringApplication.class");
        assert_eq!(items[0].dir, "spring-boot-4.1.1.jar › org.springframework.boot");
    }
}
