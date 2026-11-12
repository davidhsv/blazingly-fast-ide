//! Find and Replace in one file: which ranges of a text a query matches, which of them comes next or before
//! the selection, and what replacing one or all of them changes.
//!
//! [`FindState`] is the searched text together with what a search keeps about it: the selection, the range
//! the search is limited to ("In Selection"), the occurrences the user excluded, the comments and string
//! literals the search context filters by, and the query. Its commands change the state the way the editor
//! is about to change and return the edits to make there, so whoever owns the real document applies them
//! and moves its selection to [`FindState::selection`].
//!
//! Offsets are UTF-16 code units from the start of the text, and the text holds `\n` line breaks only, as
//! the documents do.

use super::buffer::{self, Assoc};
use crate::find::ipc::LineMatch;
use crate::find::utf16;
use crate::platform::search::{LiteralKind, SearchContext, context_accepts, preserve_case};
use ide_core::TextChange;
use regex::{Captures, Regex, RegexBuilder};
use std::ops::Range;

/// Occurrences the counter counts; beyond them it shows `N+`.
pub const MAX_COUNT: usize = 9_999;
/// The longest selection, in UTF-16 code units, that becomes the query when the bar opens over it.
pub const MAX_QUERY: u32 = 10_000;

/// What the find bar searches for and replaces with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub search: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regexp: bool,
    pub replace: String,
    /// Where in the code an occurrence may lie: anywhere, only in or outside comments and string literals.
    pub context: SearchContext,
    /// Replace keeps the case of each occurrence.
    pub preserve_case: bool,
}

impl FindOptions {
    /// Whether both find the same occurrences and replace them with the same text. Preserve Case is not
    /// part of that: it only changes how a replacement is written.
    fn same_query(&self, other: &Self) -> bool {
        self.search == other.search
            && self.replace == other.replace
            && self.case_sensitive == other.case_sensitive
            && self.whole_word == other.whole_word
            && self.regexp == other.regexp
            && self.context == other.context
    }
}

/// Whether `source` is a regular expression the search can run.
pub fn regex_valid(source: &str) -> bool {
    RegexBuilder::new(source).multi_line(true).build().is_ok()
}

/// The pattern the options ask for: plain text is matched literally, and `^` / `$` of a regular expression
/// match at every line.
fn compile(search: &str, regexp: bool, case_sensitive: bool) -> Option<Regex> {
    let escaped;
    let pattern = if regexp {
        search
    } else {
        escaped = regex::escape(search);
        &escaped
    };
    RegexBuilder::new(pattern).case_insensitive(!case_sensitive).multi_line(true).build().ok()
}

/// A replacement of a regular expression search with its `\n`, `\r`, `\t` and `\\` escapes resolved.
fn unquote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let resolved = match chars.as_str().chars().next() {
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('\\') => '\\',
            _ => {
                out.push('\\');
                continue;
            }
        };
        out.push(resolved);
        chars.next();
    }
    out
}

/// The replacement `template` for one match: `$&` is the match, `$1`, `$2`… its groups and `$$` a dollar
/// sign. Digits after `$` name the longest group number the pattern has, so `$12` with one group is the
/// first group followed by `2`; a `$` that names nothing stays as typed. A group that took no part in the
/// match is empty.
fn expand(template: &str, captures: &Captures) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(tail) = after.strip_prefix('&') {
            out.push_str(captures.get(0).map_or("", |whole| whole.as_str()));
            rest = tail;
        } else if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else {
            let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let group = (1..=digits).rev().find_map(|len| {
                let number: usize = after[..len].parse().ok()?;
                (number > 0 && number < captures.len()).then_some((number, len))
            });
            match group {
                Some((number, len)) => {
                    out.push_str(captures.get(number).map_or("", |group| group.as_str()));
                    rest = &after[len..];
                }
                None => {
                    out.push('$');
                    rest = after;
                }
            }
        }
    }
    out.push_str(rest);
    out
}

/// `text` with the line breaks a document holds: `\r\n` and `\r` become `\n`.
fn line_feeds(text: String) -> String {
    if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text }
}

/// The options compiled for searching.
#[derive(Clone, Debug, Default)]
pub struct FindQuery {
    options: FindOptions,
    /// `None` while there is nothing to search for or the regular expression does not compile.
    regex: Option<Regex>,
    /// The replacement, with the escapes of a regular expression search resolved.
    template: String,
}

impl FindQuery {
    pub fn new(options: &FindOptions) -> Self {
        let regex = if options.search.is_empty() {
            None
        } else {
            compile(&options.search, options.regexp, options.case_sensitive)
        };
        let template = if options.regexp { unquote(&options.replace) } else { options.replace.clone() };
        Self { options: options.clone(), regex, template }
    }

    pub fn options(&self) -> &FindOptions {
        &self.options
    }

    /// Whether there is something to search for and it can be searched.
    pub fn is_valid(&self) -> bool {
        self.regex.is_some()
    }

    /// Whether the search text is a regular expression that does not compile.
    pub fn is_invalid(&self) -> bool {
        self.options.regexp && !self.options.search.is_empty() && self.regex.is_none()
    }
}

/// A comment or a string literal of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiteralRange {
    pub from: u32,
    pub to: u32,
    pub kind: LiteralKind,
}

/// Comments and string literals from highlighted spans in document order (`from`, `to`, kind): spans of one
/// kind that touch or overlap become one range, and empty ones are dropped.
pub fn literal_ranges(spans: impl IntoIterator<Item = (u32, u32, LiteralKind)>) -> Vec<LiteralRange> {
    let mut out: Vec<LiteralRange> = Vec::new();
    for (from, to, kind) in spans {
        if let Some(last) = out.last_mut().filter(|last| last.kind == kind && last.to >= from) {
            last.to = last.to.max(to);
        } else if to > from {
            out.push(LiteralRange { from, to, kind });
        }
    }
    out
}

/// What `offset` lies in: a comment, a string literal or plain code (`None`). `literals` are sorted and do
/// not overlap.
pub fn literal_kind_at(literals: &[LiteralRange], offset: u32) -> Option<LiteralKind> {
    let at = literals.partition_point(|literal| literal.to <= offset);
    literals.get(at).filter(|literal| literal.from <= offset).map(|literal| literal.kind)
}

