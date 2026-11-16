//! Version Control › GitHub: the pull request review option, Automatically mark opened files as
//! viewed. The GitHub accounts are the GitHub CLI's, so the page lists none and reaches no account.
//!
//! The option belongs to the pull request diff's settings, kept in the UI storage under
//! [`REVIEW_DIFF_SETTINGS_KEY`] with that diff's other options: the checkbox changes it at once, so
//! the page stages nothing and Apply, Reset and Cancel leave it alone. The page reads the storage
//! each time it is drawn, so a save of another window or a cleared storage shows at once, and
//! hands each change to the workbench as [`GithubSettingsEvent::SetAutoMarkViewed`].

use super::{Binding, row};
use crate::github::diff::{REVIEW_DIFF_SETTINGS_KEY, ReviewDiffSettings, ReviewDiffSettingsStore};
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{check_box, search_label, tx};
use gpui_kit::{App, ClickEvent, Context, EventEmitter, IntoElement, Render, Subscription, Window, div, prelude::*};

pub const PAGE: &str = "vcs.github";

pub const AUTO_MARK_VIEWED: &str = "Automatically mark opened files as viewed";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 1] = [AUTO_MARK_VIEWED];

/// The review diff settings as stored, each unreadable or missing value its default.
pub fn stored_review_settings(cx: &App) -> ReviewDiffSettings {
    let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(REVIEW_DIFF_SETTINGS_KEY));
    ReviewDiffSettingsStore::load(stored).settings
}

/// The JSON text to store once Automatically mark opened files as viewed is `on`, the review diff's
/// other settings as stored.
pub fn review_settings_with_auto_mark(cx: &App, on: bool) -> String {
    let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(REVIEW_DIFF_SETTINGS_KEY));
    ReviewDiffSettingsStore::load(stored).set(|settings| settings.auto_mark_viewed = on)
}

/// What the page asks of the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubSettingsEvent {
    /// The checkbox was clicked: store the option as given.
    SetAutoMarkViewed(bool),
}

pub struct GithubSettingsView {
    binding: Binding,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<GithubSettingsEvent> for GithubSettingsView {}

impl GithubSettingsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
            // Another window's save, or a cleared storage, shows on the open page.
            cx.observe_global::<UiStorage>(|_: &mut Self, cx| cx.notify()),
        ];
        Self { binding, _subscriptions: subscriptions }
    }

    /// Whether the checkbox is ticked: the option as stored now.
    pub fn auto_mark_viewed(&self, cx: &App) -> bool {
        stored_review_settings(cx).auto_mark_viewed
    }

    /// The page as text: `[x] Automatically mark opened files as viewed`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mark = if self.auto_mark_viewed(cx) { "[x]" } else { "[ ]" };
        vec![format!("{mark} {}", tx(cx, AUTO_MARK_VIEWED))]
    }
}

impl Render for GithubSettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let on = self.auto_mark_viewed(cx);
        let text = tx(cx, AUTO_MARK_VIEWED);
        div().flex().flex_col().child(
            row(cx, self.binding.matches(AUTO_MARK_VIEWED, cx))
                .id("settings-github-autoMarkViewed")
                .debug_selector(|| "settings-github-autoMarkViewed".to_owned())
                .child(check_box(cx, on))
                .child(search_label(cx, &text, &query))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(GithubSettingsEvent::SetAutoMarkViewed(!on));
                })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    // Not from a spec: the stored JSON keeps the review diff's other settings when the option changes.
    #[gpui_kit::test]
    async fn the_stored_json_keeps_the_other_review_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut storage = UiStorage::default();
            storage
                .set(REVIEW_DIFF_SETTINGS_KEY, r#"{"threadsView":"all","reviewCommentsPreferred":false}"#.to_owned());
            cx.set_global(storage);
        });
        let text = cx.update(|cx| review_settings_with_auto_mark(cx, true));
        let stored: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(stored["autoMarkViewed"], Value::Bool(true));
        assert_eq!(stored["threadsView"], Value::from("all"));
        assert_eq!(stored["reviewCommentsPreferred"], Value::Bool(false));
    }

    // Not from a spec: the checkbox shows the stored option, and a click asks for the other value.
    #[gpui_kit::test]
    async fn the_checkbox_follows_the_storage_and_asks_for_the_other_value(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| cx.set_global(UiStorage::default()));
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| GithubSettingsView::new(binding, cx));
        cx.update(|window, _| window.activate_window());
        let seen: Rc<RefCell<Vec<GithubSettingsEvent>>> = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &GithubSettingsEvent, _| log.borrow_mut().push(*event)).detach();
        });
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["[ ] Automatically mark opened files as viewed"]);
        click(cx, "settings-github-autoMarkViewed");
        assert_eq!(*seen.borrow(), [GithubSettingsEvent::SetAutoMarkViewed(true)]);
        cx.update(|_, cx| {
            cx.global_mut::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, r#"{"autoMarkViewed":true}"#.to_owned())
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["[x] Automatically mark opened files as viewed"]);
    }
}
