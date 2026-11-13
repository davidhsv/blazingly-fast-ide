//! Find Usages without a language server (IDEA's dumb mode): a certain local's references in its
//! scope, otherwise the identifier occurrences of the name across the project (comments and
//! strings excluded), labelled as text occurrences.
//!
//! [`SyntaxSearch`] asks the syntax service step by step: each answer comes back through
//! [`SyntaxSearch::answer`], which says what to ask next or what was found.

use ide_core::{DocId, IpcPath, Location, Pos, Range};
use ide_syntax::{FileIdentifiers, IdentifierOccurrence, Locals, StructureSupport};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;

/// The note beside a local's references found by syntax.
pub const LOCAL_NOTE: &str = "Based on syntax (language server not ready)";
/// The note beside identifier occurrences found by syntax.
pub const TEXT_NOTE: &str = "Text occurrences (language server not ready)";
/// What [`TEXT_NOTE`] gets when the project search stopped at its limit.
pub const LIMIT_NOTE: &str = " · search limit reached, results may be incomplete";

static IDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}_$][\p{L}\p{N}_$]*$").expect("the identifier pattern is valid"));

/// Usages found by syntax, with what to say about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxUsages {
    pub locations: Vec<Location>,
    /// Where they came from and how far to trust them.
    pub note: String,
}

/// A question for the syntax service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxRequest {
    /// `syntax_support(language)`.
    Support { language: String },
    /// `syntax_locals(doc, pos)`.
    Locals { doc: DocId, pos: Pos },
    /// `syntax_identifiers(doc, name)`.
    Identifiers { doc: DocId, name: String },
    /// The project files that mention `name` (a whole-word, case-sensitive Find in Files search).
    FilesMentioning { name: String },
    /// `syntax_identifiers_in_files(paths, name)`.
    IdentifiersInFiles { paths: Vec<String>, name: String },
}

/// The syntax service's answer to the [`SyntaxRequest`] asked last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxAnswer {
    Support(StructureSupport),
    Locals(Locals),
    Identifiers(Vec<IdentifierOccurrence>),
    /// `limit_hit`: the search stopped at its match limit; later files were not looked at.
    FilesMentioning {
        paths: Vec<String>,
        limit_hit: bool,
    },
    IdentifiersInFiles(Vec<FileIdentifiers>),
}

/// What the search needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxStep {
    Ask(SyntaxRequest),
    Found(SyntaxUsages),
    /// The syntax service cannot answer for this file (no grammar, no identifier to search).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Support,
    Locals,
    ScopeIdentifiers { scope: Range },
    Files { name: String },
    Occurrences { limit_hit: bool },
    Done,
}

/// A syntax-based usages search from a caret.
#[derive(Debug, Clone)]
pub struct SyntaxSearch {
    doc: DocId,
    path: String,
    pos: Pos,
    word: Option<String>,
    stage: Stage,
}

fn location(path: &str, range: Range) -> Location {
    Location { path: IpcPath::from(PathBuf::from(path)), range, uri: None }
}

fn inside(range: Range, pos: Pos) -> bool {
    range.start <= pos && pos <= range.end
}

impl SyntaxSearch {
    /// Starts at `pos` of `path` (document `doc`, of `language`), `word` being the identifier
    /// there. Without a language nothing can be asked.
    pub fn start(doc: DocId, path: &str, pos: Pos, word: Option<&str>, language: Option<&str>) -> (Self, SyntaxStep) {
        let mut search = Self { doc, path: path.to_owned(), pos, word: word.map(str::to_owned), stage: Stage::Support };
        let step = match language {
            Some(language) => SyntaxStep::Ask(SyntaxRequest::Support { language: language.to_owned() }),
            None => search.finish(SyntaxStep::Unsupported),
        };
        (search, step)
    }

    fn finish(&mut self, step: SyntaxStep) -> SyntaxStep {
        self.stage = Stage::Done;
        step
    }

    fn found(&mut self, locations: Vec<Location>, note: String) -> SyntaxStep {
        self.finish(SyntaxStep::Found(SyntaxUsages { locations, note }))
    }

    /// Identifier occurrences of `name` across the project, if it is an identifier.
    fn search_project(&mut self, name: Option<String>) -> SyntaxStep {
        match name.filter(|name| IDENT.is_match(name)) {
            Some(name) => {
                self.stage = Stage::Files { name: name.clone() };
                SyntaxStep::Ask(SyntaxRequest::FilesMentioning { name })
            }
            None => self.finish(SyntaxStep::Unsupported),
        }
    }

