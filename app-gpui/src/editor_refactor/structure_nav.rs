//! Outline-based navigation: Next / Previous Method (⌃⇧↓ / ⌃⇧↑) and Super Method (⌘U). The
//! outline is the language server's document symbols, or the syntax tree's without a server.

use ide_core::ipc_path::to_ipc;
use ide_core::{Location, OutlineItem, Pos, SymbolKind};
use ide_lsp::types::HierarchyItem;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

fn is_method(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor)
}

fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Module
            | SymbolKind::Namespace
    )
}

fn inside(item: &OutlineItem, pos: Pos) -> bool {
    item.range.start <= pos && pos <= item.range.end
}

/// The stops of Next / Previous Method: methods and types, in document order.
pub fn method_stops(outline: &[OutlineItem]) -> Vec<Pos> {
    fn walk(items: &[OutlineItem], out: &mut Vec<Pos>) {
        for item in items {
            if is_method(item.kind) || is_type(item.kind) {
                out.push(item.selection_range.start);
            }
            walk(&item.children, out);
        }
    }
    let mut out = Vec::new();
    walk(outline, &mut out);
    out.sort();
    out.dedup();
    out
}

/// The next (`forward`) or previous stop from `caret`, comparing by line like IDEA.
pub fn method_stop(outline: &[OutlineItem], caret: Pos, forward: bool) -> Option<Pos> {
    let stops = method_stops(outline);
    if forward {
        stops.into_iter().find(|stop| stop.line > caret.line)
    } else {
        stops.into_iter().rev().find(|stop| *stop < caret)
    }
}

/// The member at the caret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaretMember<'a> {
    /// The innermost method at the caret.
    pub method: Option<&'a OutlineItem>,
    /// The innermost type around the caret (around `method` when there is one).
    pub r#type: Option<&'a OutlineItem>,
}

pub fn member_at(outline: &[OutlineItem], caret: Pos) -> CaretMember<'_> {
    let mut member = CaretMember::default();
    let mut level = outline;
    while let Some(hit) = level.iter().find(|item| inside(item, caret)) {
        if is_method(hit.kind) {
            member.method = Some(hit);
        } else if is_type(hit.kind) && member.method.is_none() {
            member.r#type = Some(hit);
        }
        level = &hit.children;
    }
    member
}

static PARAMETERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[(<].*$").expect("the pattern is valid"));
static RETURN_TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*:.*$").expect("the pattern is valid"));

/// `beta(int)` (as jdtls names methods) and `beta` name the same method.
pub fn base_name(name: &str) -> String {
    PARAMETERS.replace(name, "").into_owned()
}

/// `beta(int) : void` is the signature `beta(int)`; plain names stay as they are.
fn signature(name: &str) -> String {
    RETURN_TYPE.replace(name, "").into_owned()
}

/// The outline node of the type declared at `item` (found by position).
pub fn type_of<'a>(outline: &'a [OutlineItem], item: &HierarchyItem) -> Option<&'a OutlineItem> {
    fn find(items: &[OutlineItem], at: Pos) -> Option<&OutlineItem> {
        for item in items {
            if is_type(item.kind) && item.selection_range.start.line == at.line {
                return Some(item);
            }
            if inside(item, at)
                && let Some(deeper) = find(&item.children, at)
            {
                return Some(deeper);
            }
        }
        None
    }
    find(outline, item.selection_range.start)
}

/// The methods named `name` directly inside the type declared at `item`: the overload with the
/// same signature when the server names them with one (`eat(String)`), else every method of
/// that name.
pub fn methods_in_type<'a>(outline: &'a [OutlineItem], item: &HierarchyItem, name: &str) -> Vec<&'a OutlineItem> {
    let bare = base_name(name);
    let named: Vec<&OutlineItem> = type_of(outline, item)
        .map(|found| found.children.iter().filter(|c| is_method(c.kind) && base_name(&c.name) == bare).collect())
        .unwrap_or_default();
    let wanted = signature(name);
    let exact: Vec<&OutlineItem> = named.iter().copied().filter(|c| signature(&c.name) == wanted).collect();
    if exact.is_empty() { named } else { exact }
}

/// A super method and the supertype declaring it.
#[derive(Debug, Clone)]
pub struct SuperMethod {
    pub location: Location,
    pub owner: HierarchyItem,
    pub method: OutlineItem,
}

/// What a [`SuperMethodSearch`] asks of the caller.
#[derive(Debug, Clone)]
pub enum SuperStep {
    /// The supertypes of the type at `pos` of `path` (type hierarchy, incoming); answer with
    /// [`SuperMethodSearch::supertypes`].
    Supertypes { ticket: u64, path: String, pos: Pos },
    /// The outline of `path`; answer with [`SuperMethodSearch::outline`].
    Outline { ticket: u64, path: String },
    /// The super methods, nearest level first; empty when there is none.
    Done(Vec<SuperMethod>),
}

