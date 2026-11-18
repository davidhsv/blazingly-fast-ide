//! Files › Synchronize files on frame activation: once the window has been active again for a moment,
//! the backend checks every open document against its file, the ones outside the watched folders
//! included. A file changed while its document has no unsaved edits is read again (its editor keeps
//! the carets and scroll position the change allows); one with unsaved edits stays as it is and a
//! warning says the file changed on disk. In a project the Project view and the version control
//! status are read again afterwards. The debounce, the follow-up of a focus during a check and the
//! drop of a check whose project went away are [`DiskSync`]'s.

use super::{Content, Workbench};
use crate::backend::Backend;
use crate::editor_model::disk_sync::{DiskSync, SYNC_DELAY_MS, SyncWindow};
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::local_time::now_ms;
use gpui_kit::{Context, Task, Window};
use std::time::Duration;

/// The window's disk check: its debounce, the timer of the scheduled check, and how many checks
/// have finished.
#[derive(Default)]
pub(super) struct DiskSyncSlot {
    sync: DiskSync,
    timer: Option<Task<()>>,
    finished: u64,
}

/// The milliseconds the debounce counts in.
fn now() -> u64 {
    now_ms().max(0.0) as u64
}

impl Workbench {
    /// The window as the disk check sees it now.
    fn sync_window(&self, cx: &gpui_kit::App) -> SyncWindow {
        let settings = crate::platform::settings::effective(&self.label, cx);
        SyncWindow {
            project: self.project_root(cx).map(|root| root.to_string_lossy().into_owned()),
            light_edit: matches!(self.content, Content::Editor { .. }),
            opening: matches!(self.content, Content::Opening(_)),
            sync_on_focus: settings.files.sync_on_focus,
            can_sync: !self.close_requests.deciding(),
        }
    }

    /// The window became the active one: the check is scheduled, replacing one that was waiting.
    pub(super) fn disk_sync_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.sync_window(cx);
        self.disk_sync.sync.focused(now(), &state);
        self.schedule_disk_sync(window, cx);
    }

    /// Arms the timer of the scheduled check, if one is scheduled; a timer armed before is dropped
    /// with its check.
    fn schedule_disk_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disk_sync.timer = self.disk_sync.sync.deadline().map(|due| {
            cx.spawn_in(window, async move |workbench, cx| {
                cx.background_executor().timer(Duration::from_millis(SYNC_DELAY_MS)).await;
                let _ = workbench.update_in(cx, |workbench, window, cx| workbench.disk_sync_due(due, window, cx));
            })
        });
    }

    /// The scheduled check is due: it runs on the backend unless the window changed meanwhile (its
    /// project, the setting, a close being decided), or a check still running takes it as its
    /// follow-up. The timer's own deadline is the time, so a check is never early or late by the
    /// clock it was scheduled with.
    fn disk_sync_due(&mut self, due: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.disk_sync.timer = None;
        let state = self.sync_window(cx);
        let Some(ticket) = self.disk_sync.sync.fire(due, &state) else { return };
        let label = self.label.clone();
        let checked = cx
            .global::<Backend>()
            .with_host(move |host| async move { ide_host::ops::doc::sync_from_disk(&host, &label).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let ok = match checked.await {
                Ok(Ok(())) => true,
                Ok(Err(error)) => {
                    tracing::warn!("cannot synchronize external file changes: {}", error.message);
                    false
                }
                Err(error) => {
                    tracing::warn!("cannot synchronize external file changes: {error:#}");
                    false
                }
            };
            let _ =
                workbench.update_in(cx, |workbench, window, cx| workbench.disk_sync_finished(ticket, ok, window, cx));
        })
        .detach();
    }

    /// Check `ticket` finished: the Project view and the version control status are read again when
    /// the check was of the project the window still shows, and a focus that came during the check
    /// schedules the follow-up.
    fn disk_sync_finished(&mut self, ticket: u64, ok: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.disk_sync.finished += 1;
        let state = self.sync_window(cx);
        let refresh = self.disk_sync.sync.finished(ticket, ok, now(), &state);
        self.schedule_disk_sync(window, cx);
        if refresh {
            // The actions update this workbench: they run after the current update.
            window.defer(cx, |window, cx| {
                for id in ["project.refresh", "vcs.refresh"] {
                    if cx.global::<ActionRegistry>().contains(id) {
                        execute_action(id, None, window, cx);
                    }
                }
            });
        }
    }

    /// How many disk checks of this window have finished.
    #[cfg(test)]
    pub(super) fn disk_syncs_finished(&self) -> u64 {
        self.disk_sync.finished
    }
}
