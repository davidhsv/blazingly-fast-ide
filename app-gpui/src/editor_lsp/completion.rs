//! Code completion: when to ask the language server, which of its items the lookup lists, and
//! what accepting one does to the document.
//!
//! ↵ inserts over the typed prefix, ⇥ also replaces the rest of the identifier. Item ranges refer
//! to the document as it was when completion was asked for; text typed since then is taken into
//! account. Accepting returns the changes and the selection, plus follow-ups for the caller:
//! signature help for a call left open, a server command, a resolve for edits a server computes
//! late.

use super::availability::FeatureBackoff;
use super::completion_items::{
    LookupEntry, construct_documentation, identifier_end, is_word_char, lookup_entry, trigger_character,
    without_duplicates,
};
use super::file_references::{path_context, path_like};
use super::lsp::{Inflight, is_unavailable};
use super::markdown::is_empty_markup;
use super::positions::{apply_changes, pos_to_offset, text_edits_to_changes};
use super::snippet::{SnippetClock, lsp_snippet_to_template, snippet_variables};
use crate::editor::buffer::{self, Assoc};
use crate::find::utf16;
use crate::platform::collate::{self, Sensitivity};
use crate::templates::snippet::{ActiveSnippet, apply_snippet};
use ide_core::{DocId, IdeError, RequestId, TextChange};
use ide_lsp::types::{Command, CompletionItem, CompletionKind, CompletionList, Markup};
use regex::Regex;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// The document a completion was asked for in; the ranges of its items refer to this text.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub doc: DocId,
    pub path: String,
    pub language: Option<String>,
    /// The caret when completion was asked for.
    pub pos: u32,
    pub text: ropey::Rope,
}

/// A language-server item in the lookup.
#[derive(Debug, Clone)]
pub struct LspCompletion {
    pub item: CompletionItem,
    pub entry: LookupEntry,
    pub req: Arc<CompletionRequest>,
    /// The item once `completionItem/resolve` answered (documentation, late edits).
    pub resolved: Option<CompletionItem>,
}

/// The lookup row of `item`. A keyword the server leaves undocumented gets the editor's own
/// documentation of the language construct; commit characters are the lookup's business (see
/// [`commits_lookup`]), so the row carries none.
pub fn make_completion(item: CompletionItem, req: &Arc<CompletionRequest>) -> LspCompletion {
    let mut item = item;
    if item.kind == Some(CompletionKind::Keyword)
        && is_empty_markup(item.documentation.as_ref())
        && let Some(value) = construct_documentation(req.language.as_deref(), &item.label)
    {
        item.documentation = Some(Markup { kind: "markdown".into(), value });
    }
    let mut entry = lookup_entry(&item);
    entry.commit_characters.clear();
    LspCompletion { item, entry, req: Arc::clone(req), resolved: None }
}

// ---------------------------------------------------------------------------
// Asking

static WORD_BEFORE: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\p{L}\p{N}_$]+$"));

/// What a completion request asks at the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionQuery {
    /// Where the typed word starts (the caret without one): what the lookup replaces.
    pub from: u32,
    /// The word typed before the caret.
    pub typed: String,
    pub trigger: Option<char>,
}

/// Whether to ask at all, given the text of the line before the caret (at document offset
/// `pos`). Typing asks after a word character or a trigger character, but not inside a number.
pub fn completion_query(before: &str, pos: u32, explicit: bool) -> Option<CompletionQuery> {
    let word = WORD_BEFORE.find(before).map(|m| m.as_str()).unwrap_or_default();
    let tail = utf16::slice_from(before, utf16::len(before).saturating_sub(2));
    let trigger = trigger_character(tail);
    if !explicit && word.is_empty() && trigger.is_none() {
        return None;
    }
    if !explicit && trigger.is_none() && word.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(CompletionQuery { from: pos - utf16::len(word), typed: word.to_owned(), trigger })
}

/// What the source does now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The server could not complete here a moment ago: use the language's own words.
    Fallback,
    /// Send `id`; `cancel` is the request it supersedes.
    Request { id: RequestId, cancel: Option<RequestId> },
}

/// What came of an answer.
#[derive(Debug, Clone)]
pub enum Answered {
    /// The answer of a superseded or abandoned request.
    Stale,
    /// No server, or nothing from it: the language's own words.
    Fallback,
    /// Failed otherwise: no list.
    Nothing,
    List(CompletionList),
}

/// The completion requests of one editor: one in flight at a time, and a back-off for documents
/// without a server.
#[derive(Debug, Clone, Default)]
pub struct CompletionSource {
    inflight: Inflight,
    backoff: FeatureBackoff,
}

impl CompletionSource {
    pub fn ask(&mut self, doc: DocId, id: RequestId, now: u64) -> Ask {
        if self.backoff.active(doc, now) {
            return Ask::Fallback;
        }
        Ask::Request { id, cancel: self.inflight.begin(id) }
    }

    /// The lookup closed or the caret moved away; returns the request to cancel.
    pub fn abort(&mut self) -> Option<RequestId> {
        self.inflight.cancel()
    }

    pub fn answer(
        &mut self,
        doc: DocId,
        id: RequestId,
        answer: Result<CompletionList, IdeError>,
        now: u64,
    ) -> Answered {
        if !self.inflight.finish(id) {
            return Answered::Stale;
        }
        match answer {
            Err(error) if is_unavailable(&error) => {
                self.backoff.mark(doc, now);
                Answered::Fallback
            }
            Err(_) => Answered::Nothing,
            Ok(list) if list.items.is_empty() => Answered::Fallback,
            Ok(list) => Answered::List(list),
        }
    }
}

/// Settings › Code Completion › Match case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCase {
    Off,
    FirstLetter,
    AllLetters,
}

