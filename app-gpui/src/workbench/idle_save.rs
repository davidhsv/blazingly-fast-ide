//! System Settings › Save files if the IDE is idle for: once no key, press or edit has come to the window
//! for the configured seconds, File › Save All runs, a failure told as any failed save is. The countdown
//! runs only while something is unsaved, so a window with nothing to save never wakes up for it, and a
//! save that fails is not tried again before the next input.

use super::Workbench;
use crate::settings_model::runtime::runtime_settings;
use gpui_kit::{Context, Window};
use std::time::Duration;

impl Workbench {
    /// Input came to the window: the countdown starts over when idle saving is on and something is
    /// unsaved; otherwise none runs.
    pub(super) fn idle_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.idle_save = None;
        if self.unsaved_count(cx) == 0 {
            return;
        }
        let settings = crate::platform::settings::effective(&self.label, cx);
        let Some(seconds) = runtime_settings(&settings).save_after_idle_seconds else { return };
        let delay = Duration::from_secs(u64::from(seconds.max(1)));
        self.idle_save = Some(cx.spawn_in(window, async move |workbench, cx| {
            cx.background_executor().timer(delay).await;
            let _ = workbench.update(cx, |workbench, _| workbench.idle_save = None);
            let _ = cx.update(|window, cx| crate::platform::actions::execute_action("file.save", None, window, cx));
        }));
    }

    /// The settings changed: with idle saving off, the countdown stops; a new one waits for input.
    pub(super) fn idle_save_settings_changed(&mut self, cx: &mut Context<Self>) {
        let settings = crate::platform::settings::effective(&self.label, cx);
        if runtime_settings(&settings).save_after_idle_seconds.is_none() {
            self.idle_save = None;
        }
    }

    /// A document was saved: once nothing is unsaved, the countdown stops. The editors take the news
    /// in the same round of events, so the count is read after it.
    pub(super) fn idle_save_document_saved(&mut self, cx: &mut Context<Self>) {
        if self.idle_save.is_some() {
            let this = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = this.update(cx, |workbench, cx| {
                    if workbench.unsaved_count(cx) == 0 {
                        workbench.idle_save = None;
                    }
                });
            });
        }
    }

    /// Whether the idle countdown runs.
    pub fn idle_save_pending(&self) -> bool {
        self.idle_save.is_some()
    }
}
