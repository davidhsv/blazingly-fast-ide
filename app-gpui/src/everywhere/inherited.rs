//! Structure › Show Inherited: the members of each type's supertypes (the language server's type hierarchy
//! and the supertypes' document symbols), minus the ones the type itself or a nearer supertype declares.
//!
//! Collecting them takes backend answers level by level. [`InheritedWalk`] holds the state in between: it
//! says what to ask next and is handed the answer, so a view can drive it without blocking.

use ide_core::{OutlineItem, Pos, Range, SymbolKind};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};

/// A member a type inherits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InheritedMember {
    pub item: OutlineItem,
    /// File (or virtual document URI) declaring the member.
    pub path: String,
    /// Name of the supertype declaring it.
    pub owner: String,
}

/// A supertype as the type hierarchy reports it. Mirrors the fields of `ide_lsp::types::HierarchyItem`
/// read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyItem {
    pub name: String,
    /// File path, or the URI of a virtual document.
    pub path: String,
    /// The identifier of the type.
    pub selection_range: Range,
}

/// How many levels of supertypes Show Inherited follows.
pub const INHERITED_DEPTH: usize = 6;

/// A member's name without its signature: `beta(int)` (jdtls) and `beta` name the same member.
fn base_name(name: &str) -> &str {
    name.find(['(', '<']).map_or(name, |signature| name[..signature].trim_end())
}

/// Key of a type in the current file: its identifier's position.
pub fn type_key(item: &OutlineItem) -> String {
    format!("{}:{}", item.selection_range.start.line, item.selection_range.start.col)
}

/// Every type declared in `items`, nested ones included, each before the types inside it.
pub fn types_in(items: &[OutlineItem]) -> Vec<&OutlineItem> {
    let mut out = Vec::new();
    collect_types(items, &mut out);
    out
}

fn collect_types<'a>(items: &'a [OutlineItem], out: &mut Vec<&'a OutlineItem>) {
    for item in items {
        if item.kind.is_type_like() {
            out.push(item);
        }
        collect_types(&item.children, out);
    }
}

/// The type declared at `at` in `outline`, matched by its identifier's line.
fn type_at(outline: &[OutlineItem], at: Pos) -> Option<&OutlineItem> {
    for item in outline {
        if item.kind.is_type_like() && item.selection_range.start.line == at.line {
            return Some(item);
        }
        if (item.range.start.line..=item.range.end.line).contains(&at.line)
            && let Some(deeper) = type_at(&item.children, at)
        {
            return Some(deeper);
        }
    }
    None
}

/// What an [`InheritedWalk`] needs from the backend next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InheritedRequest {
    /// The supertypes of the type whose identifier is at `pos` in `path`.
    Supertypes { path: String, pos: Pos },
    /// The document symbols of `path`.
    Outline { path: String },
}

/// Collects the inherited members of one type, nearest supertypes first.
///
/// Call [`InheritedWalk::request`] and answer it with [`InheritedWalk::supertypes_loaded`] or
/// [`InheritedWalk::outline_loaded`] (with nothing, when the backend fails) until it returns `None`; then
/// [`InheritedWalk::into_members`] has the result.
#[derive(Debug, Clone)]
pub struct InheritedWalk {
    /// Names declared by the type and by the supertypes handled so far: a nearer declaration hides
    /// farther ones.
    own: HashSet<String>,
    out: Vec<InheritedMember>,
    /// Supertypes met so far, by file and identifier line.
    seen: HashSet<(String, u32)>,
    outlines: HashMap<String, Vec<OutlineItem>>,
    /// Levels of the hierarchy still to follow, the current one included.
    levels_left: usize,
    /// Types of the current level whose supertypes are still to be asked for.
    level: VecDeque<(String, Pos)>,
    /// Types of the next level.
    next: Vec<(String, Pos)>,
    /// The type whose supertypes were asked for and have not arrived.
    asking: Option<(String, Pos)>,
    /// Supertypes of that type still to handle.
    supertypes: VecDeque<HierarchyItem>,
    /// The supertype being handled, waiting for the outline of its file.
    pending: Option<HierarchyItem>,
}

