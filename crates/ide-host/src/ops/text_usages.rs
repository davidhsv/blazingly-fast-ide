//! Find Usages and Call Hierarchy without a language server (Blaze mode, or a
//! language none serves): whole-word text matches outside comments and string
//! literals, placed with the syntax outline. The results are approximate and
//! say so (`approximate`), so the UI can offer Deep mode for exact ones.

use crate::Project;
use ide_core::text::OutlineItem;
use ide_core::{CancellationToken, DocId, IdeError, IdeResult, LanguageId, Location, Pos, Range, SymbolKind};
use ide_language::Syntax;
use ide_lsp::types::{CallHierarchy, HierarchyDirection, HierarchyItem, References, ReferencesIncomplete};
use ide_search::{FileMatches, SearchContext, SearchQuery, SearchScope};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Matches checked before the rest is reported as missing (`TooManyFiles`).
const MAX_MATCHES: u32 = 20_000;
/// How often the locations found so far go to the UI.
const PARTIAL_INTERVAL: Duration = Duration::from_millis(150);
/// Declarations one name may resolve to (overloads, same-named methods elsewhere).
const MAX_DECLARATIONS: usize = 50;

const CALLABLE: [SymbolKind; 3] = [SymbolKind::Function, SymbolKind::Method, SymbolKind::Constructor];

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| is_ident_char(c) && !c.is_ascii_digit()) && chars.all(is_ident_char)
}

/// The identifier at `pos` (UTF-16 column), as Find Usages picks it: the word
/// under the caret, or the one just before a caret at its end.
pub fn word_at(text: &str, pos: Pos) -> Option<String> {
    let line = text.split('\n').nth(pos.line as usize)?.trim_end_matches('\r');
    let mut at = line.len();
    let mut utf16 = 0u32;
    for (i, c) in line.char_indices() {
        if utf16 >= pos.col {
            at = i;
            break;
        }
        utf16 += c.len_utf16() as u32;
    }
    let mut start = at;
    if !line[at..].chars().next().is_some_and(is_ident_char) {
        let before = line[..at].chars().next_back().filter(|&c| is_ident_char(c))?;
        start = at - before.len_utf8();
    }
    while let Some(c) = line[..start].chars().next_back().filter(|&c| is_ident_char(c)) {
        start -= c.len_utf8();
    }
    let mut end = start;
    while let Some(c) = line[end..].chars().next().filter(|&c| is_ident_char(c)) {
        end += c.len_utf8();
    }
    let word = &line[start..end];
    is_identifier(word).then(|| word.to_owned())
}

/// Find in Files over the project: whole words (or `pattern` as a regex), code only.
fn query(project: &Project, pattern: String, is_regex: bool) -> SearchQuery {
    SearchQuery {
        pattern,
        is_regex,
        case_sensitive: true,
        whole_word: !is_regex,
        include: Vec::new(),
        exclude: project.search_excludes(),
        scope: SearchScope::Project,
        max_matches: MAX_MATCHES,
        context: SearchContext::ExceptCommentsAndStringLiterals,
    }
}

/// `$` is the only identifier character with a regex meaning.
fn regex_word(name: &str) -> String {
    name.replace('$', "\\$")
}

fn locations_of(file: &FileMatches) -> impl Iterator<Item = Location> + '_ {
    file.matches.iter().flat_map(move |m| {
        m.ranges.iter().map(move |&(s, e)| {
            let range = Range::new(Pos::new(m.line, m.text_offset + s), Pos::new(m.line, m.text_offset + e));
            Location::file(file.path.0.clone(), range)
        })
    })
}

fn sort_locations(locations: &mut [Location]) {
    locations.sort_by(|a, b| a.path.0.cmp(&b.path.0).then(a.range.start.cmp(&b.range.start)));
}

/// Whole-word occurrences of `name` across the project, outside comments and
/// strings. `partial` receives the locations found so far while the search runs.
pub fn text_references(
    project: &Project,
    syntax: &Syntax,
    name: &str,
    cancel: &CancellationToken,
    partial: &(dyn Fn(Vec<Location>) + Sync),
) -> IdeResult<References> {
    if !is_identifier(name) {
        return Err(IdeError::not_found("No identifier at the caret to find usages of"));
    }
    let query = query(project, name.to_owned(), false);
    let found: Mutex<(Vec<Location>, Instant)> = Mutex::new((Vec::new(), Instant::now()));
    let summary = super::search::search_project(project, syntax, &query, cancel, &|file| {
        let mut found = found.lock();
        found.0.extend(locations_of(&file));
        if found.1.elapsed() >= PARTIAL_INTERVAL {
            let mut snapshot = found.0.clone();
            sort_locations(&mut snapshot);
            partial(snapshot);
            found.1 = Instant::now();
        }
    })?;
    if summary.cancelled || cancel.is_cancelled() {
        return Err(IdeError::cancelled());
    }
    let (mut locations, _) = found.into_inner();
    sort_locations(&mut locations);
    Ok(References {
        locations,
        incomplete: summary.limit_hit.then_some(ReferencesIncomplete::TooManyFiles),
        approximate: true,
        ..Default::default()
    })
}

