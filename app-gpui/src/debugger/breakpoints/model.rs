//! Line breakpoint lists and the operations on them (0-based lines).

use crate::editor::buffer::{self, Assoc};
use crate::platform::local_time;
use ide_core::TextChange;
use ide_dap::Breakpoint;
use ropey::Rope;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointEntry {
    pub id: String,
    pub line: u32,
    pub enabled: bool,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub hit_condition: Option<String>,
    /// Temporary line breakpoint: removed after the first hit.
    pub temporary: bool,
    /// `false`: the breakpoint never suspends (IDEA's "Suspend" unchecked).
    pub suspend: bool,
    /// Log the "Breakpoint reached at file:line" message on every hit.
    pub log_hit: bool,
}

/// What a session's adapter made of a breakpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointStatus {
    pub verified: bool,
    pub message: Option<String>,
    /// Line the adapter actually bound to (may differ from the requested one).
    pub line: u32,
}

/// The properties of a new breakpoint that differ from a plain one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreakpointProps {
    /// `None`: enabled.
    pub enabled: Option<bool>,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub hit_condition: Option<String>,
    pub temporary: bool,
    /// `None`: it suspends.
    pub suspend: Option<bool>,
    pub log_hit: bool,
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// An id no other breakpoint of this process has.
pub fn new_breakpoint_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    format!("bp{}-{seq}", base36(local_time::now_ms() as u64))
}

pub fn make_breakpoint(line: u32, props: BreakpointProps) -> BreakpointEntry {
    BreakpointEntry {
        id: new_breakpoint_id(),
        line,
        enabled: props.enabled.unwrap_or(true),
        condition: props.condition,
        log_message: props.log_message,
        hit_condition: props.hit_condition,
        temporary: props.temporary,
        suspend: props.suspend.unwrap_or(true),
        log_hit: props.log_hit,
    }
}

/// The "Breakpoint hit" message IDEA logs for `path:line` (0-based line).
pub fn hit_message(path: &str, line: u32) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    format!("Breakpoint reached at {name}:{}", line + 1)
}

fn trimmed(text: &Option<String>) -> Option<&str> {
    text.as_deref().map(str::trim).filter(|text| !text.is_empty())
}

/// What a hit logs: the hit message and/or the "Evaluate and log" template, one per line.
pub fn log_parts(b: &BreakpointEntry, path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if b.log_hit {
        parts.push(hit_message(path, b.line));
    }
    if let Some(message) = trimmed(&b.log_message) {
        parts.push(message.to_owned());
    }
    parts
}

/// A piece of a log template (DAP logpoint syntax).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogPiece {
    Text(String),
    /// A `{expression}` placeholder, trimmed.
    Expression(String),
}

/// Splits a log template into its text and its `{expression}` placeholders.
pub fn log_pieces(template: &str) -> Vec<LogPiece> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let inner = &rest[open + 1..];
        match inner.find(['{', '}']) {
            Some(close) if close > 0 && inner[close..].starts_with('}') => {
                text.push_str(&rest[..open]);
                pieces.push(LogPiece::Text(std::mem::take(&mut text)));
                pieces.push(LogPiece::Expression(inner[..close].trim().to_owned()));
                rest = &inner[close + 1..];
            }
            _ => {
                text.push_str(&rest[..=open]);
                rest = inner;
            }
        }
    }
    text.push_str(rest);
    pieces.push(LogPiece::Text(text));
    pieces
}

/// The expressions of a log template, in order.
pub fn log_expressions(template: &str) -> Vec<String> {
    log_pieces(template)
        .into_iter()
        .filter_map(|piece| match piece {
            LogPiece::Expression(expression) => Some(expression),
            LogPiece::Text(_) => None,
        })
        .collect()
}

/// The template with each placeholder replaced by its value: `values` are the results of
/// [`log_expressions`], in order. A failure renders as the error text, like adapters do.
pub fn render_log(template: &str, values: &[Result<String, String>]) -> String {
    let mut values = values.iter();
    log_pieces(template)
        .into_iter()
        .map(|piece| match piece {
            LogPiece::Text(text) => text,
            LogPiece::Expression(_) => match values.next() {
                Some(Ok(value)) => value.clone(),
                Some(Err(error)) => format!("<{error}>"),
                None => String::new(),
            },
        })
        .collect()
}

/// Fills the `{expression}` placeholders of a log template with `evaluate`.
pub fn interpolate_log(template: &str, evaluate: &mut dyn FnMut(&str) -> Result<String, String>) -> String {
    let values: Vec<Result<String, String>> =
        log_expressions(template).iter().map(|expression| evaluate(expression)).collect();
    render_log(template, &values)
}

/// `list` ordered by line; breakpoints on the same line keep their order.
pub fn sort_by_line(mut list: Vec<BreakpointEntry>) -> Vec<BreakpointEntry> {
    list.sort_by_key(|b| b.line);
    list
}