/// The occurrences of a query, in document order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub ranges: Vec<Range<u32>>,
    /// Index into `ranges` of the occurrence that is exactly the selection.
    pub current: Option<usize>,
    /// The search stopped at its limit: there may be more occurrences than `ranges` holds.
    pub capped: bool,
}

impl Matches {
    pub fn total(&self) -> usize {
        self.ranges.len()
    }

    /// Where each occurrence starts.
    pub fn starts(&self) -> Vec<u32> {
        self.ranges.iter().map(|range| range.start).collect()
    }

    /// Marks the occurrence that is exactly `selection` as the current one.
    pub fn select(&mut self, selection: &Range<u32>) {
        let at = self.ranges.binary_search_by(|range| range.start.cmp(&selection.start)).ok();
        self.current = at.filter(|&at| self.ranges[at] == *selection);
    }
}

/// A place in a text that only moves forwards, known as a byte index and as a UTF-16 offset: converting a
/// run of ascending positions reads the text once.
#[derive(Clone, Copy)]
struct Cursor<'a> {
    text: &'a str,
    byte: usize,
    units: u32,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, byte: 0, units: 0 }
    }

    /// Moves to the byte index `byte`, a character boundary not before the current place, and returns its
    /// UTF-16 offset.
    fn seek_byte(&mut self, byte: usize) -> u32 {
        let byte = byte.clamp(self.byte, self.text.len());
        self.units += utf16::len(&self.text[self.byte..byte]);
        self.byte = byte;
        self.units
    }

    /// Moves to the UTF-16 offset `units` and returns its byte index. An offset before the current place
    /// stays there, one past the end is the end and one inside a surrogate pair rounds down.
    fn seek_units(&mut self, units: u32) -> usize {
        if units > self.units {
            let rest = &self.text[self.byte..];
            let step = buffer::byte_of_col(rest, units - self.units);
            self.units += utf16::len(&rest[..step]);
            self.byte += step;
        }
        self.byte
    }
}

/// Where each line of a text starts, as a byte index and as a UTF-16 offset.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<(usize, u32)>,
}

impl<'a> Lines<'a> {
    fn of(text: &'a str) -> Self {
        let mut starts = vec![(0, 0)];
        let mut units = 0;
        for (byte, c) in text.char_indices() {
            units += c.len_utf16() as u32;
            if c == '\n' {
                starts.push((byte + 1, units));
            }
        }
        Self { text, starts }
    }

    /// The 0-based line holding the UTF-16 offset `offset`.
    fn line_at(&self, offset: u32) -> usize {
        self.starts.partition_point(|&(_, units)| units <= offset).saturating_sub(1)
    }

    /// The text of `line`, without its line break.
    fn text(&self, line: usize) -> &'a str {
        let from = self.starts[line].0;
        let to = self.starts.get(line + 1).map_or(self.text.len(), |next| next.0 - 1);
        &self.text[from..to]
    }

    /// The UTF-16 offsets of the text of `line`, without its line break.
    fn span(&self, line: usize) -> Range<u32> {
        let from = self.starts[line].1;
        let to = self.starts.get(line + 1).map_or_else(|| from + utf16::len(self.text(line)), |next| next.1 - 1);
        from..to
    }
}

/// The 0-based line of the UTF-16 offset `offset` in `text`.
pub fn line_of(text: &str, offset: u32) -> u32 {
    utf16::slice(text, 0, offset).bytes().filter(|&byte| byte == b'\n').count() as u32
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether the match at `bytes` of `text` is no part of a longer word: a word character at either of its
/// ends has no word character beside it outside the match.
fn stands_alone(text: &str, bytes: &Range<usize>) -> bool {
    let word = |c: Option<char>| c.is_some_and(is_word_char);
    let found = &text[bytes.clone()];
    let continues_before = word(text[..bytes.start].chars().next_back()) && word(found.chars().next());
    let continues_after = word(text[bytes.end..].chars().next()) && word(found.chars().next_back());
    !continues_before && !continues_after
}

/// `text` after `changes`, which are ordered, do not overlap and are in the coordinates of `text`.
fn apply_changes(text: &str, changes: &[TextChange]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = Cursor::new(text);
    let mut copied = 0;
    for change in changes {
        let from = cursor.seek_units(change.from);
        let to = cursor.seek_units(change.to);
        out.push_str(&text[copied..from]);
        out.push_str(&change.insert);
        copied = to;
    }
    out.push_str(&text[copied..]);
    out
}

/// One change with the effect of all of `changes` on `text`: it replaces what lies between the first and
/// the last of them, so a document that takes one edit at a time makes them in one undoable step. `None`
/// without changes.
pub fn merged(text: &str, changes: &[TextChange]) -> Option<TextChange> {
    let (first, last) = (changes.first()?, changes.last()?);
    if changes.len() == 1 {
        return Some(first.clone());
    }
    let within: Vec<TextChange> = changes
        .iter()
        .map(|change| TextChange {
            from: change.from - first.from,
            to: change.to - first.from,
            insert: change.insert.clone(),
        })
        .collect();
    let insert = apply_changes(utf16::slice(text, first.from, last.to), &within);
    Some(TextChange { from: first.from, to: last.to, insert })
}

/// The one change that turns `old` into `new`: what lies between their common start and their common end.
/// `None` when they are the same text.
pub fn difference(old: &str, new: &str) -> Option<TextChange> {
    if old == new {
        return None;
    }
    let prefix: usize =
        old.chars().zip(new.chars()).take_while(|(before, after)| before == after).map(|(c, _)| c.len_utf8()).sum();
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix: usize = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(before, after)| before == after)
        .map(|(c, _)| c.len_utf8())
        .sum();
    let from = utf16::len(&old[..prefix]);
    let removed = &old_rest[..old_rest.len() - suffix];
    let insert = new_rest[..new_rest.len() - suffix].to_owned();
    Some(TextChange { from, to: from + utf16::len(removed), insert })
}

