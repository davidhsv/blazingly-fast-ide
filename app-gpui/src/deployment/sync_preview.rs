//! The content comparison under the Sync dialog's file list: the selected row's local file and the
//! server's copy side by side, or why there is none (no row selected, still loading, a binary or
//! too large file, a read error with Retry).
//!
//! The view owns a [`PreviewPane`]: showing another row or Retry starts a load, told to the owner
//! as [`SyncPreviewEvent::Requests`] with the load an earlier one replaces. The owner reads both
//! sides and answers with [`SyncPreviewView::loaded`]; a late answer for a row no longer shown is
//! dropped.

use crate::features_b::deployment::comparison::{DeploymentPreview, DiffSide};
use crate::features_b::deployment::preview::{PreviewPane, PreviewRequests, PreviewState};
use crate::features_b::deployment::types::SyncEntry;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::vcs_diff::diff_options::{DiffMode, DiffViewOptions};
use crate::vcs_diff::view::DiffView;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Window,
    div, prelude::*, px,
};

/// What a load of a row came to: the preview and, for a text preview of an existing local file,
/// that file's text as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedPreview {
    pub preview: DeploymentPreview,
    pub local_text: String,
}

/// What the view asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPreviewEvent {
    /// Abort the load `abort` replaces, and start `load`.
    Requests(PreviewRequests),
}

pub struct SyncPreviewView {
    focus: FocusHandle,
    pane: PreviewPane,
    diff: Option<Entity<DiffView>>,
}

impl EventEmitter<SyncPreviewEvent> for SyncPreviewView {}

fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// The text of a side: a fixed text, or the local file's text read with the preview.
fn side_text(side: &DiffSide, local_text: &str) -> (String, String) {
    match side {
        DiffSide::File { label, .. } => (local_text.to_owned(), label.clone()),
        DiffSide::Text { text, label } => (text.clone(), label.clone()),
    }
}