/// Adds a breakpoint on `line`, or removes the one there.
pub fn toggle_line(list: &[BreakpointEntry], line: u32) -> Vec<BreakpointEntry> {
    let mut next: Vec<BreakpointEntry> = list.iter().filter(|b| b.line != line).cloned().collect();
    if next.len() == list.len() {
        next.push(make_breakpoint(line, BreakpointProps::default()));
        next = sort_by_line(next);
    }
    next
}

/// True when the breakpoint has a condition, hit condition or logs something.
pub fn is_conditional(b: &BreakpointEntry) -> bool {
    trimmed(&b.condition).is_some()
        || trimmed(&b.hit_condition).is_some()
        || trimmed(&b.log_message).is_some()
        || b.log_hit
}

/// Maps lines through an edit: each kept line as (index into `lines`, new line), ordered by the old
/// line. A line follows its start; when lines merge, the first one wins and the others are dropped.
pub fn map_line_numbers(lines: &[u32], old_doc: &Rope, changes: &[TextChange], new_doc: &Rope) -> Vec<(usize, u32)> {
    if changes.is_empty() {
        return lines.iter().copied().enumerate().collect();
    }
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by_key(|index| lines[*index]);
    let old_lines = buffer::line_count(old_doc);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for index in order {
        let line = lines[index];
        let mapped = if line >= old_lines {
            line
        } else {
            // Text inserted at the very start of the line pushes the breakpoint down with it.
            let start = buffer::line_range(old_doc, line).start;
            buffer::line_col(new_doc, buffer::map_offset(start, changes, Assoc::After)).0
        };
        if seen.insert(mapped) {
            out.push((index, mapped));
        }
    }
    out
}

/// Maps breakpoint lines through an edit (see [`map_line_numbers`]).
pub fn map_lines(
    list: &[BreakpointEntry],
    old_doc: &Rope,
    changes: &[TextChange],
    new_doc: &Rope,
) -> Vec<BreakpointEntry> {
    let lines: Vec<u32> = list.iter().map(|b| b.line).collect();
    map_line_numbers(&lines, old_doc, changes, new_doc)
        .into_iter()
        .map(|(index, line)| BreakpointEntry { line, ..list[index].clone() })
        .collect()
}