/// Code Completion › Match case: the first letter only, or every typed letter (as a subsequence).
pub fn matches_case(label: &str, typed: &str, all_letters: bool) -> bool {
    let Some(first) = typed.chars().next() else { return true };
    if !all_letters {
        let Some(head) = label.chars().next() else { return true };
        return !head.to_lowercase().eq(first.to_lowercase()) || head == first;
    }
    let mut wanted = typed.chars().peekable();
    for c in label.chars() {
        if wanted.peek() == Some(&c) {
            wanted.next();
        }
    }
    wanted.peek().is_none()
}

/// The lookup rows of a server's answer: filtered by the match-case setting, identical items
/// once, and a server snippet named like one of the editor's live templates left out (the
/// template wins).
pub fn completion_options(
    list: CompletionList,
    req: &Arc<CompletionRequest>,
    typed: &str,
    match_case: MatchCase,
    template_names: &[String],
) -> Vec<LspCompletion> {
    let items = match match_case {
        MatchCase::Off => list.items,
        MatchCase::FirstLetter | MatchCase::AllLetters => list
            .items
            .into_iter()
            .filter(|item| {
                matches_case(
                    item.filter_text.as_deref().unwrap_or(&item.label),
                    typed,
                    match_case == MatchCase::AllLetters,
                )
            })
            .collect(),
    };
    without_duplicates(items)
        .into_iter()
        .filter(|item| item.kind != Some(CompletionKind::Snippet) || !template_names.contains(&item.label))
        .map(|item| make_completion(item, req))
        .collect()
}

/// Whether the lookup re-filters the same rows on typing (`true`), or asks again on every
/// keystroke: an incomplete list, and "all letters" matching that re-checks case.
pub fn reuses_rows(list_incomplete: bool, match_case: MatchCase) -> bool {
    !list_incomplete && match_case != MatchCase::AllLetters
}

/// Whether text typed since the rows came keeps them: a word.
pub fn rows_valid_for(typed: &str) -> bool {
    typed.chars().all(is_word_char)
}

// ---------------------------------------------------------------------------
// Resolve and documentation

/// Whether an item's documentation or edits come from `completionItem/resolve`.
pub fn needs_resolve(item: &CompletionItem) -> bool {
    item.data.is_some() || is_empty_markup(item.documentation.as_ref())
}

/// Whether to resolve the selected item ahead of acceptance: a server may compute its extra edits
/// (an import) only then, against the text as it was.
pub fn prefetches_resolve(item: &CompletionItem) -> bool {
    item.data.is_some() && item.additional_edits.is_empty()
}

/// The resolved item, keeping the list's documentation when the resolve has none.
pub fn merge_resolved(original: &CompletionItem, resolved: CompletionItem) -> CompletionItem {
    if is_empty_markup(resolved.documentation.as_ref()) && !is_empty_markup(original.documentation.as_ref()) {
        CompletionItem { documentation: original.documentation.clone(), ..resolved }
    } else {
        resolved
    }
}

/// Documentation of a lookup item as markup: the detail as a code block, then the docs.
pub fn lookup_doc(item: &CompletionItem, language: Option<&str>) -> Option<Markup> {
    let mut parts = Vec::new();
    if let Some(detail) = item.detail.as_deref().filter(|detail| !detail.is_empty()) {
        parts.push(format!("```{}\n{detail}\n```", language.unwrap_or_default()));
    }
    if let Some(documentation) = item.documentation.as_ref().filter(|doc| !is_empty_markup(Some(doc))) {
        parts.push(documentation.value.clone());
    }
    (!parts.is_empty()).then(|| Markup { kind: "markdown".into(), value: parts.join("\n\n") })
}

/// What F1 in an open lookup does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupDocs {
    /// The docs now follow the selection beside the lookup.
    Shown,
    /// They already show: open them focused instead.
    Showing,
    /// No lookup is open.
    None,
}

// ---------------------------------------------------------------------------
// Accepting

const CALLABLE: [CompletionKind; 3] = [CompletionKind::Function, CompletionKind::Method, CompletionKind::Constructor];

fn is_callable(kind: Option<CompletionKind>) -> bool {
    kind.is_some_and(|kind| CALLABLE.contains(&kind))
}

/// Where Insert parentheses automatically leaves the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallParens {
    /// Between them: the call takes arguments.
    Inside,
    /// After them: the signature shows no parameters.
    After,
}

/// `()` after a function or method name inserted as plain text (snippets bring their own), unless
/// the name is followed by `(` or an identifier, or names an import. `line_text` is the line of
/// `start` and `end`, which starts at document offset `line_from`.
pub fn call_parens(item: &CompletionItem, line_text: &str, line_from: u32, start: u32, end: u32) -> Option<CallParens> {
    static OPENS: LazyLock<Regex> = LazyLock::new(|| pattern(r"[(<\[{]"));
    static IMPORT_LINE: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^\s*(import|from|use|pub\s+use|export\s+\{|#include|require)\b"));
    static NO_PARAMS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*\(\s*\)|^\s*fn\s*\(\s*\)|^\s*\(\)\s*(=>|->|:)"));
    if item.is_snippet || !is_callable(item.kind) || OPENS.is_match(&item.insert_text) {
        return None;
    }
    let after = utf16::slice_from(line_text, end.saturating_sub(line_from));
    if after.chars().next().is_some_and(|next| next == '(' || is_word_char(next) || "'\"`".contains(next)) {
        return None;
    }
    if IMPORT_LINE.is_match(utf16::slice(line_text, 0, start.saturating_sub(line_from))) {
        return None;
    }
    let signature = item.label_detail.as_deref().or(item.detail.as_deref()).unwrap_or_default();
    Some(if NO_PARAMS.is_match(signature) { CallParens::After } else { CallParens::Inside })
}

/// ↵ inserts; ⇥ also overwrites the rest of the identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    Insert,
    Replace,
}

/// How accepting behaves.
#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions<'a> {
    pub mode: ApplyMode,
    /// Code Completion › Insert parentheses automatically.
    pub insert_parentheses: bool,
    /// What a leading tab of a snippet line becomes.
    pub indent_unit: &'a str,
    /// The time snippet variables read.
    pub clock: SnippetClock,
}