fn contains(range: &Range, pos: Pos) -> bool {
    range.start <= pos && pos <= range.end
}

/// The innermost function / method / constructor whose extent contains `pos`.
pub fn enclosing_callable(items: &[OutlineItem], pos: Pos) -> Option<&OutlineItem> {
    items.iter().find(|item| contains(&item.range, pos)).and_then(|item| {
        enclosing_callable(&item.children, pos).or_else(|| CALLABLE.contains(&item.kind).then_some(item))
    })
}

/// Whether `pos` is on a declared name (the declaration itself, not a call).
fn on_declaration(items: &[OutlineItem], pos: Pos) -> bool {
    items.iter().any(|item| {
        contains(&item.range, pos) && (contains(&item.selection_range, pos) || on_declaration(&item.children, pos))
    })
}

/// The name an outline item declares, without a signature suffix (`run(int)`).
fn declared_name(item: &OutlineItem) -> &str {
    let end = item.name.find(|c: char| !is_ident_char(c)).unwrap_or(item.name.len());
    &item.name[..end]
}

/// A file's language and text: the editor's unsaved text when open, else the disk.
fn load(project: &Project, path: &Path) -> Option<(LanguageId, String)> {
    if let Some(doc) = project.documents.by_path(path) {
        let snap = project.documents.text_snapshot(doc).ok()?;
        return (!snap.large).then_some((snap.language, snap.text));
    }
    let text = project.text_of(path).ok()?;
    let language = project.languages.detect(path, text.lines().next());
    Some((language, text))
}

fn hierarchy_item(path: PathBuf, item: &OutlineItem, from_ranges: Vec<Range>) -> HierarchyItem {
    HierarchyItem {
        name: declared_name(item).to_owned(),
        kind: item.kind,
        detail: item.detail.clone(),
        path: path.into(),
        uri: None,
        range: item.range,
        selection_range: item.selection_range,
        from_ranges,
    }
}

/// Callers or callees of the function enclosing `pos` in `doc`, by text.
pub fn call_hierarchy(
    project: &Project,
    syntax: &Syntax,
    doc: DocId,
    pos: Pos,
    direction: HierarchyDirection,
    cancel: &CancellationToken,
) -> IdeResult<CallHierarchy> {
    let no_target = || IdeError::not_found("No method at the caret to show the call hierarchy of");
    let snap = project.documents.text_snapshot(doc)?;
    let (path, _) = project.documents.identity(doc)?;
    let outline = syntax.outline(&snap.language, &snap.text);
    let item = enclosing_callable(&outline, pos).ok_or_else(no_target)?;
    let name = declared_name(item);
    if !is_identifier(name) {
        return Err(no_target());
    }
    match direction {
        HierarchyDirection::Incoming => incoming(project, syntax, name, cancel),
        HierarchyDirection::Outgoing => outgoing(project, syntax, &snap.language, &snap.text, &path, item, cancel),
    }
}

/// `name(` outside comments and strings, grouped by the function the call sits in.
fn incoming(project: &Project, syntax: &Syntax, name: &str, cancel: &CancellationToken) -> IdeResult<CallHierarchy> {
    let query = query(project, format!(r"\b{}\s*\(", regex_word(name)), true);
    let files: Mutex<Vec<FileMatches>> = Mutex::default();
    let summary = super::search::search_project(project, syntax, &query, cancel, &|file| files.lock().push(file))?;
    if summary.cancelled || cancel.is_cancelled() {
        return Err(IdeError::cancelled());
    }
    let mut files = files.into_inner();
    files.sort_by(|a, b| a.path.0.cmp(&b.path.0));
    let name_len = name.encode_utf16().count() as u32;
    let mut items: Vec<HierarchyItem> = Vec::new();
    let mut by_caller: HashMap<(PathBuf, Pos), usize> = HashMap::new();
    for file in files {
        if cancel.is_cancelled() {
            return Err(IdeError::cancelled());
        }
        let path = file.path.0;
        let Some((language, text)) = load(project, &path) else { continue };
        let outline = syntax.outline(&language, &text);
        for m in &file.matches {
            for &(start, _) in &m.ranges {
                let at = Pos::new(m.line, m.text_offset + start);
                if on_declaration(&outline, at) {
                    continue;
                }
                let Some(caller) = enclosing_callable(&outline, at) else { continue };
                let site = Range::new(at, Pos::new(at.line, at.col + name_len));
                let key = (path.clone(), caller.selection_range.start);
                match by_caller.get(&key) {
                    Some(&i) => items[i].from_ranges.push(site),
                    None => {
                        by_caller.insert(key, items.len());
                        items.push(hierarchy_item(path.clone(), caller, vec![site]));
                    }
                }
            }
        }
    }
    Ok(CallHierarchy {
        items,
        incomplete: summary.limit_hit.then_some(ReferencesIncomplete::TooManyFiles),
        approximate: true,
    })
}

