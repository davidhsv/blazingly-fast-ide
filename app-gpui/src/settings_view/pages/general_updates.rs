//! Appearance & Behavior › System Settings › Updates: the version running, when updates were last
//! checked for, and Check Now, which runs Help › Check for Updates…. The page is listed only by
//! builds that update themselves (those that register that action).

use super::{Binding, ControlEvent, row, row_label};
use crate::backend::Backend;
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::date_format::{CivilDateTime, date_format_options, format_clock, format_day};
use crate::platform::local_time::utc_offset_minutes;
use crate::settings_view::controls::{button, tx};
use gpui_kit::{App, ClickEvent, Context, EventEmitter, IntoElement, Render, SharedString, Window, div, prelude::*};
use ide_core::IdeEvent;
use ide_core::updates::UpdateStatus;

pub const PAGE: &str = "general.updates";
/// The slot the rows fill.
pub const SLOT: &str = "Current version";
/// The action Check Now runs.
pub const CHECK_ACTION: &str = "app.checkForUpdates";

pub struct UpdateStatusView {
    binding: Binding,
    version: String,
    /// The last status the backend reported; none until it does.
    status: Option<UpdateStatus>,
    listener: Option<u64>,
}

impl EventEmitter<ControlEvent> for UpdateStatusView {}

impl UpdateStatusView {
    pub fn new(binding: Binding, version: &str, cx: &mut Context<Self>) -> Self {
        cx.on_release(|view: &mut Self, cx: &mut App| {
            if let Some(id) = view.listener.take()
                && cx.has_global::<Backend>()
            {
                cx.global_mut::<Backend>().unlisten(id);
            }
        })
        .detach();
        Self { binding, version: version.to_owned(), status: None, listener: None }
    }

    /// Follows the update status the backend announces while the view is open.
    pub fn follow_backend(&mut self, cx: &mut Context<Self>) {
        if self.listener.is_some() || !cx.has_global::<Backend>() {
            return;
        }
        let view = cx.weak_entity();
        let id = cx.global_mut::<Backend>().listen(move |event, cx| {
            if let IdeEvent::UpdateStatusChanged { status } = &event.event {
                let status = status.clone();
                view.update(cx, |view, cx| view.set_status(status, cx)).ok();
            }
        });
        self.listener = Some(id);
    }

    pub fn set_status(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
        self.status = Some(status);
        cx.notify();
    }

    /// Check Now can run: the action is there and no check is under way.
    pub fn can_check(&self, cx: &App) -> bool {
        let registered = cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(CHECK_ACTION));
        registered && self.status.as_ref().is_some_and(|status| !status.checking)
    }

    /// When updates were last checked for, in the date format applied: "Loading…" until the status is
    /// known, "Never" before the first check.
    pub fn last_checked(&self, cx: &App) -> String {
        let Some(status) = &self.status else { return tx(cx, "Loading…") };
        let Some(ms) = status.last_checked else { return tx(cx, "Never") };
        let general = crate::platform::settings::global(cx).general;
        let options = date_format_options(serde_json::to_value(&general.date_format).ok().as_ref());
        let date = CivilDateTime::from_unix_ms(ms, utc_offset_minutes(ms));
        format!("{} {}", format_day(date, &options, false), format_clock(date, &options))
    }

    fn button_label(&self, cx: &App) -> String {
        let checking = self.status.as_ref().is_some_and(|status| status.checking);
        tx(cx, if checking { "Checking for updates…" } else { "Check Now" })
    }

    /// The rows as text: `Current version: 1.2.3`, `Last checked: … [Check Now]` with `(disabled)`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let flag = if self.can_check(cx) { "" } else { " (disabled)" };
        vec![
            format!("{}: {}", tx(cx, "Current version"), self.version),
            format!("{}: {} [{}]{flag}", tx(cx, "Last checked"), self.last_checked(cx), self.button_label(cx)),
        ]
    }
}

impl Render for UpdateStatusView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let version_label = tx(cx, "Current version");
        let checked_label = tx(cx, "Last checked");
        let enabled = self.can_check(cx);
        let check = div()
            .id("settings-updates-check")
            .debug_selector(|| "settings-updates-check".to_owned())
            .child(button(cx, &self.button_label(cx), false, enabled))
            .when(enabled, |el| {
                el.on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                    execute_action(CHECK_ACTION, None, window, cx);
                }))
            });
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, self.binding.matches("Current version", cx))
                    .child(row_label(cx, &version_label, &query))
                    .child(SharedString::from(self.version.clone())),
            )
            .child(
                row(cx, self.binding.matches("Last checked", cx))
                    .child(row_label(cx, &checked_label, &query))
                    .child(
                        div()
                            .debug_selector(|| "settings-updates-last-checked".to_owned())
                            .child(SharedString::from(self.last_checked(cx))),
                    )
                    .child(check),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: the version is shown; Last checked says Loading… and Check Now is off until
    // a status is known, then says Never before the first check.
    #[gpui_kit::test]
    async fn the_status_rows_follow_the_status_reported(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| UpdateStatusView::new(binding, "9.9.9", cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["Current version: 9.9.9", "Last checked: Loading… [Check Now] (disabled)"]);
        view.update(cx, |view, cx| view.set_status(UpdateStatus { last_checked: None, checking: true }, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[1], "Last checked: Never [Checking for updates…] (disabled)");
    }
}
