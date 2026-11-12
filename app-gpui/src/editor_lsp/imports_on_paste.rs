//! Imports on paste: after code is pasted, the language server's import fixes for the names it
//! cannot resolve in the pasted text are offered (Ask), applied (Always) or left alone (Never).
//!
//! A paste waits for its text to reach the server, then for a fresh diagnostic publication; once
//! publications settle, each unresolved name in the pasted ranges is asked for its quick fixes.
//! Only an unambiguous import fix counts, and the fixes merge into one edit guarded by the
//! snapshot they were computed against. Any edit, a read-only switch, losing focus without the
//! dialog, or the deadline cancels the whole thing; answers of a cancelled paste are dropped.

use super::diagnostics::DiagnosticPublication;
use super::markdown::percent_decode;
use super::positions::{compare_pos, range_to_offsets};
use crate::editor::buffer;
use ide_core::editor_settings::ImportsOnPaste;
use ide_core::ipc_path::to_ipc;
use ide_core::{
    Diagnostic, DocId, FileEdit, IdeError, Pos, Range, RequestId, TextChange, TextEdit, WorkspaceEdit,
    WorkspaceEditPrecondition,
};
use ide_lsp::types::{CodeAction, CodeActionQuery};
use regex::Regex;
use ropey::Rope;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// How long a paste waits for diagnostics and fixes.
pub const WAIT_MS: u64 = 8000;
/// How long publications settle before the fixes are asked for.
pub const SETTLE_MS: u64 = 200;

static IMPORT_FIX: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?i)^(?:(?:add|insert|update) (?:an? )?import\b|import\b|add .+ to (?:an? )?existing import\b)")
});
static IMPORT_ALL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^import (?:all|missing)\b"));
static UNRESOLVED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)cannot find (?:name|namespace|type|value|function|macro|trait|module)|cannot be resolved|failed to resolve|unresolved (?:reference|name|type|symbol|import)|use of undeclared|undeclared (?:name|identifier)|undefined(?::| (?:name|variable))|not defined|unknown (?:identifier|symbol|type|name)|does not exist in (?:the )?current context|type or namespace name .+ could not be found",
    )
});
static UNRESOLVED_CODE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"^(?:2304|2552|2503|reportUndefinedVariable|UNRESOLVED_REFERENCE|undeclared_var_use|E0405|E0412|E0422|E0425|E0432|E0433|CS0103|CS0246)$",
    )
});

/// The setting, defaulting a missing or unknown value to Ask. `editor` is the raw editor
/// settings object.
pub fn imports_on_paste_of(editor: Option<&Value>) -> ImportsOnPaste {
    editor
        .and_then(|editor| editor.get("autoImport"))
        .and_then(|auto| auto.get("importsOnPaste"))
        .and_then(|mode| serde_json::from_value(mode.clone()).ok())
        .unwrap_or_default()
}

fn in_document(doc: &Rope, pos: Pos) -> bool {
    pos.line < buffer::line_count(doc) && {
        let line = buffer::line_range(doc, pos.line);
        pos.col <= line.end - line.start
    }
}

/// The unresolved-name diagnostics lying inside the pasted ranges (UTF-16 offsets of `doc`).
pub fn pasted_diagnostics(doc: &Rope, ranges: &[std::ops::Range<u32>], items: &[Diagnostic]) -> Vec<Diagnostic> {
    items
        .iter()
        .filter(|d| {
            if d.unnecessary
                || d.deprecated
                || (!UNRESOLVED.is_match(&d.message)
                    && !UNRESOLVED_CODE.is_match(d.code.as_deref().unwrap_or_default()))
            {
                return false;
            }
            if compare_pos(d.range.start, d.range.end) != Ordering::Less {
                return false;
            }
            if !in_document(doc, d.range.start) || !in_document(doc, d.range.end) {
                return false;
            }
            let offsets = range_to_offsets(doc, d.range);
            ranges.iter().any(|range| range.start <= offsets.start && offsets.end <= range.end)
        })
        .cloned()
        .collect()
}

/// The one import fix among `actions`; `None` when there is none or several.
pub fn pasted_import(actions: &[CodeAction]) -> Option<&CodeAction> {
    let mut imports = actions.iter().filter(|action| {
        action.disabled_reason.is_none()
            && action.kind.as_deref().is_none_or(|kind| kind == "quickfix" || kind.starts_with("quickfix."))
            && IMPORT_FIX.is_match(&action.title)
            && !IMPORT_ALL.is_match(&action.title)
    });
    let first = imports.next()?;
    imports.next().is_none().then_some(first)
}

/// A document's edits in an LSP `WorkspaceEdit`: URI, version, edits.
type DocumentEdits = (String, Option<i32>, Vec<TextEdit>);

