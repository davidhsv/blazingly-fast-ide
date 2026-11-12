//! Files › Synchronize files on frame activation (`files.syncOnFocus`): one debounced disk check
//! per window activation, scoped to the project the window had when it was activated. The caller
//! runs the check ([`DiskSync::fire`]) and, after it, refreshes the project tree and VCS when
//! [`DiskSync::finished`] says so. Time is passed in, in milliseconds.

/// How long focus must settle before the check runs.
pub const SYNC_DELAY_MS: u64 = 200;

/// The window as the check sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncWindow {
    /// The window's project (its root); `None` in a welcome or LightEdit window.
    pub project: Option<String>,
    pub light_edit: bool,
    /// A project is being opened.
    pub opening: bool,
    /// `files.syncOnFocus`; settings without it count as on.
    pub sync_on_focus: bool,
    /// The editor allows a check (no close decision is pending).
    pub can_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Scope {
    project: Option<String>,
    light_edit: bool,
}

impl Scope {
    fn of(window: &SyncWindow) -> Self {
        Self { project: window.project.clone(), light_edit: window.light_edit }
    }
}

/// The disk-check debounce of one window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiskSync {
    due: Option<(u64, Scope)>,
    running: Option<(u64, Scope)>,
    pending: bool,
    disposed: bool,
    ticket: u64,
}

impl DiskSync {
    pub fn new() -> Self {
        Self::default()
    }

    fn enabled(&self, window: &SyncWindow) -> bool {
        !self.disposed
            && window.can_sync
            && !window.opening
            && (window.project.is_some() || window.light_edit)
            && window.sync_on_focus
    }

    /// The window was activated: the check is (re)scheduled, or dropped when it may not run.
    pub fn focused(&mut self, now: u64, window: &SyncWindow) {
        self.due = None;
        if self.enabled(window) {
            self.due = Some((now + SYNC_DELAY_MS, Scope::of(window)));
        }
    }

    /// When the scheduled check is due, for the caller's timer.
    pub fn deadline(&self) -> Option<u64> {
        self.due.as_ref().map(|(at, _)| *at)
    }

    /// The timer fired: the ticket of the check to run now, if any. A check still running takes a
    /// follow-up instead; a changed project, setting or close drops it.
    pub fn fire(&mut self, now: u64, window: &SyncWindow) -> Option<u64> {
        let (_, scope) = self.due.take_if(|(at, _)| *at <= now)?;
        if !self.enabled(window) || scope != Scope::of(window) {
            return None;
        }
        if self.running.is_some() {
            self.pending = true;
            return None;
        }
        self.ticket += 1;
        self.running = Some((self.ticket, scope));
        Some(self.ticket)
    }

    /// Check `ticket` finished (`ok == false`: it failed and was logged). Returns whether the
    /// project tree and VCS refresh: only after a successful check of a project that is still the
    /// window's (never in LightEdit). A focus that came during the check schedules a follow-up.
    pub fn finished(&mut self, ticket: u64, ok: bool, now: u64, window: &SyncWindow) -> bool {
        let Some((_, scope)) = self.running.take_if(|(running, _)| *running == ticket) else { return false };
        let refresh = ok && self.enabled(window) && window.project == scope.project && !scope.light_edit;
        if std::mem::take(&mut self.pending) {
            self.focused(now, window);
        }
        refresh
    }

    /// The window's editor went away: nothing is checked any more.
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.due = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> SyncWindow {
        SyncWindow {
            project: Some("/project".into()),
            light_edit: false,
            opening: false,
            sync_on_focus: true,
            can_sync: true,
        }
    }

    #[test]
    fn debounces_focus_events_and_refreshes_the_project_tree_and_vcs() {
        let mut sync = DiskSync::new();
        let w = window();
        sync.focused(0, &w);
        assert_eq!(sync.fire(150, &w), None);
        sync.focused(150, &w);
        assert_eq!(sync.fire(349, &w), None);
        let ticket = sync.fire(350, &w).expect("the check runs");
        assert_eq!(sync.fire(350, &w), None);
        assert!(sync.finished(ticket, true, 360, &w));
    }

    #[test]
    fn defaults_on_for_older_settings_and_does_nothing_when_the_setting_is_off() {
        let mut sync = DiskSync::new();
        let older = window();
        sync.focused(0, &older);
        let ticket = sync.fire(200, &older).expect("the check runs");
        sync.finished(ticket, true, 210, &older);
        let off = SyncWindow { sync_on_focus: false, ..window() };
        sync.focused(300, &off);
        assert_eq!(sync.deadline(), None);
        assert_eq!(sync.fire(800, &off), None);
    }

    #[test]
    fn cancels_pending_sync_on_changes() {
        for change in ["setting", "project", "opening", "closing", "dispose"] {
            let mut sync = DiskSync::new();
            sync.focused(0, &window());
            let mut w = window();
            match change {
                "setting" => w.sync_on_focus = false,
                "project" => w.project = Some("/other".into()),
                "opening" => w.opening = true,
                "closing" => w.can_sync = false,
                _ => sync.dispose(),
            }
            assert_eq!(sync.fire(200, &w), None, "{change}");
        }
    }

    #[test]
    fn ignores_welcome_windows_and_synchronizes_light_edit_without_project_refreshes() {
        let mut sync = DiskSync::new();
        let welcome = SyncWindow { project: None, ..window() };
        sync.focused(0, &welcome);
        assert_eq!(sync.fire(200, &welcome), None);
        let light = SyncWindow { light_edit: true, ..welcome };
        sync.focused(200, &light);
        let ticket = sync.fire(400, &light).expect("LightEdit checks its files");
        assert!(!sync.finished(ticket, true, 410, &light));
    }

    #[test]
    fn coalesces_focus_during_a_slow_check_into_a_follow_up_and_removes_its_listener() {
        let mut sync = DiskSync::new();
        let w = window();
        sync.focused(0, &w);
        let slow = sync.fire(200, &w).expect("the check runs");
        sync.focused(200, &w);
        assert_eq!(sync.fire(400, &w), None);
        sync.finished(slow, true, 400, &w);
        let follow_up = sync.fire(600, &w).expect("the follow-up runs");
        sync.finished(follow_up, true, 610, &w);
        sync.dispose();
        sync.focused(700, &w);
        assert_eq!(sync.fire(900, &w), None);
    }

    #[test]
    fn does_not_refresh_a_replacement_project_when_an_old_check_finishes() {
        let mut sync = DiskSync::new();
        sync.focused(0, &window());
        let ticket = sync.fire(200, &window()).expect("the check runs");
        let replaced = SyncWindow { project: Some("/other".into()), ..window() };
        assert!(!sync.finished(ticket, true, 210, &replaced));
    }
}
