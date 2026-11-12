//! Which of the debugger's overlays are open: the breakpoint properties popover, Evaluate, View
//! Breakpoints, the configuration chooser, Attach to Process and Smart Step Into.

use ide_core::DebugSessionId;
use ide_dap::StepInTarget;
use ide_dap::attach::AttachableProcess;
use ide_run::RunConfig;

/// A Smart Step Into target placed on the paused line (UTF-16 columns within the line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineStepTarget {
    pub id: i64,
    pub from: u32,
    pub to: u32,
}

/// The properties popover of a breakpoint, at a point of the window.
#[derive(Debug, Clone, PartialEq)]
pub struct BreakpointPopover {
    pub path: String,
    pub id: String,
    pub x: f32,
    pub y: f32,
    /// The log message field gets the focus.
    pub focus_log: bool,
}

/// The breakpoint View Breakpoints opens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointSelection {
    pub path: String,
    pub id: String,
}

/// The Attach to Process popup; `processes` is `None` while the list loads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttachPopup {
    pub processes: Option<Vec<AttachableProcess>>,
    pub error: Option<String>,
}

/// Picking in the editor: the targets on the paused line, sorted by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineStepPick {
    pub path: String,
    pub line: u32,
    pub ranges: Vec<InlineStepTarget>,
    /// The selected one of `ranges`.
    pub index: usize,
}

/// Smart Step Into: picked in the editor when `inline` is set, else in the chooser dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartStep {
    pub session: DebugSessionId,
    pub thread: i64,
    pub targets: Vec<StepInTarget>,
    pub inline: Option<InlineStepPick>,
}

#[derive(Debug, Clone, Default)]
pub struct Overlays {
    pub popover: Option<BreakpointPopover>,
    /// Evaluate Expression, with its initial text.
    pub evaluate: Option<String>,
    /// View Breakpoints; the inner value is the breakpoint to select.
    pub breakpoints: Option<Option<BreakpointSelection>>,
    /// The run configurations to choose one to debug from.
    pub chooser: Option<Vec<RunConfig>>,
    pub attach: Option<AttachPopup>,
    pub smart_step: Option<SmartStep>,
}

impl Overlays {
    /// Whether a breakpoint popover, debugger dialog or chooser is open.
    pub fn any_open(&self) -> bool {
        self.popover.is_some()
            || self.evaluate.is_some()
            || self.breakpoints.is_some()
            || self.chooser.is_some()
            || self.attach.is_some()
            || self.smart_step.is_some()
    }

    pub fn open_breakpoint_popover(&mut self, path: &str, id: &str, x: f32, y: f32, focus_log: bool) {
        self.popover = Some(BreakpointPopover { path: path.to_owned(), id: id.to_owned(), x, y, focus_log });
    }

    pub fn close_breakpoint_popover(&mut self) {
        self.popover = None;
    }

    pub fn open_evaluate_dialog(&mut self, expression: &str) {
        self.evaluate = Some(expression.to_owned());
    }

    pub fn open_breakpoints_dialog(&mut self, select: Option<BreakpointSelection>) {
        self.breakpoints = Some(select);
    }

    pub fn open_config_chooser(&mut self, configs: Vec<RunConfig>) {
        self.chooser = Some(configs);
    }

    pub fn open_smart_step_chooser(&mut self, session: DebugSessionId, thread: i64, targets: Vec<StepInTarget>) {
        self.smart_step = Some(SmartStep { session, thread, targets, inline: None });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: every overlay counts as open, and closing the popover leaves the others.
    #[test]
    fn any_overlay_counts_as_open() {
        let mut overlays = Overlays::default();
        assert!(!overlays.any_open());
        overlays.open_breakpoint_popover("/p/a.py", "bp1", 10.0, 20.0, false);
        overlays.open_evaluate_dialog("total");
        assert!(overlays.any_open());
        overlays.close_breakpoint_popover();
        assert_eq!(overlays.evaluate.as_deref(), Some("total"));
        overlays.evaluate = None;
        overlays.open_breakpoints_dialog(None);
        assert_eq!(overlays.breakpoints, Some(None));
        overlays.breakpoints = None;
        overlays.open_smart_step_chooser(DebugSessionId(3), 1, Vec::new());
        assert!(overlays.smart_step.as_ref().is_some_and(|step| step.inline.is_none()));
        overlays.smart_step = None;
        overlays.open_config_chooser(Vec::new());
        assert!(overlays.any_open());
    }
}
