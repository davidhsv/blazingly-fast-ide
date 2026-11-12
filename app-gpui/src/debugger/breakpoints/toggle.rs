//! Toggle Line Breakpoint as IDEA does it: in Java, a method declaration line gets a method
//! breakpoint and a field declaration a field watchpoint; any other line (and every other
//! language) a line breakpoint. An existing breakpoint of any kind is removed.

use super::special::SpecialBreakpoints;
use super::store::BreakpointStore;
use super::symbols::{field_at, method_at};

/// Toggles the breakpoint on `line` (0-based) of `path`, a file of `language` whose text is `text`.
pub fn toggle_breakpoint_smart(
    breakpoints: &mut BreakpointStore,
    special: &mut SpecialBreakpoints,
    path: &str,
    line: u32,
    language: &str,
    text: &str,
    now_ms: f64,
) {
    if let Some(existing) = special.special_on_line(path, line) {
        special.remove_special(existing.id(), now_ms);
        return;
    }
    if language == "java" && breakpoints.at_line(path, line).is_none() {
        if let Some(field) = field_at(language, text, line) {
            special.toggle_field_at(path, line, &field.data_id, &field.label, now_ms);
            return;
        }
        if let Some(method) = method_at(language, text, line) {
            special.toggle_method_at(path, line, &method.name, now_ms);
            return;
        }
    }
    breakpoints.toggle_breakpoint(path, line, now_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(breakpoints: &BreakpointStore, path: &str) -> Vec<u32> {
        breakpoints.breakpoints_of(path).iter().map(|b| b.line).collect()
    }

    // src/features/debugger/__tests__/special.test.ts › method breakpoints and field watchpoints
    #[test]
    fn toggles_like_idea_java_declarations_get_method_and_field_breakpoints_other_lines_line_breakpoints() {
        let mut breakpoints = BreakpointStore::default();
        let mut special = SpecialBreakpoints::default();
        breakpoints.load(Some("/p"), None, 0.0);
        special.load(Some("/p"), None, 0.0);
        let src = "package a;\nclass A {\n  int count;\n  void run() {\n    count++;\n  }\n}";
        let mut toggle = |path: &str, line: u32, language: &str, text: &str| {
            toggle_breakpoint_smart(&mut breakpoints, &mut special, path, line, language, text, 0.0);
            (
                special.methods().iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
                special.fields().iter().map(|f| f.data_id.clone()).collect::<Vec<_>>(),
            )
        };
        toggle("/p/A.java", 3, "java", src);
        toggle("/p/A.java", 2, "java", src);
        let (methods, fields) = toggle("/p/A.java", 4, "java", src);
        assert_eq!(methods, ["a.A#run"]);
        assert_eq!(fields, ["a.A#count"]);
        // Again on the declaration: removed.
        let (methods, _) = toggle("/p/A.java", 3, "java", src);
        assert!(methods.is_empty());
        // Python `def` lines stay line breakpoints.
        toggle("/p/a.py", 0, "python", "def run():\n  pass");
        assert_eq!(lines(&breakpoints, "/p/A.java"), [4]);
        assert_eq!(lines(&breakpoints, "/p/a.py"), [0]);
    }
}