impl InheritedWalk {
    /// A walk over the supertypes of `type_item`, declared in `path`, up to `depth` levels.
    pub fn new(path: &str, type_item: &OutlineItem, depth: usize) -> Self {
        Self {
            own: type_item.children.iter().map(|child| base_name(&child.name).to_owned()).collect(),
            out: Vec::new(),
            seen: HashSet::new(),
            outlines: HashMap::new(),
            levels_left: depth,
            level: VecDeque::from([(path.to_owned(), type_item.selection_range.start)]),
            next: Vec::new(),
            asking: None,
            supertypes: VecDeque::new(),
            pending: None,
        }
    }

    /// What to ask the backend next; `None` once every level is done. The same request is returned until
    /// it is answered.
    pub fn request(&mut self) -> Option<InheritedRequest> {
        loop {
            if let Some(supertype) = self.pending.take() {
                let Some(outline) = self.outlines.get(&supertype.path) else {
                    let path = supertype.path.clone();
                    self.pending = Some(supertype);
                    return Some(InheritedRequest::Outline { path });
                };
                // Overloads within one supertype all show: its names hide farther ones only once it is done.
                let mut declared = Vec::new();
                let declaration = type_at(outline, supertype.selection_range.start);
                for member in declaration.into_iter().flat_map(|declaration| &declaration.children) {
                    let name = base_name(&member.name);
                    let skipped = member.kind == SymbolKind::Constructor || member.kind.is_type_like();
                    if skipped || self.own.contains(name) {
                        continue;
                    }
                    declared.push(name.to_owned());
                    self.out.push(InheritedMember {
                        item: member.clone(),
                        path: supertype.path.clone(),
                        owner: base_name(&supertype.name).to_owned(),
                    });
                }
                self.own.extend(declared);
                continue;
            }
            if let Some(supertype) = self.supertypes.pop_front() {
                if self.seen.insert((supertype.path.clone(), supertype.selection_range.start.line)) {
                    self.next.push((supertype.path.clone(), supertype.selection_range.start));
                    self.pending = Some(supertype);
                }
                continue;
            }
            if let Some((path, pos)) = &self.asking {
                return Some(InheritedRequest::Supertypes { path: path.clone(), pos: *pos });
            }
            if self.levels_left == 0 {
                return None;
            }
            if let Some(subtype) = self.level.pop_front() {
                self.asking = Some(subtype);
                continue;
            }
            self.level = std::mem::take(&mut self.next).into();
            self.levels_left = if self.level.is_empty() { 0 } else { self.levels_left - 1 };
        }
    }

    /// Answers the pending [`InheritedRequest::Supertypes`]; ignored when none is pending.
    pub fn supertypes_loaded(&mut self, supertypes: Vec<HierarchyItem>) {
        if self.asking.take().is_some() {
            self.supertypes = supertypes.into();
        }
    }

    /// Answers an [`InheritedRequest::Outline`]. The first outline given for a file is kept.
    pub fn outline_loaded(&mut self, path: &str, outline: Vec<OutlineItem>) {
        self.outlines.entry(path.to_owned()).or_insert(outline);
    }

    /// The members collected so far: all of them once [`InheritedWalk::request`] returned `None`.
    pub fn into_members(self) -> Vec<InheritedMember> {
        self.out
    }
}

