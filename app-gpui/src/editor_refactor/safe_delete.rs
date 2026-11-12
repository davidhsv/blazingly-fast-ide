//! Safe Delete (⌘⌦): resolves the declaration at the caret (the server's definition and document
//! symbols; the syntax tree's locals for local declarations only), asks "Delete 'x'?" (optionally
//! searching comments and strings too), searches its usages (the server's references, the
//! declaration's own body excluded), then deletes it with its attached doc comments and
//! annotations and the trailing blank line as one edit, or, while it is still used, asks like
//! IDEA: "Usages Detected" with View Usages / Delete Anyway / Cancel.
//!
//! Files (project view): references to the files' top-level symbols from other files, when the
//! language server can answer ([`FileUsagesFlow`]).
//!
//! Nothing here calls the backend: the flows answer requests tagged with a ticket, and an answer
//! whose ticket is not awaited is dropped.

use super::catalog::cannot_perform;
use super::preview::{Preview, PreviewEntry, PreviewSummary, occurrences_of};
use super::text::{basename, utf16_len, word_at};
use crate::editor::buffer;
use crate::editor_commands::{CommentTokens, Edit, Selection, caret, change};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::lsp::{is_cancelled, is_unavailable, unavailable_text};
use crate::editor_lsp::navigation::SHOW_USAGE_RESULTS_ACTION;
use crate::editor_lsp::positions::{compare_pos, offset_to_pos, range_contains, range_to_offsets};
use crate::platform::i18n::{I18n, Param};
use crate::project_view::operations::{UsageSearch, UsagesAnswer};
use ide_core::ipc_path::to_ipc;
use ide_core::{DocId, IdeError, Location, OutlineItem, Pos, Range, SymbolKind};
use ide_syntax::{IdentifierOccurrence, LocalKind, LocalSymbol, Locals, StructuralRanges, SyntaxNode};
use regex::Regex;
use ropey::Rope;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

// ---------------------------------------------------------------------------
// Deletion range

/// The comment syntax of C-like languages, used when the language's is not known.
pub fn c_like_comments() -> CommentTokens {
    CommentTokens::new(Some("//"), Some(("/*", "*/")))
}

/// Annotation, attribute and decorator lines belong to the declaration below them.
static ANNOTATION: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(@[A-Za-z_][0-9A-Za-z_.]*|#\[)"));
static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[;,]\s*"));
static BLOCK_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[}\])]|^end\b"));

/// The text to remove for a declaration spanning `from..to`: whole lines, plus the doc comments
/// and annotations directly above it and the trailing blank line (or the blank line above it when
/// nothing follows in its block). `None` when the declaration shares a line with other code
/// (`int a, b;`).
pub fn deletion_range(doc: &Rope, from: u32, to: u32, comments: &CommentTokens) -> Option<(u32, u32)> {
    let lines = buffer::line_count(doc);
    // 1-based line numbers, as the walk reads best with them.
    let line = |number: u32| -> (u32, u32, String) {
        let range = buffer::line_range(doc, number - 1);
        (range.start, range.end, buffer::slice(doc, range.start, range.end))
    };
    let number_at = |offset: u32| buffer::line_col(doc, offset).0 + 1;
    let first = number_at(from);
    let last = number_at(from.max(to));
    let (first_from, _, _) = line(first);
    let (_, last_to, _) = line(last);
    if !buffer::slice(doc, first_from, from).trim().is_empty() {
        return None;
    }
    let rest_text = buffer::slice(doc, to, last_to);
    let rest = rest_text.trim();
    let line_comment = comments.line.as_deref();
    let rest_ok = rest.is_empty()
        || rest == ";"
        || rest == ","
        || line_comment.is_some_and(|token| {
            rest.starts_with(token)
                || SEPARATOR.find(rest).is_some_and(|separator| rest[separator.end()..].starts_with(token))
        });
    if !rest_ok {
        return None;
    }

    // Attached comments and annotations above (no blank line in between).
    let mut start_line = first;
    let mut n = first - 1;
    while n >= 1 {
        let text = line(n).2;
        let text = text.trim();
        if text.is_empty() {
            break;
        }
        if let Some((open_token, close_token)) = &comments.block
            && text.ends_with(close_token.as_str())
        {
            let mut open = n;
            while open >= 1 && !line(open).2.contains(open_token.as_str()) {
                open -= 1;
            }
            if open < 1 || !line(open).2.trim().starts_with(open_token.as_str()) {
                break;
            }
            start_line = open;
            n = open - 1;
            continue;
        }
        if line_comment.is_some_and(|token| text.starts_with(token)) || ANNOTATION.is_match(text) {
            start_line = n;
            n -= 1;
            continue;
        }
        break;
    }

    let blank = |number: u32| number >= 1 && number <= lines && line(number).2.trim().is_empty();
    let mut start = line(start_line).0;
    let mut end = last_to;
    if last < lines {
        let next = last + 1;
        end = line(next).0;
        if blank(next) && next < lines {
            end = line(next + 1).0;
        } else if blank(next) {
            end = line(next).1;
        } else if blank(start_line - 1) && BLOCK_END.is_match(line(next).2.trim()) {
            start = line(start_line - 1).0;
        }
    } else if start_line > 1 {
        // The last line: take the line break before it (and a blank line above).
        let above = if blank(start_line - 1) && start_line > 2 { start_line - 2 } else { start_line - 1 };
        start = line(above).1;
    }
    Some((start, end))
}

// ---------------------------------------------------------------------------
// Declaration at the caret

/// What Safe Delete deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeDeleteTarget {
    pub name: String,
    /// "Function", "Class", "Variable"… for messages.
    pub kind: String,
    /// The declaring identifier (references are asked there).
    pub at: Pos,
    /// The declaration's extent in the document it was resolved against.
    pub range: Range,
    /// Resolved from the syntax tree without a language server: its usages are the local's
    /// references.
    pub local: Option<LocalSymbol>,
}

/// The label of an outline symbol kind in messages.
pub fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "File",
        SymbolKind::Module => "Module",
        SymbolKind::Namespace => "Namespace",
        SymbolKind::Package => "Package",
        SymbolKind::Class => "Class",
        SymbolKind::Interface => "Interface",
        SymbolKind::Struct => "Struct",
        SymbolKind::Enum => "Enum",
        SymbolKind::EnumMember => "Enum constant",
        SymbolKind::Trait => "Trait",
        SymbolKind::Function => "Function",
        SymbolKind::Method => "Method",
        SymbolKind::Constructor => "Constructor",
        SymbolKind::Field => "Field",
        SymbolKind::Property => "Property",
        SymbolKind::Variable => "Variable",
        SymbolKind::Constant => "Constant",
        SymbolKind::TypeParameter => "Type parameter",
        SymbolKind::TypeAlias => "Type alias",
        SymbolKind::Macro => "Macro",
        SymbolKind::Event => "Event",
        SymbolKind::Operator => "Operator",
        SymbolKind::Key => "Key",
        SymbolKind::Unknown => "Unknown",
    }
}

/// The label of a local's kind in messages (a plain definition reads as a variable).
pub fn local_kind_label(kind: LocalKind) -> &'static str {
    match kind {
        LocalKind::Var | LocalKind::Other => "Variable",
        LocalKind::Parameter => "Parameter",
        LocalKind::Function => "Function",
        LocalKind::Type => "Type",
        LocalKind::Import => "Import",
        LocalKind::Macro => "Macro",
        LocalKind::Label => "Label",
    }
}

/// The innermost outline item whose name range contains `pos`.
pub fn outline_item_at(items: &[OutlineItem], pos: Pos) -> Option<&OutlineItem> {
    for item in items {
        if !range_contains(item.range, pos) {
            continue;
        }
        if let Some(inner) = outline_item_at(&item.children, pos) {
            return Some(inner);
        }
        if range_contains(item.selection_range, pos) {
            return Some(item);
        }
    }
    None
}

fn undeletable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::File | SymbolKind::Package | SymbolKind::Namespace)
}

static DECLARATION_STATEMENT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"declaration|assignment|^let|local|^var|^const|import|^use|",
        r"expression_statement|property|short_var|definition$",
    ))
});
static COMPOUND_STATEMENT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(for|while|if|with|try|catch|switch|match|do|loop|foreach|enhanced_for|case)"));
static FUNCTION_STATEMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"function|method|^def|^fn|func|procedure"));
static TYPE_ARGUMENTS: LazyLock<Regex> = LazyLock::new(|| pattern(r"<[^<>]*>"));
static OTHER_NAMES: LazyLock<Regex> = LazyLock::new(|| pattern(r"[,(\[{]"));

