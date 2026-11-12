//! Inlay Run to Cursor: while the debugger is paused, hovering the start of a line shows a small
//! button left of its text that runs to that line (Resume on the execution line itself).
//! Settings › Debugger › "Show inlay Run to Cursor popup".

use crate::platform::virtual_docs::class_file_breakpoint_reason;

/// Hover area from the left edge of the text, in characters.
const HOVER_CHARS: f32 = 4.0;
/// The button's side, in pixels.
pub const BUTTON: f32 = 20.0;

/// The 0-based line whose start the pointer is over, or `None`. `text_left`: where the line's text
/// begins; `line_number`: the 1-based number of the line under the pointer.
pub fn hovered_line(text_left: f32, char_width: f32, x: f32, line_number: u32, line_text: &str) -> Option<u32> {
    if x < text_left || x - text_left > char_width * HOVER_CHARS {
        return None;
    }
    (!line_text.trim().is_empty()).then(|| line_number.saturating_sub(1))
}

/// Whether editors of `path` get the inlay at all: not output classes, and of the virtual
/// documents only library sources.
pub fn inlay_applies(path: &str, is_virtual: bool) -> bool {
    class_file_breakpoint_reason(path).is_none() && (!is_virtual || path.starts_with("jdt:"))
}

/// Whether the inlay shows right now: the setting is on and the active session is suspended.
pub fn inlay_enabled(setting: bool, session_paused: bool) -> bool {
    setting && session_paused
}

/// What the button on a line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlayButton {
    /// On the execution line: Resume Program.
    Resume,
    RunToCursor,
}

impl InlayButton {
    /// The English label (tooltip and accessible name).
    pub fn label(self) -> &'static str {
        match self {
            Self::Resume => "Resume Program",
            Self::RunToCursor => "Run to Cursor",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::RunToCursor => "run-to-cursor",
        }
    }
}

/// The button for `line` of `path`, given where execution is suspended.
pub fn inlay_button(path: &str, line: u32, execution: Option<(&str, u32)>) -> InlayButton {
    if execution == Some((path, line)) { InlayButton::Resume } else { InlayButton::RunToCursor }
}

/// Where the button goes, relative to the editor: left of the first non-space character (over the
/// gutter edge when the indent is too narrow), centred on the line.
pub fn inlay_position(text_x: f32, line_top: f32, line_height: f32) -> (f32, f32) {
    (text_x - BUTTON - 2.0, line_top + (line_height - BUTTON) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "    foo()";

    #[test]
    fn targets_a_non_blank_line_when_hovering_its_first_characters() {
        assert_eq!(hovered_line(100.0, 8.0, 100.0, 3, LINE), Some(2));
        assert_eq!(hovered_line(100.0, 8.0, 132.0, 3, LINE), Some(2));
    }

    #[test]
    fn ignores_the_pointer_further_right_left_of_the_text_and_blank_lines() {
        assert_eq!(hovered_line(100.0, 8.0, 133.0, 3, LINE), None);
        assert_eq!(hovered_line(100.0, 8.0, 99.0, 3, LINE), None);
        assert_eq!(hovered_line(100.0, 8.0, 104.0, 4, "   "), None);
    }

    // Not from a spec: the execution line offers Resume, every other line Run to Cursor; output
    // classes and virtual documents other than library sources get no inlay.
    #[test]
    fn offers_resume_on_the_execution_line_only() {
        assert_eq!(inlay_button("/p/a.py", 4, Some(("/p/a.py", 4))), InlayButton::Resume);
        assert_eq!(inlay_button("/p/a.py", 5, Some(("/p/a.py", 4))), InlayButton::RunToCursor);
        assert_eq!(inlay_button("/p/b.py", 4, Some(("/p/a.py", 4))).label(), "Run to Cursor");
        assert_eq!(inlay_button("/p/a.py", 4, None).icon(), "run-to-cursor");
        assert!(inlay_applies("/p/a.py", false) && inlay_applies("jdt://contents/A.class", true));
        assert!(!inlay_applies("/p/out/A.class", false) && !inlay_applies("git:/p/a.py", true));
        assert!(inlay_enabled(true, true) && !inlay_enabled(true, false) && !inlay_enabled(false, true));
        assert_eq!(inlay_position(100.0, 40.0, 24.0), (78.0, 42.0));
    }
}