/// The super methods of `method` (a member of the type at `type_pos` of `path`), searching up to
/// `depth` levels of supertypes, level by level.
#[derive(Debug, Clone)]
pub struct SuperMethodSearch {
    method: String,
    depth: usize,
    level_index: usize,
    level: Vec<(String, Pos)>,
    current: usize,
    next: Vec<(String, Pos)>,
    found: Vec<SuperMethod>,
    seen: HashSet<(String, u32)>,
    supers: Vec<HierarchyItem>,
    super_index: usize,
    ticket: u64,
}

impl SuperMethodSearch {
    pub fn start(path: &str, type_pos: Pos, method: &str, depth: usize) -> (Self, SuperStep) {
        let mut search = Self {
            method: method.to_owned(),
            depth,
            level_index: 0,
            level: vec![(path.to_owned(), type_pos)],
            current: 0,
            next: Vec::new(),
            found: Vec::new(),
            seen: HashSet::new(),
            supers: Vec::new(),
            super_index: 0,
            ticket: 0,
        };
        let step = search.ask_supertypes();
        (search, step)
    }

    fn ask_supertypes(&mut self) -> SuperStep {
        loop {
            if self.level_index >= self.depth || self.level.is_empty() {
                return SuperStep::Done(Vec::new());
            }
            if let Some((path, pos)) = self.level.get(self.current).cloned() {
                self.ticket += 1;
                return SuperStep::Supertypes { ticket: self.ticket, path, pos };
            }
            // The level is done.
            if !self.found.is_empty() {
                return SuperStep::Done(std::mem::take(&mut self.found));
            }
            self.level = std::mem::take(&mut self.next);
            self.level_index += 1;
            self.current = 0;
        }
    }

    fn next_super(&mut self) -> SuperStep {
        while let Some(item) = self.supers.get(self.super_index) {
            let path = to_ipc(&item.path);
            if self.seen.insert((path.clone(), item.selection_range.start.line)) {
                self.ticket += 1;
                return SuperStep::Outline { ticket: self.ticket, path };
            }
            self.super_index += 1;
        }
        self.current += 1;
        self.ask_supertypes()
    }

    /// The supertypes asked for (a failure counts as none).
    pub fn supertypes(
        &mut self,
        ticket: u64,
        result: Result<Vec<HierarchyItem>, ide_core::IdeError>,
    ) -> Option<SuperStep> {
        if ticket != self.ticket {
            return None;
        }
        self.supers = result.unwrap_or_default();
        self.super_index = 0;
        Some(self.next_super())
    }

    /// The outline asked for (a failure counts as empty).
    pub fn outline(&mut self, ticket: u64, result: Result<Vec<OutlineItem>, ide_core::IdeError>) -> Option<SuperStep> {
        if ticket != self.ticket {
            return None;
        }
        let outline = result.unwrap_or_default();
        let item = self.supers.get(self.super_index)?.clone();
        let methods = methods_in_type(&outline, &item, &self.method);
        if methods.is_empty() {
            self.next.push((to_ipc(&item.path), item.selection_range.start));
        }
        for method in methods {
            self.found.push(SuperMethod {
                location: Location { path: item.path.clone(), range: method.selection_range, uri: item.uri.clone() },
                owner: item.clone(),
                method: method.clone(),
            });
        }
        self.super_index += 1;
        Some(self.next_super())
    }
}