/// The declaration statement of a local the syntax tree resolved, or why it cannot be deleted
/// on its own.
pub fn local_declaration(doc: &Rope, symbol: &LocalSymbol, statement: Option<&SyntaxNode>) -> Result<Range, String> {
    match symbol.kind {
        LocalKind::Parameter => {
            return Err("Deleting a parameter changes its call sites; use Change Signature".to_owned());
        }
        LocalKind::Label | LocalKind::Type | LocalKind::Macro => {
            return Err("Caret should be on a variable or function declaration".to_owned());
        }
        _ => {}
    }
    if !symbol.certain {
        return Err(format!("'{}' is declared more than once in this scope", symbol.name));
    }
    let Some(statement) = statement.filter(|statement| range_contains(statement.range, symbol.definition.start)) else {
        return Err("The declaration could not be determined".to_owned());
    };
    let kind = statement.kind.as_str();
    if symbol.kind == LocalKind::Function {
        return if FUNCTION_STATEMENT.is_match(kind) {
            Ok(statement.range)
        } else {
            Err("The declaration could not be determined".to_owned())
        };
    }
    if COMPOUND_STATEMENT.is_match(kind) || !DECLARATION_STATEMENT.is_match(kind) {
        return Err(format!("'{}' is not declared by a separate statement", symbol.name));
    }
    let offsets = range_to_offsets(doc, statement.range);
    let definition_end = range_to_offsets(doc, symbol.definition).end;
    // Up to the initializer the statement must declare only this name (no `a, b`, no destructuring).
    let after = buffer::slice(doc, definition_end, offsets.end);
    let cut = after.find(['=', ';']).unwrap_or(after.len());
    let mut declarator = buffer::slice(doc, offsets.start, definition_end) + &after[..cut];
    for _ in 0..5 {
        declarator = TYPE_ARGUMENTS.replace_all(&declarator, "").into_owned();
    }
    if OTHER_NAMES.is_match(&declarator) {
        return Err("The declaration declares other names too".to_owned());
    }
    Ok(statement.range)
}

// ---------------------------------------------------------------------------
// Comments and strings

fn is_identifier_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || c == '_' || c == '$'
}

/// Mentions of `name` in the file's comments and strings: whole-word text matches that are not
/// identifier tokens (`identifiers`, by start offset), outside `exclude`. UTF-16 offsets.
pub fn text_occurrences(text: &str, name: &str, identifiers: &[(u32, u32)], exclude: (u32, u32)) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    if name.is_empty() {
        return out;
    }
    let ids: HashSet<u32> = identifiers.iter().map(|(from, _)| *from).collect();
    let length = utf16_len(name);
    let mut search = 0;
    while let Some(found) = text[search..].find(name) {
        let byte = search + found;
        let end = byte + name.len();
        // The match must not run on into an identifier; a refused match retries one character on.
        if text[end..].chars().next().is_some_and(is_identifier_char) {
            search = byte + text[byte..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        search = end;
        if text[..byte].chars().next_back().is_some_and(is_identifier_char) {
            continue;
        }
        let from = utf16_len(&text[..byte]);
        if ids.contains(&from) || (from >= exclude.0 && from < exclude.1) {
            continue;
        }
        out.push((from, from + length));
    }
    out
}

// ---------------------------------------------------------------------------
// Usages Detected

/// The "Usages Detected" question and summary, in the UI language. `subject` is "Function 'foo'",
/// "File 'a.ts'" or, with `plural`, "3 files".
pub fn usages_message(i18n: &mut I18n, subject: &str, usages: &[Location], plural: bool) -> (String, String) {
    let n = usages.len();
    let files = usages.iter().map(|usage| &usage.path).collect::<HashSet<_>>().len();
    let place = if files > 1 { i18n.tx_built(&[" (in ", " files)"], &[Param::from(files)]) } else { String::new() };
    let values = [Param::from(subject), Param::from(n), Param::from(place)];
    let strings: [&str; 4] = match (plural, n == 1) {
        (true, true) => ["", " have ", " usage that is not safe to delete", "."],
        (true, false) => ["", " have ", " usages that are not safe to delete", "."],
        (false, true) => ["", " has ", " usage that is not safe to delete", "."],
        (false, false) => ["", " has ", " usages that are not safe to delete", "."],
    };
    let detail = i18n.tx_built(&strings, &values);
    let message = if n == 1 {
        i18n.tx_built(&["", " usage found"], &[Param::from(n)])
    } else {
        i18n.tx_built(&["", " usages found"], &[Param::from(n)])
    };
    (message, detail)
}

/// IDEA's "Usages Detected" question: View Usages (the primary button) or Delete Anyway;
/// dismissing it cancels. The labels are English source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsagesQuestion {
    pub title: &'static str,
    /// The summary and the detail, one line each, in the UI language.
    pub body: String,
    pub view_label: &'static str,
    pub delete_label: &'static str,
}

impl UsagesQuestion {
    pub fn new(i18n: &mut I18n, subject: &str, usages: &[Location], plural: bool) -> Self {
        let (message, detail) = usages_message(i18n, subject, usages, plural);
        Self {
            title: "Usages Detected",
            body: format!("{message}\n{detail}"),
            view_label: "View Usages",
            delete_label: "Delete Anyway",
        }
    }
}

/// The Safe Delete confirmation with its "Search in comments and strings" checkbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmQuestion {
    pub title: &'static str,
    /// In the UI language.
    pub message: String,
    pub ok_label: &'static str,
    pub checkbox: &'static str,
    pub checked: bool,
}

// ---------------------------------------------------------------------------
// The editor flow

/// What the caller asks the backend for a [`SafeDeleteFlow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeDeleteRequest {
    /// The definition of the symbol at `pos` (`lsp_goto`, definition).
    Definition { pos: Pos },
    /// The document's outline.
    Outline,
    /// The syntax tree's local at `pos`.
    Locals { pos: Pos },
    /// The structural ranges at `pos` (its statement).
    StructuralRanges { pos: Pos },
    /// References to the symbol at `pos`, the declaration excluded, project-wide.
    References { pos: Pos },
    /// The identifier tokens of the document named `name`.
    Identifiers { name: String },
}

/// A request and the ticket its answer carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeDeleteQuery {
    pub ticket: u64,
    pub request: SafeDeleteRequest,
}

