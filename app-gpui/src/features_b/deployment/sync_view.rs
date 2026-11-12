//! The Sync with Deployed dialog's own state: the row filters, the selected row, keyboard
//! handling, Compare by Binary Content and which operations Synchronize All / Synchronize Selected
//! run. The rows, choices and run state live in [`SyncDialogState`].
//!
//! A content scan checks the rows that exist on both sides one at a time
//! ([`ScanRequest`], run with [`DeploymentComparison::equal`](super::comparison::DeploymentComparison::equal))
//! and suggests an operation from what it finds, except for rows the user chose an operation
//! for. Switching the mode, refreshing or new rows make a scan void.

use super::model::{SyncChoice, default_sync_action, sync_choices, sync_steps};
use super::store::{SyncDialogState, SyncRunMode};
use super::types::{SyncEntry, SyncStatus, SyncStep, TransferReport};
use super::window::comparison_title;
use crate::platform::i18n::{I18n, Param};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareBy {
    /// Size and Timestamp: the backend's status.
    Metadata,
    /// Binary Content: the bytes of both sides.
    Content,
}

#[derive(Debug, Clone, PartialEq)]
struct ContentScan {
    ticket: u64,
    revision: u64,
    generation: u64,
    /// Whether the bytes of a row are equal.
    verified: HashMap<String, bool>,
    errors: HashMap<String, String>,
    /// Rows checked so far.
    next: usize,
    done: bool,
}

/// Compare the bytes of `entry`, then answer with [`SyncView::equal_result`] and `ticket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRequest {
    pub ticket: u64,
    pub entry: SyncEntry,
}

