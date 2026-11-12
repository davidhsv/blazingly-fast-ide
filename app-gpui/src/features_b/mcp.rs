//! What the built-in MCP server's tools know about this window: its open editors and the focused
//! one with its selection, reported to the backend a moment after they change and only when they
//! changed.
//!
//! [`McpReporter`] does no I/O and keeps no timer: a change asks for a report
//! [`REPORT_DELAY_MS`] later ([`McpReporter::deadline`]), the owner calls [`McpReporter::tick`]
//! when that time comes and hands [`McpReporter::report`] the window as it is then. What the
//! backend is to be told comes back as [`McpEffect`]s.

use crate::find::utf16;
use ide_core::{IpcPath, Pos, Range};
use ide_host::mcp::FocusedEditor;

/// How long reports wait for changes to stop.
pub const REPORT_DELAY_MS: f64 = 300.0;
/// The most selected text (UTF-16 code units) a report carries.
pub const SELECTED_TEXT_LIMIT: u32 = 65_536;

/// What the window has open when a report is due.
#[derive(Debug, Clone)]
pub struct WindowSnapshot {
    /// The project's root.
    pub root: String,
    /// The files open in editors, in tab order.
    pub paths: Vec<String>,
    /// The active editor; see [`focused_editor`].
    pub editor: Option<FocusedEditor>,
    /// The window has the keyboard focus.
    pub focused: bool,
}

/// What the backend is to be told.
#[derive(Debug, Clone)]
pub enum McpEffect {
    /// The window can answer the MCP server's requests (`mcp_ui_ready`); a ready window waits for
    /// the backend's events to be listened to first.
    UiReady(bool),
    /// Deny every confirmation the window still shows.
    DenyAll,
    /// `mcp_set_open_editors`.
    SetOpenEditors(Vec<IpcPath>),
    /// `mcp_set_focused_editor`.
    SetFocusedEditor { editor: Option<FocusedEditor>, focused: bool },
}

/// Which report the backend refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    OpenEditors,
    FocusedEditor,
}

/// The editor at `path` with the selection `from`..`to` (UTF-16 offsets into `text`, the
/// document): zero-based lines and UTF-16 columns, and at most [`SELECTED_TEXT_LIMIT`] units of
/// the selected text, never ending inside a surrogate pair.
pub fn focused_editor(path: &str, text: &str, from: u32, to: u32) -> FocusedEditor {
    FocusedEditor {
        path: IpcPath::from(path),
        selection: Range::new(position(text, from), position(text, to)),
        selected_text: utf16::slice(text, from, to.min(from.saturating_add(SELECTED_TEXT_LIMIT))).to_owned(),
    }
}

/// The line and column of the UTF-16 `offset` in `text`.
fn position(text: &str, offset: u32) -> Pos {
    let (mut line, mut line_start, mut at) = (0, 0, 0);
    for c in text.chars() {
        if at >= offset {
            break;
        }
        at += c.len_utf16() as u32;
        if c == '\n' {
            line += 1;
            line_start = at;
        }
    }
    Pos::new(line, offset.saturating_sub(line_start))
}

/// The effects of the window going away or leaving its project: open confirmations are denied,
/// and a local window stops answering.
fn unload(remote: bool) -> Vec<McpEffect> {
    let mut effects = vec![McpEffect::DenyAll];
    if !remote {
        effects.push(McpEffect::UiReady(false));
    }
    effects
}

/// A local window with a project answers the MCP server's requests.
fn ready(remote: bool, project_open: bool) -> Option<McpEffect> {
    (!remote && project_open).then_some(McpEffect::UiReady(true))
}

/// The reports of one window.
#[derive(Debug, Clone, Default)]
pub struct McpReporter {
    /// What was last sent, as compared: `None` sends the next report whatever it says.
    last_paths: Option<(String, Vec<String>)>,
    last_editor: Option<String>,
    deadline: Option<f64>,
    disposed: bool,
}

impl McpReporter {
    /// Starts reporting at `now` for a window that is `remote` (its project runs on another host)
    /// and has a project open or not. Returns the reporter and what to tell the backend.
    pub fn activate(now: f64, remote: bool, project_open: bool) -> (Self, Vec<McpEffect>) {
        let mut reporter = Self::default();
        reporter.schedule(now);
        (reporter, ready(remote, project_open).into_iter().collect())
    }

    /// The open editors, the active editor or the window's focus changed at `now`.
    pub fn schedule(&mut self, now: f64) {
        if !self.disposed {
            self.deadline = Some(now + REPORT_DELAY_MS);
        }
    }

    /// When [`Self::tick`] is due.
    pub fn deadline(&self) -> Option<f64> {
        self.deadline
    }

    /// Time has passed. Returns whether a report is due: call [`Self::report`].
    pub fn tick(&mut self, now: f64) -> bool {
        let due = self.deadline.is_some_and(|deadline| now >= deadline);
        if due {
            self.deadline = None;
        }
        due
    }

    /// The report of the window as it is (`None`: no project is open): only what changed since the
    /// last report is sent.
    pub fn report(&mut self, window: Option<&WindowSnapshot>) -> Vec<McpEffect> {
        let Some(window) = window else { return Vec::new() };
        let mut effects = Vec::new();
        let paths_key = (window.root.clone(), window.paths.clone());
        if self.last_paths.as_ref() != Some(&paths_key) {
            self.last_paths = Some(paths_key);
            let paths = window.paths.iter().map(|path| IpcPath::from(path.as_str())).collect();
            effects.push(McpEffect::SetOpenEditors(paths));
        }
        let editor_key = serde_json::to_string(&(&window.root, &window.editor, window.focused)).ok();
        if editor_key.is_none() || self.last_editor != editor_key {
            self.last_editor = editor_key;
            effects.push(McpEffect::SetFocusedEditor { editor: window.editor.clone(), focused: window.focused });
        }
        effects
    }

