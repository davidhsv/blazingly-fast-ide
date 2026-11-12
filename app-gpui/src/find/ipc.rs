//! The backend's search types as the search models hold them.
//!
//! The crates that define them (`ide-search`, `ide-index`, `ide-lsp`) are not dependencies of this app, so
//! each type here mirrors one of theirs field for field, with paths as the `/`-separated strings the UI
//! compares. Lines are 0-based; columns and match indices are UTF-16 code units.

pub use crate::platform::project_roots::ProjectFileSet;
pub use crate::platform::search::SearchContext;
pub use ide_core::{Pos, Range, RequestId, SymbolKind, TextEdit};

/// Where a search looks. Mirrors `ide_search::SearchScope`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SearchScope {
    #[default]
    Project,
    Directory {
        path: String,
    },
    Files {
        paths: Vec<String>,
    },
    /// A named scope over the project's roots ("Project Production Files" …).
    FileSet {
        set: ProjectFileSet,
    },
    /// A custom scope: the project's files matching a scope pattern (`file:src//*.ts&&!file:*test*`).
    Pattern {
        pattern: String,
    },
}

/// A text search. Mirrors `ide_search::SearchQuery`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// File masks, e.g. `*.kt`; an entry may be a comma-separated list where `!mask` excludes. Empty: all files.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub scope: SearchScope,
    /// Stop after this many matches overall (0: the backend's own limit).
    pub max_matches: u32,
    /// Where in the code a match may lie.
    pub context: SearchContext,
}

/// The matches on one line. Mirrors `ide_search::LineMatch`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineMatch {
    pub line: u32,
    /// Line text, cut around the match for very long lines.
    pub text: String,
    /// Column ranges within `text`.
    pub ranges: Vec<(u32, u32)>,
    /// Column of `text` in the real line (non-zero when cut).
    pub text_offset: u32,
}

/// All matches in one file. Mirrors `ide_search::FileMatches`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMatches {
    pub path: String,
    pub rel_path: String,
    pub matches: Vec<LineMatch>,
}

/// How a search ended. Mirrors `ide_search::SearchSummary`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSummary {
    pub files: u32,
    pub matches: u32,
    pub limit_hit: bool,
    pub cancelled: bool,
    pub elapsed_ms: u32,
}

/// One file to replace in. Mirrors `ide_search::ReplaceTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceTarget {
    pub path: String,
    /// Start positions of the matches to replace, as the search reported them; `None` replaces every match.
    pub only: Option<Vec<Pos>>,
}

/// Progress of a Replace in Files run. Mirrors `ide_search::ReplaceEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceEvent {
    /// A file was changed (`replacements` > 0) or had nothing left to replace.
    File {
        path: String,
        replacements: u32,
        done: u32,
        total: u32,
    },
    Failed {
        path: String,
        message: String,
        done: u32,
        total: u32,
    },
    Done {
        summary: ReplaceSummary,
    },
}

/// How a Replace in Files run ended. Mirrors `ide_search::ReplaceSummary`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceSummary {
    /// Files actually changed.
    pub files: u32,
    pub replacements: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub elapsed_ms: u32,
}

/// A file found by name. Mirrors `ide_index::FileMatch`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMatch {
    pub path: String,
    pub rel_path: String,
    pub score: u32,
    /// Matched indices within `rel_path`.
    pub indices: Vec<u32>,
}

/// A symbol found in the index. Mirrors `ide_index::SymbolMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolMatch {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub path: String,
    pub rel_path: String,
    pub range: Range,
    pub score: u32,
    /// Matched indices within `name`.
    pub indices: Vec<u32>,
}

/// Which symbols the index is asked for. Mirrors `ide_index::SymbolFilter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolFilter {
    All,
    /// Classes, interfaces, structs, enums, traits and type aliases.
    Types,
}

/// A symbol a language server reports. Mirrors `ide_lsp::types::WorkspaceSymbol`, with its location's path
/// and range held directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub path: String,
    pub range: Range,
}

/// How many indexed files share an extension (lower case, without the dot). Mirrors
/// `ide_index::ExtensionCount`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionCount {
    pub extension: String,
    pub files: u32,
}