/// A key pressed on the file table (not in one of its controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKey {
    Up,
    Down,
    /// Enter without modifiers, outside an IME composition.
    Enter,
    Space,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogEffect {
    /// The standalone window saves the local documents of these steps first; then call
    /// [`SyncView::prepared`].
    BeforeSynchronize { steps: Vec<SyncStep> },
    /// Run [`DeploymentStore::run_sync`](super::store::DeploymentStore::run_sync), then call
    /// [`SyncView::synchronized`] with the report it finished with.
    RunSync { steps: Vec<SyncStep>, mode: SyncRunMode },
    /// Compare again: `deploy_coverage(paths)`, then
    /// [`DeploymentStore::open_sync`](super::store::DeploymentStore::open_sync).
    Refresh { server: String, paths: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
struct PendingRun {
    steps: Vec<SyncStep>,
    mode: SyncRunMode,
    /// The comparison it was started in.
    paths: Vec<String>,
    /// The rows shown and the row selected when it started.
    shown: Vec<String>,
    selected: Option<String>,
    /// The selection state then (`None`: the first row).
    selected_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncView {
    standalone: bool,
    selected_path: Option<String>,
    pub filter: String,
    pub show_local: bool,
    pub show_different: bool,
    pub show_remote: bool,
    compare_by: CompareBy,
    scan: Option<ContentScan>,
    tickets: u64,
    /// Rows whose operation the user chose: scans suggest nothing for them.
    touched: HashSet<String>,
    synchronizing: bool,
    pending: Option<PendingRun>,
    /// Bumped by Refresh: a comparison made before it is void.
    generation: u64,
}

/// The tooltip of a row's status.
pub fn status_label(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::LocalOnly => "Local only",
        SyncStatus::RemoteOnly => "Remote only",
        SyncStatus::LocalNewer => "Local is newer",
        SyncStatus::RemoteNewer => "Remote is newer",
        SyncStatus::Different => "Contents differ",
        SyncStatus::Same => "Identical",
    }
}

/// The arrow and the name of an operation.
pub fn choice_label(choice: SyncChoice) -> (&'static str, &'static str) {
    match choice {
        SyncChoice::Skip => ("—", "Skip"),
        SyncChoice::Upload => ("→", "Upload"),
        SyncChoice::Download => ("←", "Download"),
        SyncChoice::DeleteRemote => ("× →", "Delete remote"),
        SyncChoice::DeleteLocal => ("← ×", "Delete local"),
    }
}

impl SyncView {
    /// `standalone`: the comparison fills its own window, which saves local edits before a run.
    pub fn new(standalone: bool) -> Self {
        Self {
            standalone,
            selected_path: None,
            filter: String::new(),
            show_local: true,
            show_different: true,
            show_remote: true,
            compare_by: CompareBy::Metadata,
            scan: None,
            tickets: 0,
            touched: HashSet::new(),
            synchronizing: false,
            pending: None,
            generation: 0,
        }
    }

    pub fn compare_by(&self) -> CompareBy {
        self.compare_by
    }

    /// The comparison (`DeploymentComparison`) is made again when this changes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn title(&self, sync: &SyncDialogState, i18n: &mut I18n) -> String {
        comparison_title(&sync.server_name, i18n)
    }

    fn active_scan(&self, sync: &SyncDialogState) -> Option<&ContentScan> {
        let content = self.compare_by == CompareBy::Content;
        self.scan
            .as_ref()
            .filter(|scan| content && scan.revision == sync.revision && scan.generation == self.generation)
    }

    /// Rows checked by a running content scan; `None` when none runs.
    pub fn progress(&self, sync: &SyncDialogState) -> Option<usize> {
        if self.compare_by != CompareBy::Content || sync.entries.is_none() {
            return None;
        }
        match self.active_scan(sync) {
            Some(scan) if scan.done => None,
            Some(scan) => Some(scan.next),
            None => Some(0),
        }
    }

    pub fn busy(&self, sync: &SyncDialogState) -> bool {
        sync.running || sync.entries.is_none() || self.progress(sync).is_some()
    }

    /// Why the content of a row could not be compared.
    pub fn check_error<'s>(&'s self, sync: &SyncDialogState, path: &str) -> Option<&'s str> {
        self.active_scan(sync).and_then(|scan| scan.errors.get(path)).map(String::as_str)
    }

    /// The rows with the statuses a content scan found.
    pub fn compared_entries(&self, sync: &SyncDialogState) -> Vec<SyncEntry> {
        let verified = self.active_scan(sync).map(|scan| &scan.verified);
        let entries = sync.entries.iter().flatten();
        entries
            .map(|entry| match verified.and_then(|verified| verified.get(&entry.path)) {
                Some(&equal) => {
                    let status = if equal { SyncStatus::Same } else { SyncStatus::Different };
                    SyncEntry { status, ..entry.clone() }
                }
                None => entry.clone(),
            })
            .collect()
    }

    /// The rows the name filter and the category buttons let through; rows whose content could not
    /// be compared always show.
    pub fn shown(&self, sync: &SyncDialogState) -> Vec<SyncEntry> {
        let query = self.filter.trim().to_lowercase();
        self.compared_entries(sync)
            .into_iter()
            .filter(|entry| {
                entry.path.to_lowercase().contains(&query)
                    && (self.check_error(sync, &entry.path).is_some()
                        || match entry.status {
                            SyncStatus::Same => !sync.hide_same,
                            SyncStatus::LocalOnly => self.show_local,
                            SyncStatus::RemoteOnly => self.show_remote,
                            _ => self.show_different,
                        })
            })
            .collect()
    }

    /// The selected row, else the first one shown.
    pub fn selected(&self, sync: &SyncDialogState) -> Option<SyncEntry> {
        let mut shown = self.shown(sync);
        let index = shown.iter().position(|entry| Some(&entry.path) == self.selected_path.as_ref()).unwrap_or(0);
        (index < shown.len()).then(|| shown.swap_remove(index))
    }

    /// The row the content diff shows, as the backend listed it.
    pub fn preview_entry(&self, sync: &SyncDialogState) -> Option<SyncEntry> {
        let selected = self.selected(sync)?;
        sync.entries.iter().flatten().find(|entry| entry.path == selected.path).cloned()
    }

    /// The selected row's content error, shown above the diff.
    pub fn selected_check_error(&self, sync: &SyncDialogState) -> Option<String> {
        let selected = self.selected(sync)?;
        self.check_error(sync, &selected.path).map(str::to_owned)
    }

    pub fn steps(&self, sync: &SyncDialogState) -> Vec<SyncStep> {
        sync_steps(&sync.choices)
    }

    pub fn selected_steps(&self, sync: &SyncDialogState) -> Vec<SyncStep> {
        let Some(selected) = self.selected(sync) else { return Vec::new() };
        self.steps(sync).into_iter().filter(|step| step.path == selected.path).collect()
    }

    pub fn can_synchronize_all(&self, sync: &SyncDialogState) -> bool {
        !self.busy(sync) && !self.steps(sync).is_empty()
    }

    pub fn can_synchronize_selected(&self, sync: &SyncDialogState) -> bool {
        !self.busy(sync) && !self.selected_steps(sync).is_empty()
    }

    /// "3 of 12 files will change", once the rows are listed.
    pub fn footer(&self, sync: &SyncDialogState, i18n: &mut I18n) -> Option<String> {
        let entries = sync.entries.as_ref()?;
        let values = [Param::from(self.steps(sync).len()), Param::from(entries.len())];
        Some(i18n.tx_built(&["", " of ", " files will change"], &values))
    }

    pub fn select(&mut self, path: &str) {
        self.selected_path = Some(path.to_owned());
    }

    pub fn toggle_identical(&self, sync: &mut SyncDialogState) {
        sync.hide_same = !sync.hide_same;
    }

    /// Sets a row's operation by hand; scans no longer suggest one for it.
    pub fn choose(&mut self, path: &str, choice: SyncChoice, sync: &mut SyncDialogState) {
        if sync.running {
            return;
        }
        self.touched.insert(path.to_owned());
        self.select(path);
        sync.choices.set(path, choice);
    }

    fn suggest(&self, sync: &mut SyncDialogState, path: &str, status: SyncStatus) {
        if !self.touched.contains(path) {
            sync.choices.set(path, default_sync_action(status));
        }
    }

    /// Switches Compare by (Stop content comparison switches back to metadata).
    pub fn set_compare_by(&mut self, mode: CompareBy, sync: &mut SyncDialogState) -> Option<ScanRequest> {
        self.scan = None;
        self.compare_by = mode;
        self.restart(sync)
    }

    /// Suggests operations for the current rows: from their status, or by a new content scan.
    /// Called when the mode changes and when the rows were replaced.
    pub fn restart(&mut self, sync: &mut SyncDialogState) -> Option<ScanRequest> {
        let entries = sync.entries.clone()?;
        match self.compare_by {
            CompareBy::Metadata => {
                self.scan = None;
                for entry in &entries {
                    self.suggest(sync, &entry.path, entry.status);
                }
                None
            }
            CompareBy::Content => {
                self.tickets += 1;
                self.scan = Some(ContentScan {
                    ticket: self.tickets,
                    revision: sync.revision,
                    generation: self.generation,
                    verified: HashMap::new(),
                    errors: HashMap::new(),
                    next: 0,
                    done: false,
                });
                self.scan_next(&entries)
            }
        }
    }

    fn scan_next(&mut self, entries: &[SyncEntry]) -> Option<ScanRequest> {
        let scan = self.scan.as_mut()?;
        while let Some(entry) = entries.get(scan.next) {
            if entry.local.is_some() && entry.remote.is_some() {
                return Some(ScanRequest { ticket: scan.ticket, entry: entry.clone() });
            }
            scan.next += 1;
        }
        scan.done = true;
        None
    }

    /// The answer of a [`ScanRequest`]: equal rows become identical (and skipped), others differ.
    pub fn equal_result(
        &mut self,
        ticket: u64,
        result: Result<bool, String>,
        sync: &mut SyncDialogState,
    ) -> Option<ScanRequest> {
        let current = self.active_scan(sync).is_some_and(|scan| scan.ticket == ticket && !scan.done);
        let entries = sync.entries.clone().filter(|_| current)?;
        let index = self.scan.as_ref()?.next;
        let entry = entries.get(index)?;
        match result {
            Ok(equal) => {
                if let Some(scan) = self.scan.as_mut() {
                    scan.verified.insert(entry.path.clone(), equal);
                }
                let status = match (equal, entry.status) {
                    (true, _) => SyncStatus::Same,
                    (false, SyncStatus::Same) => SyncStatus::Different,
                    (false, status) => status,
                };
                self.suggest(sync, &entry.path, status);
            }
            Err(message) => {
                if let Some(scan) = self.scan.as_mut() {
                    scan.errors.insert(entry.path.clone(), message);
                }
            }
        }
        if let Some(scan) = self.scan.as_mut() {
            scan.next += 1;
        }
        self.scan_next(&entries)
    }

    /// Synchronize All, or Synchronize Selected (the toolbar button, Enter).
    pub fn synchronize(&mut self, mode: SyncRunMode, sync: &mut SyncDialogState) -> Option<DialogEffect> {
        let steps = match mode {
            SyncRunMode::All => self.steps(sync),
            SyncRunMode::Selected => self.selected_steps(sync),
        };
        if self.busy(sync) || self.synchronizing || steps.is_empty() {
            return None;
        }
        self.synchronizing = true;
        if mode == SyncRunMode::Selected {
            self.touched.extend(steps.iter().map(|step| step.path.clone()));
        }
        self.pending = Some(PendingRun {
            steps: steps.clone(),
            mode,
            paths: sync.paths.clone(),
            shown: self.shown(sync).into_iter().map(|entry| entry.path).collect(),
            selected: self.selected(sync).map(|entry| entry.path),
            selected_path: self.selected_path.clone(),
        });
        if self.standalone {
            sync.running = true;
            return Some(DialogEffect::BeforeSynchronize { steps });
        }
        Some(DialogEffect::RunSync { steps, mode })
    }

    fn end_run(&mut self) -> Option<PendingRun> {
        self.synchronizing = false;
        self.pending.take()
    }

    /// The answer of [`DialogEffect::BeforeSynchronize`]; `sync` is the comparison open now.
    pub fn prepared(&mut self, result: Result<(), String>, sync: Option<&mut SyncDialogState>) -> Option<DialogEffect> {
        let pending = self.pending.as_ref()?;
        let Some(sync) = sync.filter(|sync| sync.paths == pending.paths) else {
            self.end_run();
            return None;
        };
        match result {
            Ok(()) => Some(DialogEffect::RunSync { steps: pending.steps.clone(), mode: pending.mode }),
            Err(message) => {
                sync.running = false;
                sync.error = Some(message);
                self.end_run();
                None
            }
        }
    }

    /// A run ended (`report`: the one it finished with). After a clean Synchronize Selected the
    /// selection moves on to the next row with an operation, unless the user selected another row
    /// meanwhile.
    pub fn synchronized(&mut self, report: Option<&TransferReport>, sync: Option<&SyncDialogState>) {
        let Some(pending) = self.end_run() else { return };
        let clean = report.is_some_and(|report| report.files > 0 && report.errors.is_empty() && report.skipped == 0);
        let Some(sync) = sync.filter(|_| clean && pending.mode == SyncRunMode::Selected) else { return };
        let index = pending.shown.iter().position(|path| Some(path) == pending.selected.as_ref());
        let (after, before) = match index {
            Some(index) => (&pending.shown[index + 1..], &pending.shown[..index]),
            None => (&pending.shown[..], &pending.shown[..0]),
        };
        let next = after
            .iter()
            .chain(before)
            .find(|path| sync.choices.get(path).is_some_and(|choice| choice != SyncChoice::Skip));
        if let Some(next) = next
            && self.selected_path == pending.selected_path
        {
            self.selected_path = Some(next.clone());
        }
    }

    /// A key on the file table: arrows move the selection, Enter synchronizes the selected row,
    /// Space cycles its operation.
    pub fn on_table_key(&mut self, key: TableKey, sync: &mut SyncDialogState) -> Option<DialogEffect> {
        match key {
            TableKey::Up | TableKey::Down => {
                let shown = self.shown(sync);
                let selected = self.selected(sync).map(|entry| entry.path);
                let index = shown.iter().position(|entry| Some(&entry.path) == selected.as_ref());
                let next = match (key, index) {
                    (TableKey::Down, Some(index)) => (index + 1).min(shown.len().saturating_sub(1)),
                    (_, Some(index)) => index.saturating_sub(1),
                    (_, None) => 0,
                };
                if let Some(entry) = shown.get(next) {
                    self.selected_path = Some(entry.path.clone());
                }
                None
            }
            TableKey::Enter => self.synchronize(SyncRunMode::Selected, sync),
            TableKey::Space => {
                let selected = self.selected(sync).filter(|_| !self.busy(sync))?;
                let choices = sync_choices(&selected);
                let current = sync.choices.get(&selected.path).unwrap_or(SyncChoice::Skip);
                let index = choices.iter().position(|choice| *choice == current).map_or(0, |index| index + 1);
                let next = choices[index % choices.len()];
                self.choose(&selected.path, next, sync);
                None
            }
        }
    }

    /// Refresh comparison: compares again, forgetting the operations chosen by hand.
    pub fn refresh(&mut self, sync: &SyncDialogState, server_available: bool) -> Option<DialogEffect> {
        if !server_available || self.busy(sync) {
            return None;
        }
        self.touched.clear();
        self.generation += 1;
        self.scan = None;
        Some(DialogEffect::Refresh { server: sync.server.clone(), paths: sync.paths.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{SyncChoices, initial_sync_choices};
    use super::super::types::{FileStamp, SyncAction};
    use super::*;

    fn entry(path: &str, status: SyncStatus) -> SyncEntry {
        let local = FileStamp { size: 2048, mtime: Some(1_700_000_000) };
        let remote = FileStamp { size: 2048, mtime: Some(1_700_000_100) };
        SyncEntry {
            path: path.to_owned(),
            remote_path: format!("/srv/app/{path}"),
            local: (status != SyncStatus::RemoteOnly).then_some(local),
            remote: (status != SyncStatus::LocalOnly).then_some(remote),
            status,
        }
    }

    fn open(entries: Vec<SyncEntry>) -> SyncDialogState {
        SyncDialogState {
            server: "s1".to_owned(),
            server_name: "Staging".to_owned(),
            paths: vec!["/work/app".to_owned()],
            skipped: Vec::new(),
            request: None,
            choices: initial_sync_choices(&entries),
            entries: Some(entries),
            error: None,
            hide_same: true,
            running: false,
            revision: 1,
        }
    }

    fn step(path: &str, action: SyncAction) -> SyncStep {
        SyncStep { path: path.to_owned(), action }
    }

    fn shown_paths(view: &SyncView, sync: &SyncDialogState) -> Vec<String> {
        view.shown(sync).into_iter().map(|entry| entry.path).collect()
    }

    // deployment directory and content comparison › fills a secondary window without creating an
    // in-page modal and saves local edits before synchronization
    #[test]
    fn fills_a_secondary_window_without_creating_an_in_page_modal_and_saves_local_edits_before_synchronization() {
        let mut i18n = I18n::default();
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalOnly)]);
        let mut view = SyncView::new(true);
        assert_eq!(view.title(&sync, &mut i18n), "Diff Between 'local files' and 'files on Staging'");
        let steps = vec![step("a.php", SyncAction::Upload)];
        let effect = view.synchronize(SyncRunMode::All, &mut sync);
        assert_eq!(effect, Some(DialogEffect::BeforeSynchronize { steps: steps.clone() }));
        assert!(!view.can_synchronize_all(&sync));
        assert_eq!(view.synchronize(SyncRunMode::All, &mut sync), None);
        let run = view.prepared(Ok(()), Some(&mut sync));
        assert_eq!(run, Some(DialogEffect::RunSync { steps, mode: SyncRunMode::All }));
    }

    // deployment directory and content comparison › shows mirrored metadata and previews the
    // selected file, with identical rows filtered
    #[test]
    fn shows_mirrored_metadata_and_previews_the_selected_file_with_identical_rows_filtered() {
        let entries = vec![
            entry("a.php", SyncStatus::LocalOnly),
            entry("b.php", SyncStatus::RemoteNewer),
            entry("c.php", SyncStatus::Same),
        ];
        let mut sync = open(entries);
        let mut view = SyncView::new(false);
        assert_eq!(shown_paths(&view, &sync), ["a.php", "b.php"]);
        let preview = |view: &SyncView, sync: &SyncDialogState| view.preview_entry(sync).map(|entry| entry.remote_path);
        assert_eq!(preview(&view, &sync).as_deref(), Some("/srv/app/a.php"));
        view.select("b.php");
        assert_eq!(view.selected(&sync).map(|entry| entry.path).as_deref(), Some("b.php"));
        assert_eq!(preview(&view, &sync).as_deref(), Some("/srv/app/b.php"));
        view.toggle_identical(&mut sync);
        assert!(shown_paths(&view, &sync).contains(&"c.php".to_owned()));
        assert_eq!(sync.choices.get("c.php"), Some(SyncChoice::Skip));
    }

    // deployment directory and content comparison › filters by name and category, navigates with
    // arrows and cycles operations with Space
    #[test]
    fn filters_by_name_and_category_navigates_with_arrows_and_cycles_operations_with_space() {
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalOnly), entry("b.php", SyncStatus::RemoteNewer)]);
        let mut view = SyncView::new(false);
        view.on_table_key(TableKey::Down, &mut sync);
        assert_eq!(view.selected(&sync).map(|entry| entry.path).as_deref(), Some("b.php"));
        view.on_table_key(TableKey::Space, &mut sync);
        assert_eq!(sync.choices.get("b.php"), Some(SyncChoice::DeleteRemote));
        view.filter = "b.php".to_owned();
        assert_eq!(shown_paths(&view, &sync), ["b.php"]);
        view.show_different = false;
        assert!(view.shown(&sync).is_empty());
    }

    // deployment directory and content comparison › updates a row's operation and only applies
    // explicit synchronization
    #[test]
    fn updates_a_rows_operation_and_only_applies_explicit_synchronization() {
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalOnly)]);
        let mut view = SyncView::new(false);
        view.choose("a.php", SyncChoice::Skip, &mut sync);
        assert!(!view.can_synchronize_all(&sync));
        view.choose("a.php", SyncChoice::Upload, &mut sync);
        let effect = view.synchronize(SyncRunMode::All, &mut sync);
        let steps = vec![step("a.php", SyncAction::Upload)];
        assert_eq!(effect, Some(DialogEffect::RunSync { steps, mode: SyncRunMode::All }));
    }

    // deployment directory and content comparison › finds content hidden by identical metadata and
    // skips byte-identical files
    #[test]
    fn finds_content_hidden_by_identical_metadata_and_skips_byte_identical_files() {
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalNewer), entry("b.php", SyncStatus::Same)]);
        let mut view = SyncView::new(false);
        let mut checked = 0;
        let mut request = view.set_compare_by(CompareBy::Content, &mut sync);
        while let Some(scan) = request {
            checked += 1;
            request = view.equal_result(scan.ticket, Ok(scan.entry.path == "a.php"), &mut sync);
        }
        assert_eq!(checked, 2);
        assert_eq!(view.progress(&sync), None);
        assert_eq!(sync.choices.get("b.php"), Some(SyncChoice::Upload));
        assert_eq!(shown_paths(&view, &sync), ["b.php"]);
        assert_eq!(sync.choices.get("a.php"), Some(SyncChoice::Skip));
        view.set_compare_by(CompareBy::Metadata, &mut sync);
        assert!(shown_paths(&view, &sync).contains(&"a.php".to_owned()));
        assert_eq!(sync.choices.get("a.php"), Some(SyncChoice::Upload));
    }

    // deployment directory and content comparison › synchronizes only the current row from the
    // toolbar or Enter, and disables skipped selections
    #[test]
    fn synchronizes_only_the_current_row_from_the_toolbar_or_enter_and_disables_skipped_selections() {
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalOnly), entry("b.php", SyncStatus::RemoteNewer)]);
        let mut view = SyncView::new(false);
        view.select("b.php");
        let effect = view.synchronize(SyncRunMode::Selected, &mut sync);
        let steps = vec![step("b.php", SyncAction::Download)];
        assert_eq!(effect, Some(DialogEffect::RunSync { steps, mode: SyncRunMode::Selected }));
        view.synchronized(None, Some(&sync));
        view.select("a.php");
        let effect = view.on_table_key(TableKey::Enter, &mut sync);
        let steps = vec![step("a.php", SyncAction::Upload)];
        assert_eq!(effect, Some(DialogEffect::RunSync { steps, mode: SyncRunMode::Selected }));
        view.synchronized(None, Some(&sync));
        view.choose("a.php", SyncChoice::Skip, &mut sync);
        assert!(!view.can_synchronize_selected(&sync));
        assert!(view.can_synchronize_all(&sync));
        assert_eq!(view.on_table_key(TableKey::Enter, &mut sync), None);
    }

    // deployment directory and content comparison › preserves manual choices, reports failures,
    // and ignores a cancelled scan
    #[test]
    fn preserves_manual_choices_reports_failures_and_ignores_a_cancelled_scan() {
        let mut sync = open(vec![entry("a.php", SyncStatus::LocalNewer)]);
        let mut view = SyncView::new(false);
        view.choose("a.php", SyncChoice::Skip, &mut sync);
        let first = view.set_compare_by(CompareBy::Content, &mut sync).expect("a.php is compared");
        // Stop content comparison.
        assert_eq!(view.set_compare_by(CompareBy::Metadata, &mut sync), None);
        assert_eq!(view.equal_result(first.ticket, Ok(true), &mut sync), None);
        assert_eq!(sync.choices.get("a.php"), Some(SyncChoice::Skip));
        let second = view.set_compare_by(CompareBy::Content, &mut sync).expect("a.php is compared again");
        view.equal_result(second.ticket, Err("Permission denied".to_owned()), &mut sync);
        assert_eq!(view.selected_check_error(&sync).as_deref(), Some("Permission denied"));
        assert_eq!(sync.choices.get("a.php"), Some(SyncChoice::Skip));
    }

    // Not from a spec: after a clean Synchronize Selected the selection moves to the next row with
    // an operation; the footer counts the changes.
    #[test]
    fn moves_on_to_the_next_row_after_synchronizing_one() {
        let mut i18n = I18n::default();
        let entries = vec![
            entry("a.php", SyncStatus::LocalOnly),
            entry("b.php", SyncStatus::LocalOnly),
            entry("c.php", SyncStatus::LocalOnly),
        ];
        let mut sync = open(entries);
        sync.choices = SyncChoices::from_pairs(&[
            ("a.php", SyncChoice::Upload),
            ("b.php", SyncChoice::Skip),
            ("c.php", SyncChoice::Upload),
        ]);
        let mut view = SyncView::new(false);
        assert_eq!(view.footer(&sync, &mut i18n).as_deref(), Some("2 of 3 files will change"));
        view.synchronize(SyncRunMode::Selected, &mut sync);
        let report = TransferReport { files: 1, skipped: 0, errors: Vec::new() };
        view.synchronized(Some(&report), Some(&sync));
        assert_eq!(view.selected(&sync).map(|entry| entry.path).as_deref(), Some("c.php"));
    }
}
