//! Smart Step Into in the editor, as in IDEA: the calls on the current line are highlighted in
//! place; ←/→ (or Tab / ⇧Tab) move between them, Enter or a click steps into the chosen one, Esc
//! cancels. Hovering a call selects it. Falls back to the chooser dialog when the calls cannot be
//! placed on the line.
//!
//! Columns are UTF-16 code units within the line.

use super::value_tooltip::is_identifier_char;
use crate::debugger::overlays::{InlineStepPick, InlineStepTarget, Overlays, SmartStep};
use crate::editor::buffer::{byte_of_col, col_of_byte};
use crate::find::utf16;
use ide_core::DebugSessionId;
use ide_dap::StepInTarget;
use std::collections::HashSet;

/// The callee name of a target label: `a.b.parse(str(i))` → `parse`.
pub fn callee_name(label: &str) -> Option<String> {
    let head = label.split('(').next().unwrap_or_default().trim();
    let name = head.rsplit(|c: char| matches!(c, '.' | ':' | '#') || c.is_whitespace()).next().unwrap_or_default();
    (!name.is_empty() && name.chars().all(is_identifier_char)).then(|| name.to_owned())
}

/// Whether a call starting at byte `from` of `text` is not the tail of a longer identifier.
fn is_call_boundary(text: &str, from: usize) -> bool {
    !text[..from].chars().next_back().is_some_and(is_identifier_char)
}

/// The calls of `name` on `text` (`name(`, blanks allowed before the parenthesis), as the UTF-16
/// column each starts at and whether it stands on its own. Calls do not overlap.
fn calls_of(text: &str, name: &str) -> Vec<(u32, bool)> {
    let mut calls = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(name) {
        let start = from + found;
        let after = text[start + name.len()..].trim_start();
        if after.starts_with('(') {
            calls.push((col_of_byte(text, start), is_call_boundary(text, start)));
            from = text.len() - after.len() + 1;
        } else {
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    calls
}

/// Where the identifier starting at column `from` ends.
fn identifier_end(text: &str, from: u32) -> u32 {
    let mut end = from;
    for c in text[byte_of_col(text, from)..].chars() {
        // A character outside the basic plane is two units, neither an identifier character.
        if c.len_utf16() != 1 || !is_identifier_char(c) {
            break;
        }
        end += 1;
    }
    end
}

/// Places each target on `line_text`: the adapter's columns when it reports them for this line,
/// else the next unused call of the target's name. `None` when any target cannot be placed.
/// Sorted by position.
pub fn locate_step_targets(line_text: &str, line: u32, targets: &[StepInTarget]) -> Option<Vec<InlineStepTarget>> {
    let len = utf16::len(line_text);
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for target in targets {
        let reported = target.column.filter(|column| target.line.is_none_or(|l| l == line) && *column < len);
        let (from, to) = match reported {
            Some(from) => {
                let to = match target.end_column {
                    Some(end) if end > from => end.min(len),
                    _ => identifier_end(line_text, from),
                };
                (from, to)
            }
            None => {
                let name = callee_name(&target.label)?;
                let call = calls_of(line_text, &name).into_iter().find(|(at, own)| *own && !used.contains(at));
                let (from, _) = call?;
                (from, from + utf16::len(&name))
            }
        };
        if to <= from || !used.insert(from) {
            return None;
        }
        out.push(InlineStepTarget { id: target.id, from, to });
    }
    out.sort_by_key(|placed| placed.from);
    Some(out)
}

/// Targets that belong to the paused line. Some adapters (debugpy) list every call left in the
/// function without positions; those are kept only when their callee appears on the line.
/// Unchanged when nothing would remain.
pub fn targets_on_line(line_text: &str, line: u32, targets: &[StepInTarget]) -> Vec<StepInTarget> {
    let on_line = |target: &&StepInTarget| {
        if let Some(target_line) = target.line {
            return target_line == line;
        }
        if target.column.is_some() {
            return true;
        }
        callee_name(&target.label).is_none_or(|name| calls_of(line_text, &name).iter().any(|(_, own)| *own))
    };
    let kept: Vec<StepInTarget> = targets.iter().filter(on_line).cloned().collect();
    if kept.is_empty() { targets.to_vec() } else { kept }
}

/// Targets of the paused line, when that line is open in an editor (`line_text`).
pub fn paused_line_targets(line_text: Option<&str>, line: u32, targets: Vec<StepInTarget>) -> Vec<StepInTarget> {
    match line_text {
        Some(text) => targets_on_line(text, line, &targets),
        None => targets,
    }
}

/// Where execution is suspended, as the editors see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PausedLine<'a> {
    pub path: &'a str,
    pub line: u32,
    /// The selected frame is the top frame.
    pub top: bool,
    /// The line's text, when an editor shows the file and has that line.
    pub text: Option<&'a str>,
}

/// Starts inline picking when the paused line is open in an editor and every target can be placed
/// on it; returns false otherwise (the caller shows the dialog). The editor showing the line
/// takes the focus.
pub fn start_inline_smart_step(
    overlays: &mut Overlays,
    paused: Option<&PausedLine>,
    session: DebugSessionId,
    thread: i64,
    targets: &[StepInTarget],
) -> bool {
    let Some(paused) = paused.filter(|paused| paused.top) else { return false };
    let Some(text) = paused.text else { return false };
    let Some(ranges) = locate_step_targets(text, paused.line, targets).filter(|ranges| ranges.len() >= 2) else {
        return false;
    };
    let first = targets.first().and_then(|first| ranges.iter().position(|r| r.id == first.id));
    let inline = InlineStepPick { path: paused.path.to_owned(), line: paused.line, ranges, index: first.unwrap_or(0) };
    overlays.smart_step = Some(SmartStep { session, thread, targets: targets.to_vec(), inline: Some(inline) });
    true
}

pub fn cancel_smart_step(overlays: &mut Overlays) {
    overlays.smart_step = None;
}

fn inline_pick(overlays: &mut Overlays) -> Option<&mut InlineStepPick> {
    overlays.smart_step.as_mut()?.inline.as_mut()
}

/// Selects the target at `index`, wrapping around at both ends.
pub fn select_step_target(overlays: &mut Overlays, index: i64) {
    if let Some(inline) = inline_pick(overlays).filter(|inline| !inline.ranges.is_empty()) {
        inline.index = index.rem_euclid(inline.ranges.len() as i64) as usize;
    }
}

/// The target to step into: the caller resumes the session with a step into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepChoice {
    pub session: DebugSessionId,
    pub thread: i64,
    pub target: i64,
}