/// `range` after `changes`: text typed at either of its edges stays outside it, and text that replaces a
/// part of it at an edge stays inside. `None` once nothing of it is left.
pub fn map_scope(range: &Range<u32>, changes: &[TextChange]) -> Option<Range<u32>> {
    let from = buffer::map_offset(range.start, changes, Assoc::After);
    let to = buffer::map_offset(range.end, changes, Assoc::Before).max(from);
    (to > from).then_some(from..to)
}

/// A selection after `changes`: a caret stays before text typed at it, a range does not grow at its edges.
fn map_selection(selection: &Range<u32>, changes: &[TextChange]) -> Range<u32> {
    if selection.is_empty() {
        let at = buffer::map_offset(selection.start, changes, Assoc::Before);
        return at..at;
    }
    let from = buffer::map_offset(selection.start, changes, Assoc::After);
    from..buffer::map_offset(selection.end, changes, Assoc::Before).max(from)
}

/// What opening the bar over a selection does to the range the search is limited to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedScope {
    /// Nothing is selected: the range stays as it is.
    Unchanged,
    /// The selection is the query, searched for in the whole file.
    File,
    /// Replace over a selection of several lines: the search is limited to it.
    Selection,
}

/// The options a bar opening over the `selected` text starts with: the selection becomes the query, escaped
/// when the search is a regular expression, unless it is too long to be one. Replace over several selected
/// lines searches in them instead, with nothing to search for yet. Without a selection the query stays,
/// except one of several lines.
pub fn seed_options(options: &mut FindOptions, selected: &str, replace: bool) -> SeedScope {
    if selected.is_empty() {
        if options.search.contains('\n') {
            options.search.clear();
        }
        return SeedScope::Unchanged;
    }
    if replace && selected.contains('\n') {
        options.search.clear();
        return SeedScope::Selection;
    }
    if utf16::len(selected) < MAX_QUERY {
        options.search = if options.regexp { regex::escape(selected) } else { selected.to_owned() };
    }
    SeedScope::File
}

/// The counter beside the search field, in English: `Bad regex`, nothing without a query, `0 results`, or
/// the current occurrence of the total (`3/12`), with `?` while the selection is no occurrence and `+` when
/// the count stopped at its limit.
pub fn counter_text(query: &FindQuery, matches: &Matches) -> String {
    if query.is_invalid() {
        "Bad regex".to_owned()
    } else if query.options.search.is_empty() {
        String::new()
    } else if matches.ranges.is_empty() {
        "0 results".to_owned()
    } else {
        let current = matches.current.map_or_else(|| "?".to_owned(), |at| (at + 1).to_string());
        format!("{current}/{}{}", matches.total(), if matches.capped { "+" } else { "" })
    }
}

/// One occurrence while the text is scanned.
struct Hit {
    range: Range<u32>,
    bytes: Range<usize>,
    /// Byte index where the searched part of the text ends.
    end: usize,
}

/// The searched text with what a search session keeps about it.
#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub text: String,
    /// The selection, as `from..to`.
    pub selection: Range<u32>,
    /// "In Selection": the range Find and Replace are limited to.
    pub scope: Option<Range<u32>>,
    /// Occurrences skipped by Next, Previous and Replace All until the query changes.
    pub excluded: Vec<Range<u32>>,
    /// The comments and string literals of the text, sorted, for the search context.
    pub literals: Vec<LiteralRange>,
    query: FindQuery,
}

impl FindState {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    pub fn query(&self) -> &FindQuery {
        &self.query
    }

    /// Searches for what `options` ask from now on. A query that finds other occurrences or replaces them
    /// with other text includes every excluded occurrence again. Returns whether the query changed.
    pub fn set_query(&mut self, options: &FindOptions) -> bool {
        let same = self.query.options.same_query(options);
        if !same {
            self.excluded.clear();
        }
        if !same || self.query.options != *options {
            self.query = FindQuery::new(options);
        }
        !same
    }

    pub fn is_excluded(&self, range: &Range<u32>) -> bool {
        self.excluded.contains(range)
    }

    /// Excludes `range`, or includes it again.
    pub fn toggle_excluded(&mut self, range: Range<u32>) {
        if self.is_excluded(&range) {
            self.excluded.retain(|excluded| *excluded != range);
        } else {
            self.excluded.push(range);
        }
    }

    /// Whether the candidate at `bytes` (`range` in UTF-16) counts: it is not excluded, stands alone when
    /// only whole words are searched for, and starts in code of the kind the search context asks for.
    fn accepts(&self, query: &FindQuery, bytes: &Range<usize>, range: &Range<u32>) -> bool {
        let options = &query.options;
        !self.excluded.contains(range)
            && (!options.whole_word || stands_alone(&self.text, bytes))
            && (options.context == SearchContext::Any
                || context_accepts(options.context, literal_kind_at(&self.literals, range.start)))
    }

    /// Hands every occurrence of `query` in the scope to `each`, in order and without overlaps, until it
    /// returns false. A match of no length is no occurrence.
    ///
    /// The search starts at the scope's start with the text before it in view, so `^` and word boundaries
    /// there mean what they mean in the whole text, and the text ends for it where the scope ends. After a
    /// candidate that does not count, a plain-text search goes on one character further, so an occurrence
    /// that overlaps it is still found; a regular expression goes on behind it.
    fn scan(&self, query: &FindQuery, mut each: impl FnMut(Hit) -> bool) {
        let Some(regex) = &query.regex else { return };
        let mut cursor = Cursor::new(&self.text);
        let (mut at, end) = match &self.scope {
            Some(scope) => {
                let from = cursor.seek_units(scope.start);
                let mut tail = cursor;
                (from, tail.seek_units(scope.end))
            }
            None => (0, self.text.len()),
        };
        let searched = &self.text[..end];
        while let Some(found) = regex.find_at(searched, at) {
            let bytes = found.range();
            let Some(first) = searched[bytes.start..].chars().next() else { break };
            if bytes.is_empty() {
                at = bytes.start + first.len_utf8();
                continue;
            }
            let from = cursor.seek_byte(bytes.start);
            let range = from..from + utf16::len(found.as_str());
            if self.accepts(query, &bytes, &range) {
                at = bytes.end;
                if !each(Hit { range, bytes, end }) {
                    break;
                }
            } else if query.options.regexp {
                at = bytes.end;
            } else {
                at = bytes.start + first.len_utf8();
            }
        }
    }