/// The backend's answer to a [`SafeDeleteRequest`] of the same name.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one per server answer, consumed at once")]
pub enum SafeDeleteAnswer {
    Definition(Result<Vec<Location>, IdeError>),
    Outline(Result<Vec<OutlineItem>, IdeError>),
    Locals(Result<Locals, IdeError>),
    StructuralRanges(Result<StructuralRanges, IdeError>),
    References(Result<Vec<Location>, IdeError>),
    Identifiers(Result<Vec<IdentifierOccurrence>, IdeError>),
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeDeleteStep {
    /// Nothing: the flow ended (cancelled, or the document changed), or it waits for more answers.
    Nothing,
    /// Send these requests, answering each with [`SafeDeleteFlow::answered`].
    Requests(Vec<SafeDeleteQuery>),
    /// Show this hint at the caret (English source text); the flow ended.
    Hint { text: String, tone: Tone },
    /// Ask; answer with [`SafeDeleteFlow::confirmed`] and remember the checkbox for next time.
    Confirm(ConfirmQuestion),
    /// Give the keyboard focus back to the editor; the flow ended.
    Focus,
    /// Ask; answer with [`SafeDeleteFlow::usages_answered`].
    UsagesDetected(UsagesQuestion),
    /// Run [`SHOW_USAGE_RESULTS_ACTION`] with these usages; the flow ended.
    ViewUsages { action: &'static str, symbol: String, locations: Vec<Location> },
    /// Show the mentions in comments and strings; its Delete button calls
    /// [`SafeDeleteFlow::preview_confirmed`].
    Preview(Preview),
    /// Apply this edit as one undo step and scroll it into view; the flow ended.
    Delete(Edit),
}

/// What the flow knows of the document's language and the user's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeDeleteSetup {
    pub path: String,
    /// The display name of the language, for the "not available" reason.
    pub language_name: String,
    /// The language's comment syntax ([`c_like_comments`] when unknown).
    pub comments: CommentTokens,
    /// "Search in comments and strings" as last confirmed (on at first, like IDEA).
    pub search_in_comments: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Definition,
    Outline,
    Locals,
    Ranges(LocalSymbol),
    Confirm,
    Search { usages: Option<Vec<Location>>, hits: Option<Vec<(u32, u32)>> },
    Usages(Vec<Location>),
    Preview,
    Done,
}

/// One Safe Delete in an editor, from the caret to the deletion. One runs per editor at a time.
#[derive(Debug, Clone)]
pub struct SafeDeleteFlow {
    setup: SafeDeleteSetup,
    /// The document the declaration was resolved against.
    doc: Rope,
    /// The document version and selection when the flow started: an edit since cancels it.
    version: u64,
    selection: Selection,
    name: String,
    /// Where the symbol is declared: the caret until the definition is known.
    definition: Pos,
    lsp: bool,
    outline_item: Option<OutlineItem>,
    target: Option<SafeDeleteTarget>,
    phase: Phase,
    tickets: Vec<u64>,
    next_ticket: u64,
}

impl SafeDeleteFlow {
    /// Starts at the main selection's head of `doc` (document version `version`), once the
    /// editor's edits reached the backend.
    pub fn start(
        doc: &Rope,
        version: u64,
        selection: Selection,
        read_only: bool,
        setup: SafeDeleteSetup,
    ) -> (Self, SafeDeleteStep) {
        let head = selection.1;
        let pos = offset_to_pos(doc, head);
        let mut flow = Self {
            setup,
            doc: doc.clone(),
            version,
            selection,
            name: String::new(),
            definition: pos,
            lsp: true,
            outline_item: None,
            target: None,
            phase: Phase::Done,
            tickets: Vec::new(),
            next_ticket: 0,
        };
        if read_only {
            return (flow, SafeDeleteStep::Hint { text: "The file is read-only".to_owned(), tone: Tone::Info });
        }
        let Some((from, to)) = word_at(doc, head) else {
            return (flow, flow_refused("Caret should be on a symbol"));
        };
        flow.name = buffer::slice(doc, from, to);
        flow.phase = Phase::Definition;
        let step = flow.ask(vec![SafeDeleteRequest::Definition { pos }]);
        (flow, step)
    }

    /// The target once resolved.
    pub fn target(&self) -> Option<&SafeDeleteTarget> {
        self.target.as_ref()
    }

    fn ask(&mut self, requests: Vec<SafeDeleteRequest>) -> SafeDeleteStep {
        let queries = requests
            .into_iter()
            .map(|request| {
                self.next_ticket += 1;
                self.tickets.push(self.next_ticket);
                SafeDeleteQuery { ticket: self.next_ticket, request }
            })
            .collect();
        SafeDeleteStep::Requests(queries)
    }

    fn end(&mut self, step: SafeDeleteStep) -> SafeDeleteStep {
        self.phase = Phase::Done;
        self.tickets.clear();
        step
    }

    fn refuse(&mut self, reason: &str) -> SafeDeleteStep {
        self.end(flow_refused(reason))
    }

    /// The backend answered the request of `ticket`; `version` is the document's version now.
    pub fn answered(&mut self, ticket: u64, answer: SafeDeleteAnswer, version: u64, i18n: &mut I18n) -> SafeDeleteStep {
        let Some(index) = self.tickets.iter().position(|t| *t == ticket) else { return SafeDeleteStep::Nothing };
        self.tickets.remove(index);
        match (std::mem::replace(&mut self.phase, Phase::Done), answer) {
            (Phase::Definition, SafeDeleteAnswer::Definition(result)) => match result {
                Err(error) if is_cancelled(&error) => self.end(SafeDeleteStep::Nothing),
                Err(_) => {
                    self.lsp = false;
                    self.ask_outline()
                }
                Ok(locations) => {
                    let here = locations.iter().find(|location| to_ipc(&location.path) == self.setup.path);
                    if let Some(here) = here {
                        self.definition = here.range.start;
                    } else if let Some(first) = locations.first() {
                        let reason = format!(
                            "'{}' is declared in {}; Safe Delete it there",
                            self.name,
                            basename(&to_ipc(&first.path))
                        );
                        return self.refuse(&reason);
                    }
                    self.ask_outline()
                }
            },
            (Phase::Outline, SafeDeleteAnswer::Outline(result)) => {
                let outline = result.unwrap_or_default();
                let item = outline_item_at(&outline, self.definition).cloned();
                if let Some(item) = item.as_ref().filter(|item| !undeletable(item.kind) && self.lsp) {
                    let target = SafeDeleteTarget {
                        name: item.name.clone(),
                        kind: symbol_kind_label(item.kind).to_owned(),
                        at: item.selection_range.start,
                        range: item.range,
                        local: None,
                    };
                    return self.resolved(target, i18n);
                }
                self.outline_item = item;
                self.phase = Phase::Locals;
                self.ask(vec![SafeDeleteRequest::Locals { pos: self.definition }])
            }
            (Phase::Locals, SafeDeleteAnswer::Locals(result)) => match result {
                Err(error) if is_cancelled(&error) => self.end(SafeDeleteStep::Nothing),
                Ok(Locals::Local { symbol }) => {
                    let pos = symbol.definition.start;
                    self.phase = Phase::Ranges(symbol);
                    self.ask(vec![SafeDeleteRequest::StructuralRanges { pos }])
                }
                _ if !self.lsp => {
                    let reason = format!(
                        "The language server for {} is not available; \
                         only local declarations can be deleted safely without it",
                        self.setup.language_name
                    );
                    self.refuse(&reason)
                }
                _ if self.outline_item.is_some() => {
                    let reason = format!("'{}' can't be deleted", self.name);
                    self.refuse(&reason)
                }
                _ => self.refuse("Caret should be on the name of a declaration"),
            },
            (Phase::Ranges(symbol), SafeDeleteAnswer::StructuralRanges(result)) => {
                let statement = result.ok().and_then(|ranges| ranges.statement);
                match local_declaration(&self.doc, &symbol, statement.as_ref()) {
                    Err(reason) => self.refuse(&reason),
                    Ok(range) => {
                        let kind = local_kind_label(symbol.kind).to_owned();
                        let target = SafeDeleteTarget {
                            name: symbol.name.clone(),
                            kind,
                            at: symbol.definition.start,
                            range,
                            local: (!self.lsp).then_some(symbol),
                        };
                        self.resolved(target, i18n)
                    }
                }
            }
            (Phase::Search { hits, .. }, SafeDeleteAnswer::References(result)) => match result {
                Ok(locations) => {
                    let usages = self.outside_declaration(locations);
                    self.searched(Some(usages), hits, version, i18n)
                }
                Err(error) => self.failed(&error),
            },
            (Phase::Search { usages, .. }, SafeDeleteAnswer::Identifiers(result)) => match result {
                Err(error) if is_cancelled(&error) => self.end(SafeDeleteStep::Nothing),
                result => {
                    let hits = self.comment_and_string_hits(result.unwrap_or_default());
                    self.searched(usages, Some(hits), version, i18n)
                }
            },
            (phase, _) => {
                // An answer of another kind than awaited ends nothing: the flow waits on.
                self.phase = phase;
                SafeDeleteStep::Nothing
            }
        }
    }

    fn ask_outline(&mut self) -> SafeDeleteStep {
        self.phase = Phase::Outline;
        self.ask(vec![SafeDeleteRequest::Outline])
    }

    fn failed(&mut self, error: &IdeError) -> SafeDeleteStep {
        if is_cancelled(error) {
            return self.end(SafeDeleteStep::Nothing);
        }
        let reason = if is_unavailable(error) {
            unavailable_text(error, "No language server is available")
        } else {
            error.message.as_str()
        };
        let text = cannot_perform(reason);
        self.end(SafeDeleteStep::Hint { text, tone: Tone::Info })
    }

    fn deletion(&self, target: &SafeDeleteTarget) -> Option<(u32, u32)> {
        let offsets = range_to_offsets(&self.doc, target.range);
        deletion_range(&self.doc, offsets.start, offsets.end, &self.setup.comments)
    }

    /// The target is known: a declaration that cannot be removed by lines is refused before
    /// asking; IDEA always confirms first and searches usages after OK.
    fn resolved(&mut self, target: SafeDeleteTarget, i18n: &mut I18n) -> SafeDeleteStep {
        if self.deletion(&target).is_none() {
            let reason = format!("'{}' shares its line with other code", target.name);
            return self.refuse(&reason);
        }
        let message = i18n.tx_built(
            &["Delete ", " '", "'?"],
            &[Param::from(target.kind.as_str()), Param::from(target.name.as_str())],
        );
        self.target = Some(target);
        self.phase = Phase::Confirm;
        SafeDeleteStep::Confirm(ConfirmQuestion {
            title: "Safe Delete",
            message,
            ok_label: "OK",
            checkbox: "Search in comments and strings",
            checked: self.setup.search_in_comments,
        })
    }

