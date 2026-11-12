//! The content diff below the Sync dialog's file list: the preview of the selected row, loaded
//! with [`DeploymentComparison::preview`](super::comparison::DeploymentComparison::preview).
//!
//! Each load has a ticket; selecting another row (or retrying) starts a new one and aborts the
//! old, whose late answer is dropped.

use super::comparison::{DeploymentDiff, DeploymentPreview};
use super::types::SyncEntry;
use crate::platform::i18n::I18n;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewState {
    /// No row selected.
    Empty,
    Loading,
    Ready(DeploymentPreview),
    Failed(String),
}

/// A preview to load: run a comparison preview of `entry`, then answer with
/// [`PreviewPane::loaded`] and `ticket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLoad {
    pub ticket: u64,
    pub entry: SyncEntry,
}

/// What a change of the pane asks for: abort the operation of the load `abort`, start `load`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewRequests {
    pub abort: Option<u64>,
    pub load: Option<PreviewLoad>,
}

/// The input of the embedded diff viewer (editor kind `diff`), side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffInput {
    pub kind: &'static str,
    pub key: String,
    pub path: String,
    pub data: DeploymentDiff,
    pub embedded: bool,
    pub mode: &'static str,
}

pub fn diff_input(data: &DeploymentDiff) -> DiffInput {
    DiffInput {
        kind: "diff",
        key: format!("deployment:{}", data.path),
        path: data.path.clone(),
        data: data.clone(),
        embedded: true,
        mode: "split",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewPane {
    entry: Option<SyncEntry>,
    ticket: u64,
    /// The load in flight.
    loading: Option<u64>,
    pub state: PreviewState,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self { entry: None, ticket: 0, loading: None, state: PreviewState::Empty }
    }
}

impl PreviewPane {
    fn load(&mut self) -> PreviewRequests {
        let abort = self.loading.take();
        let Some(entry) = self.entry.clone() else {
            self.state = PreviewState::Empty;
            return PreviewRequests { abort, load: None };
        };
        self.ticket += 1;
        self.loading = Some(self.ticket);
        self.state = PreviewState::Loading;
        PreviewRequests { abort, load: Some(PreviewLoad { ticket: self.ticket, entry }) }
    }

    /// Shows the preview of `entry` (the selected row, `None` without one).
    pub fn show(&mut self, entry: Option<SyncEntry>) -> PreviewRequests {
        if entry == self.entry && self.state != PreviewState::Empty {
            return PreviewRequests::default();
        }
        self.entry = entry;
        self.load()
    }

    /// Loads the preview again after a failure (Retry).
    pub fn retry(&mut self) -> PreviewRequests {
        self.load()
    }

    /// The answer of a [`PreviewLoad`]: an error message or the preview.
    pub fn loaded(&mut self, ticket: u64, result: Result<DeploymentPreview, String>, i18n: &I18n) {
        if self.loading != Some(ticket) {
            return;
        }
        self.loading = None;
        self.state = match result {
            Ok(preview) => PreviewState::Ready(preview),
            Err(message) => PreviewState::Failed(i18n.tx_template(&message)),
        };
    }

    /// The diff to embed, once a text preview is loaded.
    pub fn diff(&self) -> Option<DiffInput> {
        match &self.state {
            PreviewState::Ready(DeploymentPreview::Text(data)) => Some(diff_input(data)),
            _ => None,
        }
    }

    /// The text shown instead of a diff; `None` while a diff or an error is shown.
    pub fn message(&self, i18n: &I18n) -> Option<String> {
        let text = match &self.state {
            PreviewState::Empty => "Select a file to compare its contents",
            PreviewState::Loading => "Loading file contents…",
            PreviewState::Ready(DeploymentPreview::Binary) => "Binary files cannot be shown as text",
            PreviewState::Ready(DeploymentPreview::TooLarge) => {
                "This file is too large for the text preview. Use Binary Content to compare it."
            }
            PreviewState::Ready(DeploymentPreview::Text(_)) | PreviewState::Failed(_) => return None,
        };
        Some(i18n.tx(text))
    }
}

#[cfg(test)]
mod tests {
    use super::super::comparison::DiffSide;
    use super::super::types::SyncStatus;
    use super::*;

    fn entry(path: &str) -> SyncEntry {
        SyncEntry {
            path: path.to_owned(),
            remote_path: format!("/srv/{path}"),
            local: None,
            remote: None,
            status: SyncStatus::Same,
        }
    }

    fn result(text: &str) -> DeploymentPreview {
        DeploymentPreview::Text(DeploymentDiff {
            path: "/work/file.php".to_owned(),
            title: "file.php".to_owned(),
            left: DiffSide::Text { text: "local".to_owned(), label: "Local".to_owned() },
            right: DiffSide::Text { text: text.to_owned(), label: "Remote".to_owned() },
        })
    }

    fn right_text(pane: &PreviewPane) -> Option<String> {
        match pane.diff()?.data.right {
            DiffSide::Text { text, .. } | DiffSide::File { path: text, .. } => Some(text),
        }
    }

    // deployment diff preview › ignores a late response for a file that is no longer selected
    #[test]
    fn ignores_a_late_response_for_a_file_that_is_no_longer_selected() {
        let i18n = I18n::default();
        let mut pane = PreviewPane::default();
        let first = pane.show(Some(entry("first.php"))).load.expect("the first file loads");
        let second = pane.show(Some(entry("second.php")));
        assert_eq!(second.abort, Some(first.ticket));
        let second = second.load.expect("the second file loads");
        assert_eq!(second.entry.path, "second.php");
        pane.loaded(second.ticket, Ok(result("current")), &i18n);
        assert_eq!(right_text(&pane).as_deref(), Some("current"));
        pane.loaded(first.ticket, Ok(result("stale")), &i18n);
        assert_eq!(right_text(&pane).as_deref(), Some("current"));
        assert_eq!(pane.diff().map(|input| input.mode), Some("split"));
    }

    // deployment diff preview › shows read errors and allows retrying without closing the comparison
    #[test]
    fn shows_read_errors_and_allows_retrying_without_closing_the_comparison() {
        let i18n = I18n::default();
        let mut pane = PreviewPane::default();
        let load = pane.show(Some(entry("file.php"))).load.expect("the file loads");
        pane.loaded(load.ticket, Err("Offline".to_owned()), &i18n);
        assert_eq!(pane.state, PreviewState::Failed("Offline".to_owned()));
        let retry = pane.retry().load.expect("Retry loads again");
        assert_eq!(retry.entry.path, "file.php");
        pane.loaded(retry.ticket, Ok(result("recovered")), &i18n);
        assert_eq!(right_text(&pane).as_deref(), Some("recovered"));
    }

    // deployment diff preview › explains why %s files have no text preview
    #[test]
    fn explains_why_files_have_no_text_preview() {
        let i18n = I18n::default();
        for (kind, text) in [
            (DeploymentPreview::Binary, "Binary files cannot be shown as text"),
            (DeploymentPreview::TooLarge, "too large for the text preview"),
        ] {
            let mut pane = PreviewPane::default();
            let load = pane.show(Some(entry("file.bin"))).load.expect("the file loads");
            pane.loaded(load.ticket, Ok(kind), &i18n);
            assert!(pane.message(&i18n).is_some_and(|message| message.contains(text)));
            assert_eq!(pane.diff(), None);
        }
    }
}