/// What the caller does after an item went in.
#[derive(Debug, Clone)]
pub enum FollowUp {
    /// Show parameter info for the call just opened.
    SignatureHelp,
    /// A call snippet without stops left the caret between its parentheses at this offset: show
    /// parameter info, or move past `)` when nothing takes arguments.
    ParameterInfoOrLeave(u32),
    /// Open completion again.
    Suggest,
    /// Run a server command (an auto-import, a postfix template).
    ExecuteCommand(Command),
    /// Resolve the item, then apply its edits before the insertion at `inserted_at` with
    /// [`late_edits`].
    ResolveLateEdits { inserted_at: u32 },
}

/// The document change of an accepted item.
#[derive(Debug, Clone)]
pub struct AppliedCompletion {
    /// Sorted, non-overlapping, in the coordinates of the document before them.
    pub changes: Vec<TextChange>,
    /// The selection afterwards: the caret, or every occurrence of a snippet's first field.
    pub selection: Vec<Range<u32>>,
    /// A snippet's fields to walk with Tab.
    pub snippet: Option<ActiveSnippet>,
    pub follow_ups: Vec<FollowUp>,
}

fn sorted(mut changes: Vec<TextChange>) -> Vec<TextChange> {
    changes.sort_by_key(|change| change.from);
    changes
}

/// Accepts `completion` over `from..to` of `doc` (the document now; `to` is the caret).
#[expect(clippy::single_range_in_vec_init, reason = "a caret is a selection of one range")]
pub fn apply_lsp_completion(
    completion: &LspCompletion,
    doc: &ropey::Rope,
    from: u32,
    to: u32,
    options: ApplyOptions<'_>,
) -> AppliedCompletion {
    let req = &completion.req;
    let item = completion.resolved.as_ref().unwrap_or(&completion.item);
    let len = i64::from(buffer::len_utf16(doc));
    let delta = i64::from(to) - i64::from(req.pos);
    // An offset of the request's document in the current one: text typed since went in at the caret.
    let map_old = |offset: u32, before: bool| -> u32 {
        let moved = if offset < req.pos || (offset == req.pos && before) {
            i64::from(offset)
        } else {
            i64::from(offset) + delta
        };
        moved.clamp(0, len) as u32
    };

    let (mut start, mut end) = (from, to);
    if let Some(range) = item.replace_range {
        // An edit requested just after a trigger must include the prefix typed since then.
        start = map_old(pos_to_offset(&req.text, range.start), true).min(to);
        end = map_old(pos_to_offset(&req.text, range.end), false).max(to);
    }
    if options.mode == ApplyMode::Replace {
        if let Some(tab) = item.replace_range_tab {
            end = end.max(map_old(pos_to_offset(&req.text, tab.end), false));
        } else {
            let (line, col) = buffer::line_col(doc, end);
            end = buffer::line_range(doc, line).start + identifier_end(&buffer::line_text(doc, line), col);
        }
    }

    let extra: Vec<TextChange> = text_edits_to_changes(&req.text, &item.additional_edits)
        .into_iter()
        .map(|change| TextChange { from: map_old(change.from, false), to: map_old(change.to, false), ..change })
        .filter(|change| change.to <= start || change.from >= end)
        .collect();

    let mut follow_ups = Vec::new();
    let mut parens = None;
    let mut bare_call = None;
    let (changes, selection, snippet) = if item.is_snippet {
        let after_extra = apply_changes(doc, &extra);
        let (s, e) = (buffer::map_offset(start, &extra, Assoc::After), buffer::map_offset(end, &extra, Assoc::After));
        let template = lsp_snippet_to_template(&item.insert_text, &snippet_variables(&req.path, options.clock, &[]));
        let insertion = apply_snippet(&template, &after_extra, e..e, s, e, options.indent_unit);
        let mut selection = insertion.selection;
        if selection.is_empty() {
            let caret = s + utf16::len(&insertion.insert);
            // A call snippet without a stop (`ArrayList<>()`): the caret goes inside until
            // signature help says there is nothing to pass.
            let caret = if options.insert_parentheses
                && is_callable(item.kind)
                && item.insert_text.ends_with("()")
                && !item.insert_text.contains('$')
                && insertion.insert.ends_with("()")
            {
                bare_call = Some(caret - 1);
                caret - 1
            } else {
                caret
            };
            selection = vec![caret..caret];
        }
        let mut changes = extra;
        changes.push(TextChange { from: start, to: end, insert: insertion.insert });
        (sorted(changes), selection, insertion.active)
    } else {
        if options.insert_parentheses {
            let (line, _) = buffer::line_col(doc, start);
            parens = call_parens(item, &buffer::line_text(doc, line), buffer::line_range(doc, line).start, start, end);
        }
        let insert = if parens.is_some() { format!("{}()", item.insert_text) } else { item.insert_text.clone() };
        let mut changes = extra;
        changes.push(TextChange { from: start, to: end, insert });
        let changes = sorted(changes);
        let caret = buffer::map_offset(end, &changes, Assoc::After) - u32::from(parens == Some(CallParens::Inside));
        (changes, vec![caret..caret], None)
    };

    if completion.resolved.is_none() && item.additional_edits.is_empty() && item.data.is_some() {
        follow_ups.push(FollowUp::ResolveLateEdits { inserted_at: start });
    }
    static CALL_SNIPPET: LazyLock<Regex> = LazyLock::new(|| pattern(r"\(\$\{?[0-9]"));
    let call_snippet = item.is_snippet && is_callable(item.kind) && CALL_SNIPPET.is_match(&item.insert_text);
    // Unless the item's command asks for it itself.
    let command = item.command.as_ref().map(|command| command.command.as_str());
    let hints_command = command == Some(TRIGGER_PARAMETER_HINTS);
    if (parens == Some(CallParens::Inside) || call_snippet) && !hints_command {
        follow_ups.push(FollowUp::SignatureHelp);
    } else if let Some(at) = bare_call {
        follow_ups.push(FollowUp::ParameterInfoOrLeave(at));
    }
    follow_ups.extend(command_follow_up(item.command.as_ref()));
    AppliedCompletion { changes, selection, snippet, follow_ups }
}