impl SyncPreviewView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle(), pane: PreviewPane::default(), diff: None }
    }

    fn requested(&mut self, requests: PreviewRequests, cx: &mut Context<Self>) {
        if requests.load.is_some() || requests.abort.is_some() {
            self.diff = None;
            cx.emit(SyncPreviewEvent::Requests(requests));
        }
        cx.notify();
    }

    /// Shows the comparison of `entry` (the selected row; `None` without one).
    pub fn show(&mut self, entry: Option<SyncEntry>, cx: &mut Context<Self>) {
        let requests = self.pane.show(entry);
        self.requested(requests, cx);
    }

    /// Retry after a read error.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let requests = self.pane.retry();
        self.requested(requests, cx);
    }

    /// The answer of the load `ticket`: its preview, or why it could not be read.
    pub fn loaded(&mut self, ticket: u64, result: Result<LoadedPreview, String>, cx: &mut Context<Self>) {
        let waiting = self.pane.state == PreviewState::Loading;
        let local_text = result.as_ref().map(|loaded| loaded.local_text.clone()).unwrap_or_default();
        {
            let i18n = &cx.default_global::<Localization>().0;
            self.pane.loaded(ticket, result.map(|loaded| loaded.preview), i18n);
        }
        if !waiting || self.pane.state == PreviewState::Loading {
            return;
        }
        self.diff = self.pane.diff().map(|input| {
            let (left, left_label) = side_text(&input.data.left, &local_text);
            let (right, right_label) = side_text(&input.data.right, &local_text);
            let options = DiffViewOptions { mode: DiffMode::Split, ..DiffViewOptions::default() };
            cx.new(|cx| {
                let mut view = DiffView::new(&left, &right, options, cx);
                view.set_labels(left_label, right_label, cx);
                view
            })
        });
        cx.notify();
    }

    pub fn state(&self) -> &PreviewState {
        &self.pane.state
    }

    /// The text shown in place of a comparison.
    pub fn message(&self, cx: &App) -> Option<String> {
        let i18n = &cx.try_global::<Localization>()?.0;
        self.pane.message(i18n)
    }

    /// The read error shown with Retry.
    pub fn error(&self) -> Option<&str> {
        match &self.pane.state {
            PreviewState::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// The side-by-side comparison, once a text preview is loaded.
    pub fn diff(&self) -> Option<&Entity<DiffView>> {
        self.diff.as_ref()
    }
}

impl Focusable for SyncPreviewView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SyncPreviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = div()
            .debug_selector(|| "sync-preview".to_owned())
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_1()
            .border_color(theme::color(cx, "border"));
        if let Some(diff) = &self.diff {
            return frame.child(diff.clone());
        }
        if let Some(error) = self.error().map(str::to_owned) {
            return frame.child(
                div()
                    .debug_selector(|| "sync-preview-error".to_owned())
                    .p(px(18.0))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(12.0))
                    .child(div().text_color(theme::color(cx, "danger")).child(SharedString::from(error)))
                    .child(
                        crate::vcs::prompt_dialogs::button(cx, tx(cx, "Retry"), false, false, true)
                            .id("sync-preview-retry")
                            .debug_selector(|| "sync-preview-retry".to_owned())
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.retry(cx))),
                    ),
            );
        }
        let message = self.message(cx).unwrap_or_default();
        frame.child(
            div()
                .debug_selector(|| "sync-preview-message".to_owned())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(message)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::deployment::comparison::DeploymentDiff;
    use crate::features_b::deployment::types::SyncStatus;
    use crate::vcs_diff::diff_options::Side;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<PreviewRequests>>>;

    fn entry(path: &str) -> SyncEntry {
        SyncEntry {
            path: path.to_owned(),
            remote_path: format!("/srv/{path}"),
            local: None,
            remote: None,
            status: SyncStatus::Same,
        }
    }

    fn result(text: &str) -> Result<LoadedPreview, String> {
        let preview = DeploymentPreview::Text(DeploymentDiff {
            path: "/work/file.php".to_owned(),
            title: "file.php".to_owned(),
            left: DiffSide::Text { text: "local".to_owned(), label: "Local".to_owned() },
            right: DiffSide::Text { text: text.to_owned(), label: "Remote".to_owned() },
        });
        Ok(LoadedPreview { preview, local_text: String::new() })
    }

    fn open(cx: &mut TestAppContext) -> (Entity<SyncPreviewView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(Localization::default());
        });
        let (view, cx) = cx.add_window_view(|_, cx| SyncPreviewView::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &SyncPreviewEvent, _| {
                let SyncPreviewEvent::Requests(requests) = event;
                seen.borrow_mut().push(requests.clone());
            })
            .detach();
        });
        (view, events, cx)
    }

    fn last_load(events: &Events) -> u64 {
        events.borrow().last().and_then(|requests| requests.load.as_ref()).map(|load| load.ticket).expect("a load")
    }

    fn right_text(view: &Entity<SyncPreviewView>, cx: &mut VisualTestContext) -> Option<String> {
        let diff = view.read_with(cx, |view, _| view.diff().cloned())?;
        Some(diff.read_with(cx, |diff, _| diff.side_text(Side::B)))
    }

    // deployment diff preview › ignores a late response for a file that is no longer selected
    #[gpui_kit::test]
    async fn ignores_a_late_response_for_a_file_that_is_no_longer_selected(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        view.update(cx, |view, cx| view.show(Some(entry("first.php")), cx));
        let first = last_load(&events);
        view.update(cx, |view, cx| view.show(Some(entry("second.php")), cx));
        let second = events.borrow().last().cloned().expect("the second file loads");
        assert_eq!(second.abort, Some(first), "the first load is aborted");
        let second = second.load.expect("the second file loads");
        assert_eq!(second.entry.path, "second.php");
        view.update(cx, |view, cx| view.loaded(second.ticket, result("current"), cx));
        view.update(cx, |view, cx| view.loaded(first, result("stale"), cx));
        cx.run_until_parked();
        assert_eq!(right_text(&view, cx).as_deref(), Some("current"));
        let diff = view.read_with(cx, |view, _| view.diff().cloned()).expect("the comparison shows");
        assert_eq!(diff.read_with(cx, |diff, _| diff.options().mode), DiffMode::Split, "side by side");
    }

    // deployment diff preview › shows read errors and allows retrying without closing the comparison
    #[gpui_kit::test]
    async fn shows_read_errors_and_allows_retrying_without_closing_the_comparison(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        view.update(cx, |view, cx| view.show(Some(entry("file.php")), cx));
        let ticket = last_load(&events);
        view.update(cx, |view, cx| view.loaded(ticket, Err("Offline".to_owned()), cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.error().map(str::to_owned)).as_deref(), Some("Offline"));
        assert!(cx.debug_bounds("sync-preview-error").is_some());
        let retry = cx.debug_bounds("sync-preview-retry").expect("Retry is drawn");
        cx.simulate_click(retry.center(), Modifiers::default());
        cx.run_until_parked();
        let again = events.borrow().last().and_then(|requests| requests.load.clone()).expect("Retry loads again");
        assert_eq!(again.entry.path, "file.php");
        view.update(cx, |view, cx| view.loaded(again.ticket, result("recovered"), cx));
        cx.run_until_parked();
        assert_eq!(right_text(&view, cx).as_deref(), Some("recovered"));
    }

    // deployment diff preview › explains why %s files have no text preview
    #[gpui_kit::test]
    async fn explains_why_files_have_no_text_preview(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        for (kind, text) in [
            (DeploymentPreview::Binary, "Binary files cannot be shown as text"),
            (DeploymentPreview::TooLarge, "too large for the text preview"),
        ] {
            view.update(cx, |view, cx| view.show(None, cx));
            view.update(cx, |view, cx| view.show(Some(entry("file.bin")), cx));
            let ticket = last_load(&events);
            let loaded = LoadedPreview { preview: kind, local_text: String::new() };
            view.update(cx, |view, cx| view.loaded(ticket, Ok(loaded), cx));
            cx.run_until_parked();
            let message = view.read_with(cx, |view, cx| view.message(cx)).unwrap_or_default();
            assert!(message.contains(text), "{message}");
            assert!(view.read_with(cx, |view, _| view.diff().is_none()));
            assert!(cx.debug_bounds("sync-preview-message").is_some());
        }
    }
}
