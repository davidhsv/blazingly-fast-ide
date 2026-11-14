//! Ranges an editor draws behind or over its text: the occurrences of a search, its scope, the
//! occurrences it skips. A mark follows the edits of its document until its owner replaces it.

use super::buffer::{self, Assoc};
use ide_core::TextChange;
use std::ops::Range;

/// Who set a group of marks: each owner replaces its own marks only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkLayer {
    /// The Find / Replace bar of the editor's tab.
    Find,
    /// The problems the language servers and inspections report for the document.
    Diagnostics,
    /// The debugger's breakpoints on the document's lines.
    Breakpoints,
    /// The line where the debugger is suspended.
    Execution,
    /// The usages Highlight Usages in File marks, until Escape.
    Usages,
}

/// What a mark shows, and so how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkKind {
    /// An occurrence of the search: a background behind the text.
    FindOccurrence,
    /// The occurrence the search is on: a stronger background with an outline.
    CurrentFindOccurrence,
    /// The range a search is limited to: the inactive selection's background.
    FindScope,
    /// An occurrence the search skips: struck through.
    ExcludedOccurrence,
    /// An error: a wavy underline in the error colour.
    ErrorProblem,
    /// A warning: a wavy underline in the warning colour.
    WarningProblem,
    /// A weak warning or information: a wavy underline in the muted warning colour.
    WeakProblem,
    /// A line with an active breakpoint: its row is tinted and a dot is drawn in the gutter.
    BreakpointLine,
    /// A line with a breakpoint a running session's debugger did not bind: its row is tinted and a
    /// grey dot is drawn in the gutter.
    UnverifiedBreakpoint,
    /// A line with a disabled or muted breakpoint: only a hollow dot in the gutter.
    InactiveBreakpoint,
    /// The line where the debugger is suspended: its row is tinted.
    ExecutionLine,
}

impl MarkKind {
    /// Drawn below the selection (the search scope), not above it.
    pub fn is_below_selection(self) -> bool {
        self == Self::FindScope
    }

    /// Drawn as a wavy line under the text instead of behind it.
    pub fn is_underline(self) -> bool {
        matches!(self, Self::ErrorProblem | Self::WarningProblem | Self::WeakProblem)
    }

    /// The text inside takes the occurrence's foreground colour.
    pub fn recolors_text(self) -> bool {
        matches!(self, Self::FindOccurrence | Self::CurrentFindOccurrence)
    }

    /// Tints the whole row of its line instead of the text.
    pub fn is_line(self) -> bool {
        matches!(self, Self::BreakpointLine | Self::UnverifiedBreakpoint | Self::ExecutionLine)
    }

    /// Draws a breakpoint dot in the gutter of its line.
    pub fn is_breakpoint(self) -> bool {
        matches!(self, Self::BreakpointLine | Self::UnverifiedBreakpoint | Self::InactiveBreakpoint)
    }
}

/// A range of the document (UTF-16 offsets) drawn as `kind` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub range: Range<u32>,
    pub kind: MarkKind,
}

impl Mark {
    pub fn new(range: Range<u32>, kind: MarkKind) -> Self {
        Self { range, kind }
    }
}

/// `marks` after `changes`: a mark keeps covering its text, does not grow over text inserted at
/// its ends, and disappears when its text does.
pub fn map_marks(marks: &[Mark], changes: &[TextChange]) -> Vec<Mark> {
    marks
        .iter()
        .filter_map(|mark| {
            let from = buffer::map_offset(mark.range.start, changes, Assoc::After);
            let to = buffer::map_offset(mark.range.end, changes, Assoc::Before);
            (to > from).then(|| Mark::new(from..to, mark.kind))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    #[test]
    fn marks_follow_edits_and_disappear_with_their_text() {
        let marks = [Mark::new(2..5, MarkKind::FindOccurrence), Mark::new(8..10, MarkKind::FindScope)];
        // Text inserted before a mark moves it; inserted at its ends does not join it.
        let moved = map_marks(&marks, &[change(0, 0, "xx"), change(5, 5, "yy")]);
        assert_eq!(moved, [Mark::new(4..7, MarkKind::FindOccurrence), Mark::new(12..14, MarkKind::FindScope)]);
        // A deletion covering a mark removes it; one inside shrinks it.
        let deleted = map_marks(&marks, &[change(1, 6, ""), change(8, 9, "")]);
        assert_eq!(deleted, [Mark::new(3..4, MarkKind::FindScope)]);
    }
}