    /// The backend refused a report at `now`: it is sent again with the next one, which is
    /// scheduled.
    pub fn report_failed(&mut self, kind: ReportKind, now: f64) {
        match kind {
            ReportKind::OpenEditors => self.last_paths = None,
            ReportKind::FocusedEditor => self.last_editor = None,
        }
        self.schedule(now);
    }

    /// The window's project root changed at `now` (also to the same root after closing it):
    /// confirmations of the old project are denied and everything is reported again.
    pub fn project_changed(&mut self, now: f64, remote: bool, project_open: bool) -> Vec<McpEffect> {
        let mut effects = vec![McpEffect::DenyAll];
        effects.extend(ready(remote, project_open));
        self.resend(now);
        effects
    }

    /// The remote connection was made again at `now`: the new server knows nothing yet, so
    /// everything is reported again, also when the root is the same.
    pub fn remote_session_changed(&mut self, now: f64) {
        self.resend(now);
    }

    fn resend(&mut self, now: f64) {
        self.last_paths = None;
        self.last_editor = None;
        self.schedule(now);
    }

    /// The window is being unloaded (reloaded or closed).
    pub fn unload(&self, remote: bool) -> Vec<McpEffect> {
        unload(remote)
    }

    /// Stops reporting: nothing pending is sent and a later failure schedules nothing.
    pub fn dispose(&mut self, remote: bool) -> Vec<McpEffect> {
        self.disposed = true;
        self.deadline = None;
        unload(remote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(paths: &[&str]) -> WindowSnapshot {
        WindowSnapshot {
            root: "/p".to_owned(),
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            editor: Some(focused_editor("/p/a.txt", "abchi!", 3, 5)),
            focused: true,
        }
    }

    /// Advances to `now`, reporting `window` if a report is due.
    fn advance(reporter: &mut McpReporter, now: f64, window: &WindowSnapshot) -> Vec<McpEffect> {
        if reporter.tick(now) { reporter.report(Some(window)) } else { Vec::new() }
    }

    fn counts(effects: &[McpEffect]) -> (usize, usize) {
        let open = effects.iter().filter(|e| matches!(e, McpEffect::SetOpenEditors(_))).count();
        let focused = effects.iter().filter(|e| matches!(e, McpEffect::SetFocusedEditor { .. })).count();
        (open, focused)
    }

    #[test]
    fn debounces_editor_tabs_and_sends_only_changes() {
        let (mut reporter, _) = McpReporter::activate(0.0, false, true);
        let mut current = window(&["/p/a.txt"]);
        assert!(advance(&mut reporter, 200.0, &current).is_empty());
        current = window(&["/p/a.txt", "/p/b.txt"]);
        reporter.schedule(200.0);
        assert!(advance(&mut reporter, 499.0, &current).is_empty());
        let effects = advance(&mut reporter, 500.0, &current);
        assert_eq!(counts(&effects), (1, 1));
        let McpEffect::SetOpenEditors(paths) = &effects[0] else { panic!("{effects:?}") };
        assert_eq!(paths, &[IpcPath::from("/p/a.txt"), IpcPath::from("/p/b.txt")]);
        let McpEffect::SetFocusedEditor { editor: Some(editor), focused } = &effects[1] else { panic!("{effects:?}") };
        assert!(*focused);
        assert_eq!(editor.path, IpcPath::from("/p/a.txt"));
        assert_eq!(editor.selection, Range::new(Pos::new(0, 3), Pos::new(0, 5)));
        assert_eq!(editor.selected_text, "hi");

        reporter.schedule(500.0);
        assert!(advance(&mut reporter, 800.0, &current).is_empty());
    }

    #[test]
    fn reports_again_after_reopening_the_same_project() {
        let (mut reporter, _) = McpReporter::activate(0.0, false, true);
        let current = window(&["/p/a.txt"]);
        assert_eq!(counts(&advance(&mut reporter, 300.0, &current)), (1, 1));
        let effects = reporter.project_changed(300.0, false, true);
        assert!(matches!(effects[..], [McpEffect::DenyAll, McpEffect::UiReady(true)]));
        assert_eq!(counts(&advance(&mut reporter, 600.0, &current)), (1, 1));
    }

    #[test]
    fn resends_unchanged_editors_after_reconnecting_at_the_same_root() {
        let (mut reporter, effects) = McpReporter::activate(0.0, true, true);
        assert!(effects.is_empty());
        let current = window(&["/p/a.txt"]);
        assert_eq!(counts(&advance(&mut reporter, 300.0, &current)), (1, 1));
        reporter.remote_session_changed(300.0);
        assert_eq!(counts(&advance(&mut reporter, 600.0, &current)), (1, 1));
    }

    #[test]
    fn forwards_remote_editor_reports_and_stops_after_disposal() {
        let (mut reporter, _) = McpReporter::activate(0.0, true, true);
        let current = window(&["/p/a.txt"]);
        let effects = advance(&mut reporter, 300.0, &current);
        let McpEffect::SetOpenEditors(paths) = &effects[0] else { panic!("{effects:?}") };
        assert_eq!(paths, &[IpcPath::from("/p/a.txt")]);
        reporter.schedule(300.0);
        let effects = reporter.dispose(false);
        assert!(matches!(effects[..], [McpEffect::DenyAll, McpEffect::UiReady(false)]));
        assert!(advance(&mut reporter, 600.0, &window(&["/p/b.txt"])).is_empty());
        reporter.report_failed(ReportKind::OpenEditors, 600.0);
        assert_eq!(reporter.deadline(), None);
    }
}