/// How many levels of supertypes Super Method searches.
pub const SUPER_DEPTH: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Range;
    use std::collections::HashMap;

    fn r(from: u32, to: u32) -> Range {
        Range::new(Pos::new(from, 0), Pos::new(to, 1))
    }

    fn item(name: &str, kind: SymbolKind, from: u32, to: u32, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            range: r(from, to),
            selection_range: Range::new(Pos::new(from, 4), Pos::new(from, 4 + name.len() as u32)),
            children,
            visibility: None,
        }
    }

    fn outline() -> Vec<OutlineItem> {
        vec![
            item(
                "Foo",
                SymbolKind::Class,
                0,
                20,
                vec![
                    item("x", SymbolKind::Field, 1, 1, Vec::new()),
                    item("a", SymbolKind::Method, 2, 5, Vec::new()),
                    item("b", SymbolKind::Method, 7, 9, Vec::new()),
                    item("Inner", SymbolKind::Class, 11, 18, vec![item("c", SymbolKind::Method, 12, 14, Vec::new())]),
                ],
            ),
            item("main", SymbolKind::Function, 22, 25, Vec::new()),
        ]
    }

    #[test]
    fn stops_at_methods_and_types_in_document_order() {
        let lines: Vec<u32> = method_stops(&outline()).iter().map(|pos| pos.line).collect();
        assert_eq!(lines, [0, 2, 7, 11, 12, 22]);
    }

    #[test]
    fn moves_by_line() {
        let o = outline();
        let line = |caret: Pos, forward: bool| method_stop(&o, caret, forward).map(|pos| pos.line);
        assert_eq!(line(Pos::new(3, 0), true), Some(7));
        assert_eq!(line(Pos::new(7, 0), true), Some(11));
        assert_eq!(line(Pos::new(7, 8), false), Some(7));
        assert_eq!(line(Pos::new(7, 2), false), Some(2));
        assert_eq!(line(Pos::new(0, 0), false), None);
        assert_eq!(line(Pos::new(23, 0), true), None);
    }

    #[test]
    fn finds_the_method_and_type_at_the_caret() {
        let o = outline();
        let names =
            |member: CaretMember<'_>| (member.method.map(|m| m.name.clone()), member.r#type.map(|t| t.name.clone()));
        assert_eq!(names(member_at(&o, Pos::new(13, 2))), (Some("c".to_owned()), Some("Inner".to_owned())));
        assert_eq!(names(member_at(&o, Pos::new(10, 0))), (None, Some("Foo".to_owned())));
        assert_eq!(names(member_at(&o, Pos::new(23, 0))), (Some("main".to_owned()), None));
    }

    fn hier(name: &str, path: &str, line: u32) -> HierarchyItem {
        HierarchyItem {
            name: name.to_owned(),
            kind: SymbolKind::Class,
            detail: None,
            path: path.into(),
            uri: None,
            range: r(line, line + 10),
            selection_range: Range::new(Pos::new(line, 4), Pos::new(line, 7)),
            from_ranges: Vec::new(),
        }
    }

    fn search(
        method: &str,
        supers: &HashMap<&str, Vec<HierarchyItem>>,
        outlines: &dyn Fn(&str) -> Vec<OutlineItem>,
    ) -> Vec<SuperMethod> {
        let (mut flow, mut step) = SuperMethodSearch::start("/Foo.java", Pos::new(0, 4), method, SUPER_DEPTH);
        loop {
            step = match step {
                SuperStep::Supertypes { ticket, path, .. } => {
                    flow.supertypes(ticket, Ok(supers.get(path.as_str()).cloned().unwrap_or_default()))
                }
                SuperStep::Outline { ticket, path } => flow.outline(ticket, Ok(outlines(&path))),
                SuperStep::Done(found) => return found,
            }
            .expect("the answer is awaited");
        }
    }

    #[test]
    fn looks_for_the_method_in_supertypes_level_by_level() {
        let supers = HashMap::from([
            ("/Foo.java", vec![hier("Base", "/Base.java", 0)]),
            ("/Base.java", vec![hier("Root", "/Root.java", 0)]),
        ]);
        let outlines = |path: &str| match path {
            "/Base.java" => {
                vec![item("Base", SymbolKind::Class, 0, 10, vec![item("other", SymbolKind::Method, 2, 3, Vec::new())])]
            }
            "/Root.java" => {
                vec![item("Root", SymbolKind::Interface, 0, 10, vec![item("a", SymbolKind::Method, 4, 4, Vec::new())])]
            }
            _ => Vec::new(),
        };
        let found = search("a", &supers, &outlines);
        let locations: Vec<(String, Range)> =
            found.iter().map(|s| (to_ipc(&s.location.path), s.location.range)).collect();
        assert_eq!(locations, [("/Root.java".to_owned(), Range::new(Pos::new(4, 4), Pos::new(4, 5)))]);
        assert!(search("missing", &supers, &outlines).is_empty());
        // jdtls names methods with their parameters.
        assert_eq!(search("a() : void", &supers, &outlines).len(), 1);
    }

    #[test]
    fn prefers_the_overload_with_the_same_signature() {
        let supers = HashMap::from([("/Foo.java", vec![hier("Base", "/Base.java", 0)])]);
        let outlines = |_: &str| {
            vec![item(
                "Base",
                SymbolKind::Class,
                0,
                10,
                vec![
                    item("eat(String)", SymbolKind::Method, 2, 2, Vec::new()),
                    item("eat(int)", SymbolKind::Method, 4, 4, Vec::new()),
                ],
            )]
        };
        let lines = |name: &str| -> Vec<u32> {
            search(name, &supers, &outlines).iter().map(|s| s.location.range.start.line).collect()
        };
        assert_eq!(lines("eat(int)"), [4]);
        // Without a matching signature (or without signatures at all), every overload.
        assert_eq!(lines("eat(T)"), [2, 4]);
        assert_eq!(lines("eat"), [2, 4]);
    }
}