/// Leaves picking mode with the target `id` (the selected one by default) chosen.
pub fn choose_step_target(overlays: &mut Overlays, id: Option<i64>) -> Option<StepChoice> {
    let step = overlays.smart_step.take()?;
    let selected = || step.inline.as_ref().and_then(|inline| inline.ranges.get(inline.index)).map(|r| r.id);
    let target = id.or_else(selected)?;
    Some(StepChoice { session: step.session, thread: step.thread, target })
}

/// What an input came to while picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickOutcome {
    /// Not picking, or a key that means nothing on its own: the input goes where it would have.
    Ignored,
    /// Another target is selected; the input is consumed.
    Moved,
    /// Step into this target; the input is consumed.
    Chosen(StepChoice),
    /// Picking ended without a choice. A key is consumed; a click goes on to what is under it.
    Cancelled,
}

const MODIFIERS: [&str; 5] = ["Shift", "Control", "Alt", "Meta", "CapsLock"];

/// A key pressed while picking (`key`: the DOM key name). Shortcuts are suspended meanwhile:
/// every key but a bare modifier is the picker's.
pub fn pick_key_down(overlays: &mut Overlays, key: &str, shift: bool) -> PickOutcome {
    let Some(index) = inline_pick(overlays).map(|inline| inline.index as i64) else { return PickOutcome::Ignored };
    if MODIFIERS.contains(&key) {
        return PickOutcome::Ignored;
    }
    match key {
        "ArrowLeft" | "ArrowUp" => select_step_target(overlays, index - 1),
        "Tab" if shift => select_step_target(overlays, index - 1),
        "ArrowRight" | "ArrowDown" | "Tab" => select_step_target(overlays, index + 1),
        "Enter" => return choose_step_target(overlays, None).map_or(PickOutcome::Cancelled, PickOutcome::Chosen),
        _ => {
            cancel_smart_step(overlays);
            return PickOutcome::Cancelled;
        }
    }
    PickOutcome::Moved
}

/// A mouse press while picking; `target`: the id of the highlighted call under the pointer.
pub fn pick_mouse_down(overlays: &mut Overlays, target: Option<i64>) -> PickOutcome {
    if inline_pick(overlays).is_none() {
        return PickOutcome::Ignored;
    }
    match target {
        Some(id) => choose_step_target(overlays, Some(id)).map_or(PickOutcome::Cancelled, PickOutcome::Chosen),
        None => {
            cancel_smart_step(overlays);
            PickOutcome::Cancelled
        }
    }
}

/// The pointer moved over the highlighted call `target`: it becomes the selected one.
pub fn pick_mouse_over(overlays: &mut Overlays, target: Option<i64>) {
    let Some(inline) = inline_pick(overlays) else { return };
    if let Some(index) = target.and_then(|id| inline.ranges.iter().position(|r| r.id == id)) {
        inline.index = index;
    }
}