/// The list sent to the backend; muting disables everything without touching the stored flags.
/// A non-suspending breakpoint becomes a DAP logpoint; one that logs nothing has no DAP
/// equivalent and stays disabled. Suspending breakpoints log from the UI when they stop.
pub fn to_backend(list: &[BreakpointEntry], muted: bool, path: &str) -> Vec<Breakpoint> {
    list.iter()
        .map(|b| {
            let logged = || Some(log_parts(b, path).join("\n")).filter(|log| !log.is_empty());
            let log = if b.suspend { None } else { logged() };
            Breakpoint {
                line: b.line,
                condition: trimmed(&b.condition).map(str::to_owned),
                hit_condition: trimmed(&b.hit_condition).map(str::to_owned),
                enabled: b.enabled && !muted && (b.suspend || log.is_some()),
                log_message: log,
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A breakpoint on `line` with nothing else set.
    pub fn plain(line: u32) -> BreakpointEntry {
        make_breakpoint(line, BreakpointProps::default())
    }

    /// `text` after replacing the UTF-16 range `from..to` with `insert`, and the change that did it.
    pub fn edit(text: &Rope, from: u32, to: u32, insert: &str) -> (Vec<TextChange>, Rope) {
        let mut next = text.clone();
        let (start, end) = (next.utf16_cu_to_char(from as usize), next.utf16_cu_to_char(to as usize));
        next.remove(start..end);
        next.insert(start, insert);
        (vec![TextChange { from, to, insert: insert.to_owned() }], next)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{edit, plain};
    use super::*;

    fn doc(lines: usize) -> Rope {
        Rope::from_str(&(0..lines).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"))
    }

    fn line_start(text: &Rope, line: u32) -> u32 {
        buffer::line_range(text, line).start
    }

    fn lines(list: &[BreakpointEntry]) -> Vec<u32> {
        list.iter().map(|b| b.line).collect()
    }

    fn backend(line: u32, condition: Option<&str>, log_message: Option<&str>, enabled: bool) -> Breakpoint {
        Breakpoint {
            line,
            condition: condition.map(str::to_owned),
            hit_condition: None,
            log_message: log_message.map(str::to_owned),
            enabled,
        }
    }

    #[test]
    fn toggles_a_line_on_and_off_keeping_the_list_sorted() {
        let mut list = toggle_line(&[], 5);
        list = toggle_line(&list, 2);
        assert_eq!(lines(&list), [2, 5]);
        list = toggle_line(&list, 5);
        assert_eq!(lines(&list), [2]);
    }

    #[test]
    fn treats_conditions_hit_counts_and_log_messages_as_conditional() {
        let with = |props: BreakpointProps| is_conditional(&make_breakpoint(1, props));
        assert!(!with(BreakpointProps::default()));
        assert!(with(BreakpointProps { condition: Some("x > 1".to_owned()), ..Default::default() }));
        assert!(!with(BreakpointProps { condition: Some("  ".to_owned()), ..Default::default() }));
        assert!(with(BreakpointProps { log_message: Some("x={x}".to_owned()), ..Default::default() }));
        assert!(with(BreakpointProps { hit_condition: Some("3".to_owned()), ..Default::default() }));
    }

    #[test]
    fn maps_lines_down_when_lines_are_inserted_above() {
        let text = doc(10);
        let list = [plain(3), plain(7)];
        // Insert two lines at the start of line 1.
        let at = line_start(&text, 1);
        let (changes, next) = edit(&text, at, at, "a\nb\n");
        assert_eq!(lines(&map_lines(&list, &text, &changes, &next)), [5, 9]);
    }

    #[test]
    fn moves_a_breakpoint_with_its_line_when_text_is_inserted_at_the_line_start() {
        let text = doc(5);
        let at = line_start(&text, 2);
        let (changes, next) = edit(&text, at, at, "\n");
        assert_eq!(lines(&map_lines(&[plain(2)], &text, &changes, &next)), [3]);
    }

    #[test]
    fn keeps_breakpoints_above_an_edit_and_preserves_identity() {
        let text = doc(5);
        let bp = plain(1);
        let at = line_start(&text, 3);
        let (changes, next) = edit(&text, at, at, "x\n");
        assert_eq!(map_lines(std::slice::from_ref(&bp), &text, &changes, &next), [bp]);
    }

    #[test]
    fn maps_up_on_deletion_and_merges_breakpoints_that_collapse_onto_one_line() {
        let text = doc(10);
        let list = [plain(2), plain(3), plain(8)];
        // Delete lines 2..3 (0-based) entirely.
        let (changes, next) = edit(&text, line_start(&text, 2), line_start(&text, 4), "");
        assert_eq!(lines(&map_lines(&list, &text, &changes, &next)), [2, 6]);
    }

    #[test]
    fn works_through_editor_transactions() {
        let text = Rope::from_str("a\nb\nc\nd");
        let (changes, next) = edit(&text, 0, 0, "zero\n");
        assert_eq!(lines(&map_lines(&[plain(3)], &text, &changes, &next)), [4]);
    }

    #[test]
    fn serialises_for_the_backend_muting_disables_everything() {
        let list = [
            make_breakpoint(1, BreakpointProps { condition: Some(" x ".to_owned()), ..Default::default() }),
            make_breakpoint(4, BreakpointProps { enabled: Some(false), ..Default::default() }),
        ];
        assert_eq!(to_backend(&list, false, ""), [backend(1, Some("x"), None, true), backend(4, None, None, false)]);
        assert!(to_backend(&list, true, "").iter().all(|b| !b.enabled));
    }

    #[test]
    fn sends_non_suspending_breakpoints_as_logpoints_with_the_hit_message_and_the_evaluated_template() {
        let quiet = |log_hit: bool, log_message: Option<&str>| BreakpointProps {
            suspend: Some(false),
            log_hit,
            log_message: log_message.map(str::to_owned),
            ..Default::default()
        };
        let list = [
            make_breakpoint(1, quiet(true, None)),
            make_breakpoint(2, quiet(true, Some("i={i}"))),
            make_breakpoint(3, quiet(false, None)),
            make_breakpoint(4, BreakpointProps { suspend: None, ..quiet(true, Some("i={i}")) }),
        ];
        assert_eq!(
            to_backend(&list, false, "/p/a.py"),
            [
                backend(1, None, Some("Breakpoint reached at a.py:2"), true),
                backend(2, None, Some("Breakpoint reached at a.py:3\ni={i}"), true),
                // Logs nothing and never suspends: no DAP equivalent.
                backend(3, None, None, false),
                // Suspending breakpoints log from the UI on stop.
                backend(4, None, None, true),
            ],
        );
    }

    #[test]
    fn interpolates_expressions_in_log_templates() {
        let out = interpolate_log("a={a} b={ b }", &mut |expression| {
            if expression == "b" { Err("undefined".to_owned()) } else { Ok(expression.to_uppercase()) }
        });
        assert_eq!(out, "a=A b=<undefined>");
    }

    // Not from a spec: braces that hold no expression are text, and ids never repeat.
    #[test]
    fn leaves_braces_without_an_expression_alone() {
        assert_eq!(log_expressions("{} {{a}} {b"), ["a"]);
        assert_eq!(render_log("{} {{a}} {b", &[Ok("1".to_owned())]), "{} {1} {b");
        assert_ne!(new_breakpoint_id(), new_breakpoint_id());
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }
}