    /// The occurrences in the scope, at most `limit` of them, and which one the selection is.
    pub fn matches(&self, limit: usize) -> Matches {
        let mut found = Matches::default();
        self.scan(&self.query, |hit| {
            if hit.range == self.selection {
                found.current = Some(found.ranges.len());
            }
            found.ranges.push(hit.range);
            found.capped = found.ranges.len() >= limit;
            !found.capped
        });
        found
    }

    /// The occurrences the counter counts: at most [`MAX_COUNT`].
    pub fn count(&self) -> Matches {
        self.matches(MAX_COUNT)
    }

    /// The first occurrence starting at `offset` or after it; past the last one, the first of all.
    fn first_from(&self, offset: u32) -> Option<Range<u32>> {
        let (mut first, mut next) = (None, None);
        self.scan(&self.query, |hit| {
            if hit.range.start >= offset {
                next = Some(hit.range);
                return false;
            }
            if first.is_none() {
                first = Some(hit.range);
            }
            true
        });
        next.or(first)
    }

    /// The last occurrence ending at `offset` or before it; before the first one, the last of all.
    fn last_until(&self, offset: u32) -> Option<Range<u32>> {
        let (mut before, mut last) = (None, None);
        self.scan(&self.query, |hit| {
            if hit.range.end <= offset {
                before = Some(hit.range.clone());
            }
            last = Some(hit.range);
            true
        });
        before.or(last)
    }

    /// The occurrence that is exactly `range`.
    fn hit_at(&self, range: &Range<u32>) -> Option<Hit> {
        let mut found = None;
        self.scan(&self.query, |hit| {
            let reached = hit.range.start >= range.start;
            if hit.range == *range {
                found = Some(hit);
            }
            !reached
        });
        found
    }

    /// Selects the next occurrence after the selection, going round to the first one. Returns whether
    /// there is an occurrence.
    pub fn find_next(&mut self) -> bool {
        let next = self.first_from(self.selection.end);
        self.select(next)
    }

    /// Selects the occurrence before the selection, going round to the last one.
    pub fn find_previous(&mut self) -> bool {
        let previous = self.last_until(self.selection.start);
        self.select(previous)
    }

    fn select(&mut self, range: Option<Range<u32>>) -> bool {
        let found = range.is_some();
        if let Some(range) = range {
            self.selection = range;
        }
        found
    }

    /// The text that replaces `hit`: the replacement as typed, or for a regular expression with its groups
    /// filled in, in the case of the occurrence when `preserve` is set.
    fn replacement(&self, hit: &Hit, preserve: bool) -> String {
        let query = &self.query;
        let captures = match &query.regex {
            Some(regex) if query.options.regexp => regex.captures_at(&self.text[..hit.end], hit.bytes.start),
            _ => None,
        };
        let insert = match captures.filter(|found| found.get(0).is_some_and(|whole| whole.range() == hit.bytes)) {
            Some(captures) => expand(&query.template, &captures),
            None => query.template.clone(),
        };
        let insert = line_feeds(insert);
        if preserve { preserve_case(&self.text[hit.bytes.clone()], &insert) } else { insert }
    }

    /// Replace: when the selection is an occurrence it is replaced and the next occurrence after the new
    /// text is selected; otherwise the next occurrence is selected and nothing is replaced. Returns the
    /// edit made, in the coordinates of the text before it.
    pub fn replace_next(&mut self, preserve: bool) -> Vec<TextChange> {
        let selection = self.selection.clone();
        let Some(hit) = self.hit_at(&selection) else {
            self.find_next();
            return Vec::new();
        };
        let insert = self.replacement(&hit, preserve);
        let after = hit.range.start + utf16::len(&insert);
        let change = TextChange { from: hit.range.start, to: hit.range.end, insert };
        self.apply(std::slice::from_ref(&change));
        self.selection = after..after;
        self.find_next();
        vec![change]
    }

    /// Replace All: replaces every occurrence in the scope. Returns the edits made, in order and in the
    /// coordinates of the text before them.
    pub fn replace_all(&mut self, preserve: bool) -> Vec<TextChange> {
        let mut changes = Vec::new();
        self.scan(&self.query, |hit| {
            let insert = self.replacement(&hit, preserve);
            changes.push(TextChange { from: hit.range.start, to: hit.range.end, insert });
            true
        });
        self.apply(&changes);
        changes
    }

    /// Exclude: the selected occurrence is skipped from now on and the next one is selected; an excluded
    /// one is included again. A selection that is no occurrence moves to the next one. Returns whether
    /// anything changed.
    pub fn exclude_current(&mut self) -> bool {
        let selection = self.selection.clone();
        let excluded = self.is_excluded(&selection);
        if !excluded && self.hit_at(&selection).is_none() {
            return self.find_next();
        }
        self.toggle_excluded(selection);
        if !excluded {
            self.find_next();
        }
        true
    }

    /// Toggles "In Selection": the search is limited to the selection the bar was opened over when that is
    /// still in the text and larger than the current one, else to the current selection; a limited search
    /// covers the file again. Returns whether the scope changed.
    pub fn toggle_scope(&mut self, opened_over: Option<&Range<u32>>) -> bool {
        if self.scope.take().is_some() {
            return true;
        }
        let length = utf16::len(&self.text);
        let size = |range: &Range<u32>| range.end.saturating_sub(range.start);
        let saved = opened_over.filter(|saved| saved.end <= length && size(saved) > size(&self.selection));
        let range = saved.unwrap_or(&self.selection).clone();
        if range.is_empty() {
            return false;
        }
        self.scope = Some(range);
        true
    }

    /// The text changes by `changes` (ordered, not overlapping, in its coordinates before them): the
    /// selection, the scope, the excluded occurrences and the literals move with it.
    pub fn apply(&mut self, changes: &[TextChange]) {
        if changes.is_empty() {
            return;
        }
        self.text = apply_changes(&self.text, changes);
        self.selection = map_selection(&self.selection, changes);
        self.scope = self.scope.as_ref().and_then(|scope| map_scope(scope, changes));
        self.excluded = self.excluded.iter().filter_map(|range| map_scope(range, changes)).collect();
        self.literals = self
            .literals
            .iter()
            .filter_map(|literal| {
                let range = map_scope(&(literal.from..literal.to), changes)?;
                Some(LiteralRange { from: range.start, to: range.end, kind: literal.kind })
            })
            .collect();
    }