    /// The answer to the request asked last.
    pub fn answer(&mut self, answer: SyntaxAnswer) -> SyntaxStep {
        match (std::mem::replace(&mut self.stage, Stage::Done), answer) {
            (Stage::Support, SyntaxAnswer::Support(support)) => {
                if !support.tree {
                    self.finish(SyntaxStep::Unsupported)
                } else if support.locals {
                    self.stage = Stage::Locals;
                    SyntaxStep::Ask(SyntaxRequest::Locals { doc: self.doc, pos: self.pos })
                } else {
                    let word = self.word.clone();
                    self.search_project(word)
                }
            }
            (Stage::Locals, SyntaxAnswer::Locals(locals)) => match locals {
                Locals::NoIdentifier => self.found(Vec::new(), LOCAL_NOTE.to_owned()),
                Locals::Local { symbol } if symbol.certain => {
                    let found = symbol.references.iter().map(|range| location(&self.path, *range)).collect();
                    self.found(found, LOCAL_NOTE.to_owned())
                }
                // Re-declared in the same scope: every plain occurrence in that scope, as text.
                Locals::Local { symbol } => {
                    self.stage = Stage::ScopeIdentifiers { scope: symbol.scope };
                    SyntaxStep::Ask(SyntaxRequest::Identifiers { doc: self.doc, name: symbol.name })
                }
                Locals::NotLocal { name, .. } => self.search_project(Some(name)),
            },
            (Stage::ScopeIdentifiers { scope }, SyntaxAnswer::Identifiers(occurrences)) => {
                let found = occurrences
                    .iter()
                    .filter(|occurrence| !occurrence.qualified && inside(scope, occurrence.range.start))
                    .map(|occurrence| location(&self.path, occurrence.range))
                    .collect();
                self.found(found, TEXT_NOTE.to_owned())
            }
            (Stage::Files { name }, SyntaxAnswer::FilesMentioning { mut paths, limit_hit }) => {
                if !paths.contains(&self.path) {
                    paths.push(self.path.clone());
                }
                self.stage = Stage::Occurrences { limit_hit };
                SyntaxStep::Ask(SyntaxRequest::IdentifiersInFiles { paths, name })
            }
            (Stage::Occurrences { limit_hit }, SyntaxAnswer::IdentifiersInFiles(files)) => {
                let found = files
                    .iter()
                    .flat_map(|file| {
                        file.occurrences.iter().map(|occurrence| Location {
                            path: file.path.clone(),
                            range: occurrence.range,
                            uri: None,
                        })
                    })
                    .collect();
                let note = if limit_hit { format!("{TEXT_NOTE}{LIMIT_NOTE}") } else { TEXT_NOTE.to_owned() };
                self.found(found, note)
            }
            // An answer to a question not asked: the search cannot go on.
            _ => self.finish(SyntaxStep::Unsupported),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(line: u32, col: u32, end: u32) -> Range {
        Range::new(Pos::new(line, col), Pos::new(line, end))
    }

    // Not from a spec: a name that is not an identifier, or an answer to a question not asked,
    // leaves the search to the server's error.
    #[test]
    fn gives_up_without_an_identifier_or_on_an_unexpected_answer() {
        let doc = DocId(3);
        let (mut search, step) = SyntaxSearch::start(doc, "/p/a.ts", Pos::new(1, 0), Some("+="), Some("typescript"));
        assert_eq!(step, SyntaxStep::Ask(SyntaxRequest::Support { language: "typescript".to_owned() }));
        let support = StructureSupport { tree: true, locals: false };
        assert_eq!(search.answer(SyntaxAnswer::Support(support)), SyntaxStep::Unsupported);

        let (mut search, _) = SyntaxSearch::start(doc, "/p/a.ts", Pos::new(1, 0), Some("x"), Some("typescript"));
        assert_eq!(search.answer(SyntaxAnswer::Identifiers(Vec::new())), SyntaxStep::Unsupported);
        let (_, step) = SyntaxSearch::start(doc, "/p/a.ts", Pos::new(1, 0), Some("x"), None);
        assert_eq!(step, SyntaxStep::Unsupported);
    }

    // Not from a spec: a local re-declared in its scope lists the plain occurrences in that scope.
    #[test]
    fn lists_an_uncertain_locals_plain_occurrences_in_its_scope_as_text() {
        let doc = DocId(3);
        let (mut search, _) = SyntaxSearch::start(doc, "/p/a.js", Pos::new(1, 0), Some("x"), Some("javascript"));
        let step = search.answer(SyntaxAnswer::Support(StructureSupport { tree: true, locals: true }));
        assert_eq!(step, SyntaxStep::Ask(SyntaxRequest::Locals { doc, pos: Pos::new(1, 0) }));
        let symbol = ide_syntax::LocalSymbol {
            name: "x".to_owned(),
            kind: ide_syntax::LocalKind::Var,
            definition: r(0, 4, 5),
            scope: r(0, 0, 20),
            references: Vec::new(),
            certain: false,
            conflicts: vec![r(0, 12, 13)],
        };
        let step = search.answer(SyntaxAnswer::Locals(Locals::Local { symbol }));
        assert_eq!(step, SyntaxStep::Ask(SyntaxRequest::Identifiers { doc, name: "x".to_owned() }));
        let occurrence =
            |range: Range, qualified: bool| IdentifierOccurrence { range, kind: "identifier".to_owned(), qualified };
        let step = search.answer(SyntaxAnswer::Identifiers(vec![
            occurrence(r(0, 4, 5), false),
            occurrence(r(0, 8, 9), true),
            occurrence(r(3, 0, 1), false),
        ]));
        let expected = SyntaxUsages { locations: vec![location("/p/a.js", r(0, 4, 5))], note: TEXT_NOTE.to_owned() };
        assert_eq!(step, SyntaxStep::Found(expected));
    }
}
