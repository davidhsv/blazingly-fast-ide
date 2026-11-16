//! The document's structure as the backend's tree-sitter service describes it, for the editor's
//! commands: whether the language has a syntax tree, the statement and declaration ranges around a
//! position, the syntax errors, and the chains of nodes around positions.
//!
//! Answers are asked off the window's thread ([`commands`]) and kept per document and version,
//! so repeated presses (Move Statement Up three times) cost one backend round trip per position
//! and none while nothing changed. An answer for a version the document has left is dropped; a
//! failed query (no grammar, too large, not open) is kept as "no answer" for that version.
//!
//! The highlights of the current text, which come from the same grammars, give the trees the
//! commands that only ask "is this code" or "is this an escape" read without a round trip.

pub mod commands;
#[cfg(test)]
mod proofs25_structure;

use super::find_model::literal_ranges;
use super::view::{Editor, HighlightSpan};
use crate::editor_commands::EditState;
use crate::editor_commands::tree::{SyntaxTree, TreeNode, pascal_name};
use crate::editor_lsp::positions::pos_to_offset;
use crate::platform::search::LiteralKind;
use gpui_kit::{App, EntityId, Global};
use ide_core::{DocId, IdeResult, LanguageId, Pos};
use ide_language::SyntaxHighlightKind;
use ide_syntax::{NodeAt, StructuralRanges, StructureSupport, SyntaxErrors, SyntaxNode};
use regex::Regex;
use ropey::Rope;
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

/// Positions whose answers are kept per document version, at most; the oldest goes first.
pub const MAX_CACHED_POSITIONS: usize = 64;
/// Documents whose answers are kept, at most.
const MAX_CACHED_DOCUMENTS: usize = 32;

/// What the cache knows of a query: its answer (`None` when the service could not answer), or
/// that the backend must be asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    Known(T),
    Ask,
}

/// The editing modes that parse the whole document with a grammar of their own, rather than
/// colouring a stream of tokens: their editors complete statements from the text rules alone,
/// except Java's.
const PARSED_MODES: [&str; 33] = [
    "C",
    "C++",
    "CQL",
    "CSS",
    "Go",
    "HTML",
    "Java",
    "JavaScript",
    "Jinja",
    "JSON",
    "JSX",
    "LESS",
    "Liquid",
    "MariaDB SQL",
    "Markdown",
    "MS SQL",
    "MySQL",
    "PHP",
    "PLSQL",
    "PostgreSQL",
    "Python",
    "Rust",
    "Sass",
    "SCSS",
    "SQL",
    "SQLite",
    "TSX",
    "TypeScript",
    "WebAssembly",
    "XML",
    "YAML",
    "Vue",
    "Angular Template",
];

/// A document of editing mode `mode` (`DocumentInfo::ui_mode`) has a syntax tree of its editor's
/// own; a mode that colours a token stream, or no mode, has none. Angular's TypeScript mode
/// extends the TypeScript grammar.
pub fn parses_itself(mode: Option<&str>) -> bool {
    mode.is_some_and(|mode| mode == "Angular TypeScript" || PARSED_MODES.contains(&mode))
}

/// What the structure service and the editing mode offer for a document's language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Support {
    /// The backend has a syntax tree of the language ([`has_syntax_tree`]).
    pub tree: bool,
    /// The document's editing mode parses it itself ([`parses_itself`]).
    pub parsed_mode: bool,
}

/// The structure answers of one document.
#[derive(Debug, Default)]
pub struct StructureCache {
    /// The document version the answers below are of.
    version: Option<u32>,
    /// What the language offers; it does not change with the text.
    support: Option<(LanguageId, Support)>,
    ranges: VecDeque<(Pos, Option<StructuralRanges>)>,
    errors: Option<Option<SyntaxErrors>>,
    nodes: VecDeque<(Pos, Option<NodeAt>)>,
}

/// `queue` with `answer` for `pos` added, the oldest position leaving once there are too many.
fn remember<T>(queue: &mut VecDeque<(Pos, Option<T>)>, pos: Pos, answer: Option<T>) {
    queue.retain(|(known, _)| *known != pos);
    if queue.len() >= MAX_CACHED_POSITIONS {
        queue.pop_front();
    }
    queue.push_back((pos, answer));
}

fn recall<T: Clone>(queue: &VecDeque<(Pos, Option<T>)>, pos: Pos) -> Lookup<Option<T>> {
    queue.iter().find(|(known, _)| *known == pos).map_or(Lookup::Ask, |(_, answer)| Lookup::Known(answer.clone()))
}