/// The debugger moved on (another line, frame or file, or resumed): picking no longer applies.
pub fn execution_point_moved(overlays: &mut Overlays) {
    if inline_pick(overlays).is_some() {
        cancel_smart_step(overlays);
    }
}

/// A highlighted call of the line being picked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepMark {
    pub id: i64,
    pub from: u32,
    pub to: u32,
    pub selected: bool,
}

/// The calls to highlight in the editor of `path`. `line_len`: the length of the picked line in
/// that editor, `None` when it has no such line.
pub fn step_marks(overlays: &Overlays, path: &str, line_len: Option<u32>) -> Vec<StepMark> {
    let inline = overlays.smart_step.as_ref().and_then(|step| step.inline.as_ref()).filter(|i| i.path == path);
    let (Some(inline), Some(line_len)) = (inline, line_len) else { return Vec::new() };
    inline
        .ranges
        .iter()
        .enumerate()
        .filter(|(_, range)| range.to <= line_len)
        .map(|(index, range)| {
            let InlineStepTarget { id, from, to } = *range;
            StepMark { id, from, to, selected: index == inline.index }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: DebugSessionId = DebugSessionId(3);
    const LINE: &str = "    x = parse(str(i))";

    fn target(id: i64, label: &str) -> StepInTarget {
        StepInTarget { id, label: label.to_owned(), line: None, column: None, end_column: None }
    }

    fn at(id: i64, label: &str, column: u32, end_column: Option<u32>) -> StepInTarget {
        StepInTarget { column: Some(column), end_column, ..target(id, label) }
    }

    fn placed(id: i64, from: u32, to: u32) -> InlineStepTarget {
        InlineStepTarget { id, from, to }
    }

    fn ids(targets: &[StepInTarget]) -> Vec<i64> {
        targets.iter().map(|t| t.id).collect()
    }

    fn paused(path: &str) -> PausedLine<'_> {
        PausedLine { path, line: 1, top: true, text: (path == "/p/app.py").then_some(LINE) }
    }

    /// Picking between `str(i)` and `parse(str(i))` on the paused line.
    fn picking() -> Overlays {
        let mut overlays = Overlays::default();
        let targets = [target(1, "str(i)"), target(2, "parse(str(i))")];
        assert!(start_inline_smart_step(&mut overlays, Some(&paused("/p/app.py")), SESSION, 1, &targets));
        overlays
    }

    /// The highlighted calls as (text, selected).
    fn marked(overlays: &Overlays) -> Vec<(&'static str, bool)> {
        step_marks(overlays, "/p/app.py", Some(utf16::len(LINE)))
            .iter()
            .map(|mark| (utf16::slice(LINE, mark.from, mark.to), mark.selected))
            .collect()
    }

    #[test]
    fn names_the_callee_of_a_label() {
        assert_eq!(callee_name("parse(str(i))").as_deref(), Some("parse"));
        assert_eq!(callee_name("java.lang.String.valueOf(int)").as_deref(), Some("valueOf"));
        assert_eq!(callee_name("<lambda>"), None);
    }

    #[test]
    fn places_targets_by_name_in_line_order_or_by_adapter_columns() {
        let line = "    x = parse(str(i)) + str (j)";
        let by_name = [target(1, "str(i)"), target(2, "parse(str(i))"), target(3, "str(j)")];
        assert_eq!(
            locate_step_targets(line, 3, &by_name),
            Some(vec![placed(2, 8, 13), placed(1, 14, 17), placed(3, 24, 27)]),
        );
        let by_column = [at(7, "whatever", 8, Some(13)), at(8, "x", 14, None)];
        assert_eq!(locate_step_targets(line, 3, &by_column), Some(vec![placed(7, 8, 13), placed(8, 14, 17)]));
        assert_eq!(locate_step_targets(line, 3, &[target(1, "missing()")]), None);
    }

    #[test]
    fn rejects_identifier_suffixes_preserving_utf16_offsets_and_nested_calls() {
        let line = "𐐀foo() αfoo() $foo() _foo() foo(foo())";
        let from = col_of_byte(line, line.find("foo(foo").expect("the nested call"));
        assert_eq!(
            locate_step_targets(line, 0, &[target(1, "foo()"), target(2, "foo()")]),
            Some(vec![placed(1, from, from + 3), placed(2, from + 4, from + 7)]),
        );
        assert_eq!(ids(&targets_on_line("𐐀foo() bar()", 0, &[target(1, "foo()"), target(2, "bar()")])), [2]);
    }

    #[test]
    fn keeps_only_the_paused_lines_calls_when_the_adapter_lists_the_whole_function() {
        let line = "        total = add(total, square(i))";
        let all = [target(0, "square(i)"), target(1, "add(total, square(i))"), target(2, "print(\"total\", total)")];
        assert_eq!(ids(&targets_on_line(line, 11, &all)), [0, 1]);
        let with_lines = [
            StepInTarget { line: Some(11), ..target(5, "print()") },
            StepInTarget { line: Some(12), ..target(6, "add()") },
        ];
        assert_eq!(ids(&targets_on_line(line, 11, &with_lines)), [5]);
        assert_eq!(targets_on_line("pass", 0, &all), all);
        assert_eq!(paused_line_targets(None, 11, all.to_vec()), all);
        assert_eq!(ids(&paused_line_targets(Some(line), 11, all.to_vec())), [0, 1]);
    }

    #[test]
    fn highlights_the_calls_moves_with_arrows_and_steps_in_with_enter() {
        let mut overlays = picking();
        assert_eq!(marked(&overlays), [("parse", false), ("str", true)]);
        assert_eq!(pick_key_down(&mut overlays, "ArrowLeft", false), PickOutcome::Moved);
        assert_eq!(marked(&overlays)[0], ("parse", true));
        assert_eq!(
            pick_key_down(&mut overlays, "Enter", false),
            PickOutcome::Chosen(StepChoice { session: SESSION, thread: 1, target: 2 }),
        );
        assert!(overlays.smart_step.is_none());
        assert!(marked(&overlays).is_empty());
    }

    #[test]
    fn escape_cancels_a_click_on_a_call_steps_into_it() {
        let mut overlays = picking();
        assert_eq!(pick_key_down(&mut overlays, "Escape", false), PickOutcome::Cancelled);
        assert!(overlays.smart_step.is_none());

        let mut overlays = picking();
        assert_eq!(
            pick_mouse_down(&mut overlays, Some(2)),
            PickOutcome::Chosen(StepChoice { session: SESSION, thread: 1, target: 2 }),
        );
    }

    #[test]
    fn uses_the_dialog_when_the_line_is_not_open_or_the_calls_cannot_be_placed() {
        let mut overlays = Overlays::default();
        let unplaced = [target(1, "a()"), target(2, "b()")];
        assert!(!start_inline_smart_step(&mut overlays, Some(&paused("/p/app.py")), SESSION, 1, &unplaced));
        let targets = [target(1, "str(i)"), target(2, "parse(str(i))")];
        assert!(!start_inline_smart_step(&mut overlays, Some(&paused("/p/other.py")), SESSION, 1, &targets));
        assert!(overlays.smart_step.is_none());
    }

    // Not from a spec: Tab and the arrows wrap around, hovering selects, a bare modifier and a click
    // elsewhere do what they would without the picker, and the debugger moving on ends picking.
    #[test]
    fn wraps_around_follows_the_pointer_and_ends_when_the_debugger_moves_on() {
        let mut overlays = picking();
        assert_eq!(pick_key_down(&mut overlays, "Tab", false), PickOutcome::Moved);
        assert_eq!(marked(&overlays), [("parse", true), ("str", false)]);
        assert_eq!(pick_key_down(&mut overlays, "Tab", true), PickOutcome::Moved);
        assert_eq!(marked(&overlays), [("parse", false), ("str", true)]);
        assert_eq!(pick_key_down(&mut overlays, "Shift", true), PickOutcome::Ignored);
        pick_mouse_over(&mut overlays, Some(2));
        assert_eq!(marked(&overlays)[0], ("parse", true));
        assert!(step_marks(&overlays, "/p/other.py", Some(40)).is_empty());
        assert!(step_marks(&overlays, "/p/app.py", None).is_empty());
        assert_eq!(pick_mouse_down(&mut overlays, None), PickOutcome::Cancelled);
        assert_eq!(pick_mouse_down(&mut overlays, None), PickOutcome::Ignored);
        assert_eq!(pick_key_down(&mut overlays, "Enter", false), PickOutcome::Ignored);

        let mut overlays = picking();
        execution_point_moved(&mut overlays);
        assert!(overlays.smart_step.is_none());
        // A step the dialog chooses carries no selection: the id decides.
        overlays.open_smart_step_chooser(SESSION, 1, vec![target(4, "run()")]);
        execution_point_moved(&mut overlays);
        assert_eq!(choose_step_target(&mut overlays, None), None);
        overlays.open_smart_step_chooser(SESSION, 1, vec![target(4, "run()")]);
        assert_eq!(choose_step_target(&mut overlays, Some(4)).map(|choice| choice.target), Some(4));
    }
}