    /// The lines with occurrences of `options` in the scope, as a search result lists them: every
    /// occurrence, however many there are, and one that spans lines on each of them. The scope, the excluded
    /// occurrences and the search context apply as they do to the counter.
    pub fn result_lines(&self, options: &FindOptions) -> Vec<LineMatch> {
        let query = FindQuery::new(options);
        if !query.is_valid() {
            return Vec::new();
        }
        let lines = Lines::of(&self.text);
        let mut out: Vec<LineMatch> = Vec::new();
        self.scan(&query, |hit| {
            let Range { start: from, end: to } = hit.range;
            for line in lines.line_at(from)..=lines.line_at(from.max(to.saturating_sub(1))) {
                let span = lines.span(line);
                if out.last().is_none_or(|last| last.line as usize != line) {
                    let text = lines.text(line).to_owned();
                    out.push(LineMatch { line: line as u32, text, ranges: Vec::new(), text_offset: 0 });
                }
                let range = (from.max(span.start) - span.start, to.min(span.end).saturating_sub(span.start));
                if let Some(last) = out.last_mut() {
                    last.ranges.push(range);
                }
            }
            true
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(search: &str) -> FindOptions {
        FindOptions { search: search.to_owned(), ..FindOptions::default() }
    }

    fn replacing(search: &str, replace: &str) -> FindOptions {
        FindOptions { replace: replace.to_owned(), ..options(search) }
    }

    fn make(doc: &str, options: &FindOptions, scope: Option<Range<u32>>) -> FindState {
        let mut state = FindState::new(doc);
        state.set_query(options);
        state.scope = scope;
        state
    }

    fn sel(state: &FindState) -> (u32, u32) {
        (state.selection.start, state.selection.end)
    }

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    const SCOPED_DOC: &str = "a1 a2\na3 a4\na5 a6\n";
    /// The second line of [`SCOPED_DOC`].
    const SECOND_LINE: Range<u32> = 6..11;

    /// A call, a string literal and a comment on the first line, plain code on the second.
    const CODE: &str = "foo(\"foo\") // foo\nfoo";

    fn make_code(options: &FindOptions) -> FindState {
        let mut state = make(CODE, options, None);
        state.literals = literal_ranges([(4, 9, LiteralKind::String), (11, 17, LiteralKind::Comment)]);
        state
    }

    fn in_context(search: &str, context: SearchContext) -> FindOptions {
        FindOptions { context, ..options(search) }
    }

    type LineRanges = Vec<(u32, Vec<(u32, u32)>)>;

    fn line_ranges(lines: &[LineMatch]) -> LineRanges {
        lines.iter().map(|line| (line.line, line.ranges.clone())).collect()
    }

    // src/editor/core/cm/find.test.ts › find backgrounds › keeps matches inside %s backgrounds and selections
    // inside matches. How the marks nest is the editor's to draw; the search owes it the occurrence under a
    // selection that lies inside it.
    #[test]
    fn keeps_matches_inside_backgrounds_and_selections_inside_matches() {
        let mut state = make("Session", &FindOptions { case_sensitive: true, ..options("Session") }, None);
        state.selection = 1..6;
        let found = state.count();
        assert_eq!(found.ranges, std::slice::from_ref(&(0..7)));
        assert_eq!(found.current, None, "a selection inside an occurrence is not that occurrence");
        state.selection = 0..7;
        assert_eq!(state.count().current, Some(0));
    }

    // src/editor/core/cm/find.test.ts › find in selection › counts only matches inside the scope
    #[test]
    fn counts_only_matches_inside_the_scope() {
        assert_eq!(make(SCOPED_DOC, &options("a"), None).count().total(), 6);
        assert_eq!(make(SCOPED_DOC, &options("a"), Some(SECOND_LINE)).count().total(), 2);
    }

    // src/editor/core/cm/find.test.ts › find in selection › next / previous wrap around inside the scope
    #[test]
    fn next_previous_wrap_around_inside_the_scope() {
        let mut state = make(SCOPED_DOC, &options("a"), Some(SECOND_LINE));
        assert!(state.find_next());
        assert_eq!(sel(&state), (6, 7));
        state.find_next();
        assert_eq!(sel(&state), (9, 10));
        state.find_next();
        assert_eq!(sel(&state), (6, 7));
        state.find_previous();
        assert_eq!(sel(&state), (9, 10));
    }

    // src/editor/core/cm/find.test.ts › find in selection › replace all touches the scope only, with regex groups
    #[test]
    fn replace_all_touches_the_scope_only_with_regex_groups() {
        let query = FindOptions { regexp: true, ..replacing("a(\\d)", "b$1") };
        let mut state = make(SCOPED_DOC, &query, Some(SECOND_LINE));
        assert!(!state.replace_all(false).is_empty());
        assert_eq!(state.text, "a1 a2\nb3 b4\na5 a6\n");
        assert_eq!(state.scope, Some(6..11));
    }

    // src/editor/core/cm/find.test.ts › find in selection › replace next replaces the selected match and
    // selects the next one in scope
    #[test]
    fn replace_next_replaces_the_selected_match_and_selects_the_next_one_in_scope() {
        let mut state = make(SCOPED_DOC, &replacing("a", "xyz"), Some(SECOND_LINE));
        state.find_next();
        assert_eq!(state.replace_next(false), [change(6, 7, "xyz")]);
        assert_eq!(state.text, "a1 a2\nxyz3 a4\na5 a6\n");
        assert_eq!(sel(&state), (11, 12));
        state.replace_next(false);
        assert_eq!(state.text, "a1 a2\nxyz3 xyz4\na5 a6\n");
        assert_eq!(state.scope, Some(6..15));
    }

    // src/editor/core/cm/find.test.ts › find in selection › plain-text search is literal
    #[test]
    fn plain_text_search_is_literal() {
        let mut state = make("a\\nb\na\nb", &replacing("a\\nb", "\\t"), None);
        assert_eq!(state.count().total(), 1);
        state.replace_all(false);
        assert_eq!(state.text, "\\t\na\nb");
    }

    // src/editor/core/cm/find.test.ts › find in selection › maps the scope through edits without growing at
    // its edges
    #[test]
    fn maps_the_scope_through_edits_without_growing_at_its_edges() {
        let insert = |at| [change(at, at, "xx")];
        assert_eq!(map_scope(&(2..5), &insert(2)), Some(4..7));
        assert_eq!(map_scope(&(2..5), &insert(5)), Some(2..5));
        assert_eq!(map_scope(&(2..5), &[change(2, 5, "")]), None);
    }

    // src/editor/core/cm/find.test.ts › search context, exclude and preserve case › exports the same %s syntax
    // filter as the bar
    #[test]
    fn exports_the_same_syntax_filter_as_the_bar() {
        let contexts = [
            SearchContext::InComments,
            SearchContext::InStringLiterals,
            SearchContext::ExceptComments,
            SearchContext::ExceptCommentsAndStringLiterals,
        ];
        for context in contexts {
            let query = in_context("foo", context);
            let state = make_code(&query);
            let line_starts = Lines::of(CODE);
            let exported: Vec<u32> = state
                .result_lines(&query)
                .iter()
                .flat_map(|line| {
                    let start = line_starts.span(line.line as usize).start;
                    line.ranges.iter().map(move |&(from, _)| start + from)
                })
                .collect();
            assert_eq!(exported, state.count().starts(), "{context:?}");
        }
    }

    // src/editor/core/cm/find.test.ts › search context, exclude and preserve case › filters matches by comment /
    // string literal
    #[test]
    fn filters_matches_by_comment_string_literal() {
        let starts = |context| make_code(&in_context("foo", context)).count().starts();
        assert_eq!(starts(SearchContext::Any), [0, 5, 14, 18]);
        assert_eq!(starts(SearchContext::InComments), [14]);
        assert_eq!(starts(SearchContext::InStringLiterals), [5]);
        assert_eq!(starts(SearchContext::ExceptComments), [0, 5, 18]);
        assert_eq!(starts(SearchContext::ExceptCommentsAndStringLiterals), [0, 18]);
        assert_eq!(literal_kind_at(&make_code(&options("foo")).literals, 14), Some(LiteralKind::Comment));
    }

    // src/editor/core/cm/find.test.ts › search context, exclude and preserve case › excluded matches are skipped
    // by next and replace all, until the query changes
    #[test]
    fn excluded_matches_are_skipped_by_next_and_replace_all_until_the_query_changes() {
        let mut state = make_code(&replacing("foo", "bar"));
        state.find_next();
        assert_eq!(sel(&state), (0, 3));
        assert!(state.exclude_current());
        assert_eq!(sel(&state), (5, 8));
        assert!(state.is_excluded(&(0..3)));
        assert_eq!(state.count().total(), 3);
        state.replace_all(false);
        assert_eq!(state.text, "foo(\"bar\") // bar\nbar");
        assert!(state.set_query(&options("o")));
        assert!(state.excluded.is_empty());
    }

    // src/editor/core/cm/find.test.ts › search context, exclude and preserve case › excluding an excluded match
    // includes it again
    #[test]
    fn excluding_an_excluded_match_includes_it_again() {
        let mut state = make_code(&options("foo"));
        state.toggle_excluded(0..3);
        state.selection = 0..3;
        assert!(state.exclude_current());
        assert!(!state.is_excluded(&(0..3)));
        assert_eq!(sel(&state), (0, 3), "including an occurrence again leaves the selection on it");
    }

    // src/editor/core/cm/find.test.ts › search context, exclude and preserve case › preserve case keeps each
    // occurrence's case
    #[test]
    fn preserve_case_keeps_each_occurrences_case() {
        let mut all = make("foo Foo FOO", &replacing("foo", "bar"), None);
        all.replace_all(true);
        assert_eq!(all.text, "bar Bar BAR");
        let mut one = make("Foo foo", &replacing("foo", "bar"), None);
        one.find_next();
        one.replace_next(true);
        assert_eq!(one.text, "Bar foo");
        assert_eq!(sel(&one), (4, 7));
    }

    // src/editor/core/cm/find.test.ts › in-file Find result lines › preserves matching options %j and UTF-16
    // columns
    #[test]
    fn preserves_matching_options_and_utf16_columns() {
        let doc = "😀 foo FOO foobar\nfoo";
        let cases: [(FindOptions, LineRanges); 7] = [
            (options("foo"), vec![(0, vec![(3, 6), (7, 10), (11, 14)]), (1, vec![(0, 3)])]),
            (
                FindOptions { case_sensitive: true, ..options("foo") },
                vec![(0, vec![(3, 6), (11, 14)]), (1, vec![(0, 3)])],
            ),
            (FindOptions { whole_word: true, ..options("foo") }, vec![(0, vec![(3, 6), (7, 10)]), (1, vec![(0, 3)])]),
            (
                FindOptions { regexp: true, case_sensitive: true, ..options("f.o") },
                vec![(0, vec![(3, 6), (11, 14)]), (1, vec![(0, 3)])],
            ),
            (options("missing"), Vec::new()),
            (options(""), Vec::new()),
            (FindOptions { regexp: true, ..options("[") }, Vec::new()),
        ];
        for (query, expected) in cases {
            let state = make(doc, &query, None);
            assert_eq!(line_ranges(&state.result_lines(&query)), expected, "{query:?}");
        }
    }

    // src/editor/core/cm/find.test.ts › in-file Find result lines › searches edited text in the selection scope,
    // grouping ranges on the same line
    #[test]
    fn searches_edited_text_in_the_selection_scope_grouping_ranges_on_the_same_line() {
        let query = options("foo");
        let mut state = make("foo\nfoo foo\nfoo", &query, Some(4..11));
        state.apply(&[change(4, 7, "FOO")]);
        let expected = LineMatch { line: 1, text: "FOO foo".to_owned(), text_offset: 0, ranges: vec![(0, 3), (4, 7)] };
        assert_eq!(state.result_lines(&query), [expected]);
    }

    // src/editor/core/cm/find.test.ts › in-file Find result lines › splits multiline matches into highlighted
    // result lines
    #[test]
    fn splits_multiline_matches_into_highlighted_result_lines() {
        let query = options("foo\nbar");
        let state = make("before foo\nbar after", &query, None);
        let line =
            |line, text: &str, range| LineMatch { line, text: text.to_owned(), text_offset: 0, ranges: vec![range] };
        assert_eq!(state.result_lines(&query), [line(0, "before foo", (7, 10)), line(1, "bar after", (0, 3))]);
    }

    // src/editor/core/cm/find.test.ts › in-file Find result lines › retains excluded occurrences
    #[test]
    fn retains_excluded_occurrences() {
        let query = options("foo");
        let mut state = make("foo foo foo", &query, None);
        state.toggle_excluded(4..7);
        assert_eq!(state.result_lines(&query)[0].ranges, [(0, 3), (8, 11)]);
    }

    // src/editor/core/cm/find.test.ts › in-file Find result lines › exports every match beyond the bar counter
    // limit
    #[test]
    fn exports_every_match_beyond_the_bar_counter_limit() {
        let query = options("foo");
        let state = make(&"foo\n".repeat(10_005), &query, None);
        assert_eq!(state.result_lines(&query).len(), 10_005);
        let counted = state.count();
        assert_eq!((counted.total(), counted.capped), (MAX_COUNT, true));
    }

    // Not from a spec: without a scope Next and Previous go round the whole text, from the selection's ends.
    #[test]
    fn next_and_previous_go_round_the_whole_text() {
        let mut state = make("ab ab ab", &options("ab"), None);
        state.selection = 4..4;
        assert!(state.find_next());
        assert_eq!(sel(&state), (6, 8));
        state.find_next();
        assert_eq!(sel(&state), (0, 2));
        state.find_previous();
        assert_eq!(sel(&state), (6, 8));
        state.find_previous();
        assert_eq!(sel(&state), (3, 5));
        assert_eq!(state.count().current, Some(1));
        let mut none = make("ab", &options("x"), None);
        assert!(!none.find_next() && !none.find_previous());
        assert_eq!(sel(&none), (0, 0));
    }

    // Not from a spec: Replace on a selection that is no occurrence only goes to the next one.
    #[test]
    fn replace_goes_to_the_next_occurrence_before_it_replaces() {
        let mut state = make("one two one", &replacing("one", "1"), None);
        state.selection = 2..2;
        assert!(state.replace_next(false).is_empty());
        assert_eq!((state.text.as_str(), sel(&state)), ("one two one", (8, 11)));
        assert_eq!(state.replace_next(false), [change(8, 11, "1")]);
        assert_eq!((state.text.as_str(), sel(&state)), ("one two 1", (0, 3)));
        state.replace_next(false);
        assert_eq!((state.text.as_str(), sel(&state)), ("1 two 1", (1, 1)), "nothing is left to select");
    }

    // Not from a spec: what a regular expression's replacement stands for.
    #[test]
    fn regex_replacements_fill_in_groups_escapes_and_the_match() {
        let replaced = |search: &str, replace: &str, doc: &str| {
            let mut state = make(doc, &FindOptions { regexp: true, ..replacing(search, replace) }, None);
            state.replace_all(false);
            state.text
        };
        assert_eq!(replaced("(\\w+)@(\\w+)", "$2 at $1", "me@home"), "home at me");
        assert_eq!(replaced("b", "[$&]", "abc"), "a[b]c");
        assert_eq!(replaced("b", "$$1 $0 $7", "abc"), "a$1 $0 $7c");
        assert_eq!(replaced("(b)", "$12", "abc"), "ab2c", "the longest group number the pattern has");
        assert_eq!(replaced("(b)|(c)", "<$1$2>", "abc"), "a<b><c>", "a group that did not match is empty");
        assert_eq!(replaced("b", "\\n\\t\\\\\\q", "abc"), "a\n\t\\\\qc");
        assert_eq!(replaced("b", "\\r\\n", "abc"), "a\nc", "a document holds line feeds only");
        assert_eq!(replaced("^", "> ", "a\nb"), "a\nb", "a match of no length is no occurrence");
        assert_eq!(replaced("a$", "!", "a\nba"), "!\nb!", "$ matches at the end of every line");
        assert_eq!(replaced("A", "x", "aA"), "xx", "case is ignored unless asked for");
    }

    // Not from a spec: a whole word has no word character beside the word characters at its ends.
    #[test]
    fn whole_words_stand_alone() {
        let starts = |search: &str, regexp: bool, doc: &str| {
            make(doc, &FindOptions { whole_word: true, regexp, ..options(search) }, None).count().starts()
        };
        assert_eq!(starts("foo", false, "foo foo_bar xfoo (foo) föo foo"), [0, 18, 27]);
        assert_eq!(starts("(foo)", false, "x(foo)x"), [1], "punctuation at the ends needs no boundary");
        assert_eq!(starts("a.a", false, "ba.a.a"), [3], "an occurrence overlapping a rejected one is found");
        assert_eq!(starts("fo+", true, "fo foo xfoo"), [0, 3]);
        let scoped = make("xfoo foo", &FindOptions { whole_word: true, ..options("foo") }, Some(1..8));
        assert_eq!(scoped.count().starts(), [5], "the text before the scope still counts as its neighbour");
    }

    // Not from a spec: the scope ends the text for the search and keeps what lies before it in view.
    #[test]
    fn the_scope_limits_the_search_without_hiding_what_precedes_it() {
        let regex = |search: &str| FindOptions { regexp: true, ..options(search) };
        assert_eq!(make("aaa", &options("aa"), Some(1..3)).count().starts(), [1]);
        assert_eq!(make("ab\nab", &regex("^ab"), Some(3..5)).count().starts(), [3]);
        assert_eq!(make("xab ab", &regex("^ab"), Some(1..6)).count().total(), 0, "the scope starts inside a line");
        assert_eq!(make("aaaa", &regex("a+"), Some(1..3)).count().ranges, std::slice::from_ref(&(1..3)));
        assert_eq!(make("😀a😀a", &options("a"), Some(3..6)).count().starts(), [5]);
    }

    // Not from a spec: "In Selection" takes the larger of the selection the bar opened over and the current one.
    #[test]
    fn in_selection_toggles_between_a_selection_and_the_file() {
        let mut state = make("one\ntwo\nthree", &options("t"), None);
        assert!(!state.toggle_scope(None), "nothing is selected");
        state.selection = 4..5;
        assert!(state.toggle_scope(Some(&(0..7))));
        assert_eq!(state.scope, Some(0..7));
        assert!(state.toggle_scope(Some(&(0..7))));
        assert_eq!(state.scope, None);
        assert!(state.toggle_scope(Some(&(0..99))), "a saved selection past the end is not used");
        assert_eq!(state.scope, Some(4..5));
        state.apply(&[change(0, 13, "t")]);
        assert_eq!(state.scope, None, "a scope whose text is gone ends");
    }

    // Not from a spec: an edit moves the selection, the excluded occurrences and the literals with the text.
    #[test]
    fn edits_move_what_the_state_keeps_about_the_text() {
        let mut state = make_code(&options("foo"));
        state.toggle_excluded(5..8);
        state.selection = 14..17;
        state.apply(&[change(0, 0, "😀"), change(5, 8, "")]);
        assert_eq!(state.text, "😀foo(\"\") // foo\nfoo");
        assert_eq!(sel(&state), (13, 16));
        assert!(state.excluded.is_empty(), "an excluded occurrence that is deleted is forgotten");
        let literals = [
            LiteralRange { from: 6, to: 8, kind: LiteralKind::String },
            LiteralRange { from: 10, to: 16, kind: LiteralKind::Comment },
        ];
        assert_eq!(state.literals, literals);
        assert_eq!(state.count().starts(), [2, 13, 17]);
    }

    // Not from a spec: several edits as one, and the edit between two texts.
    #[test]
    fn merges_edits_and_finds_the_edit_between_two_texts() {
        let doc = "😀 foo FOO foo";
        let edits = [change(3, 6, "x"), change(11, 14, "long")];
        assert_eq!(merged(doc, &edits), Some(change(3, 14, "x FOO long")));
        assert_eq!(merged(doc, &edits[..1]), Some(change(3, 6, "x")));
        assert_eq!(merged(doc, &[]), None);
        assert_eq!(apply_changes(doc, &edits), "😀 x FOO long");

        assert_eq!(difference("abc", "abc"), None);
        assert_eq!(difference("😀 abc", "😀 aXc"), Some(change(4, 5, "X")));
        assert_eq!(difference("aa", "aaa"), Some(change(2, 2, "a")));
        assert_eq!(difference("abc", ""), Some(change(0, 3, "")));
        assert_eq!(difference("é", "è"), Some(change(0, 1, "è")));
    }

    // Not from a spec: what the bar starts with when it opens over a selection.
    #[test]
    fn a_selection_seeds_the_query() {
        let mut find = options("old");
        assert_eq!(seed_options(&mut find, "", false), SeedScope::Unchanged);
        assert_eq!(find.search, "old");
        assert_eq!(seed_options(&mut find, "new", false), SeedScope::File);
        assert_eq!(find.search, "new");
        assert_eq!(seed_options(&mut find, "two\nlines", false), SeedScope::File);
        assert_eq!(find.search, "two\nlines", "Find searches for several selected lines");
        assert_eq!(seed_options(&mut find, "", true), SeedScope::Unchanged);
        assert_eq!(find.search, "", "a query of several lines is not kept without a selection");
        find.search = "kept".to_owned();
        assert_eq!(seed_options(&mut find, "two\nlines", true), SeedScope::Selection);
        assert_eq!(find.search, "", "Replace searches in several selected lines");
        assert_eq!(seed_options(&mut find, &"x".repeat(MAX_QUERY as usize), false), SeedScope::File);
        assert_eq!(find.search, "", "a selection too long to be a query is not one");
        find.regexp = true;
        seed_options(&mut find, "a.b(c)", false);
        assert_eq!(find.search, "a\\.b\\(c\\)");
        assert_eq!(make("xa.b(c)", &find, None).count().starts(), [1]);
    }

    // Not from a spec: the counter beside the search field.
    #[test]
    fn the_counter_names_the_current_occurrence_of_the_total() {
        let counter = |state: &FindState| counter_text(state.query(), &state.count());
        let mut state = make("foo foo", &options("foo"), None);
        assert_eq!(counter(&state), "?/2");
        state.find_next();
        assert_eq!(counter(&state), "1/2");
        state.find_next();
        assert_eq!(counter(&state), "2/2");
        state.set_query(&options("missing"));
        assert_eq!(counter(&state), "0 results");
        state.set_query(&options(""));
        assert_eq!(counter(&state), "");
        state.set_query(&FindOptions { regexp: true, ..options("[") });
        assert_eq!(counter(&state), "Bad regex");
        assert!(state.query().is_invalid() && !regex_valid("[") && regex_valid("") && regex_valid("f.o"));
        let many = make(&"a".repeat(MAX_COUNT + 5), &options("a"), None);
        assert_eq!(counter(&many), format!("?/{MAX_COUNT}+"));
    }

    // Not from a spec: changing how a replacement is written keeps the excluded occurrences; changing what is
    // found or what replaces it includes them again.
    #[test]
    fn only_a_different_query_forgets_the_excluded_occurrences() {
        let mut state = make("foo foo", &replacing("foo", "bar"), None);
        state.toggle_excluded(0..3);
        assert!(!state.set_query(&FindOptions { preserve_case: true, ..replacing("foo", "bar") }));
        assert!(state.is_excluded(&(0..3)) && state.query().options().preserve_case);
        assert!(state.set_query(&replacing("foo", "baz")));
        assert!(state.excluded.is_empty());
    }

    // Not from a spec: lines and marks of the current occurrence.
    #[test]
    fn finds_lines_and_marks_the_selected_occurrence() {
        assert_eq!(line_of("a\n😀\nb", 0), 0);
        assert_eq!(line_of("a\n😀\nb", 4), 1);
        assert_eq!(line_of("a\n😀\nb", 5), 2);
        assert_eq!(line_of("a\nb", 99), 1);
        let mut found = make("ab ab", &options("ab"), None).count();
        found.select(&(3..5));
        assert_eq!(found.current, Some(1));
        found.select(&(3..4));
        assert_eq!(found.current, None);
    }
}