/// Identifiers followed by `(` in the body of `item`, resolved by name through
/// the index to function / method / constructor declarations (and types, for
/// constructor calls). Unresolved names are dropped.
fn outgoing(
    project: &Project,
    syntax: &Syntax,
    language: &LanguageId,
    text: &str,
    path: &Path,
    item: &OutlineItem,
    cancel: &CancellationToken,
) -> IdeResult<CallHierarchy> {
    let document = ide_text::Document::new(text);
    let body = document.pos_to_byte(item.range.start)..document.pos_to_byte(item.range.end);
    let own = document.pos_to_byte(item.selection_range.start)..document.pos_to_byte(item.selection_range.end);
    let literals = syntax.literals(language, text).unwrap_or_default();
    let in_literal = |start: usize| literals.iter().any(|l| l.start <= start && start < l.end);
    let bytes = text.as_bytes();
    let mut calls: Vec<(String, Vec<Range>)> = Vec::new();
    let mut i = body.start;
    while i < body.end {
        let Some(c) = text[i..].chars().next() else { break };
        if !is_ident_char(c) {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        while let Some(c) = text[i..].chars().next().filter(|&c| is_ident_char(c)) {
            i += c.len_utf8();
        }
        let mut after = i;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        let name = &text[start..i];
        if bytes.get(after) != Some(&b'(') || !is_identifier(name) || in_literal(start) || own.contains(&start) {
            continue;
        }
        let site = Range::new(document.byte_to_pos(start), document.byte_to_pos(i));
        match calls.iter_mut().find(|(n, _)| n == name) {
            Some((_, sites)) => sites.push(site),
            None => calls.push((name.to_owned(), vec![site])),
        }
    }
    let mut items = Vec::new();
    for (name, sites) in calls {
        if cancel.is_cancelled() {
            return Err(IdeError::cancelled());
        }
        for symbol in project.index.symbols_named(&name, None, MAX_DECLARATIONS) {
            let callable = CALLABLE.contains(&symbol.kind) || symbol.kind.is_type_like();
            if !callable || (symbol.path.0 == path && contains(&symbol.range, item.selection_range.start)) {
                continue;
            }
            items.push(HierarchyItem {
                name: symbol.name,
                kind: symbol.kind,
                detail: symbol.container,
                path: symbol.path,
                uri: None,
                range: symbol.range,
                selection_range: symbol.range,
                from_ranges: sites.clone(),
            });
        }
    }
    Ok(CallHierarchy { items, incomplete: None, approximate: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_at_takes_the_identifier_under_or_before_the_caret() {
        let text = "int total = sum(a, b);\n  café$1 + x\n";
        assert_eq!(word_at(text, Pos::new(0, 13)).as_deref(), Some("sum"));
        assert_eq!(word_at(text, Pos::new(0, 15)).as_deref(), Some("sum"));
        assert_eq!(word_at(text, Pos::new(0, 12)).as_deref(), Some("sum"));
        assert_eq!(word_at(text, Pos::new(0, 9)).as_deref(), Some("total"));
        assert_eq!(word_at(text, Pos::new(0, 16)).as_deref(), Some("a"));
        assert_eq!(word_at(text, Pos::new(1, 4)).as_deref(), Some("café$1"));
        assert_eq!(word_at(text, Pos::new(0, 11)), None);
        assert_eq!(word_at(text, Pos::new(0, 18)), None);
        assert_eq!(word_at(text, Pos::new(5, 0)), None);
    }

    #[test]
    fn declared_names_drop_signatures_and_identifiers_are_checked() {
        let item = OutlineItem {
            name: "run(int, String)".into(),
            detail: None,
            kind: SymbolKind::Method,
            range: Range::default(),
            selection_range: Range::default(),
            children: Vec::new(),
            visibility: None,
        };
        assert_eq!(declared_name(&item), "run");
        assert!(is_identifier("_x$9"));
        assert!(!is_identifier("9x"));
        assert!(!is_identifier(""));
        assert_eq!(regex_word("a$b"), "a\\$b");
    }
}