const TRIGGER_PARAMETER_HINTS: &str = "editor.action.triggerParameterHints";
const TRIGGER_SUGGEST: &str = "editor.action.triggerSuggest";

/// What an item's command asks for: the editor handles the client commands servers commonly
/// send, and other `editor.` commands are ignored.
fn command_follow_up(command: Option<&Command>) -> Option<FollowUp> {
    let command = command.filter(|command| !command.command.is_empty())?;
    match command.command.as_str() {
        TRIGGER_PARAMETER_HINTS => Some(FollowUp::SignatureHelp),
        TRIGGER_SUGGEST => Some(FollowUp::Suggest),
        name if name.starts_with("editor.") => None,
        _ => Some(FollowUp::ExecuteCommand(command.clone())),
    }
}

/// The edits a resolve computed late (an auto-import), for the part of the document before the
/// insertion; nothing once that part changed since completion was asked for.
pub fn late_edits(
    completion: &LspCompletion,
    resolved: &CompletionItem,
    doc: &ropey::Rope,
    inserted_at: u32,
) -> Vec<TextChange> {
    let req = &completion.req;
    let limit_at = inserted_at.min(req.pos);
    let edits: Vec<TextChange> = text_edits_to_changes(&req.text, &resolved.additional_edits)
        .into_iter()
        .filter(|change| change.to <= limit_at)
        .collect();
    let Some(limit) = edits.iter().map(|change| change.to).max() else { return Vec::new() };
    if buffer::slice(doc, 0, limit) != buffer::slice(&req.text, 0, limit) {
        return Vec::new();
    }
    edits
}

/// How another feature's lookup row goes in (live templates, postfix, words).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContributedApply {
    Text(String),
    /// An editor snippet template.
    Template(String),
}

/// Accepts a contributed row over `from..to` of `doc`.
#[expect(clippy::single_range_in_vec_init, reason = "a caret is a selection of one range")]
pub fn apply_contributed(
    doc: &ropey::Rope,
    from: u32,
    to: u32,
    apply: &ContributedApply,
    indent_unit: &str,
) -> AppliedCompletion {
    match apply {
        ContributedApply::Text(text) => {
            let caret = from + utf16::len(text);
            AppliedCompletion {
                changes: vec![TextChange { from, to, insert: text.clone() }],
                selection: vec![caret..caret],
                snippet: None,
                follow_ups: Vec::new(),
            }
        }
        ContributedApply::Template(template) => {
            let insertion = apply_snippet(template, doc, to..to, from, to, indent_unit);
            let caret = from + utf16::len(&insertion.insert);
            let selection =
                if insertion.selection.is_empty() { vec![caret..caret] } else { insertion.selection.clone() };
            AppliedCompletion {
                changes: vec![insertion.change()],
                selection,
                snippet: insertion.active,
                follow_ups: Vec::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The lookup

/// What the lookup filters on and shows of a row.
pub trait LookupLabel {
    fn label(&self) -> &str;
    fn display_label(&self) -> Option<&str>;
}

impl LookupLabel for LookupEntry {
    fn label(&self) -> &str {
        &self.label
    }
    fn display_label(&self) -> Option<&str> {
        self.display_label.as_deref()
    }
}

impl LookupLabel for LspCompletion {
    fn label(&self) -> &str {
        &self.entry.label
    }
    fn display_label(&self) -> Option<&str> {
        self.entry.display_label.as_deref()
    }
}

/// The matched ranges of `typed` in `label` as UTF-16 `[from, to, from, to, …]`, ignoring case:
/// a substring, else a subsequence; `None` without a match.
pub fn match_ranges(label: &str, typed: &str) -> Option<Vec<u32>> {
    if typed.is_empty() {
        return Some(Vec::new());
    }
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let mut offsets = Vec::new();
    let mut at = 0;
    for c in label.chars() {
        offsets.push((at, c.len_utf16() as u32, lower(c)));
        at += c.len_utf16() as u32;
    }
    let wanted: Vec<char> = typed.chars().map(lower).collect();
    if let Some(start) =
        offsets.windows(wanted.len()).position(|window| window.iter().zip(&wanted).all(|((_, _, c), w)| c == w))
    {
        let (from, _, _) = offsets[start];
        let (last, width, _) = offsets[start + wanted.len() - 1];
        return Some(vec![from, last + width]);
    }
    let mut ranges: Vec<u32> = Vec::new();
    let mut next = wanted.iter().peekable();
    for (from, width, c) in offsets {
        if next.peek() != Some(&&c) {
            continue;
        }
        next.next();
        match ranges.last_mut() {
            Some(end) if *end == from => *end = from + width,
            _ => ranges.extend([from, from + width]),
        }
    }
    next.peek().is_none().then_some(ranges)
}

/// Code Completion › Sort suggestions alphabetically: the rows matching `typed` (substring, then
/// subsequence), by name. Each is the index of a row and the ranges to highlight (none for a row
/// shown under another name than it matched).
pub fn sort_alphabetically<T: LookupLabel>(rows: &[T], typed: &str) -> Vec<(usize, Vec<u32>)> {
    let mut matched: Vec<(usize, Vec<u32>)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let ranges = match_ranges(row.label(), typed)?;
            Some((i, if row.display_label().is_some() { Vec::new() } else { ranges }))
        })
        .collect();
    let name = |i: usize| rows[i].display_label().unwrap_or(rows[i].label());
    matched.sort_by(|(a, _), (b, _)| {
        collate::compare_with(name(*a), name(*b), false, Sensitivity::Base)
            .then_with(|| collate::compare(name(*a), name(*b)))
    });
    matched
}

/// The characters that finish a focused lookup with its selected item before being typed.
pub const COMMIT_CHARS: [char; 7] = ['.', ',', ';', '=', ' ', ':', '('];

/// Whether typing `ch` finishes the open lookup with its selected item: a focused lookup (opened
/// explicitly or navigated) always, an automatic one only with "Insert selected suggestion by
/// pressing space, dot…" on.
pub fn commits_lookup(ch: char, focused: bool, insert_on_special_keys: bool, item_chars: &[String]) -> bool {
    if !focused && !insert_on_special_keys {
        return false;
    }
    COMMIT_CHARS.contains(&ch) || item_chars.iter().any(|item| item.chars().eq(std::iter::once(ch)))
}

/// The state of the lookup, as the editor reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupStatus {
    /// The sources are still answering.
    Pending,
    Active,
}

/// How long a sole candidate of an explicit completion waits before it goes in.
pub const AUTO_INSERT_DELAY_MS: u64 = 80;
/// How long "No suggestions" shows.
pub const NO_SUGGESTIONS_TIMEOUT_MS: u64 = 2000;
pub const NO_SUGGESTIONS: &str = "No suggestions";

/// What the caller does after the lookup changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupEvent {
    None,
    /// An explicit completion found nothing: show [`NO_SUGGESTIONS`] at the caret.
    NoSuggestions,
    /// An explicit completion found one candidate: after [`AUTO_INSERT_DELAY_MS`], accept it if
    /// [`auto_insert_due`] still holds.
    AutoInsert,
}