/// The inherited members of `type_item` (declared in `path`), nearest supertypes first, up to `depth`
/// levels, with the backend answering at once: `supertypes` gives the supertypes of the type at a
/// position, `outline` the document symbols of a file; either gives nothing when it fails.
pub fn inherited_members(
    path: &str,
    type_item: &OutlineItem,
    depth: usize,
    mut supertypes: impl FnMut(&str, Pos) -> Vec<HierarchyItem>,
    mut outline: impl FnMut(&str) -> Vec<OutlineItem>,
) -> Vec<InheritedMember> {
    let mut walk = InheritedWalk::new(path, type_item, depth);
    while let Some(request) = walk.request() {
        match request {
            InheritedRequest::Supertypes { path, pos } => walk.supertypes_loaded(supertypes(path.as_str(), pos)),
            InheritedRequest::Outline { path } => {
                let symbols = outline(path.as_str());
                walk.outline_loaded(&path, symbols);
            }
        }
    }
    walk.into_members()
}

#[cfg(test)]
mod tests {
    use super::super::outline::{key_outline, symbol_path_at, with_inherited};
    use super::*;

    fn item(name: &str, kind: SymbolKind, line: u32, children: Vec<OutlineItem>) -> OutlineItem {
        let len = name.len() as u32;
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            range: Range::new(Pos::new(line, 0), Pos::new(line + 5, 1)),
            selection_range: Range::new(Pos::new(line, 6), Pos::new(line, 6 + len)),
            children,
            visibility: None,
        }
    }

    fn hier(name: &str, path: &str, line: u32) -> HierarchyItem {
        let len = name.len() as u32;
        HierarchyItem {
            name: name.to_owned(),
            path: path.to_owned(),
            selection_range: Range::new(Pos::new(line, 6), Pos::new(line, 6 + len)),
        }
    }

    fn dog() -> OutlineItem {
        let members = vec![item("bark", SymbolKind::Method, 1, vec![]), item("eat()", SymbolKind::Method, 2, vec![])];
        item("Dog", SymbolKind::Class, 0, members)
    }

    fn outline_of(path: &str) -> Vec<OutlineItem> {
        match path {
            "/p/Dog.java" => vec![dog()],
            "/p/Animal.java" => {
                let members = vec![
                    item("Animal", SymbolKind::Constructor, 1, vec![]),
                    item("eat(String)", SymbolKind::Method, 2, vec![]),
                    item("sleep()", SymbolKind::Method, 3, vec![]),
                    item("sleep(int)", SymbolKind::Method, 4, vec![]),
                ];
                vec![item("Animal", SymbolKind::Class, 0, members)]
            }
            "/p/Living.java" => {
                let members =
                    vec![item("sleep()", SymbolKind::Method, 1, vec![]), item("age", SymbolKind::Field, 2, vec![])];
                vec![item("Living", SymbolKind::Interface, 0, members)]
            }
            _ => Vec::new(),
        }
    }

    fn supertypes_of(path: &str, pos: Pos) -> Vec<HierarchyItem> {
        match (path, pos.line) {
            ("/p/Dog.java", 0) => vec![hier("Animal", "/p/Animal.java", 0)],
            ("/p/Animal.java", 0) => vec![hier("Living", "/p/Living.java", 0)],
            _ => Vec::new(),
        }
    }

    fn members_of_dog() -> Vec<InheritedMember> {
        inherited_members("/p/Dog.java", &dog(), INHERITED_DEPTH, supertypes_of, outline_of)
    }

    #[test]
    fn collects_supertype_members_level_by_level_without_overridden_ones_or_constructors() {
        let members = members_of_dog();
        let listed: Vec<String> =
            members.iter().map(|member| format!("{}.{}", member.owner, member.item.name)).collect();
        assert_eq!(listed, ["Animal.sleep()", "Animal.sleep(int)", "Living.age"]);
        assert_eq!(members[0].path, "/p/Animal.java");
    }

    #[test]
    fn finds_nested_types_and_appends_inherited_members_after_a_types_own_ones() {
        let outline = [item("Outer", SymbolKind::Class, 0, vec![item("Inner", SymbolKind::Class, 1, vec![])])];
        let types: Vec<&str> = types_in(&outline).into_iter().map(|found| found.name.as_str()).collect();
        assert_eq!(types, ["Outer", "Inner"]);

        let members = members_of_dog();
        let inherited = HashMap::from([(type_key(&dog()), members.clone())]);
        let nodes = with_inherited(&key_outline(&[dog()]), Some(&inherited));
        let children: Vec<(&str, Option<&str>)> = nodes[0]
            .children
            .iter()
            .map(|child| (child.item.name.as_str(), child.inherited.as_ref().map(|from| from.owner.as_str())))
            .collect();
        let expected = [
            ("bark", None),
            ("eat()", None),
            ("sleep()", Some("Animal")),
            ("sleep(int)", Some("Animal")),
            ("age", Some("Living")),
        ];
        assert_eq!(children, expected);
        // Inherited members live in other files: the caret never selects them.
        let empty = item("Puppy", SymbolKind::Class, 0, vec![]);
        let inherited = HashMap::from([(type_key(&empty), members)]);
        let bare = with_inherited(&key_outline(&[empty]), Some(&inherited));
        assert_eq!(bare[0].children.len(), 3);
        assert_eq!(symbol_path_at(&bare, Pos::new(3, 0)), [bare[0].key.clone()]);
    }

    // Not from a spec: the walk asks for one thing at a time, repeats an unanswered request, asks for a
    // file's outline once and treats failed answers as empty.
    #[test]
    fn the_walk_asks_step_by_step() {
        let mut walk = InheritedWalk::new("/p/Dog.java", &dog(), INHERITED_DEPTH);
        let start = Pos::new(0, 6);
        let supertypes = |path: &str| Some(InheritedRequest::Supertypes { path: path.to_owned(), pos: start });
        assert_eq!(walk.request(), supertypes("/p/Dog.java"));
        assert_eq!(walk.request(), supertypes("/p/Dog.java"));
        walk.supertypes_loaded(vec![hier("Animal", "/p/Animal.java", 0), hier("Animal", "/p/Animal.java", 0)]);
        let outline = Some(InheritedRequest::Outline { path: "/p/Animal.java".to_owned() });
        assert_eq!(walk.request(), outline);
        assert_eq!(walk.request(), outline);
        walk.outline_loaded("/p/Animal.java", outline_of("/p/Animal.java"));
        assert_eq!(walk.request(), supertypes("/p/Animal.java"));
        walk.supertypes_loaded(Vec::new());
        assert_eq!(walk.request(), None);
        let names: Vec<String> = walk.into_members().into_iter().map(|member| member.item.name).collect();
        assert_eq!(names, ["sleep()", "sleep(int)"]);
    }

    // Not from a spec: the depth limits the levels followed, and a supertype whose outline is missing
    // contributes nothing but its own supertypes.
    #[test]
    fn depth_limits_the_levels_and_missing_outlines_contribute_nothing() {
        let one_level = inherited_members("/p/Dog.java", &dog(), 1, supertypes_of, outline_of);
        assert!(one_level.iter().all(|member| member.owner == "Animal"));
        assert_eq!(one_level.len(), 2);
        assert!(inherited_members("/p/Dog.java", &dog(), 0, supertypes_of, outline_of).is_empty());
        let without_animal = |path: &str| if path == "/p/Animal.java" { Vec::new() } else { outline_of(path) };
        let members = inherited_members("/p/Dog.java", &dog(), INHERITED_DEPTH, supertypes_of, without_animal);
        let listed: Vec<&str> = members.iter().map(|member| member.item.name.as_str()).collect();
        assert_eq!(listed, ["sleep()", "age"]);
    }

    // Not from a spec: signatures and type parameters are not part of a member's name.
    #[test]
    fn base_names_drop_signatures() {
        assert_eq!(base_name("beta(int)"), "beta");
        assert_eq!(base_name("map <T>(f)"), "map");
        assert_eq!(base_name("List<T>"), "List");
        assert_eq!(base_name("age"), "age");
    }
}
