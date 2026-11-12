//! Execution point in the editor: the suspended line (background only, as in IDEA) and inline
//! values of the frame's locals after line ends.

use crate::debugger::values::{identifiers_in, inline_value};
use crate::editor::buffer::{self, Assoc};
use crate::find::utf16;
use ide_core::TextChange;
use ide_dap::Variable;
use ropey::Rope;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// How far above the execution point inline values are shown, in lines.
const INLINE_SPAN: u32 = 60;
const MAX_PER_LINE: usize = 4;
/// The longest value shown inline, in UTF-16 code units; a longer one gets a "Show" button.
const INLINE_MAX: u32 = 40;

/// One `name: value` of a line's inline values.
#[derive(Debug, Clone)]
pub struct InlinePart {
    pub name: String,
    /// `name: value`, the value shortened.
    pub text: String,
    /// The variable behind it, for the hint a click shows.
    pub variable: Variable,
    pub truncated: bool,
}

/// `name: value` pairs for the identifiers on `text` that are locals of the frame, in the order
/// they appear; `None` when there are none.
pub fn inline_values_for(text: &str, locals: &BTreeMap<String, Variable>) -> Option<Vec<InlinePart>> {
    let parts: Vec<InlinePart> = identifiers_in(text)
        .into_iter()
        .filter_map(|id| {
            let variable = locals.get(&id)?;
            Some(InlinePart {
                text: format!("{id}: {}", inline_value(&variable.value, INLINE_MAX)),
                truncated: utf16::len(&variable.value) > INLINE_MAX,
                variable: variable.clone(),
                name: id,
            })
        })
        .take(MAX_PER_LINE)
        .collect();
    (!parts.is_empty()).then_some(parts)
}

/// The execution point in one editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exec {
    pub line: u32,
    /// The selected frame is the top frame (current position, not a caller).
    pub top: bool,
}

/// The execution point of the editor showing `path`, given where the selected frame is.
pub fn current_exec(path: &str, point: Option<(&str, u32, bool)>) -> Option<Exec> {
    point.filter(|(at, ..)| *at == path).map(|(_, line, top)| Exec { line, top })
}

/// Whether the editor's execution line has to be redrawn for `next`.
pub fn exec_moved(current: Option<Exec>, next: Option<Exec>) -> bool {
    current != next
}

/// The execution point after an edit: it follows the start of its line.
pub fn map_exec(exec: Exec, old_doc: &Rope, changes: &[TextChange], new_doc: &Rope) -> Exec {
    if changes.is_empty() || exec.line >= buffer::line_count(old_doc) {
        return exec;
    }
    let start = buffer::line_range(old_doc, exec.line).start;
    Exec { line: buffer::line_col(new_doc, buffer::map_offset(start, changes, Assoc::After)).0, ..exec }
}

/// The line whose background marks the execution point, when the document has it.
pub fn exec_line(doc: &Rope, exec: Option<Exec>) -> Option<u32> {
    exec.map(|exec| exec.line).filter(|line| *line < buffer::line_count(doc))
}