/// The lookup of one editor: whether the user engaged with it, and explicit invocations waiting
/// for their first result.
#[derive(Debug, Clone, Default)]
pub struct LookupSession {
    focused: bool,
    explicit_pending: bool,
    docs_requested: bool,
    status: Option<LookupStatus>,
}

impl LookupSession {
    /// ⌃Space; `started` is whether completion could start. An explicit lookup is focused.
    pub fn explicit(&mut self, started: bool) {
        self.explicit_pending = started;
        self.focused |= started;
    }

    /// ↑, ↓, Page Up or Page Down moved in the lookup.
    pub fn navigated(&mut self) {
        if self.status == Some(LookupStatus::Active) {
            self.focused = true;
        }
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// The lookup changed status. `options` is the number of rows once active, `doc_changed`
    /// whether the update that closed it edited the document, `auto_insert_single` the setting
    /// Automatically insert single suggestions.
    pub fn status(
        &mut self,
        status: Option<LookupStatus>,
        options: usize,
        doc_changed: bool,
        auto_insert_single: bool,
    ) -> LookupEvent {
        let previous = std::mem::replace(&mut self.status, status);
        match status {
            None => {
                self.focused = false;
                self.docs_requested = false;
                let pending = std::mem::take(&mut self.explicit_pending);
                if pending && previous == Some(LookupStatus::Pending) && !doc_changed {
                    LookupEvent::NoSuggestions
                } else {
                    LookupEvent::None
                }
            }
            Some(LookupStatus::Active) if self.explicit_pending => {
                self.explicit_pending = false;
                if options == 1 && auto_insert_single { LookupEvent::AutoInsert } else { LookupEvent::None }
            }
            Some(_) => LookupEvent::None,
        }
    }

    /// F1 with the lookup open; `show_documentation` is the setting that shows the docs anyway.
    pub fn show_docs(&mut self, has_selection: bool, show_documentation: bool) -> LookupDocs {
        if self.status != Some(LookupStatus::Active) || !has_selection {
            return LookupDocs::None;
        }
        if self.docs_requested || show_documentation {
            return LookupDocs::Showing;
        }
        self.docs_requested = true;
        LookupDocs::Shown
    }

    /// Whether the selected row's docs show beside the lookup.
    pub fn docs_visible(&self, show_documentation: bool) -> bool {
        show_documentation || self.docs_requested
    }
}

/// Whether the sole candidate still goes in once the delay passed: the document is unchanged and
/// the lookup still shows that one candidate alone.
pub fn auto_insert_due(doc_unchanged: bool, still_active: bool, same_sole_candidate: bool) -> bool {
    doc_unchanged && still_active && same_sole_candidate
}

/// Languages whose strings name config keys as often as files: paths only once typed like one.
const PATH_ONLY_WHEN_PATH_LIKE: [&str; 2] = ["php", "blade"];

/// Whether file paths are offered inside the string at the end of `before`.
pub fn offers_paths(language: &str, before: &str) -> bool {
    if !PATH_ONLY_WHEN_PATH_LIKE.contains(&language) {
        return true;
    }
    path_context(before, language).is_some_and(|found| path_like(&found.prefix))
}

#[cfg(test)]
mod tests {
    use super::super::completion_items::testing::item;
    use super::*;
    use ide_core::{Pos, Range as LspRange, TextEdit};
    use ropey::Rope;

    fn options(mode: ApplyMode) -> ApplyOptions<'static> {
        ApplyOptions { mode, insert_parentheses: true, indent_unit: "  ", clock: SnippetClock::default() }
    }

    fn range(line: u32, from: u32, to: u32) -> LspRange {
        LspRange::new(Pos::new(line, from), Pos::new(line, to))
    }