    /// The confirmation's answer: `None` when cancelled, else the checkbox. `version` and
    /// `selection` are the editor's now: an edit or a caret move meanwhile ends the flow.
    pub fn confirmed(
        &mut self,
        answer: Option<bool>,
        version: u64,
        selection: Selection,
        i18n: &mut I18n,
    ) -> SafeDeleteStep {
        if self.phase != Phase::Confirm {
            return SafeDeleteStep::Nothing;
        }
        let Some(search_in_comments) = answer else { return self.end(SafeDeleteStep::Focus) };
        self.setup.search_in_comments = search_in_comments;
        if version != self.version || selection != self.selection {
            return self.end(SafeDeleteStep::Nothing);
        }
        let Some(target) = self.target.clone() else { return self.end(SafeDeleteStep::Nothing) };
        let mut requests = Vec::new();
        let usages = match &target.local {
            Some(local) => {
                let ranges = local.references.iter().map(|range| Location {
                    path: self.setup.path.as_str().into(),
                    range: *range,
                    uri: None,
                });
                Some(self.outside_declaration(ranges.collect()))
            }
            None => {
                requests.push(SafeDeleteRequest::References { pos: target.at });
                None
            }
        };
        let hits = if search_in_comments {
            requests.push(SafeDeleteRequest::Identifiers { name: target.name.clone() });
            None
        } else {
            Some(Vec::new())
        };
        if requests.is_empty() {
            return self.searched(usages, hits, version, i18n);
        }
        self.phase = Phase::Search { usages, hits };
        self.ask(requests)
    }

    /// Usages that are not inside the declaration itself (recursive calls).
    fn outside_declaration(&self, locations: Vec<Location>) -> Vec<Location> {
        let Some(target) = &self.target else { return locations };
        locations
            .into_iter()
            .filter(|location| {
                to_ipc(&location.path) != self.setup.path
                    || compare_pos(location.range.start, target.range.start) == Ordering::Less
                    || compare_pos(location.range.start, target.range.end) == Ordering::Greater
            })
            .collect()
    }

    fn comment_and_string_hits(&self, identifiers: Vec<IdentifierOccurrence>) -> Vec<(u32, u32)> {
        let Some(target) = &self.target else { return Vec::new() };
        let ids: Vec<(u32, u32)> = identifiers
            .iter()
            .map(|occurrence| {
                let offsets = range_to_offsets(&self.doc, occurrence.range);
                (offsets.start, offsets.end)
            })
            .collect();
        let exclude = range_to_offsets(&self.doc, target.range);
        text_occurrences(&self.doc.to_string(), &target.name, &ids, (exclude.start, exclude.end))
    }

    fn searched(
        &mut self,
        usages: Option<Vec<Location>>,
        hits: Option<Vec<(u32, u32)>>,
        version: u64,
        i18n: &mut I18n,
    ) -> SafeDeleteStep {
        let (usages, hits) = match (usages, hits) {
            (Some(usages), Some(hits)) => (usages, hits),
            (usages, hits) => {
                // One of the searches is still running.
                self.phase = Phase::Search { usages, hits };
                return SafeDeleteStep::Nothing;
            }
        };
        if version != self.version {
            return self.end(SafeDeleteStep::Nothing);
        }
        let Some(target) = self.target.clone() else { return self.end(SafeDeleteStep::Nothing) };
        if !usages.is_empty() {
            let subject = i18n
                .tx_built(&["", " '", "'"], &[Param::from(target.kind.as_str()), Param::from(target.name.as_str())]);
            let question = UsagesQuestion::new(i18n, &subject, &usages, false);
            self.phase = Phase::Usages(usages);
            return SafeDeleteStep::UsagesDetected(question);
        }
        if !hits.is_empty() {
            let title = i18n.tx_built(
                &["Safe Delete ", " '", "'"],
                &[Param::from(target.kind.as_str()), Param::from(target.name.as_str())],
            );
            let occurrences = occurrences_of(&self.doc, &hits);
            self.phase = Phase::Preview;
            return SafeDeleteStep::Preview(Preview {
                title,
                summary: PreviewSummary {
                    files: 1,
                    occurrences: occurrences.len(),
                    has_file_operations: false,
                    entries: vec![PreviewEntry { path: self.setup.path.clone(), edits: occurrences.len() }],
                },
                warnings: vec![format!(
                    "'{}' is mentioned in comments or strings. The declaration will be deleted; these mentions stay.",
                    target.name
                )],
                occurrences,
                confirm_label: "Delete",
            });
        }
        let step = self.apply(version);
        self.end(step)
    }

    /// The answer to "Usages Detected".
    pub fn usages_answered(&mut self, answer: UsagesAnswer, version: u64) -> SafeDeleteStep {
        let Phase::Usages(usages) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return SafeDeleteStep::Nothing;
        };
        let step = match answer {
            UsagesAnswer::View => SafeDeleteStep::ViewUsages {
                action: SHOW_USAGE_RESULTS_ACTION,
                symbol: self.target.as_ref().map(|target| target.name.clone()).unwrap_or_default(),
                locations: usages,
            },
            UsagesAnswer::Delete => self.apply(version),
            UsagesAnswer::Cancel => SafeDeleteStep::Focus,
        };
        self.end(step)
    }

    /// The preview's Delete button.
    pub fn preview_confirmed(&mut self, version: u64) -> SafeDeleteStep {
        if self.phase != Phase::Preview {
            return SafeDeleteStep::Nothing;
        }
        let step = self.apply(version);
        self.end(step)
    }

    fn apply(&self, version: u64) -> SafeDeleteStep {
        if version != self.version {
            return SafeDeleteStep::Hint {
                text: "The file changed; Safe Delete cancelled".to_owned(),
                tone: Tone::Warning,
            };
        }
        let Some(target) = &self.target else { return SafeDeleteStep::Nothing };
        match self.deletion(target) {
            None => flow_refused(&format!("'{}' shares its line with other code", target.name)),
            Some((from, to)) => SafeDeleteStep::Delete(Edit::new(vec![change(from, to, "")], vec![caret(from)], 0)),
        }
    }
}

fn flow_refused(reason: &str) -> SafeDeleteStep {
    SafeDeleteStep::Hint { text: cannot_perform(reason), tone: Tone::Info }
}

// ---------------------------------------------------------------------------
// Files (project view)

/// At most this many symbols per file are searched.
pub const MAX_SYMBOLS: usize = 60;

/// The symbols a file declares at its top level. Packages and namespaces are shared with other
/// files (IDEA searches a Java file's classes, not its package), so their members stand in for
/// them.
pub fn declared_symbols(items: &[OutlineItem]) -> Vec<&OutlineItem> {
    items
        .iter()
        .flat_map(|item| match item.kind {
            SymbolKind::Package | SymbolKind::Namespace => declared_symbols(&item.children),
            _ => vec![item],
        })
        .collect()
}

/// What a [`FileUsagesFlow`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileUsagesAction {
    /// The document of `path`: an open editor's, or one opened for the search. Answer with
    /// [`FileUsagesFlow::document`].
    Document { ticket: u64, path: String },
    /// The document's outline; answer with [`FileUsagesFlow::outline`].
    Outline { ticket: u64, doc: DocId },
    /// References to the symbol at `pos`, in the server's own scope (a project search per symbol
    /// would be too slow); answer with [`FileUsagesFlow::references`].
    References { ticket: u64, doc: DocId, pos: Pos },
    /// Close the document opened for the search.
    CloseDocument(DocId),
    /// The search is over.
    Done(UsageSearch),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Document,
    Outline,
    References,
    Nothing,
}

/// The references from outside `paths` to the symbols the files declare ([`declared_symbols`]).
/// Unsupported when no reference search could run for any of them.
#[derive(Debug, Clone)]
pub struct FileUsagesFlow {
    paths: Vec<String>,
    next_path: usize,
    /// The document of the file searched now, and whether the search opened it.
    doc: Option<(DocId, bool)>,
    /// The positions of the file's symbols still to search.
    symbols: Vec<Pos>,
    found: Vec<Location>,
    keys: HashSet<(String, u32, u32)>,
    searched: bool,
    reason: String,
    wait: Wait,
    ticket: u64,
}