/// The inline values of the `visible` lines: from [`INLINE_SPAN`] lines above the execution point
/// down to it. `enabled`: Settings › Debugger › "Show values inline".
pub fn inline_values_in(
    doc: &Rope,
    exec: Option<Exec>,
    visible: RangeInclusive<u32>,
    locals: &BTreeMap<String, Variable>,
    enabled: bool,
) -> Vec<(u32, Vec<InlinePart>)> {
    let Some(exec) = exec.filter(|_| enabled && !locals.is_empty()) else { return Vec::new() };
    let first = exec.line.saturating_sub(INLINE_SPAN).max(*visible.start());
    let last = exec.line.min(*visible.end()).min(buffer::line_count(doc).saturating_sub(1));
    (first..=last).filter_map(|line| Some((line, inline_values_for(&buffer::line_text(doc, line), locals)?))).collect()
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A local variable `name` with `value`.
    pub fn local(name: &str, value: &str) -> Variable {
        Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            type_name: None,
            variables_reference: 0,
            evaluate_name: Some(name.to_owned()),
        }
    }

    /// The locals of a frame, by name.
    pub fn locals(values: &[(&str, &str)]) -> BTreeMap<String, Variable> {
        values.iter().map(|(name, value)| ((*name).to_owned(), local(name, value))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::locals;
    use super::*;

    fn texts(parts: Option<Vec<InlinePart>>) -> Option<String> {
        parts.map(|parts| parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(", "))
    }

    // src/features/debugger/__tests__/values.test.ts › value classification
    #[test]
    fn formats_inline_values_for_known_locals_on_a_line() {
        let values = locals(&[("req", "CreateOrderRequest@4127"), ("total", "3"), ("unused", "1")]);
        assert_eq!(
            texts(inline_values_for("fun create(req: CreateOrderRequest): OrderDto {", &values)).as_deref(),
            Some("req: CreateOrderRequest@4127"),
        );
        assert!(inline_values_for("return 1", &values).is_none());
    }

    // src/features/debugger/__tests__/values.test.ts › value classification › only displays an own local named %s
    #[test]
    fn only_displays_an_own_local_whatever_its_name() {
        let names = [
            "constructor",
            "__defineGetter__",
            "__defineSetter__",
            "hasOwnProperty",
            "__lookupGetter__",
            "__lookupSetter__",
            "isPrototypeOf",
            "propertyIsEnumerable",
            "toString",
            "valueOf",
            "__proto__",
            "toLocaleString",
        ];
        for name in names {
            assert!(inline_values_for(name, &locals(&[("count", "40")])).is_none(), "{name}");
            let own = inline_values_for(name, &locals(&[(name, "known")]));
            assert_eq!(texts(own), Some(format!("{name}: known")));
        }
    }

    // src/features/debugger/__tests__/editor.test.ts › execution point
    #[test]
    fn highlights_the_suspended_line_and_shows_inline_values() {
        let doc = Rope::from_str("x = 1\ny = x + 1\nprint(y)");
        let exec = current_exec("/proj/a.py", Some(("/proj/a.py", 1, true)));
        assert_eq!(exec_line(&doc, exec), Some(1));
        let inline = inline_values_in(&doc, exec, 0..=2, &locals(&[("x", "1"), ("y", "2")]), true);
        let shown: Vec<(u32, Option<String>)> =
            inline.into_iter().map(|(line, parts)| (line, texts(Some(parts)))).collect();
        assert_eq!(shown, [(0, Some("x: 1".to_owned())), (1, Some("y: 2, x: 1".to_owned()))]);
        // Resumed: no line, no values.
        let gone = current_exec("/proj/a.py", None);
        assert_eq!(exec_line(&doc, gone), None);
        assert!(inline_values_in(&doc, gone, 0..=2, &locals(&[("x", "1")]), true).is_empty());
    }

    // src/features/debugger/__tests__/editor.test.ts › execution point
    #[test]
    fn keeps_refreshing_the_execution_line_around_object_prototype_names() {
        let doc = Rope::from_str("String text = Integer.toString(40);\nint count = 40;\ncount++;\ncount++;");
        let first = current_exec("/proj/a.py", Some(("/proj/a.py", 1, true)));
        let next = current_exec("/proj/a.py", Some(("/proj/a.py", 3, true)));
        assert!(exec_moved(first, next));
        assert_eq!(exec_line(&doc, next), Some(3));
        let inline = inline_values_in(&doc, next, 0..=3, &locals(&[("text", "40"), ("count", "42")]), true);
        let lines: Vec<u32> = inline.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [0, 1, 2, 3]);
        assert_eq!(texts(Some(inline[0].1.clone())).as_deref(), Some("text: 40"));
        assert!(exec_moved(next, current_exec("/proj/a.py", None)));
    }

    // src/features/debugger/__tests__/editor.test.ts › execution point › refreshes a cached editor state when
    // remounted at %s
    #[test]
    fn an_editor_shown_again_takes_the_execution_point_of_the_moment() {
        let doc = Rope::from_str("first\nsecond\nthird");
        let cached = current_exec("/proj/a.py", Some(("/proj/a.py", 0, true)));
        for line in [Some(2), None] {
            let now = current_exec("/proj/a.py", line.map(|line| ("/proj/a.py", line, true)));
            assert!(exec_moved(cached, now));
            assert_eq!(exec_line(&doc, now), line);
        }
        // Another file's execution point is none of this editor's.
        assert_eq!(current_exec("/proj/a.py", Some(("/proj/b.py", 2, true))), None);
    }

    // Not from a spec: the execution line follows its text through edits, values are off with the
    // setting, long values are marked for the "Show" button, and a line has at most four values.
    #[test]
    fn follows_edits_and_limits_what_a_line_shows() {
        let old = Rope::from_str("a\nb\nc");
        let new = Rope::from_str("zero\na\nb\nc");
        let changes = [TextChange { from: 0, to: 0, insert: "zero\n".to_owned() }];
        assert_eq!(map_exec(Exec { line: 1, top: false }, &old, &changes, &new), Exec { line: 2, top: false });
        assert_eq!(map_exec(Exec { line: 9, top: true }, &old, &changes, &new), Exec { line: 9, top: true });
        let doc = Rope::from_str("a + b + c + d + e");
        let exec = Some(Exec { line: 0, top: true });
        let many = locals(&[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4"), ("e", "5")]);
        assert!(inline_values_in(&doc, exec, 0..=0, &many, false).is_empty());
        assert_eq!(inline_values_in(&doc, exec, 0..=0, &many, true)[0].1.len(), 4);
        let long = "x".repeat(41);
        let parts = inline_values_for("a", &locals(&[("a", long.as_str())])).expect("one value");
        assert!(parts[0].truncated && parts[0].text.ends_with('…') && parts[0].variable.value == long);
    }
}