    /// `|` marks the caret where completion was asked for; `typed` is typed afterwards. Returns
    /// the document now, the request and the caret of the request.
    fn setup(text: &str, typed: &str) -> (Rope, Arc<CompletionRequest>, u32) {
        let at = text.find('|').expect("the text marks the caret");
        let pos = utf16::len(&text[..at]);
        let doc = text.replacen('|', "", 1);
        let req = Arc::new(CompletionRequest {
            doc: DocId(1),
            path: "/p/a.ts".into(),
            language: None,
            pos,
            text: Rope::from_str(&doc),
        });
        let mut now = Rope::from_str(&doc);
        now.insert(now.utf16_cu_to_char(pos as usize), typed);
        (now, req, pos)
    }

    fn php_request() -> Arc<CompletionRequest> {
        Arc::new(CompletionRequest {
            doc: DocId(1),
            path: "/p/a.php".into(),
            language: Some("PHP".into()),
            pos: 3,
            text: Rope::from_str("emp"),
        })
    }

    /// The text after `applied`, and the head of its main selection.
    fn after(doc: &Rope, applied: &AppliedCompletion) -> (String, u32) {
        (apply_changes(doc, &applied.changes).to_string(), applied.selection[0].end)
    }

    /// Accepts `item` with ↵ over `from` up to the caret (the end of what was typed).
    fn accept(doc: &Rope, req: &Arc<CompletionRequest>, item: CompletionItem, from: u32, to: u32) -> (String, u32) {
        let applied = apply_lsp_completion(&make_completion(item, req), doc, from, to, options(ApplyMode::Insert));
        after(doc, &applied)
    }

    #[test]
    fn documents_a_keyword_without_changing_its_insertion() {
        let keyword = CompletionItem {
            label: "empty".into(),
            kind: Some(CompletionKind::Keyword),
            insert_text: "empty".into(),
            ..item()
        };
        let completion = make_completion(keyword, &php_request());
        let documentation = completion.item.documentation.map(|doc| doc.value).unwrap_or_default();
        assert!(documentation.contains("empty(mixed $var): bool"));
        assert_eq!(completion.item.insert_text, "empty");
    }

    #[test]
    fn preserves_server_documentation_and_leaves_methods_and_other_languages_alone() {
        let keyword = CompletionItem { label: "empty".into(), kind: Some(CompletionKind::Keyword), ..item() };
        let server_docs = Markup { kind: "markdown".into(), value: "Server documentation".into() };
        let documented =
            make_completion(CompletionItem { documentation: Some(server_docs), ..keyword.clone() }, &php_request());
        assert_eq!(documented.item.documentation.map(|doc| doc.value).as_deref(), Some("Server documentation"));
        let method = CompletionItem { label: "empty".into(), ..item() };
        assert!(make_completion(method, &php_request()).item.documentation.is_none());
        let javascript =
            Arc::new(CompletionRequest { language: Some("JavaScript".into()), ..(*php_request()).clone() });
        assert!(make_completion(keyword, &javascript).item.documentation.is_none());
    }

    #[test]
    fn inserts_over_the_typed_prefix_and_keeps_text_after_the_caret() {
        let (doc, req, _) = setup("service.cr|Order()", "");
        assert_eq!(accept(&doc, &req, item(), 8, 10), ("service.createOrder()".into(), 14));
    }

    #[test]
    fn keeps_overloads_sharing_a_filter_text_apart() {
        let (_, req, _) = setup("x.ti|", "");
        let [a, b] = ["times(other: Int)", "times(other: Long)"].map(|label| {
            make_completion(CompletionItem { label: label.into(), filter_text: Some("times".into()), ..item() }, &req)
        });
        assert_eq!([a.entry.label.as_str(), b.entry.label.as_str()], ["times", "times"]);
        assert_ne!(a.entry.display_label, b.entry.display_label);
    }

    #[test]
    fn replaces_the_rest_of_the_identifier_with_tab() {
        let (doc, req, _) = setup("service.cr|Order()", "");
        let applied = apply_lsp_completion(&make_completion(item(), &req), &doc, 8, 10, options(ApplyMode::Replace));
        assert_eq!(after(&doc, &applied).0, "service.create()");
    }

    #[test]
    fn leaves_the_caret_inside_a_call_snippet_without_stops_past_the_parenthesis_once_nothing_takes_arguments() {
        let (doc, req, _) = setup("new Arr|", "");
        let constructor = CompletionItem {
            kind: Some(CompletionKind::Constructor),
            insert_text: "ArrayList<>()".into(),
            is_snippet: true,
            ..item()
        };
        let applied = apply_lsp_completion(&make_completion(constructor, &req), &doc, 4, 7, options(ApplyMode::Insert));
        assert_eq!(after(&doc, &applied), ("new ArrayList<>()".into(), 16));
        // Parameter info decides whether the caret moves past `)`.
        assert!(matches!(applied.follow_ups.as_slice(), [FollowUp::ParameterInfoOrLeave(16)]));
    }

    #[test]
    fn uses_the_servers_tab_range_when_given() {
        let (doc, req, _) = setup("a.cr|Order.x", "");
        let field = CompletionItem {
            kind: Some(CompletionKind::Field),
            replace_range: Some(range(0, 2, 4)),
            replace_range_tab: Some(range(0, 2, 9)),
            ..item()
        };
        let applied = apply_lsp_completion(&make_completion(field, &req), &doc, 2, 4, options(ApplyMode::Replace));
        assert_eq!(after(&doc, &applied).0, "a.create.x");
    }

    #[test]
    fn honours_the_servers_replace_range_shifted_by_text_typed_since_the_request() {
        let (doc, req, _) = setup("x.c|", "re");
        let call = CompletionItem { insert_text: "create()".into(), replace_range: Some(range(0, 2, 3)), ..item() };
        assert_eq!(accept(&doc, &req, call, 2, 5).0, "x.create()");
    }