impl FileUsagesFlow {
    pub fn start(paths: Vec<String>) -> (Self, Vec<FileUsagesAction>) {
        let mut flow = Self {
            paths,
            next_path: 0,
            doc: None,
            symbols: Vec::new(),
            found: Vec::new(),
            keys: HashSet::new(),
            searched: false,
            reason: "No language server supports usage search for these files".to_owned(),
            wait: Wait::Nothing,
            ticket: 0,
        };
        let actions = flow.next_file(Vec::new());
        (flow, actions)
    }

    fn take(&mut self, ticket: u64, wait: Wait) -> bool {
        let awaited = self.wait == wait && self.ticket == ticket;
        if awaited {
            self.wait = Wait::Nothing;
        }
        awaited
    }

    fn next_file(&mut self, mut actions: Vec<FileUsagesAction>) -> Vec<FileUsagesAction> {
        if let Some((doc, true)) = self.doc.take() {
            actions.push(FileUsagesAction::CloseDocument(doc));
        }
        self.symbols.clear();
        let Some(path) = self.paths.get(self.next_path).cloned() else {
            let outcome = if self.searched {
                UsageSearch::Found(std::mem::take(&mut self.found))
            } else {
                UsageSearch::Unsupported(self.reason.clone())
            };
            actions.push(FileUsagesAction::Done(outcome));
            return actions;
        };
        self.next_path += 1;
        self.ticket += 1;
        self.wait = Wait::Document;
        actions.push(FileUsagesAction::Document { ticket: self.ticket, path });
        actions
    }

    fn failed(&mut self, error: &IdeError) -> Vec<FileUsagesAction> {
        if is_cancelled(error) {
            return self.cancel();
        }
        self.reason = error.message.clone();
        self.next_file(Vec::new())
    }

    /// The document of the file asked for: its id and whether it was opened for the search (it
    /// is closed afterwards), or why it could not be opened.
    pub fn document(&mut self, ticket: u64, result: Result<(DocId, bool), IdeError>) -> Vec<FileUsagesAction> {
        if !self.take(ticket, Wait::Document) {
            return Vec::new();
        }
        match result {
            Err(error) => self.failed(&error),
            Ok((doc, opened)) => {
                self.doc = Some((doc, opened));
                self.ticket += 1;
                self.wait = Wait::Outline;
                vec![FileUsagesAction::Outline { ticket: self.ticket, doc }]
            }
        }
    }

    pub fn outline(&mut self, ticket: u64, result: Result<Vec<OutlineItem>, IdeError>) -> Vec<FileUsagesAction> {
        if !self.take(ticket, Wait::Outline) {
            return Vec::new();
        }
        match result {
            Err(error) => self.failed(&error),
            Ok(items) => {
                let mut symbols: Vec<Pos> =
                    declared_symbols(&items).iter().take(MAX_SYMBOLS).map(|item| item.selection_range.start).collect();
                symbols.reverse();
                self.symbols = symbols;
                self.next_symbol()
            }
        }
    }

    fn next_symbol(&mut self) -> Vec<FileUsagesAction> {
        match (self.symbols.pop(), self.doc) {
            (Some(pos), Some((doc, _))) => {
                self.ticket += 1;
                self.wait = Wait::References;
                vec![FileUsagesAction::References { ticket: self.ticket, doc, pos }]
            }
            _ => self.next_file(Vec::new()),
        }
    }

    pub fn references(&mut self, ticket: u64, result: Result<Vec<Location>, IdeError>) -> Vec<FileUsagesAction> {
        if !self.take(ticket, Wait::References) {
            return Vec::new();
        }
        match result {
            Err(error) if is_cancelled(&error) => self.cancel(),
            Err(error) if is_unavailable(&error) => {
                // No server for this file: its other symbols cannot be searched either.
                self.reason = error.message;
                self.next_file(Vec::new())
            }
            Err(_) => self.next_symbol(),
            Ok(locations) => {
                self.searched = true;
                for location in locations {
                    let path = to_ipc(&location.path);
                    if self.paths.contains(&path) {
                        continue;
                    }
                    if self.keys.insert((path, location.range.start.line, location.range.start.col)) {
                        self.found.push(location);
                    }
                }
                self.next_symbol()
            }
        }
    }