impl StructureCache {
    /// Forgets the answers of other versions than `version`.
    fn fresh(&mut self, version: u32) {
        if self.version != Some(version) {
            self.version = Some(version);
            self.ranges.clear();
            self.errors = None;
            self.nodes.clear();
        }
    }

    /// The cache is at `version` for storing an answer of it; an answer of a version the document
    /// has left is dropped.
    fn accepts(&mut self, version: u32) -> bool {
        if self.version.is_some_and(|known| known > version) {
            return false;
        }
        self.fresh(version);
        true
    }

    /// What documents of `language` have.
    pub fn support(&self, language: &LanguageId) -> Lookup<Support> {
        match &self.support {
            Some((known, support)) if known == language => Lookup::Known(*support),
            _ => Lookup::Ask,
        }
    }

    pub fn set_support(&mut self, language: LanguageId, support: Support) {
        self.support = Some((language, support));
    }

    /// The statement and declaration ranges at `pos` of the text at `version`.
    pub fn ranges(&mut self, version: u32, pos: Pos) -> Lookup<Option<StructuralRanges>> {
        self.fresh(version);
        recall(&self.ranges, pos)
    }

    pub fn set_ranges(&mut self, version: u32, pos: Pos, answer: IdeResult<StructuralRanges>) {
        if self.accepts(version) {
            remember(&mut self.ranges, pos, answer.ok());
        }
    }

    /// The syntax errors of the text at `version`.
    pub fn errors(&mut self, version: u32) -> Lookup<Option<SyntaxErrors>> {
        self.fresh(version);
        self.errors.clone().map_or(Lookup::Ask, Lookup::Known)
    }

    pub fn set_errors(&mut self, version: u32, answer: IdeResult<SyntaxErrors>) {
        if self.accepts(version) {
            self.errors = Some(answer.ok());
        }
    }

    /// The node at `pos` of the text at `version`, with its ancestors.
    pub fn node(&mut self, version: u32, pos: Pos) -> Lookup<Option<NodeAt>> {
        self.fresh(version);
        recall(&self.nodes, pos)
    }

    pub fn set_node(&mut self, version: u32, pos: Pos, answer: IdeResult<Option<NodeAt>>) {
        if self.accepts(version) {
            remember(&mut self.nodes, pos, answer.ok().flatten());
        }
    }
}

/// The service's answer to whether a language has a syntax tree: a grammar is bundled for it. A
/// failed question answers no.
pub fn has_syntax_tree(answer: &IdeResult<StructureSupport>) -> bool {
    answer.as_ref().is_ok_and(|support| support.tree)
}

/// The structure commands waiting on one editor, in the order they were pressed.
#[derive(Debug, Default)]
pub(super) struct Queue {
    pub(super) pending: VecDeque<commands::Command>,
}

/// The window-independent structure state: the answers per document and the commands waiting per
/// editor.
#[derive(Default)]
pub struct StructureService {
    docs: HashMap<DocId, StructureCache>,
    queues: HashMap<EntityId, Queue>,
}

impl Global for StructureService {}

impl StructureService {
    /// The answers of `doc`; the others are dropped once too many documents are known.
    pub fn cache(&mut self, doc: DocId) -> &mut StructureCache {
        if !self.docs.contains_key(&doc) && self.docs.len() >= MAX_CACHED_DOCUMENTS {
            self.docs.clear();
        }
        self.docs.entry(doc).or_default()
    }
}

/// No structure command is waiting for the backend in any editor.
pub fn is_idle(cx: &App) -> bool {
    cx.try_global::<StructureService>().is_none_or(|service| service.queues.is_empty())
}

// --- trees ------------------------------------------------------------------------------------

/// The tree the comment commands read from the chains of nodes around some positions: every node
/// of the chains once, in document order, under a node covering the document.
pub fn chain_tree(chains: &[NodeAt], rope: &Rope) -> SyntaxTree {
    let mut nodes: Vec<(u32, u32, usize, &str)> = Vec::new();
    for chain in chains {
        let outward = std::iter::once(&chain.node).chain(&chain.ancestors);
        let depth = chain.ancestors.len();
        for (index, node) in outward.enumerate() {
            let from = pos_to_offset(rope, node.range.start);
            let to = pos_to_offset(rope, node.range.end);
            let entry = (from, to, depth - index.min(depth), node.kind.as_str());
            if !nodes.iter().any(|known| (known.0, known.1, known.3) == (entry.0, entry.1, entry.3)) {
                nodes.push(entry);
            }
        }
    }
    // A parent comes before its children: by start, the longer first, the outer of equal spans first.
    nodes.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let mut tree = vec![TreeNode::new("Document", 0, crate::editor::buffer::len_utf16(rope))];
    tree.extend(nodes.into_iter().map(|(from, to, _, kind)| TreeNode::new(kind, from, to)));
    SyntaxTree::new(tree)
}