    #[test]
    fn replaces_a_mixed_case_prefix_typed_after_a_request() {
        for mode in [ApplyMode::Insert, ApplyMode::Replace] {
            for shape in ["textEdit", "insertReplace", "noEdit"] {
                for is_snippet in [false, true] {
                    let (doc, req, pos) = setup("/* 😀 */ System.out.|Suffix", "Print");
                    let empty = range(0, pos, pos);
                    let print = CompletionItem {
                        label: "print".into(),
                        filter_text: Some("print(Object obj) : void".into()),
                        insert_text: (if is_snippet { "print(${0})" } else { "print()" }).into(),
                        is_snippet,
                        replace_range: (shape != "noEdit").then_some(empty),
                        replace_range_tab: (shape == "insertReplace").then_some(range(0, pos, pos + 6)),
                        ..item()
                    };
                    let applied =
                        apply_lsp_completion(&make_completion(print, &req), &doc, pos, pos + 5, options(mode));
                    let suffix = if mode == ApplyMode::Insert { "Suffix" } else { "" };
                    assert_eq!(
                        after(&doc, &applied).0,
                        format!("/* 😀 */ System.out.print(){suffix}"),
                        "{mode:?} {shape} snippet: {is_snippet}"
                    );
                }
            }
        }
    }

    #[test]
    fn accepts_the_fresh_jdt_ls_print_response_range() {
        for mode in [ApplyMode::Insert, ApplyMode::Replace] {
            let (doc, req, _) = setup("        System.out.Print|", "");
            let print = CompletionItem {
                label: "print".into(),
                filter_text: Some("print(Object obj) : void".into()),
                insert_text: "print(${0});".into(),
                is_snippet: true,
                replace_range: Some(range(0, 19, 24)),
                ..item()
            };
            let applied = apply_lsp_completion(&make_completion(print, &req), &doc, 19, 24, options(mode));
            assert_eq!(after(&doc, &applied).0, "        System.out.print();", "{mode:?}");
        }
    }

    #[test]
    fn accepts_a_cached_lookup_requested_before_typing_print() {
        for mode in [ApplyMode::Insert, ApplyMode::Replace] {
            let (doc, req, pos) = setup("System.out.|Suffix", "Print");
            let print = CompletionItem {
                label: "print".into(),
                filter_text: Some("print(Object obj) : void".into()),
                insert_text: "print(${0})".into(),
                is_snippet: true,
                replace_range: Some(range(0, pos, pos)),
                ..item()
            };
            // The rows stay valid for the word typed since: the source is not asked again.
            assert!(reuses_rows(false, MatchCase::Off) && rows_valid_for("Print"));
            let applied = apply_lsp_completion(&make_completion(print, &req), &doc, pos, pos + 5, options(mode));
            let suffix = if mode == ApplyMode::Insert { "Suffix" } else { "" };
            assert_eq!(after(&doc, &applied).0, format!("System.out.print(){suffix}"), "{mode:?}");
        }
    }

    #[test]
    fn replaces_mixed_case_prefixes_in_contributed_completions() {
        for (apply, expected) in [
            (ContributedApply::Text("print".into()), "print"),
            (ContributedApply::Template("print(#{})".into()), "print()"),
        ] {
            let doc = Rope::from_str("Print");
            let applied = apply_contributed(&doc, 0, 5, &apply, "  ");
            assert_eq!(after(&doc, &applied).0, expected);
        }
    }

    #[test]
    fn applies_additional_edits_imports_in_the_same_change() {
        let (doc, req, _) = setup("\nfoo.ba|", "");
        let field = CompletionItem {
            kind: Some(CompletionKind::Field),
            insert_text: "bar".into(),
            additional_edits: vec![TextEdit { range: range(0, 0, 0), new_text: "import bar;".into() }],
            ..item()
        };
        assert_eq!(accept(&doc, &req, field, 5, 7).0, "import bar;\nfoo.bar");
    }

    #[test]
    fn adds_parentheses_to_a_called_function_and_opens_its_parameters() {
        let (doc, req, _) = setup("svc.cr|", "");
        let call = CompletionItem { label_detail: Some("(req: Req)".into()), ..item() };
        assert_eq!(accept(&doc, &req, call, 4, 6), ("svc.create()".into(), 11));
    }

    #[test]
    fn places_the_caret_after_the_parentheses_of_a_parameterless_call() {
        let (doc, req, _) = setup("svc.cl|", "");
        let close = CompletionItem { insert_text: "close".into(), label_detail: Some("()".into()), ..item() };
        assert_eq!(accept(&doc, &req, close, 4, 6), ("svc.close()".into(), 11));
    }

    #[test]
    fn expands_snippets_with_fields() {
        let (doc, req, _) = setup("pri|", "");
        let snippet = CompletionItem { insert_text: "println!(${1:arg})$0".into(), is_snippet: true, ..item() };
        let applied = apply_lsp_completion(&make_completion(snippet, &req), &doc, 0, 3, options(ApplyMode::Insert));
        let text = apply_changes(&doc, &applied.changes);
        assert_eq!(text.to_string(), "println!(arg)");
        let selected = &applied.selection[0];
        assert_eq!(buffer::slice(&text, selected.start, selected.end), "arg");
    }

    #[test]
    fn applies_to_plain_callables_only() {
        assert_eq!(call_parens(&item(), "a.cr", 0, 2, 4), Some(CallParens::Inside));
        assert_eq!(call_parens(&CompletionItem { kind: Some(CompletionKind::Field), ..item() }, "a.cr", 0, 2, 4), None);
        assert_eq!(call_parens(&CompletionItem { is_snippet: true, ..item() }, "a.cr", 0, 2, 4), None);
        assert_eq!(call_parens(&CompletionItem { insert_text: "create()".into(), ..item() }, "a.cr", 0, 2, 4), None);
    }