/// The edits of an LSP `WorkspaceEdit` value per document. `None` for a document change that is not a
/// text edit.
fn lsp_edits_of(arg: &Value) -> Option<Vec<DocumentEdits>> {
    fn position(value: &Value) -> Pos {
        let number = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
        Pos::new(number("line"), number("character"))
    }
    fn edits(value: &Value) -> Vec<TextEdit> {
        value
            .as_array()
            .map(|edits| {
                edits
                    .iter()
                    .map(|edit| TextEdit {
                        range: Range::new(position(&edit["range"]["start"]), position(&edit["range"]["end"])),
                        new_text: edit["newText"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    let Some(object) = arg.as_object() else { return Some(Vec::new()) };
    if let Some(changes) = object.get("documentChanges") {
        let mut out = Vec::new();
        for change in changes.as_array().map(Vec::as_slice).unwrap_or_default() {
            let document = change.get("textDocument")?;
            let edit_list = change.get("edits")?;
            let uri = document.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned();
            let version = document.get("version").and_then(Value::as_i64).and_then(|v| i32::try_from(v).ok());
            out.push((uri, version, edits(edit_list)));
        }
        return Some(out);
    }
    Some(
        object
            .get("changes")
            .and_then(Value::as_object)
            .map(|changes| changes.iter().map(|(uri, list)| (uri.clone(), None, edits(list))).collect())
            .unwrap_or_default(),
    )
}

/// The path of a `file:` URI.
fn uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:")?;
    let rest = match rest.strip_prefix("//") {
        Some(authority) => authority.find('/').map_or("", |slash| &authority[slash..]),
        None => rest,
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let path = percent_decode(rest);
    let drive = path.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) && path.as_bytes().get(2) == Some(&b':');
    let path = if drive { path[1..].to_owned() } else { path };
    Some(path.replace('\\', "/"))
}

/// A fix sent as a command carrying an LSP `WorkspaceEdit` instead of an edit, lifted into one
/// when it only edits `path`, so it merges and applies like any other fix.
pub fn with_command_edit(action: &CodeAction, path: &str) -> CodeAction {
    let Some(command) = action.command.as_ref().filter(|_| action.edit.is_none()) else { return action.clone() };
    let path = path.replace('\\', "/");
    let mut files = Vec::new();
    for raw in &command.arguments {
        let parsed;
        let arg = match raw {
            Value::String(text) => match serde_json::from_str::<Value>(text) {
                Ok(value) => {
                    parsed = value;
                    &parsed
                }
                Err(_) => continue,
            },
            other => other,
        };
        let Some(documents) = lsp_edits_of(arg) else { return action.clone() };
        for (uri, version, edits) in documents {
            if uri_path(&uri).as_deref() != Some(path.as_str()) {
                return action.clone();
            }
            files.push(FileEdit::Edit { path: path.as_str().into(), edits, version });
        }
    }
    if files.is_empty() {
        return action.clone();
    }
    CodeAction { edit: Some(WorkspaceEdit { files, preconditions: Vec::new() }), ..action.clone() }
}

/// The import fixes of one snapshot as one edit of `path`, keeping their content and version
/// guards; `None` when they conflict, touch another file or do anything but insert text.
pub fn merge_pasted_imports(path: &str, actions: &[CodeAction]) -> Option<WorkspaceEdit> {
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut preconditions: BTreeMap<String, String> = BTreeMap::new();
    let mut version: Option<i32> = None;
    for action in actions {
        let edit = action.edit.as_ref().filter(|edit| !edit.files.is_empty())?;
        for precondition in &edit.preconditions {
            let key = to_ipc(&precondition.path);
            if preconditions.get(&key).is_some_and(|hash| *hash != precondition.hash) {
                return None;
            }
            preconditions.insert(key, precondition.hash.clone());
        }
        for file in &edit.files {
            let FileEdit::Edit { path: own, edits: file_edits, version: own_version } = file else { return None };
            if to_ipc(own) != path || file_edits.is_empty() {
                return None;
            }
            if version.is_some() && own_version.is_some() && version != *own_version {
                return None;
            }
            version = version.or(*own_version);
            for text_edit in file_edits {
                if text_edit.new_text.trim().is_empty()
                    || compare_pos(text_edit.range.start, text_edit.range.end) == Ordering::Greater
                {
                    return None;
                }
                if !edits.contains(text_edit) {
                    edits.push(text_edit.clone());
                }
            }
        }
    }
    edits.sort_by(|a, b| compare_pos(a.range.start, b.range.start).then(compare_pos(a.range.end, b.range.end)));
    let mut merged: Vec<TextEdit> = Vec::new();
    for edit in edits {
        if let Some(previous) = merged.last_mut() {
            let same_start = previous.range.start == edit.range.start;
            let both_insert = previous.range.is_empty() && edit.range.is_empty();
            if same_start && both_insert {
                // Only whole import lines can be concatenated at a shared insertion point.
                if !previous.new_text.ends_with('\n') || !edit.new_text.ends_with('\n') {
                    return None;
                }
                previous.new_text.push_str(&edit.new_text);
                continue;
            }
            if same_start || compare_pos(edit.range.start, previous.range.end) == Ordering::Less {
                return None;
            }
        }
        merged.push(edit);
    }
    (!merged.is_empty()).then(|| WorkspaceEdit {
        files: vec![FileEdit::Edit { path: path.into(), edits: merged, version }],
        preconditions: preconditions
            .into_iter()
            .map(|(path, hash)| WorkspaceEditPrecondition { path: path.into(), hash })
            .collect(),
    })
}

/// Whether a transaction is a paste whose text imports are offered for: a plain paste typed in
/// this editor, not mirrored from another one nor a transient change.
pub fn is_paste_event(user_event: Option<&str>, external: bool, transient: bool) -> bool {
    user_event == Some("input.paste") && !external && !transient
}

/// One transaction of an editor update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteTransaction {
    /// In the coordinates of the document before this transaction.
    pub changes: Vec<TextChange>,
    /// See [`is_paste_event`].
    pub paste: bool,
}

/// An editor update as imports on paste sees it.
#[derive(Debug, Clone)]
pub struct PasteUpdate<'a> {
    pub transactions: &'a [PasteTransaction],
    /// The document after the update.
    pub doc: &'a Rope,
    /// The editor or its document is read-only.
    pub read_only: bool,
    pub mode: ImportsOnPaste,
    /// The editor lost focus.
    pub blurred: bool,
}

/// What the caller does.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "a few per paste, each carried out at once")]
pub enum PasteEffect {
    /// Report [`PasteSession::synced`] once the paste reached the backend.
    WaitForSync {
        ticket: u64,
    },
    /// Report [`PasteSession::deadline`] after this long.
    Deadline {
        ticket: u64,
        ms: u64,
    },
    /// Report [`PasteSession::settled`] after this long (a newer settle timer replaces it).
    Settle {
        ticket: u64,
        ms: u64,
    },
    /// Ask for the quick fixes at `range` (`lspCodeActionQuery` with only `quickfix`).
    Query {
        id: RequestId,
        doc: DocId,
        range: Range,
    },
    /// Resolve the fix (`codeAction/resolve`); report [`PasteSession::resolve_answer`].
    Resolve {
        ticket: u64,
        action: CodeAction,
    },
    CancelRequest(RequestId),
    /// Ask which imports to add; report [`PasteSession::popup_apply`] or [`PasteSession::popup_closed`].
    OpenPopup {
        imports: Vec<String>,
    },
    ClosePopup,
    /// Apply the merged imports (`applyWorkspaceEdit`), unless the document changed meanwhile.
    Apply(WorkspaceEdit),
}

#[derive(Debug, Clone)]
struct Pending {
    ticket: u64,
    doc: Rope,
    ranges: Vec<std::ops::Range<u32>>,
    diagnostics: Vec<(String, Vec<Diagnostic>)>,
    synced: bool,
    running: bool,
    deadline: bool,
    popup: bool,
    mode: ImportsOnPaste,
    settle: u64,
    queue: VecDeque<Diagnostic>,
    query: Option<RequestId>,
    resolving: bool,
    imports: Vec<CodeAction>,
}

/// Imports on paste for one editor.
#[derive(Debug, Clone)]
pub struct PasteSession {
    doc: DocId,
    path: String,
    pending: Option<Pending>,
    tickets: u64,
}

/// The ranges (in the document after the update) each paste inserted, followed through the later
/// transactions of the update.
fn pasted_ranges(transactions: &[PasteTransaction]) -> Vec<std::ops::Range<u32>> {
    let mut ranges: Vec<std::ops::Range<u32>> = Vec::new();
    for transaction in transactions {
        let changes = &transaction.changes;
        for range in &mut ranges {
            let from = buffer::map_offset(range.start, changes, buffer::Assoc::After);
            let to = buffer::map_offset(range.end, changes, buffer::Assoc::Before);
            *range = from..to.max(from);
        }
        if !transaction.paste {
            continue;
        }
        let mut delta: i64 = 0;
        for change in changes {
            let inserted = change.insert.encode_utf16().count() as i64;
            let from = (i64::from(change.from) + delta) as u32;
            if inserted > 0 {
                ranges.push(from..from + inserted as u32);
            }
            delta += inserted - i64::from(change.to - change.from);
        }
    }
    ranges
}

impl PasteSession {
    pub fn new(doc: DocId, path: &str) -> Self {
        Self { doc, path: path.to_owned(), pending: None, tickets: 0 }
    }

    /// Whether a paste is being followed.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    fn cancel(&mut self, effects: &mut Vec<PasteEffect>) {
        let Some(pending) = self.pending.take() else { return };
        if let Some(id) = pending.query {
            effects.push(PasteEffect::CancelRequest(id));
        }
        if pending.popup {
            effects.push(PasteEffect::ClosePopup);
        }
    }

    fn current(&mut self, ticket: u64) -> Option<&mut Pending> {
        self.pending.as_mut().filter(|pending| pending.ticket == ticket)
    }

    pub fn update(&mut self, update: &PasteUpdate<'_>) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        let off = update.read_only || update.mode == ImportsOnPaste::Never;
        if let Some(pending) = &mut self.pending {
            pending.mode = update.mode.clone();
        }
        let popup = self.pending.as_ref().is_some_and(|pending| pending.popup);
        if self.pending.is_some() && (off || (update.blurred && !popup)) {
            self.cancel(&mut effects);
        }
        if update.transactions.iter().all(|transaction| transaction.changes.is_empty()) {
            return effects;
        }
        self.cancel(&mut effects);
        if off {
            return effects;
        }
        let ranges = pasted_ranges(update.transactions);
        if ranges.is_empty() {
            return effects;
        }
        self.tickets += 1;
        let ticket = self.tickets;
        self.pending = Some(Pending {
            ticket,
            doc: update.doc.clone(),
            ranges,
            diagnostics: Vec::new(),
            synced: false,
            running: false,
            deadline: true,
            popup: false,
            mode: update.mode.clone(),
            settle: 0,
            queue: VecDeque::new(),
            query: None,
            resolving: false,
            imports: Vec::new(),
        });
        effects.push(PasteEffect::WaitForSync { ticket });
        effects.push(PasteEffect::Deadline { ticket, ms: WAIT_MS });
        effects
    }

    /// The paste reached the backend; publications from now on are fresh.
    pub fn synced(&mut self, ticket: u64) {
        if let Some(pending) = self.current(ticket) {
            pending.synced = true;
        }
    }

    /// Syncing failed: the paste is left alone.
    pub fn sync_failed(&mut self, ticket: u64) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        if self.current(ticket).is_some() {
            self.cancel(&mut effects);
        }
        effects
    }

    /// The diagnostics of the file changed; `publication` is set for a fresh backend publication
    /// (an inspection's own update is not one).
    pub fn published(&mut self, publication: Option<&DiagnosticPublication>) -> Vec<PasteEffect> {
        let Some(publication) = publication else { return Vec::new() };
        let Some(pending) = self.pending.as_mut().filter(|pending| pending.synced && !pending.running) else {
            return Vec::new();
        };
        match pending.diagnostics.iter_mut().find(|(source, _)| *source == publication.source) {
            Some((_, items)) => items.clone_from(&publication.items),
            None => pending.diagnostics.push((publication.source.clone(), publication.items.clone())),
        }
        self.tickets += 1;
        pending.settle = self.tickets;
        vec![PasteEffect::Settle { ticket: self.tickets, ms: SETTLE_MS }]
    }

    /// A settle timer elapsed: the unresolved names of the paste are asked for their fixes.
    pub fn settled(&mut self, ticket: u64, popup_busy: bool) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        let Some(pending) = self.pending.as_mut().filter(|pending| pending.settle == ticket && !pending.running) else {
            return effects;
        };
        let items: Vec<Diagnostic> = pending.diagnostics.iter().flat_map(|(_, items)| items.iter().cloned()).collect();
        let diagnostics = pasted_diagnostics(&pending.doc, &pending.ranges, &items);
        if diagnostics.is_empty() {
            return effects;
        }
        pending.running = true;
        let mut seen = HashSet::new();
        pending.queue = diagnostics.into_iter().filter(|d| seen.insert(d.range)).collect();
        self.next(&mut effects, popup_busy);
        effects
    }

    /// Asks about the next unresolved name, or finishes: Always applies the imports, Ask offers
    /// them unless another popup is open.
    fn next(&mut self, effects: &mut Vec<PasteEffect>, popup_busy: bool) {
        let doc = self.doc;
        let Some(pending) = self.pending.as_mut() else { return };
        if let Some(diagnostic) = pending.queue.pop_front() {
            let id = RequestId::next();
            pending.query = Some(id);
            effects.push(PasteEffect::Query { id, doc, range: diagnostic.range });
            return;
        }
        pending.query = None;
        if pending.imports.is_empty() {
            self.cancel(effects);
            return;
        }
        // The fixes are found: the deadline no longer applies.
        pending.deadline = false;
        if pending.mode == ImportsOnPaste::Always {
            let edit = merge_pasted_imports(&self.path, &pending.imports);
            self.cancel(effects);
            effects.extend(edit.map(PasteEffect::Apply));
        } else if popup_busy {
            self.cancel(effects);
        } else {
            pending.popup = true;
            let imports = pending.imports.iter().map(|action| action.title.clone()).collect();
            effects.push(PasteEffect::OpenPopup { imports });
        }
    }

    /// Considers an import fix that carries its edit.
    fn consider(&mut self, fix: &CodeAction) {
        let path = self.path.clone();
        let Some(pending) = self.pending.as_mut() else { return };
        let fix = with_command_edit(fix, &path);
        if pasted_import(std::slice::from_ref(&fix)).is_none()
            || merge_pasted_imports(&path, std::slice::from_ref(&fix)).is_none()
        {
            return;
        }
        if pending.imports.iter().any(|known| known.edit == fix.edit) {
            return;
        }
        let mut candidates = pending.imports.clone();
        candidates.push(fix.clone());
        if merge_pasted_imports(&path, &candidates).is_some() {
            pending.imports.push(fix);
        }
    }

    /// The quick fixes at an unresolved name; an error (a missing or slow server) leaves the
    /// paste alone.
    pub fn query_answer(
        &mut self,
        id: RequestId,
        answer: Result<CodeActionQuery, IdeError>,
        popup_busy: bool,
    ) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        let Some(pending) = self.pending.as_mut().filter(|pending| pending.query == Some(id)) else {
            return effects;
        };
        pending.query = None;
        let ticket = pending.ticket;
        let actions = match answer {
            Ok(query) => query.actions,
            Err(_) => {
                self.cancel(&mut effects);
                return effects;
            }
        };
        let Some(fix) = pasted_import(&actions).map(|fix| with_command_edit(fix, &self.path)) else {
            self.next(&mut effects, popup_busy);
            return effects;
        };
        if fix.edit.is_none() {
            pending.resolving = true;
            effects.push(PasteEffect::Resolve { ticket, action: fix });
            return effects;
        }
        self.consider(&fix);
        self.next(&mut effects, popup_busy);
        effects
    }

    /// The resolve of a lazy fix answered.
    pub fn resolve_answer(
        &mut self,
        ticket: u64,
        answer: Result<CodeAction, IdeError>,
        popup_busy: bool,
    ) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        let Some(pending) = self.current(ticket).filter(|pending| pending.resolving) else { return effects };
        pending.resolving = false;
        match answer {
            Ok(fix) => {
                self.consider(&fix);
                self.next(&mut effects, popup_busy);
            }
            Err(_) => self.cancel(&mut effects),
        }
        effects
    }

    /// The deadline of paste `ticket` passed: it is given up unless its fixes are found.
    pub fn deadline(&mut self, ticket: u64) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        if self.current(ticket).is_some_and(|pending| pending.deadline) {
            self.cancel(&mut effects);
        }
        effects
    }

    /// The dialog's Add with the chosen imports (indexes into its list).
    pub fn popup_apply(&mut self, selected: &[usize]) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        let Some(pending) = self.pending.as_ref().filter(|pending| pending.popup) else { return effects };
        let chosen: Vec<CodeAction> = pending
            .imports
            .iter()
            .enumerate()
            .filter(|(i, _)| selected.contains(i))
            .map(|(_, action)| action.clone())
            .collect();
        let edit = merge_pasted_imports(&self.path, &chosen);
        self.cancel(&mut effects);
        effects.extend(edit.map(PasteEffect::Apply));
        effects
    }

    /// The dialog was closed (Cancel, Escape): the paste is left alone.
    pub fn popup_closed(&mut self) {
        self.pending = None;
    }

    /// The editor went away.
    pub fn destroyed(&mut self) -> Vec<PasteEffect> {
        let mut effects = Vec::new();
        self.cancel(&mut effects);
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::ServerId;
    use ide_core::Severity;
    use ide_core::error::ErrorKind;
    use ide_lsp::types::Command;
    use serde_json::json;

    const PATH: &str = "/p/Main.java";

    fn range(line: u32, col: u32, end_col: u32) -> Range {
        Range::new(Pos::new(line, col), Pos::new(line, end_col))
    }

    fn line(line: u32) -> Range {
        range(line, 0, 9)
    }

    fn diagnostic(range: Range) -> Diagnostic {
        Diagnostic {
            range,
            severity: Severity::Error,
            message: "ArrayList cannot be resolved to a type".into(),
            source: Some("Java".into()),
            code: Some("16777218".into()),
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn edit(text: &str, range: Range) -> TextEdit {
        TextEdit { range, new_text: text.into() }
    }

    fn import_edit(text: &str) -> TextEdit {
        edit(text, Range::new(Pos::new(0, 0), Pos::new(0, 0)))
    }

    fn file(path: &str, edits: Vec<TextEdit>, version: Option<i32>) -> FileEdit {
        FileEdit::Edit { path: path.into(), edits, version }
    }

    fn action() -> CodeAction {
        CodeAction {
            title: "Import 'ArrayList' (java.util)".into(),
            kind: Some("quickfix".into()),
            is_preferred: false,
            disabled_reason: None,
            edit: Some(WorkspaceEdit {
                files: vec![file(PATH, vec![import_edit("import java.util.ArrayList;\n")], Some(7))],
                preconditions: vec![WorkspaceEditPrecondition { path: PATH.into(), hash: "snapshot".into() }],
            }),
            command: None,
            data: None,
            server: ServerId(1),
        }
    }

    fn titled(title: &str) -> CodeAction {
        CodeAction { title: title.into(), ..action() }
    }

    fn json_of<T: serde::Serialize>(value: &T) -> Value {
        serde_json::to_value(value).expect("serializes")
    }

    #[test]
    fn limits_unresolved_diagnostics_to_pasted_ranges_using_utf_16_offsets() {
        let doc = Rope::from_str("😀 ArrayList old\nmissing");
        let inside = diagnostic(range(0, 3, 12));
        let warning = Diagnostic {
            severity: Severity::Warning,
            message: "\"missing\" is not defined".into(),
            code: Some("reportUndefinedVariable".into()),
            ..diagnostic(range(1, 0, 7))
        };
        let items = [
            inside.clone(),
            warning.clone(),
            diagnostic(range(0, 0, 2)),
            diagnostic(range(0, 3, 13)),
            diagnostic(range(0, 3, 3)),
            diagnostic(range(8, 0, 4)),
            diagnostic(range(0, 3, 90)),
            diagnostic(Range::new(Pos::new(0, 12), Pos::new(0, 3))),
            Diagnostic { message: "Incompatible types".into(), code: Some("2322".into()), ..inside.clone() },
            Diagnostic { unnecessary: true, ..inside.clone() },
        ];
        assert_eq!(pasted_diagnostics(&doc, &[3..12, 17..24], &items), [inside, warning]);
    }

    #[test]
    fn recognizes_the_import_quick_fix() {
        for title in [
            "Import 'ArrayList' (java.util)",
            "Add import from \"react\"",
            "Update import from \"react\"",
            "Import `std::collections::HashMap`",
            "Add import: \"fmt\"",
            "Add 'useState' to existing import declaration from 'react'",
        ] {
            let actions = [titled(title), titled("Create class ArrayList")];
            assert_eq!(pasted_import(&actions).map(|a| a.title.as_str()), Some(title));
        }
    }

    #[test]
    fn skips_ambiguous_disabled_source_and_unrelated_import_actions() {
        assert!(pasted_import(&[action(), titled("Import 'ArrayList' (other)")]).is_none());
        let variants = [
            CodeAction { disabled_reason: Some("unavailable".into()), ..action() },
            CodeAction { disabled_reason: Some(String::new()), ..action() },
            CodeAction { kind: Some("source.organizeImports".into()), ..action() },
            CodeAction { kind: Some("quickfixOther".into()), ..action() },
            titled("Remove unused import"),
            titled("Import all missing names"),
            titled("Optimize imports"),
        ];
        for variant in variants {
            assert!(pasted_import(std::slice::from_ref(&variant)).is_none(), "{}", variant.title);
        }
        let preferred = CodeAction { is_preferred: true, ..action() };
        assert!(pasted_import(&[preferred, titled("Import 'ArrayList' (other)")]).is_none());
    }

    #[test]
    fn merges_independent_insertions_once_preserving_preconditions_and_versions() {
        let first = action();
        let mut second = action();
        if let Some(edit) = &mut second.edit {
            edit.files = vec![file(PATH, vec![import_edit("import java.util.List;\n")], Some(7))];
        }
        let merged = merge_pasted_imports(PATH, &[first.clone(), second, first.clone()]).expect("the imports merge");
        assert_eq!(Some(&merged.preconditions), first.edit.as_ref().map(|edit| &edit.preconditions));
        assert_eq!(
            merged.files,
            [file(PATH, vec![import_edit("import java.util.ArrayList;\nimport java.util.List;\n")], Some(7))]
        );
        assert_eq!(first.edit, action().edit);
    }

    #[test]
    fn rejects_conflicting_replacements_cross_file_edits_file_operations_and_command_only_fixes() {
        let with_files = |files: Vec<FileEdit>, preconditions: Vec<WorkspaceEditPrecondition>| CodeAction {
            edit: Some(WorkspaceEdit { files, preconditions }),
            ..action()
        };
        let replacing = with_files(vec![file(PATH, vec![edit("import A;", range(0, 0, 10))], None)], Vec::new());
        let conflict = with_files(vec![file(PATH, vec![edit("import B;", range(0, 0, 10))], None)], Vec::new());
        assert!(merge_pasted_imports(PATH, &[replacing, conflict]).is_none());
        assert!(merge_pasted_imports(PATH, &[CodeAction { edit: None, ..action() }]).is_none());
        let create = with_files(vec![FileEdit::Create { path: PATH.into(), overwrite: false }], Vec::new());
        assert!(merge_pasted_imports(PATH, &[create]).is_none());
        let other = with_files(vec![file("/other", vec![import_edit("import x;\n")], None)], Vec::new());
        assert!(merge_pasted_imports(PATH, &[other]).is_none());
        let changed =
            with_files(Vec::new(), vec![WorkspaceEditPrecondition { path: PATH.into(), hash: "changed".into() }]);
        assert!(merge_pasted_imports(PATH, &[action(), changed]).is_none());
    }

    #[test]
    fn defaults_missing_and_invalid_settings_to_ask() {
        let invalid = [
            None,
            Some(json!({})),
            Some(json!({ "autoImport": { "importsOnPaste": "invalid" } })),
            Some(json!({ "autoImport": { "importsOnPaste": 4 } })),
        ];
        for raw in invalid {
            assert_eq!(imports_on_paste_of(raw.as_ref()), ImportsOnPaste::Ask);
        }
        for (raw, mode) in
            [("ask", ImportsOnPaste::Ask), ("always", ImportsOnPaste::Always), ("never", ImportsOnPaste::Never)]
        {
            assert_eq!(imports_on_paste_of(Some(&json!({ "autoImport": { "importsOnPaste": raw } }))), mode);
        }
    }

    type Answer = Result<CodeActionQuery, IdeError>;

    fn answer(actions: Vec<CodeAction>) -> Answer {
        Ok(CodeActionQuery { actions, advertised_kinds: Some(vec!["quickfix".into()]) })
    }

    /// An editor of `PATH` with imports on paste, the timers, the server and the popup played by
    /// hand.
    struct Harness {
        session: PasteSession,
        doc: Rope,
        mode: ImportsOnPaste,
        read_only: bool,
        /// Report the sync of a paste at once.
        auto_sync: bool,
        sync: Option<u64>,
        settle: Option<u64>,
        deadline: Option<u64>,
        /// Queries asked and not answered yet.
        open_queries: VecDeque<RequestId>,
        queries: Vec<Range>,
        cancelled: Vec<RequestId>,
        /// Scripted answers, in order; then [`action`] for every query.
        answers: VecDeque<Answer>,
        /// Answer queries as they come.
        answer_queries: bool,
        /// Resolves asked and not answered yet, and how many were asked in all.
        resolves: VecDeque<(u64, CodeAction)>,
        resolve_count: usize,
        /// Scripted resolve answers; then the action with [`action`]'s edit.
        resolved: VecDeque<CodeAction>,
        hold_resolves: bool,
        popup: Option<Vec<String>>,
        other_popup: bool,
        applied: Vec<WorkspaceEdit>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                session: PasteSession::new(DocId(3), PATH),
                doc: Rope::new(),
                mode: ImportsOnPaste::Ask,
                read_only: false,
                auto_sync: true,
                sync: None,
                settle: None,
                deadline: None,
                open_queries: VecDeque::new(),
                queries: Vec::new(),
                cancelled: Vec::new(),
                answers: VecDeque::new(),
                answer_queries: true,
                resolves: VecDeque::new(),
                resolve_count: 0,
                resolved: VecDeque::new(),
                hold_resolves: false,
                popup: None,
                other_popup: false,
                applied: Vec::new(),
            }
        }

        fn run(&mut self, effects: Vec<PasteEffect>) {
            for effect in effects {
                match effect {
                    PasteEffect::WaitForSync { ticket } => {
                        if self.auto_sync {
                            self.session.synced(ticket);
                        } else {
                            self.sync = Some(ticket);
                        }
                    }
                    PasteEffect::Deadline { ticket, .. } => self.deadline = Some(ticket),
                    PasteEffect::Settle { ticket, .. } => self.settle = Some(ticket),
                    PasteEffect::Query { id, doc, range } => {
                        assert_eq!(doc, DocId(3));
                        self.queries.push(range);
                        self.open_queries.push_back(id);
                    }
                    PasteEffect::Resolve { ticket, action } => {
                        self.resolve_count += 1;
                        self.resolves.push_back((ticket, action));
                    }
                    PasteEffect::CancelRequest(id) => self.cancelled.push(id),
                    PasteEffect::OpenPopup { imports } => self.popup = Some(imports),
                    PasteEffect::ClosePopup => self.popup = None,
                    PasteEffect::Apply(edit) => self.applied.push(edit),
                }
            }
            self.drive();
        }

        /// Answers what the server is asked, as scripted.
        fn drive(&mut self) {
            if !self.hold_resolves
                && let Some((ticket, lazy)) = self.resolves.pop_front()
            {
                let resolved = self.resolved.pop_front().unwrap_or_else(|| CodeAction { edit: action().edit, ..lazy });
                let effects = self.session.resolve_answer(ticket, Ok(resolved), self.other_popup);
                return self.run(effects);
            }
            if self.answer_queries
                && let Some(id) = self.open_queries.pop_front()
            {
                let answer = self.answers.pop_front().unwrap_or_else(|| answer(vec![action()]));
                let effects = self.session.query_answer(id, answer, self.other_popup);
                self.run(effects);
            }
        }

        fn transaction(&mut self, changes: Vec<TextChange>, paste: bool) {
            self.doc = super::super::positions::apply_changes(&self.doc, &changes);
            let transactions = [PasteTransaction { changes, paste }];
            let update = PasteUpdate {
                transactions: &transactions,
                doc: &self.doc,
                read_only: self.read_only,
                mode: self.mode.clone(),
                blurred: false,
            };
            let effects = self.session.update(&update);
            self.run(effects);
        }

        fn paste(&mut self, text: &str) {
            let end = buffer::len_utf16(&self.doc);
            self.transaction(vec![TextChange { from: end, to: end, insert: text.into() }], true);
        }

        fn typed(&mut self, at: u32, text: &str) {
            self.transaction(vec![TextChange { from: at, to: at, insert: text.into() }], false);
        }

        fn reconfigure(&mut self, read_only: bool) {
            self.read_only = read_only;
            let update =
                PasteUpdate { transactions: &[], doc: &self.doc, read_only, mode: self.mode.clone(), blurred: false };
            let effects = self.session.update(&update);
            self.run(effects);
        }

        fn publish(&mut self, items: Vec<Diagnostic>) {
            let publication = DiagnosticPublication { source: "jdtls".into(), items };
            let effects = self.session.published(Some(&publication));
            self.run(effects);
        }

        fn settle(&mut self) {
            if let Some(ticket) = self.settle.take() {
                let effects = self.session.settled(ticket, self.other_popup);
                self.run(effects);
            }
        }

        fn expire(&mut self) {
            if let Some(ticket) = self.deadline.take() {
                let effects = self.session.deadline(ticket);
                self.run(effects);
            }
        }
    }

    const PASTED: &str = "ArrayList<String> items;";

    #[test]
    fn waits_for_the_paste_to_sync_and_then_requires_a_new_diagnostic_publication() {
        let mut editor = Harness { auto_sync: false, ..Harness::new() };
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert!(editor.queries.is_empty());
        let ticket = editor.sync.take().expect("the paste waits for its sync");
        editor.session.synced(ticket);
        editor.settle();
        assert!(editor.queries.is_empty());
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert_eq!(editor.queries.len(), 1);
    }

    #[test]
    fn tracks_each_pasted_range_in_a_multi_cursor_transaction() {
        let mut editor = Harness::new();
        editor.typed(0, "old\nold");
        editor.transaction(
            vec![
                TextChange { from: 0, to: 0, insert: "ArrayList".into() },
                TextChange { from: 4, to: 4, insert: "List".into() },
            ],
            true,
        );
        editor.publish(vec![diagnostic(line(0)), diagnostic(range(1, 0, 4)), diagnostic(range(0, 9, 12))]);
        editor.settle();
        assert_eq!(editor.queries, [line(0), range(1, 0, 4)]);
    }

    #[test]
    fn ignores_paste_transactions_mirrored_from_another_editor() {
        let mut editor = Harness::new();
        let paste = is_paste_event(Some("input.paste"), true, false);
        editor.transaction(vec![TextChange { from: 0, to: 0, insert: "ArrayList".into() }], paste);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert!(editor.queries.is_empty());
    }

    #[test]
    fn requests_quick_fixes_only_after_fresh_diagnostics_and_asks_by_default() {
        let mut editor = Harness::new();
        editor.publish(vec![diagnostic(line(0))]);
        editor.paste(PASTED);
        editor.settle();
        assert!(editor.queries.is_empty());
        // A local inspection or profile update is not a fresh publication.
        let effects = editor.session.published(None);
        editor.run(effects);
        editor.settle();
        assert!(editor.queries.is_empty());
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert_eq!(editor.queries, [line(0)]);
        assert_eq!(editor.popup.as_deref(), Some(&[action().title][..]));
        assert!(editor.applied.is_empty());
        let effects = editor.session.popup_apply(&[0]);
        editor.run(effects);
        assert_eq!(json_of(&editor.applied), json_of(&[merge_pasted_imports(PATH, &[action()])]));
        assert_eq!(editor.popup, None);
    }

    #[test]
    fn always_applies_all_distinct_imports_in_one_edit() {
        let mut editor = Harness { mode: ImportsOnPaste::Always, ..Harness::new() };
        editor.paste("ArrayList List ArrayList");
        let second = CodeAction {
            title: "Import 'List' (java.util)".into(),
            edit: Some(WorkspaceEdit {
                files: vec![file(PATH, vec![import_edit("import java.util.List;\n")], None)],
                preconditions: Vec::new(),
            }),
            ..action()
        };
        editor.answers = VecDeque::from([answer(vec![action()]), answer(vec![second.clone()])]);
        editor.publish(vec![diagnostic(line(0)), diagnostic(range(0, 10, 14)), diagnostic(range(0, 15, 24))]);
        editor.settle();
        assert_eq!(json_of(&editor.applied), json_of(&[merge_pasted_imports(PATH, &[action(), second])]));
        assert_eq!(editor.popup, None);
    }

    #[test]
    fn collects_a_large_paste_within_the_deadline_and_lists_a_repeated_import_once() {
        let mut editor = Harness::new();
        editor.paste(&vec!["ArrayList"; 40].join("\n"));
        editor.publish((0..40).map(|n| diagnostic(line(n))).collect());
        editor.settle();
        assert_eq!(editor.queries.len(), 40);
        assert_eq!(editor.popup, Some(vec![action().title]));
    }

    #[test]
    fn ignores_other_input_events() {
        for event in ["input.pastePlain", "input.paste.image", "input.type"] {
            let mut editor = Harness::new();
            editor.transaction(
                vec![TextChange { from: 0, to: 0, insert: PASTED.into() }],
                is_paste_event(Some(event), false, false),
            );
            editor.publish(vec![diagnostic(line(0))]);
            editor.settle();
            assert!(editor.queries.is_empty(), "{event}");
        }
    }

    #[test]
    fn ignores_never_and_read_only_editors() {
        let never = Harness { mode: ImportsOnPaste::Never, ..Harness::new() };
        let read_only = Harness { read_only: true, ..Harness::new() };
        for mut editor in [never, read_only] {
            editor.paste(PASTED);
            editor.publish(vec![diagnostic(line(0))]);
            editor.settle();
            assert!(editor.queries.is_empty());
        }
    }

    #[test]
    fn ignores_diagnostics_outside_the_paste_and_unrelated_problems_inside_it() {
        let mut editor = Harness::new();
        editor.typed(0, "ArrayList old;\n");
        editor.paste(PASTED);
        let mismatch =
            Diagnostic { message: "Type mismatch".into(), code: Some("other".into()), ..diagnostic(line(1)) };
        editor.publish(vec![diagnostic(line(0)), mismatch]);
        editor.settle();
        assert!(editor.queries.is_empty());
    }

    #[test]
    fn resolves_lazy_fixes_and_skips_ambiguous_fixes() {
        let mut editor = Harness::new();
        editor.paste("ArrayList List");
        let lazy = CodeAction { edit: None, data: Some(json!("lazy")), ..action() };
        editor.answers = VecDeque::from([answer(vec![lazy]), answer(vec![action(), titled("Import other.List")])]);
        editor.publish(vec![diagnostic(line(0)), diagnostic(range(0, 10, 14))]);
        editor.settle();
        assert_eq!(editor.resolve_count, 1);
        assert_eq!(editor.popup, Some(vec![action().title]));
    }

    #[test]
    fn ignores_lazy_resolution_after_another_paste_and_skips_fixes_with_no_inspectable_edit() {
        let mut editor = Harness { hold_resolves: true, ..Harness::new() };
        let lazy = CodeAction { edit: None, data: Some(json!("lazy")), ..action() };
        editor.answers = VecDeque::from([answer(vec![lazy])]);
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        let (ticket, _) = editor.resolves.pop_front().expect("the lazy fix is resolved");
        editor.paste("\nArrayList");
        let effects = editor.session.resolve_answer(ticket, Ok(action()), false);
        editor.run(effects);
        assert_eq!(editor.popup, None);
        editor.hold_resolves = false;
        let command = Command { title: "Import".into(), command: "server.import".into(), arguments: Vec::new() };
        let command_only = CodeAction { edit: None, command: Some(command), ..action() };
        editor.answers = VecDeque::from([answer(vec![command_only.clone()])]);
        editor.resolved = VecDeque::from([command_only]);
        editor.publish(vec![diagnostic(line(1))]);
        editor.settle();
        assert_eq!(editor.popup, None);
        assert!(editor.applied.is_empty());
    }

    #[test]
    fn cancel_and_an_empty_selection_leave_the_pasted_code_alone() {
        let mut editor = Harness::new();
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        editor.session.popup_closed();
        editor.popup = None;
        assert!(editor.applied.is_empty());
        editor.paste("\nArrayList");
        editor.publish(vec![diagnostic(line(1))]);
        editor.settle();
        assert!(editor.popup.is_some());
        let effects = editor.session.popup_apply(&[]);
        editor.run(effects);
        assert!(editor.applied.is_empty());
    }

    #[test]
    fn cancels_on_edits_and_ignores_a_late_query_result() {
        let mut editor = Harness { answer_queries: false, ..Harness::new() };
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        let id = editor.open_queries.pop_front().expect("the fixes are asked for");
        editor.typed(0, "x");
        assert_eq!(editor.cancelled, [id]);
        let effects = editor.session.query_answer(id, answer(vec![action()]), false);
        editor.run(effects);
        assert!(editor.applied.is_empty());
        assert_eq!(editor.popup, None);
    }

    #[test]
    fn expires_both_missing_diagnostics_and_a_stalled_query() {
        let mut editor = Harness::new();
        editor.paste(PASTED);
        editor.expire();
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert!(editor.queries.is_empty());
        editor.answer_queries = false;
        editor.paste("\nArrayList");
        editor.publish(vec![diagnostic(line(1))]);
        editor.settle();
        let id = editor.open_queries.front().copied().expect("the fixes are asked for");
        editor.expire();
        assert_eq!(editor.cancelled, [id]);
        assert!(editor.applied.is_empty());
    }

    #[test]
    fn closes_stale_dialogs_on_document_edits_and_editor_destruction() {
        let mut editor = Harness::new();
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert!(editor.popup.is_some());
        editor.typed(0, "x");
        assert_eq!(editor.popup, None);
        editor.paste("\nArrayList");
        editor.publish(vec![diagnostic(line(1))]);
        editor.settle();
        assert!(editor.popup.is_some());
        let effects = editor.session.destroyed();
        editor.run(effects);
        assert_eq!(editor.popup, None);
    }

    #[test]
    fn cancels_when_reconfigured_read_only_and_handles_server_errors_silently() {
        let mut editor = Harness::new();
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        editor.reconfigure(true);
        assert_eq!(editor.popup, None);
        editor.reconfigure(false);
        editor.answers = VecDeque::from([Err(IdeError::new(ErrorKind::Internal, "server stopped"))]);
        editor.paste("\nArrayList");
        editor.publish(vec![diagnostic(line(1))]);
        editor.settle();
        assert!(editor.applied.is_empty());
        assert_eq!(editor.popup, None);
    }

    #[test]
    fn does_not_replace_another_open_popup() {
        let mut editor = Harness { other_popup: true, ..Harness::new() };
        editor.paste(PASTED);
        editor.publish(vec![diagnostic(line(0))]);
        editor.settle();
        assert_eq!(editor.popup, None);
        assert!(editor.applied.is_empty());
    }

    fn command_action(arguments: Vec<Value>) -> CodeAction {
        CodeAction {
            title: "Update import from \"./money.ts\"".into(),
            edit: None,
            command: Some(Command {
                title: String::new(),
                command: "_typescript.applyWorkspaceEdit".into(),
                arguments,
            }),
            server: ServerId(2),
            ..action()
        }
    }

    fn cart_edit() -> Value {
        json!({
            "documentChanges": [{
                "textDocument": { "uri": "file:///p/src/cart.ts", "version": 6 },
                "edits": [{
                    "range": { "start": { "line": 0, "character": 26 }, "end": { "line": 0, "character": 26 } },
                    "newText": "formatMoney, "
                }]
            }]
        })
    }

    #[test]
    fn lifts_a_commands_lsp_workspace_edit_for_the_pasted_file() {
        let lifted = with_command_edit(&command_action(vec![cart_edit()]), "/p/src/cart.ts");
        let expected = WorkspaceEdit {
            files: vec![file("/p/src/cart.ts", vec![edit("formatMoney, ", range(0, 26, 26))], Some(6))],
            preconditions: Vec::new(),
        };
        assert_eq!(lifted.edit, Some(expected));
    }

    #[test]
    fn leaves_commands_that_touch_other_files_or_carry_no_edit_alone() {
        let edits = cart_edit()["documentChanges"][0]["edits"].clone();
        let other = json!({ "changes": { "file:///p/src/money.ts": edits } });
        assert_eq!(with_command_edit(&command_action(vec![other]), "/p/src/cart.ts").edit, None);
        assert_eq!(with_command_edit(&command_action(vec![json!("not json")]), "/p/src/cart.ts").edit, None);
    }
}