    /// Stops the search: closes the document it opened and answers cancelled. Later answers are
    /// dropped.
    pub fn cancel(&mut self) -> Vec<FileUsagesAction> {
        self.wait = Wait::Nothing;
        self.next_path = self.paths.len();
        let mut actions = Vec::new();
        if let Some((doc, true)) = self.doc.take() {
            actions.push(FileUsagesAction::CloseDocument(doc));
        }
        actions.push(FileUsagesAction::Done(UsageSearch::Failed("cancelled".to_owned())));
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{at_line, cancelled, item, location, offset_of, pos_of, range_of, unavailable};
    use super::*;
    use crate::editor_commands::apply_changes;

    fn cut(text: &str, needle: &str, comments: &CommentTokens) -> Option<String> {
        let from = offset_of(text, needle);
        let rope = Rope::from_str(text);
        let (start, end) = deletion_range(&rope, from, from + utf16_len(needle), comments)?;
        Some(apply_changes(&rope, &[change(start, end, "")]).to_string())
    }

    fn c_cut(text: &str, needle: &str) -> Option<String> {
        cut(text, needle, &c_like_comments())
    }

    #[test]
    fn removes_whole_lines_plus_the_trailing_blank_line() {
        assert_eq!(
            c_cut("fn a() {}\n\nfn unused() {}\n\nfn b() {}\n", "fn unused() {}").as_deref(),
            Some("fn a() {}\n\nfn b() {}\n")
        );
    }

    #[test]
    fn takes_the_doc_comment_and_annotations_directly_above() {
        let text = "fn a() {}\n\n/**\n * Docs.\n */\n#[inline]\nfn unused() {}\n\nfn b() {}\n";
        assert_eq!(c_cut(text, "fn unused() {}").as_deref(), Some("fn a() {}\n\nfn b() {}\n"));
        let java = "class A {\n  // keep\n\n  /// doc\n  @Deprecated\n  void unused() {}\n\n  void b() {}\n}\n";
        assert_eq!(c_cut(java, "void unused() {}").as_deref(), Some("class A {\n  // keep\n\n  void b() {}\n}\n"));
    }

    #[test]
    fn takes_the_blank_line_above_when_the_declaration_ends_its_block() {
        assert_eq!(
            c_cut("fn f() {\n    a();\n\n    let x = 1;\n}\n", "let x = 1;").as_deref(),
            Some("fn f() {\n    a();\n}\n")
        );
    }

    #[test]
    fn handles_the_last_line_of_the_document() {
        assert_eq!(c_cut("fn a() {}\n\nfn unused() {}", "fn unused() {}").as_deref(), Some("fn a() {}"));
        assert_eq!(c_cut("fn a() {}\nfn unused() {}", "fn unused() {}").as_deref(), Some("fn a() {}"));
    }

    #[test]
    fn uses_the_language_s_comment_syntax_python_comments_and_decorators() {
        let py = "import x\n\n# helper\n@cache\ndef unused():\n    pass\n\ndef b():\n    pass\n";
        let comments = CommentTokens::new(Some("#"), None);
        assert_eq!(cut(py, "def unused():\n    pass", &comments).as_deref(), Some("import x\n\ndef b():\n    pass\n"));
    }

    #[test]
    fn accepts_a_trailing_separator_or_comment_but_refuses_shared_lines() {
        assert_eq!(
            c_cut("let a = 1;\nlet b = 2; // note\nlet c = 3;\n", "let b = 2").as_deref(),
            Some("let a = 1;\nlet c = 3;\n")
        );
        assert_eq!(
            c_cut("struct S {\n    a: i32,\n    b: i32,\n}\n", "a: i32").as_deref(),
            Some("struct S {\n    b: i32,\n}\n")
        );
        assert_eq!(c_cut("let a = 1; let b = 2;\n", "let b = 2;"), None);
        assert_eq!(c_cut("f(a, b);\n", "a"), None);
    }

    #[test]
    fn finds_the_innermost_item_whose_name_contains_the_position() {
        let text = "impl S {\n    fn m() {}\n}\n";
        let m = item(text, "m", "fn m() {}", SymbolKind::Method);
        let s = OutlineItem {
            children: vec![m.clone()],
            ..item(text, "S", "impl S {\n    fn m() {}\n}", SymbolKind::Struct)
        };
        let items = [s.clone()];
        assert_eq!(outline_item_at(&items, pos_of(text, offset_of(text, "m()"))), Some(&m));
        assert_eq!(outline_item_at(&items, pos_of(text, offset_of(text, "S"))), Some(&s));
        assert_eq!(outline_item_at(&items, pos_of(text, offset_of(text, "{}"))), None);
    }

    const LOCALS_TEXT: &str =
        "fn f(p: i32) {\n    let x = 1;\n    let (a, b) = t;\n    let m: HashMap<K, V> = m();\n    for i in v {}\n}\n";

    fn sym(name: &str, kind: LocalKind, after: &str) -> LocalSymbol {
        let text = LOCALS_TEXT;
        let after_byte = text.find(after).expect("the anchor is in the text");
        // The first whole-word `name` from the anchor on.
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let byte = text[after_byte..]
            .match_indices(name)
            .map(|(at, _)| after_byte + at)
            .find(|&at| {
                !text[..at].chars().next_back().is_some_and(word)
                    && !text[at + name.len()..].chars().next().is_some_and(word)
            })
            .expect("the name is in the text");
        let at = utf16_len(&text[..byte]);
        LocalSymbol {
            name: name.to_owned(),
            kind,
            definition: range_of(text, at, at + utf16_len(name)),
            scope: range_of(text, 0, utf16_len(text)),
            references: Vec::new(),
            certain: true,
            conflicts: Vec::new(),
        }
    }

    fn node(kind: &str, text: &str, needle: &str) -> SyntaxNode {
        let at = offset_of(text, needle);
        SyntaxNode {
            kind: kind.to_owned(),
            range: range_of(text, at, at + utf16_len(needle)),
            field: None,
            named: true,
            error: false,
            missing: false,
            has_error: false,
        }
    }

    #[test]
    fn returns_the_declaring_statement_of_a_single_local() {
        let doc = Rope::from_str(LOCALS_TEXT);
        let x = node("let_declaration", LOCALS_TEXT, "let x = 1;");
        assert_eq!(local_declaration(&doc, &sym("x", LocalKind::Var, "{"), Some(&x)), Ok(x.range));
        let m = node("let_declaration", LOCALS_TEXT, "let m: HashMap<K, V> = m();");
        assert!(local_declaration(&doc, &sym("m", LocalKind::Var, "{"), Some(&m)).is_ok());
    }

    #[test]
    fn refuses_parameters_destructuring_loop_variables_and_ambiguous_locals() {
        let doc = Rope::from_str(LOCALS_TEXT);
        let reason = |symbol: LocalSymbol, statement: Option<SyntaxNode>| {
            local_declaration(&doc, &symbol, statement.as_ref()).expect_err("refused")
        };
        assert!(reason(sym("p", LocalKind::Parameter, "f"), None).contains("Change Signature"));
        let pair = node("let_declaration", LOCALS_TEXT, "let (a, b) = t;");
        assert!(reason(sym("b", LocalKind::Var, "{"), Some(pair)).contains("other names"));
        let each = node("for_expression", LOCALS_TEXT, "for i in v {}");
        assert!(reason(sym("i", LocalKind::Var, "{"), Some(each)).contains("separate statement"));
        let x = node("let_declaration", LOCALS_TEXT, "let x = 1;");
        let ambiguous = LocalSymbol { certain: false, ..sym("x", LocalKind::Var, "{") };
        assert!(reason(ambiguous, Some(x)).contains("more than once"));
    }

    #[test]
    fn checks_unicode_identifier_boundaries_without_shifting_utf_16_offsets() {
        let text = "𐐀foo αfoo $foo _foo foo 𐐀 foo foo𐐀";
        assert_eq!(text_occurrences(text, "foo", &[], (u32::MAX, u32::MAX)), [(21, 24), (28, 31)]);
    }

    #[test]
    fn finds_whole_word_mentions_that_are_not_identifier_tokens_outside_the_declaration() {
        let text = "foo(); // foo and food\nlet s = \"foo\";\nfn foo() {}";
        let last = super::super::testing::last_offset_of(text, "foo");
        let ids = [(0, 3), (last, last + 3)];
        let declaration = offset_of(text, "fn foo");
        let hits = text_occurrences(text, "foo", &ids, (declaration, utf16_len(text)));
        let shown: Vec<&str> = hits.iter().map(|(from, to)| super::super::text::slice16(text, from - 3, *to)).collect();
        assert_eq!(shown, ["// foo", "= \"foo"]);
    }

    #[test]
    fn says_how_many_usages_and_where() {
        let mut i18n = I18n::default();
        let at = |path: &str| location(path, range_of("x", 0, 1));
        assert_eq!(
            usages_message(&mut i18n, "Function 'f'", &[at("/a")], false),
            ("1 usage found".to_owned(), "Function 'f' has 1 usage that is not safe to delete.".to_owned())
        );
        assert_eq!(
            usages_message(&mut i18n, "The 2 files", &[at("/a"), at("/b")], true).1,
            "The 2 files have 2 usages that are not safe to delete (in 2 files)."
        );
    }

    // --- the editor flow ------------------------------------------------------------------

    const TEXT: &str = "fn used() {}\n\nfn unused() {\n    unused();\n}\n\nfn main() {}\n";

    fn setup(search_in_comments: bool) -> SafeDeleteSetup {
        SafeDeleteSetup {
            path: "/p/a.rs".to_owned(),
            language_name: "Rust".to_owned(),
            comments: c_like_comments(),
            search_in_comments,
        }
    }

    /// The backend's answers of a test.
    struct Backend {
        definition: Result<Vec<Location>, IdeError>,
        outline: Vec<OutlineItem>,
        locals: Locals,
        statement: Option<SyntaxNode>,
        references: Result<Vec<Location>, IdeError>,
        identifiers: Vec<IdentifierOccurrence>,
    }

    impl Default for Backend {
        fn default() -> Self {
            Self {
                definition: Ok(Vec::new()),
                outline: Vec::new(),
                locals: Locals::NoIdentifier,
                statement: None,
                references: Ok(Vec::new()),
                identifiers: Vec::new(),
            }
        }
    }

    /// Runs requests against `backend` until the flow asks something else; returns that step
    /// and the requests sent.
    fn drive(
        flow: &mut SafeDeleteFlow,
        mut step: SafeDeleteStep,
        backend: &Backend,
        i18n: &mut I18n,
    ) -> (SafeDeleteStep, Vec<SafeDeleteRequest>) {
        let mut sent = Vec::new();
        while let SafeDeleteStep::Requests(queries) = step {
            step = SafeDeleteStep::Nothing;
            for query in queries {
                sent.push(query.request.clone());
                let answer = match query.request {
                    SafeDeleteRequest::Definition { .. } => SafeDeleteAnswer::Definition(backend.definition.clone()),
                    SafeDeleteRequest::Outline => SafeDeleteAnswer::Outline(Ok(backend.outline.clone())),
                    SafeDeleteRequest::Locals { .. } => SafeDeleteAnswer::Locals(Ok(backend.locals.clone())),
                    SafeDeleteRequest::StructuralRanges { .. } => {
                        SafeDeleteAnswer::StructuralRanges(Ok(StructuralRanges {
                            statement: backend.statement.clone(),
                            ..StructuralRanges::default()
                        }))
                    }
                    SafeDeleteRequest::References { .. } => SafeDeleteAnswer::References(backend.references.clone()),
                    SafeDeleteRequest::Identifiers { .. } => {
                        SafeDeleteAnswer::Identifiers(Ok(backend.identifiers.clone()))
                    }
                };
                let next = flow.answered(query.ticket, answer, 1, i18n);
                if next != SafeDeleteStep::Nothing {
                    step = next;
                }
            }
        }
        (step, sent)
    }

    fn begin(
        text: &str,
        caret_at: u32,
        backend: &Backend,
        search: bool,
        i18n: &mut I18n,
    ) -> (SafeDeleteFlow, SafeDeleteStep, Vec<SafeDeleteRequest>) {
        let (mut flow, step) = SafeDeleteFlow::start(&Rope::from_str(text), 1, caret(caret_at), false, setup(search));
        let (step, sent) = drive(&mut flow, step, backend, i18n);
        (flow, step, sent)
    }

    /// Confirms with the checkbox `checked` and runs the searches.
    fn confirm(
        flow: &mut SafeDeleteFlow,
        checked: Option<bool>,
        caret_at: u32,
        backend: &Backend,
        i18n: &mut I18n,
    ) -> (SafeDeleteStep, Vec<SafeDeleteRequest>) {
        let step = flow.confirmed(checked, 1, caret(caret_at), i18n);
        drive(flow, step, backend, i18n)
    }

    fn deleted(text: &str, step: &SafeDeleteStep) -> String {
        let SafeDeleteStep::Delete(edit) = step else { panic!("expected a deletion, got {step:?}") };
        assert_eq!(edit.selections.len(), 1);
        assert_eq!(edit.selections[0], caret(edit.changes[0].from));
        apply_changes(&Rope::from_str(text), &edit.changes).to_string()
    }

    fn hint(step: &SafeDeleteStep) -> &str {
        let SafeDeleteStep::Hint { text, .. } = step else { panic!("expected a hint, got {step:?}") };
        text
    }

    #[test]
    fn deletes_an_unused_declaration_as_one_undo_step_ignoring_recursive_calls() {
        let mut i18n = I18n::default();
        let at = offset_of(TEXT, "unused() {") + 1;
        let own_call = offset_of(TEXT, "unused();");
        let backend = Backend {
            outline: vec![
                item(TEXT, "used", "fn used() {}", SymbolKind::Function),
                item(TEXT, "unused", "fn unused() {\n    unused();\n}", SymbolKind::Function),
            ],
            references: Ok(vec![location("/p/a.rs", range_of(TEXT, own_call, own_call + 6))]),
            ..Backend::default()
        };
        let (mut flow, step, _) = begin(TEXT, at, &backend, false, &mut i18n);
        assert!(matches!(step, SafeDeleteStep::Confirm(_)));
        let (step, sent) = confirm(&mut flow, Some(false), at, &backend, &mut i18n);
        assert!(sent.iter().any(|request| matches!(request, SafeDeleteRequest::References { .. })));
        // One edit with one change: a single undo step brings the text back.
        let SafeDeleteStep::Delete(edit) = &step else { panic!("expected a deletion") };
        assert_eq!(edit.changes.len(), 1);
        assert_eq!(deleted(TEXT, &step), "fn used() {}\n\nfn main() {}\n");
    }

    #[test]
    fn asks_when_usages_exist_view_usages_delete_anyway_cancel() {
        let usage = location("/p/b.rs", Range::new(Pos::new(3, 4), Pos::new(3, 8)));
        let backend = Backend {
            outline: vec![item(TEXT, "used", "fn used() {}", SymbolKind::Function)],
            references: Ok(vec![usage.clone()]),
            ..Backend::default()
        };
        let run = |answer: UsagesAnswer| {
            let mut i18n = I18n::default();
            let at = offset_of(TEXT, "used() {}") + 1;
            let (mut flow, _, _) = begin(TEXT, at, &backend, false, &mut i18n);
            let (step, _) = confirm(&mut flow, Some(false), at, &backend, &mut i18n);
            let SafeDeleteStep::UsagesDetected(question) = step else { panic!("expected the question, got {step:?}") };
            assert_eq!(question.title, "Usages Detected");
            assert_eq!(question.body, "1 usage found\nFunction 'used' has 1 usage that is not safe to delete.");
            assert_eq!([question.view_label, question.delete_label], ["View Usages", "Delete Anyway"]);
            flow.usages_answered(answer, 1)
        };
        assert_eq!(run(UsagesAnswer::Cancel), SafeDeleteStep::Focus);
        assert_eq!(
            run(UsagesAnswer::View),
            SafeDeleteStep::ViewUsages {
                action: "find.showUsageResults",
                symbol: "used".to_owned(),
                locations: vec![usage.clone()],
            }
        );
        assert_eq!(deleted(TEXT, &run(UsagesAnswer::Delete)), "fn unused() {\n    unused();\n}\n\nfn main() {}\n");
    }

    #[test]
    fn confirms_first_delete_function_and_searches_nothing_when_cancelled() {
        let mut i18n = I18n::default();
        let at = offset_of(TEXT, "unused() {") + 1;
        let backend = Backend {
            outline: vec![item(TEXT, "unused", "fn unused() {\n    unused();\n}", SymbolKind::Function)],
            ..Backend::default()
        };
        let (mut flow, step, _) = begin(TEXT, at, &backend, true, &mut i18n);
        let SafeDeleteStep::Confirm(question) = step else { panic!("expected the confirmation, got {step:?}") };
        assert_eq!(question.title, "Safe Delete");
        assert_eq!(question.message, "Delete Function 'unused'?");
        assert_eq!(question.checkbox, "Search in comments and strings");
        let (step, sent) = confirm(&mut flow, None, at, &backend, &mut i18n);
        assert_eq!(step, SafeDeleteStep::Focus);
        assert!(sent.is_empty());
    }

    #[test]
    fn with_search_in_comments_and_strings_previews_mentions_before_deleting() {
        let mut i18n = I18n::default();
        let src = "// calls unused\nfn unused() {}\n\nfn main() {}\n";
        let at = offset_of(src, "unused() {") + 1;
        let declaration = offset_of(src, "unused() {");
        let backend = Backend {
            outline: vec![item(src, "unused", "fn unused() {}", SymbolKind::Function)],
            identifiers: vec![IdentifierOccurrence {
                range: range_of(src, declaration, declaration + 6),
                kind: "identifier".to_owned(),
                qualified: false,
            }],
            ..Backend::default()
        };
        let (mut flow, _, _) = begin(src, at, &backend, true, &mut i18n);
        let (step, _) = confirm(&mut flow, Some(true), at, &backend, &mut i18n);
        let SafeDeleteStep::Preview(preview) = step else { panic!("expected the preview, got {step:?}") };
        assert_eq!(
            preview.occurrences,
            [super::super::preview::PreviewOccurrence { line: 0, text: "// calls unused".to_owned(), from: 9, to: 15 }]
        );
        assert!(!deleted(src, &flow.preview_confirmed(1)).contains("fn unused"));
    }

    #[test]
    fn refuses_a_declaration_in_another_file_instead_of_doing_something_else() {
        let mut i18n = I18n::default();
        let backend =
            Backend { definition: Ok(vec![location("/p/other.rs", range_of(TEXT, 0, 1))]), ..Backend::default() };
        let (_, step, _) = begin(TEXT, offset_of(TEXT, "main") + 1, &backend, false, &mut i18n);
        assert_eq!(hint(&step), "Cannot perform refactoring. 'main' is declared in other.rs; Safe Delete it there");
    }

    #[test]
    fn without_a_language_server_deletes_only_locals_using_the_syntax_references() {
        let mut i18n = I18n::default();
        let body = "fn f() {\n    let x = 1;\n    let y = 2;\n    g(y);\n}\n";
        let x = offset_of(body, "x");
        let at = offset_of(body, "let x");
        let backend = Backend {
            definition: Err(unavailable()),
            references: Err(unavailable()),
            locals: Locals::Local {
                symbol: LocalSymbol {
                    name: "x".to_owned(),
                    kind: LocalKind::Var,
                    definition: range_of(body, x, x + 1),
                    scope: range_of(body, 0, utf16_len(body)),
                    references: Vec::new(),
                    certain: true,
                    conflicts: Vec::new(),
                },
            },
            statement: Some(SyntaxNode {
                range: range_of(body, at, at + 10),
                ..node("let_declaration", body, "let x")
            }),
            ..Backend::default()
        };
        let (mut flow, _, _) = begin(body, x, &backend, false, &mut i18n);
        let (step, sent) = confirm(&mut flow, Some(false), x, &backend, &mut i18n);
        assert_eq!(deleted(body, &step), "fn f() {\n    let y = 2;\n    g(y);\n}\n");
        assert!(!sent.iter().any(|request| matches!(request, SafeDeleteRequest::References { .. })));
    }

    #[test]
    fn without_a_language_server_refuses_top_level_declarations_with_the_reason() {
        let mut i18n = I18n::default();
        let backend = Backend {
            definition: Err(unavailable()),
            outline: vec![item(TEXT, "used", "fn used() {}", SymbolKind::Function)],
            locals: Locals::NotLocal {
                name: "used".to_owned(),
                range: range_of(TEXT, 3, 7),
                reason: ide_syntax::NotLocalReason::TopLevel,
            },
            ..Backend::default()
        };
        let (_, step, _) = begin(TEXT, offset_of(TEXT, "used() {}") + 1, &backend, false, &mut i18n);
        assert_eq!(
            hint(&step),
            "Cannot perform refactoring. The language server for Rust is not available; \
             only local declarations can be deleted safely without it"
        );
    }

    #[test]
    fn needs_a_symbol_at_the_caret() {
        let mut i18n = I18n::default();
        let (_, step, sent) = begin("fn a() {}\n\n\n", 11, &Backend::default(), false, &mut i18n);
        assert_eq!(hint(&step), "Cannot perform refactoring. Caret should be on a symbol");
        assert!(sent.is_empty());
    }

    // --- files ------------------------------------------------------------------------------

    /// Runs a file usage search: every document is opened for it (id 99). Returns the outcome,
    /// the documents closed and the positions searched.
    fn search(
        paths: &[&str],
        outline: &[OutlineItem],
        references: impl Fn(Pos) -> Result<Vec<Location>, IdeError>,
    ) -> (UsageSearch, usize, Vec<Pos>) {
        let (mut flow, mut actions) = FileUsagesFlow::start(paths.iter().map(|p| (*p).to_owned()).collect());
        let (mut closed, mut searched) = (0, Vec::new());
        loop {
            let mut next = Vec::new();
            for action in actions {
                match action {
                    FileUsagesAction::Document { ticket, .. } => {
                        next.extend(flow.document(ticket, Ok((DocId(99), true))))
                    }
                    FileUsagesAction::Outline { ticket, .. } => next.extend(flow.outline(ticket, Ok(outline.to_vec()))),
                    FileUsagesAction::References { ticket, pos, .. } => {
                        searched.push(pos);
                        next.extend(flow.references(ticket, references(pos)));
                    }
                    FileUsagesAction::CloseDocument(_) => closed += 1,
                    FileUsagesAction::Done(outcome) => return (outcome, closed, searched),
                }
            }
            actions = next;
        }
    }

    #[test]
    fn collects_references_to_top_level_symbols_from_other_files() {
        let text = "pub fn a() {}\npub struct B;\n";
        let outline = [
            item(text, "a", "pub fn a() {}", SymbolKind::Function),
            item(text, "B", "pub struct B;", SymbolKind::Struct),
        ];
        let refs =
            vec![at_line("/p/a.rs", 0), at_line("/p/main.rs", 4), at_line("/p/main.rs", 4), at_line("/p/gone.rs", 1)];
        let (outcome, closed, _) = search(&["/p/a.rs", "/p/gone.rs"], &outline, |_| Ok(refs.clone()));
        assert_eq!(outcome, UsageSearch::Found(vec![at_line("/p/main.rs", 4)]));
        assert_eq!(closed, 2);
    }

    /// The name of the outline item at `pos`, for answers per searched name.
    fn name_at(text: &str, pos: Pos, names: &[&str]) -> String {
        let doc = Rope::from_str(text);
        let offset = crate::editor_lsp::positions::pos_to_offset(&doc, pos);
        let rest = buffer::slice(&doc, offset, buffer::len_utf16(&doc));
        names.iter().find(|name| rest.starts_with(**name)).map_or("?", |name| name).to_owned()
    }

    fn within(parent: OutlineItem, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem { children, ..parent }
    }

    #[test]
    fn java_the_package_s_other_classes_imported_elsewhere_are_not_usages() {
        let text = "package com.example.config;\n\npublic class MessageSourceConfig {}\n";
        let outline = [
            item(text, "com.example.config", "package com.example.config;", SymbolKind::Package),
            item(text, "MessageSourceConfig", "public class MessageSourceConfig {}", SymbolKind::Class),
        ];
        let names = ["com.example.config", "MessageSourceConfig"];
        let (outcome, _, searched) = search(&["/p/config/MessageSourceConfig.java"], &outline, |pos| {
            Ok(if name_at(text, pos, &names) == "com.example.config" {
                vec![at_line("/p/server/CommandSender.java", 2)]
            } else {
                Vec::new()
            })
        });
        assert_eq!(outcome, UsageSearch::Found(Vec::new()));
        let searched: Vec<String> = searched.iter().map(|pos| name_at(text, *pos, &names)).collect();
        assert_eq!(searched, ["MessageSourceConfig"]);
    }

    #[test]
    fn c_a_class_inside_a_namespace_is_searched() {
        let text = "namespace config {\nclass Used {};\nnamespace detail {\nint helper();\n}\n}\n";
        let namespace = item(
            text,
            "config",
            "namespace config {\nclass Used {};\nnamespace detail {\nint helper();\n}\n}",
            SymbolKind::Namespace,
        );
        let detail = item(text, "detail", "namespace detail {\nint helper();\n}", SymbolKind::Namespace);
        let outline = [within(
            namespace,
            vec![
                item(text, "Used", "class Used {};", SymbolKind::Class),
                within(detail, vec![item(text, "helper", "int helper();", SymbolKind::Function)]),
            ],
        )];
        let names = ["Used", "helper"];
        let (outcome, _, searched) = search(&["/p/config/used.h"], &outline, |pos| {
            Ok(match name_at(text, pos, &names).as_str() {
                "Used" => vec![at_line("/p/main.cpp", 4)],
                "helper" => vec![at_line("/p/main.cpp", 5)],
                _ => Vec::new(),
            })
        });
        assert_eq!(outcome, UsageSearch::Found(vec![at_line("/p/main.cpp", 4), at_line("/p/main.cpp", 5)]));
        let searched: Vec<String> = searched.iter().map(|pos| name_at(text, *pos, &names)).collect();
        assert_eq!(searched, ["Used", "helper"]);
    }

    #[test]
    fn modules_a_file_declares_ts_namespace_rust_mod_are_still_searched() {
        let text = "export namespace Utils {\n  export const version = 1;\n}\n";
        let outline = [within(
            item(text, "Utils", "export namespace Utils {\n  export const version = 1;\n}", SymbolKind::Module),
            vec![item(text, "version", "version = 1", SymbolKind::Variable)],
        )];
        let names = ["Utils"];
        let (outcome, _, searched) = search(&["/p/lib/ns.ts"], &outline, |pos| {
            Ok(if name_at(text, pos, &names) == "Utils" { vec![at_line("/p/main.ts", 5)] } else { Vec::new() })
        });
        assert_eq!(outcome, UsageSearch::Found(vec![at_line("/p/main.ts", 5)]));
        let searched: Vec<String> = searched.iter().map(|pos| name_at(text, *pos, &names)).collect();
        assert_eq!(searched, ["Utils"]);
    }

    #[test]
    fn is_unsupported_when_the_language_server_can_t_search() {
        let text = "pub fn a() {}\npub struct B;\n";
        let outline = [item(text, "a", "pub fn a() {}", SymbolKind::Function)];
        let (outcome, _, _) = search(&["/p/a.rs"], &outline, |_| Err(unavailable()));
        assert!(matches!(outcome, UsageSearch::Unsupported(_)));
    }

    #[test]
    fn is_cancellable() {
        let (mut flow, actions) = FileUsagesFlow::start(vec!["/p/a.rs".to_owned()]);
        let [FileUsagesAction::Document { ticket, .. }] = actions.as_slice() else {
            panic!("expected a document request")
        };
        assert_eq!(flow.cancel(), [FileUsagesAction::Done(UsageSearch::Failed("cancelled".to_owned()))]);
        assert!(flow.document(*ticket, Ok((DocId(99), true))).is_empty());
        // A cancelled reference search cancels the whole search.
        let (mut flow, actions) = FileUsagesFlow::start(vec!["/p/a.rs".to_owned()]);
        let [FileUsagesAction::Document { ticket, .. }] = actions.as_slice() else {
            panic!("expected a document request")
        };
        let actions = flow.document(*ticket, Ok((DocId(7), true)));
        let [FileUsagesAction::Outline { ticket, .. }] = actions.as_slice() else {
            panic!("expected an outline request")
        };
        let text = "pub fn a() {}\n";
        let outline = vec![item(text, "a", "pub fn a() {}", SymbolKind::Function)];
        let actions = flow.outline(*ticket, Ok(outline));
        let [FileUsagesAction::References { ticket, .. }] = actions.as_slice() else {
            panic!("expected a references request")
        };
        let ticket = *ticket;
        assert_eq!(
            flow.references(ticket, Err(cancelled())),
            [
                FileUsagesAction::CloseDocument(DocId(7)),
                FileUsagesAction::Done(UsageSearch::Failed("cancelled".to_owned()))
            ]
        );
    }
}