    #[test]
    fn skips_calls_that_already_have_parentheses_and_imports() {
        assert_eq!(call_parens(&item(), "a.cr(x)", 0, 2, 4), None);
        assert_eq!(call_parens(&item(), "cr'", 0, 0, 2), None);
        assert_eq!(call_parens(&item(), "cr\"", 0, 0, 2), None);
        assert_eq!(call_parens(&item(), "import { cr", 0, 9, 11), None);
        assert_eq!(call_parens(&item(), "use crate::cr", 0, 11, 13), None);
    }

    #[test]
    fn are_offered_in_php_and_blade_only_once_the_string_reads_like_a_path() {
        assert!(!offers_paths("php", "config('app."));
        assert!(!offers_paths("php", "User::create(['na"));
        assert!(offers_paths("php", "require './"));
        assert!(offers_paths("blade", "@include('partials/"));
    }

    #[test]
    fn are_offered_as_before_in_other_languages() {
        assert!(offers_paths("typescript", "import x from 'mod"));
        assert!(offers_paths("rust", "include_str!(\"da"));
    }

    fn entries(labels: &[&str]) -> Vec<LookupEntry> {
        labels.iter().map(|label| lookup_entry(&CompletionItem { label: (*label).into(), ..item() })).collect()
    }

    fn sorted_labels(rows: &[LookupEntry], typed: &str) -> Vec<String> {
        sort_alphabetically(rows, typed).into_iter().map(|(i, _)| rows[i].label.clone()).collect()
    }

    #[test]
    fn filters_and_orders_by_name() {
        let rows = entries(&["zeta", "Alpha", "beta", "gamma", "alphabet"]);
        assert_eq!(sorted_labels(&rows, "a"), ["Alpha", "alphabet", "beta", "gamma", "zeta"]);
        assert_eq!(sorted_labels(&entries(&["zeta", "Alpha", "beta"]), "al"), ["Alpha"]);
    }

    #[test]
    fn matches_substrings_then_subsequences() {
        assert_eq!(match_ranges("getValue", "val"), Some(vec![3, 6]));
        assert_eq!(match_ranges("getValue", "gv"), Some(vec![0, 1, 3, 4]));
        assert_eq!(match_ranges("getValue", "x"), None);
    }

    #[test]
    fn finishes_only_focused_lookups_unless_the_setting_asks_for_it() {
        assert!(commits_lookup('.', true, false, &[]));
        assert!(!commits_lookup('.', false, false, &[]));
        assert!(commits_lookup('.', false, true, &[]));
        assert!(!commits_lookup('-', true, false, &[]));
        assert!(commits_lookup('[', true, false, &["[".into()]));
    }

    #[test]
    fn leaves_server_commit_characters_to_the_lookup() {
        let (_, req, _) = setup("|", "");
        let completion = make_completion(CompletionItem { commit_characters: vec![".".into()], ..item() }, &req);
        assert!(completion.entry.commit_characters.is_empty());
    }

    /// An explicit completion over "pri" whose source answers `labels`; returns the document after
    /// the lookup had its say.
    fn explicit(labels: &[&str], auto_insert_single: bool) -> String {
        let doc = Rope::from_str("pri");
        let query = completion_query("pri", 3, true).expect("explicit completion always asks");
        let mut session = LookupSession::default();
        session.explicit(true);
        assert_eq!(session.status(Some(LookupStatus::Pending), 0, false, auto_insert_single), LookupEvent::None);
        let event = session.status(Some(LookupStatus::Active), labels.len(), false, auto_insert_single);
        if event == LookupEvent::AutoInsert && auto_insert_due(true, true, true) {
            let applied = apply_contributed(&doc, query.from, 3, &ContributedApply::Text(labels[0].into()), "  ");
            return apply_changes(&doc, &applied.changes).to_string();
        }
        doc.to_string()
    }

    #[test]
    fn inserts_a_sole_candidate_right_away() {
        assert_eq!(explicit(&["println"], true), "println");
    }

    #[test]
    fn keeps_a_sole_candidate_in_the_list_when_auto_insert_is_off() {
        assert_eq!(explicit(&["println"], false), "pri");
    }

    #[test]
    fn shows_the_list_when_there_are_several_candidates() {
        assert_eq!(explicit(&["println", "print"], true), "pri");
    }

    // Not from a spec: typing asks after a word or a trigger character, not inside a number; an
    // answer to a superseded request is dropped, and a missing server backs off.
    #[test]
    fn asks_on_words_and_triggers_and_drops_stale_answers() {
        assert_eq!(completion_query("x = 12", 6, false), None);
        assert_eq!(completion_query("a.", 2, false).map(|query| query.trigger), Some(Some('.')));
        assert_eq!(completion_query("foo.ba", 6, false).map(|query| query.from), Some(4));
        let mut source = CompletionSource::default();
        let list = || CompletionList { is_incomplete: false, items: vec![item()] };
        assert_eq!(source.ask(DocId(1), RequestId(1), 0), Ask::Request { id: RequestId(1), cancel: None });
        assert_eq!(
            source.ask(DocId(1), RequestId(2), 0),
            Ask::Request { id: RequestId(2), cancel: Some(RequestId(1)) }
        );
        assert!(matches!(source.answer(DocId(1), RequestId(1), Ok(list()), 0), Answered::Stale));
        assert!(matches!(source.answer(DocId(1), RequestId(2), Ok(list()), 0), Answered::List(_)));
        source.ask(DocId(1), RequestId(3), 0);
        let missing = IdeError::new(ide_core::error::ErrorKind::Unavailable, "jdtls is not running");
        assert!(matches!(source.answer(DocId(1), RequestId(3), Err(missing), 0), Answered::Fallback));
        assert_eq!(source.ask(DocId(1), RequestId(4), 1), Ask::Fallback);
    }
}