/// The tree of the escape sequences among `spans`: Toggle Case keeps their case.
pub fn escape_tree(spans: &[HighlightSpan], len: u32) -> SyntaxTree {
    let mut nodes = vec![TreeNode::new("Document", 0, len)];
    let escapes = spans.iter().filter(|span| span.kind == SyntaxHighlightKind::Escape && span.from < span.to);
    nodes.extend(escapes.map(|span| TreeNode::new("Escape", span.from, span.to)));
    SyntaxTree::new(nodes)
}

/// The tree of the text among `spans` that is not code: comments, and string, character,
/// template and regular expression literals, so that a bracket or a comma there does not count.
pub fn code_tree(spans: &[HighlightSpan], len: u32) -> SyntaxTree {
    use SyntaxHighlightKind as K;
    let literals = literal_ranges(spans.iter().filter_map(|span| {
        let kind = match span.kind {
            K::Comment | K::DocComment => LiteralKind::Comment,
            K::String | K::StringSpecial | K::Escape | K::Regexp => LiteralKind::String,
            _ => return None,
        };
        Some((span.from, span.to, kind))
    }));
    let mut nodes = vec![TreeNode::new("Document", 0, len)];
    nodes.extend(literals.iter().map(|literal| {
        let name = if literal.kind == LiteralKind::Comment { "Comment" } else { "String" };
        TreeNode::new(name, literal.from, literal.to)
    }));
    SyntaxTree::new(nodes)
}

// --- context info ---------------------------------------------------------------------------

/// Node kinds that are the body of a construct, as their names say.
static BLOCK_KIND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("Body|Block|ClassBody|Object|Array").expect("the pattern is valid"));

/// The node is a region of code with a header line: a body or block by its kind, or a node that
/// its own brackets delimit (`{ … }`, `[ … ]`, `( … )`), as the backend folds them.
fn is_block(state: &EditState, node: &SyntaxNode) -> bool {
    // `block` and `class_body` read `Block` and `ClassBody`.
    let pascal = pascal_name(&node.kind);
    let mut chars = pascal.chars();
    let name: String = chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
    if BLOCK_KIND.is_match(&name) {
        return true;
    }
    let from = pos_to_offset(&state.rope, node.range.start);
    let to = pos_to_offset(&state.rope, node.range.end);
    if to < from + 2 {
        return false;
    }
    let ends = (state.slice(from, from + 1), state.slice(to - 1, to));
    matches!((ends.0.as_str(), ends.1.as_str()), ("{", "}") | ("[", "]") | ("(", ")"))
}

/// View › Context Info from the syntax tree: the 0-based header lines of the blocks around line
/// `line` that start above it and end below it, from `at` (the node at the line's start with its
/// ancestors), outermost first, at most `max` of them (the innermost ones).
pub fn enclosing_headers(state: &EditState, line: u32, at: Option<&NodeAt>, max: u32) -> Vec<u32> {
    let Some(at) = at else { return Vec::new() };
    let mut headers: Vec<u32> = std::iter::once(&at.node)
        .chain(&at.ancestors)
        .filter(|node| is_block(state, node))
        .filter(|node| node.range.start.line < line && node.range.end.line > line)
        .map(|node| node.range.start.line)
        .collect();
    headers.sort_unstable();
    headers.dedup();
    let keep = headers.len().saturating_sub(max.max(1) as usize);
    headers.split_off(keep)
}

// --- the editor's side ----------------------------------------------------------------------

impl Editor {
    /// The highlights, when they are of the current text.
    fn current_highlights(&self) -> &[HighlightSpan] {
        if self.highlights_version == self.version() { &self.highlights } else { &[] }
    }

    /// The escape sequences of the current text ([`escape_tree`]); none while its highlights are
    /// on their way.
    pub fn escape_tree(&self) -> SyntaxTree {
        escape_tree(self.current_highlights(), crate::editor::buffer::len_utf16(self.rope()))
    }

    /// The text that is not code ([`code_tree`]); the unparsed tree while the highlights are on
    /// their way, which makes every position code.
    pub fn code_tree(&self) -> SyntaxTree {
        let spans = self.current_highlights();
        if spans.is_empty() {
            return SyntaxTree::default();
        }
        code_tree(spans, crate::editor::buffer::len_utf16(self.rope()))
    }

    /// The backend gave fold ranges for the text: Context Info reads its regions from them.
    pub fn has_backend_folds(&self) -> bool {
        !self.core.folds.backend().is_empty()
    }
}
