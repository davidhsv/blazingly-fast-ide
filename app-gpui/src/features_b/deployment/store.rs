//! Session state of deployment: the Sync with Deployed comparison and its runs, the SSH secret
//! prompt, the part that had focus last and the Remote Host trees.
//!
//! The comparison and a synchronisation are request flows: each method returns the requests to
//! send ([`SyncEffect`]) and takes the answers with their request id. An answer for a request the
//! dialog no longer waits for (closed, reopened, run again) is dropped.

use super::actions::{Eligibility, Notice, is_cancelled, report_notice, server_coverage};
use super::model::unique_by_path;
use super::model::{FocusedPart, ReportVerb, SyncChoice, SyncChoices, initial_sync_choices, server_label};
use super::remote_host::RemoteHost;
use super::types::{DeploymentCoverage, SyncAction, SyncEntry, SyncStep, TransferError, TransferReport};
use crate::platform::i18n::{I18n, Param};
use ide_core::settings::{DeploymentServer, DeploymentSettings};
use ide_core::{IdeError, RequestId};
use std::collections::{HashMap, HashSet};

/// The Sync with Deployed dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncDialogState {
    pub server: String,
    pub server_name: String,
    /// Absolute local paths selected for comparison.
    pub paths: Vec<String>,
    /// Selected paths the server does not deploy, and why.
    pub skipped: Vec<TransferError>,
    /// The comparison or synchronisation in flight.
    pub request: Option<RequestId>,
    /// `None` while comparing.
    pub entries: Option<Vec<SyncEntry>>,
    pub error: Option<String>,
    pub choices: SyncChoices,
    pub hide_same: bool,
    pub running: bool,
    /// Changes whenever `entries` is replaced, so that work started for older rows is void.
    pub revision: u64,
}

/// Synchronize All closes the dialog; Synchronize Selected keeps it and refreshes the rows run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncRunMode {
    All,
    Selected,
}

/// What a comparison or synchronisation needs done.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncEffect {
    /// `deploy_compare(path, server, request)`; answer with [`DeploymentStore::compared`].
    Compare {
        path: String,
        server: String,
        request: RequestId,
    },
    /// Save these open documents (the embedded diff edits the local side), so the server receives
    /// what the user reviewed; answer with [`DeploymentStore::documents_saved`].
    SaveDocuments {
        paths: Vec<String>,
        request: RequestId,
    },
    /// `deploy_sync(steps, server, request)`; answer with [`DeploymentStore::synced`].
    Sync {
        steps: Vec<SyncStep>,
        server: String,
        request: RequestId,
    },
    /// `cancel_request`.
    Cancel(RequestId),
    Notify(Notice),
    /// A synchronisation finished with this report.
    Finished(TransferReport),
}

#[derive(Debug, Clone)]
struct Comparing {
    request: RequestId,
    server: String,
    /// Covered paths still to compare, the next one last.
    pending: Vec<String>,
    found: Vec<SyncEntry>,
}

#[derive(Debug, Clone)]
struct Running {
    request: RequestId,
    steps: Vec<SyncStep>,
    mode: SyncRunMode,
    server: DeploymentServer,
    /// The project root without a trailing `/`.
    root: Option<String>,
    report: Option<TransferReport>,
    /// The step whose row is refreshed next.
    next: usize,
    refreshed: HashMap<String, SyncEntry>,
    checked: HashSet<String>,
    refresh_errors: Vec<String>,
}

/// The password / passphrase prompt of SSH configuration `ssh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretPrompt {
    pub ssh: String,
    pub message: String,
}

/// What the SSH Password dialog answers (`ssh_set_secret(ssh, secret, save)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretAnswer {
    pub secret: String,
    pub save: bool,
}

/// The SSH Password dialog's fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretForm {
    pub secret: String,
    pub save: bool,
}

impl SecretForm {
    /// OK (or Enter): the answer, once something is typed.
    pub fn submit(&mut self) -> Option<SecretAnswer> {
        if self.secret.is_empty() {
            return None;
        }
        let answer = SecretAnswer { secret: std::mem::take(&mut self.secret), save: self.save };
        self.save = false;
        Some(answer)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeploymentStore {
    pub sync: Option<SyncDialogState>,
    pub secret: Option<SecretPrompt>,
    /// Which workbench part had focus last (targets of actions run from Find Action).
    pub part: Option<FocusedPart>,
    /// Server shown in the Remote Host tool window.
    pub remote_server: Option<String>,
    pub host: RemoteHost,
    comparing: Option<Comparing>,
    running: Option<Running>,
}

impl DeploymentStore {
    /// The project view or the editor gained focus.
    pub fn focus_gained(&mut self, part: FocusedPart) {
        self.part = Some(part);
    }

    /// A reopened project starts with a fresh backend: forget the old one's trees and comparison.
    pub fn project_changed(&mut self) {
        self.host = RemoteHost::default();
        self.remote_server = None;
        self.sync = None;
        self.comparing = None;
        self.running = None;
    }

    fn is_current(&self, request: RequestId) -> bool {
        self.sync.as_ref().is_some_and(|sync| sync.request == Some(request))
    }

    /// Opens the comparison of `paths` with `server` and compares every covered path in turn.
    /// `coverage` is the answer of `deploy_coverage(paths)`; `request` is a fresh request id.
    pub fn open_sync(
        &mut self,
        server: &DeploymentServer,
        paths: Vec<String>,
        coverage: &[DeploymentCoverage],
        request: RequestId,
        i18n: &mut I18n,
    ) -> Vec<SyncEffect> {
        let covered = match server_coverage(coverage, server, i18n) {
            Eligibility::Unknown => return Vec::new(),
            Eligibility::Rejected(notice) => return vec![SyncEffect::Notify(notice)],
            Eligibility::Covered(covered) => covered.clone(),
        };
        let revision = self.sync.as_ref().map_or(0, |sync| sync.revision) + 1;
        self.sync = Some(SyncDialogState {
            server: server.id.clone(),
            server_name: server_label(server).to_owned(),
            paths,
            skipped: covered.skipped,
            request: Some(request),
            entries: None,
            error: None,
            choices: SyncChoices::default(),
            hide_same: true,
            running: false,
            revision,
        });
        self.running = None;
        let mut pending = covered.paths;
        pending.reverse();
        self.comparing = Some(Comparing { request, server: server.id.clone(), pending, found: Vec::new() });
        self.next_compare()
    }

    fn next_compare(&mut self) -> Vec<SyncEffect> {
        let Some(comparing) = self.comparing.as_mut() else { return Vec::new() };
        if let Some(path) = comparing.pending.pop() {
            return vec![SyncEffect::Compare { path, server: comparing.server.clone(), request: comparing.request }];
        }
        let found = std::mem::take(&mut comparing.found);
        self.comparing = None;
        if let Some(sync) = self.sync.as_mut() {
            let entries = unique_by_path(found);
            sync.choices = initial_sync_choices(&entries);
            sync.entries = Some(entries);
            sync.request = None;
            sync.revision += 1;
        }
        Vec::new()
    }

    /// The answer of a [`SyncEffect::Compare`], from opening the comparison or from refreshing the
    /// rows a selected synchronisation ran.
    pub fn compared(
        &mut self,
        request: RequestId,
        result: Result<Vec<SyncEntry>, IdeError>,
        i18n: &mut I18n,
    ) -> Vec<SyncEffect> {
        if !self.is_current(request) {
            return Vec::new();
        }
        if self.comparing.as_ref().is_some_and(|comparing| comparing.request == request) {
            return self.compared_opening(result, i18n);
        }
        if self.running.as_ref().is_some_and(|running| running.request == request) {
            return self.refreshed(result, i18n);
        }
        Vec::new()
    }

    fn compared_opening(&mut self, result: Result<Vec<SyncEntry>, IdeError>, i18n: &I18n) -> Vec<SyncEffect> {
        match result {
            Ok(entries) => {
                if let Some(comparing) = self.comparing.as_mut() {
                    comparing.found.extend(entries);
                }
                self.next_compare()
            }
            Err(error) => {
                self.comparing = None;
                if is_cancelled(&error) {
                    self.sync = None;
                } else if let Some(sync) = self.sync.as_mut() {
                    sync.error = Some(i18n.tx_template(&error.message));
                    sync.request = None;
                }
                Vec::new()
            }
        }
    }

    /// Closes the dialog, cancelling a running comparison or synchronisation.
    pub fn close_sync(&mut self) -> Vec<SyncEffect> {
        let request = self.sync.take().and_then(|sync| sync.request);
        self.comparing = None;
        self.running = None;
        request.map(SyncEffect::Cancel).into_iter().collect()
    }

    /// Runs `steps` on the dialog's server. Open documents among the uploaded files
    /// (`open_documents`, absolute paths) are saved first.
    pub fn run_sync(
        &mut self,
        steps: Vec<SyncStep>,
        mode: SyncRunMode,
        settings: &DeploymentSettings,
        project_root: Option<&str>,
        open_documents: &[String],
        request: RequestId,
    ) -> Vec<SyncEffect> {
        let Some(sync) = self.sync.as_mut() else { return Vec::new() };
        let Some(server) = settings.servers.iter().find(|server| server.id == sync.server) else {
            return Vec::new();
        };
        if steps.is_empty() {
            return Vec::new();
        }
        sync.running = true;
        sync.request = Some(request);
        sync.error = None;
        let root = project_root.filter(|root| !root.is_empty()).map(|root| root.strip_suffix('/').unwrap_or(root));
        let saves: Vec<String> = match root {
            Some(root) => steps
                .iter()
                .filter(|step| step.action == SyncAction::Upload)
                .map(|step| format!("{root}/{}", step.path))
                .filter(|path| open_documents.contains(path))
                .collect(),
            None => Vec::new(),
        };
        self.comparing = None;
        self.running = Some(Running {
            request,
            steps,
            mode,
            server: server.clone(),
            root: root.map(str::to_owned),
            report: None,
            next: 0,
            refreshed: HashMap::new(),
            checked: HashSet::new(),
            refresh_errors: Vec::new(),
        });
        if saves.is_empty() { self.sync_effect() } else { vec![SyncEffect::SaveDocuments { paths: saves, request }] }
    }

    fn running_for(&self, request: RequestId) -> bool {
        self.is_current(request) && self.running.as_ref().is_some_and(|running| running.request == request)
    }

    fn sync_effect(&self) -> Vec<SyncEffect> {
        self.running
            .iter()
            .map(|running| SyncEffect::Sync {
                steps: running.steps.clone(),
                server: running.server.id.clone(),
                request: running.request,
            })
            .collect()
    }

    /// A run stopped: by a failure (`error`), or cancelled (`None`).
    fn stop_run(&mut self, error: Option<String>) -> Vec<SyncEffect> {
        self.running = None;
        if let Some(sync) = self.sync.as_mut() {
            sync.running = false;
            sync.request = None;
            if error.is_some() {
                sync.error = error;
            }
        }
        Vec::new()
    }

    /// The answer of [`SyncEffect::SaveDocuments`]: a failed save stops the run before anything is
    /// sent to the server.
    pub fn documents_saved(&mut self, request: RequestId, result: Result<(), String>, i18n: &I18n) -> Vec<SyncEffect> {
        if !self.running_for(request) {
            return Vec::new();
        }
        match result {
            Ok(()) => self.sync_effect(),
            Err(message) => self.stop_run(Some(i18n.tx_template(&message))),
        }
    }

    /// The answer of [`SyncEffect::Sync`]. Synchronize All closes the dialog and reports what ran,
    /// counting the paths the server did not cover as skipped; Synchronize Selected compares the
    /// rows it ran again.
    pub fn synced(
        &mut self,
        request: RequestId,
        result: Result<TransferReport, IdeError>,
        i18n: &mut I18n,
    ) -> Vec<SyncEffect> {
        if !self.running_for(request) {
            return Vec::new();
        }
        let mut report = match result {
            Ok(report) => report,
            Err(error) if is_cancelled(&error) => return self.stop_run(None),
            Err(error) => return self.stop_run(Some(i18n.tx_template(&error.message))),
        };
        let Some(running) = self.running.as_mut() else { return Vec::new() };
        if running.mode == SyncRunMode::Selected && running.root.is_some() {
            running.report = Some(report);
            return self.next_refresh(i18n);
        }
        let Some(running) = self.running.take() else { return Vec::new() };
        if let Some(sync) = self.sync.take() {
            report.skipped += sync.skipped.len() as u32;
        }
        let notice = report_notice(&report, ReportVerb::Sync, &running.server, i18n);
        vec![SyncEffect::Notify(notice), SyncEffect::Finished(report)]
    }

    fn next_refresh(&mut self, i18n: &mut I18n) -> Vec<SyncEffect> {
        let next = self.running.as_ref().and_then(|running| {
            let step = running.steps.get(running.next)?;
            let root = running.root.as_ref()?;
            Some(SyncEffect::Compare {
                path: format!("{root}/{}", step.path),
                server: running.server.id.clone(),
                request: running.request,
            })
        });
        match next {
            Some(effect) => vec![effect],
            None => self.finish_selected(i18n),
        }
    }

    fn refreshed(&mut self, result: Result<Vec<SyncEntry>, IdeError>, i18n: &mut I18n) -> Vec<SyncEffect> {
        let Some(running) = self.running.as_mut() else { return Vec::new() };
        let Some(path) = running.steps.get(running.next).map(|step| step.path.clone()) else { return Vec::new() };
        running.next += 1;
        match result {
            Ok(entries) => {
                if let Some(entry) = entries.into_iter().find(|entry| entry.path == path) {
                    running.refreshed.insert(path.clone(), entry);
                }
                running.checked.insert(path);
            }
            Err(error) if is_cancelled(&error) => return self.stop_run(None),
            Err(error) => running.refresh_errors.push(format!("{path}: {}", i18n.tx_template(&error.message))),
        }
        self.next_refresh(i18n)
    }

    /// The rows a selected run synchronised become Skip when the report accounts for every one of
    /// them (a skipped or interrupted batch does not say which files completed); rows a refresh no
    /// longer lists leave the comparison.
    fn finish_selected(&mut self, i18n: &mut I18n) -> Vec<SyncEffect> {
        let Some(running) = self.running.take() else { return Vec::new() };
        let Some(sync) = self.sync.as_mut() else { return Vec::new() };
        let report = running.report.unwrap_or_default();
        let failed: HashSet<&str> = report.errors.iter().map(|error| error.path.as_str()).collect();
        let completed =
            report.skipped == 0 && i64::from(report.files) == running.steps.len() as i64 - failed.len() as i64;
        if completed {
            for step in running.steps.iter().filter(|step| !failed.contains(step.path.as_str())) {
                sync.choices.set(&step.path, SyncChoice::Skip);
            }
        }
        let mut refreshed = running.refreshed;
        if let Some(entries) = sync.entries.take() {
            let mut kept = Vec::with_capacity(entries.len());
            for entry in entries {
                if !running.checked.contains(&entry.path) {
                    kept.push(entry);
                } else if let Some(updated) = refreshed.remove(&entry.path) {
                    kept.push(updated);
                } else {
                    sync.choices.remove(&entry.path);
                }
            }
            sync.entries = Some(kept);
        }
        let mut lines: Vec<String> =
            report.errors.iter().map(|error| format!("{}: {}", error.path, error.message)).collect();
        for error in &running.refresh_errors {
            lines.push(i18n.tx_built(&["Could not refresh comparison: ", ""], &[Param::from(error.as_str())]));
        }
        sync.error = (!lines.is_empty()).then(|| lines.join("\n"));
        sync.running = false;
        sync.request = None;
        sync.revision += 1;
        let notice = report_notice(&report, ReportVerb::Sync, &running.server, i18n);
        vec![SyncEffect::Notify(notice), SyncEffect::Finished(report)]
    }

    /// Asks for the password / passphrase of SSH configuration `ssh`. A prompt already open is
    /// replaced and handed back, to be answered with nothing.
    pub fn prompt_secret(&mut self, ssh: &str, message: &str) -> Option<SecretPrompt> {
        self.secret.replace(SecretPrompt { ssh: ssh.to_owned(), message: message.to_owned() })
    }

    /// Closes the prompt (answered or cancelled); the caller answers the work waiting on it.
    pub fn close_secret(&mut self) -> Option<SecretPrompt> {
        self.secret.take()
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{FileStamp, SyncStatus};
    use super::*;
    use ide_core::error::ErrorKind;

    fn server(id: &str) -> DeploymentServer {
        DeploymentServer { id: id.to_owned(), name: id.to_owned(), ..DeploymentServer::default() }
    }

    fn settings() -> DeploymentSettings {
        DeploymentSettings {
            servers: vec![server("one"), server("two")],
            default_server: "one".to_owned(),
            ..DeploymentSettings::default()
        }
    }

    fn coverage(paths: &[&str]) -> Vec<DeploymentCoverage> {
        let message = "Not covered by this server's mappings".to_owned();
        let skipped = TransferError { path: "/project/b".to_owned(), message };
        vec![
            DeploymentCoverage {
                server: "one".to_owned(),
                paths: paths.iter().map(|&path| path.to_owned()).collect(),
                skipped: vec![skipped],
            },
            DeploymentCoverage { server: "two".to_owned(), paths: Vec::new(), skipped: Vec::new() },
        ]
    }

    fn step(path: &str, action: SyncAction) -> SyncStep {
        SyncStep { path: path.to_owned(), action }
    }

    const R: RequestId = RequestId(1);

    /// The dialog compared with server "one", its rows `[]`.
    fn opened(i18n: &mut I18n) -> DeploymentStore {
        let mut store = DeploymentStore::default();
        let paths = vec!["/project/a".to_owned(), "/project/b".to_owned()];
        store.open_sync(&server("one"), paths, &coverage(&["/project/a"]), R, i18n);
        store.compared(R, Ok(Vec::new()), i18n);
        assert_eq!(store.sync.as_ref().and_then(|sync| sync.entries.clone()), Some(Vec::new()));
        store
    }

    fn rows(status: SyncStatus, remote: Option<FileStamp>) -> Vec<SyncEntry> {
        ["a", "b"]
            .map(|path| SyncEntry {
                path: path.to_owned(),
                remote_path: format!("/remote/{path}"),
                local: Some(FileStamp { size: 10, mtime: Some(2) }),
                remote,
                status,
            })
            .to_vec()
    }

    fn with_rows(store: &mut DeploymentStore, entries: Vec<SyncEntry>, choices: SyncChoices) {
        let sync = store.sync.as_mut().expect("the dialog is open");
        sync.entries = Some(entries);
        sync.choices = choices;
    }

    fn sync_state(store: &DeploymentStore) -> &SyncDialogState {
        store.sync.as_ref().expect("the dialog is open")
    }

    fn report(files: u32, errors: &[(&str, &str)]) -> TransferReport {
        let errors =
            errors.iter().map(|(path, message)| TransferError { path: (*path).into(), message: (*message).into() });
        TransferReport { files, skipped: 0, errors: errors.collect() }
    }

    fn compare(path: &str) -> SyncEffect {
        SyncEffect::Compare { path: path.to_owned(), server: "one".to_owned(), request: R }
    }

    // deployment eligibility › saves edits made in the local diff before synchronization and stops
    // if saving fails
    #[test]
    fn saves_edits_made_in_the_local_diff_before_synchronization_and_stops_if_saving_fails() {
        let mut i18n = I18n::default();
        let mut store = opened(&mut i18n);
        let before = store.sync.clone();
        let open = ["/project/a".to_owned()];
        let steps = vec![step("a", SyncAction::Upload), step("b", SyncAction::Download)];
        let effects = store.run_sync(steps.clone(), SyncRunMode::All, &settings(), Some("/project"), &open, R);
        assert_eq!(effects, [SyncEffect::SaveDocuments { paths: open.to_vec(), request: R }]);
        let effects = store.documents_saved(R, Ok(()), &i18n);
        assert_eq!(effects, [SyncEffect::Sync { steps, server: "one".to_owned(), request: R }]);

        store.sync = before;
        let steps = vec![step("a", SyncAction::Upload)];
        store.run_sync(steps, SyncRunMode::All, &settings(), Some("/project"), &open, R);
        assert!(store.documents_saved(R, Err("Cannot save local file".to_owned()), &i18n).is_empty());
        assert_eq!(sync_state(&store).error.as_deref(), Some("Cannot save local file"));
        assert!(!sync_state(&store).running);
    }

    // deployment eligibility › compares every covered selection and retains skipped paths
    #[test]
    fn compares_every_covered_selection_and_retains_skipped_paths() {
        let mut i18n = I18n::default();
        let mut store = DeploymentStore::default();
        let paths = vec!["/project/a".to_owned(), "/project/b".to_owned()];
        let covered = coverage(&["/project/a", "/project/c"]);
        assert_eq!(store.open_sync(&server("one"), paths, &covered, R, &mut i18n), [compare("/project/a")]);
        assert_eq!(store.compared(R, Ok(Vec::new()), &mut i18n), [compare("/project/c")]);
        assert!(store.compared(R, Ok(Vec::new()), &mut i18n).is_empty());
        assert_eq!(sync_state(&store).skipped[0].path, "/project/b");
    }

    // deployment eligibility › keeps the comparison and other choices after synchronizing a single
    // file, then supports the next file
    #[test]
    fn keeps_the_comparison_and_other_choices_after_synchronizing_a_single_file_then_supports_the_next_file() {
        let mut i18n = I18n::default();
        let mut store = opened(&mut i18n);
        let entries = rows(SyncStatus::LocalNewer, Some(FileStamp { size: 5, mtime: Some(1) }));
        let choices = SyncChoices::from_pairs(&[("a", SyncChoice::Upload), ("b", SyncChoice::DeleteRemote)]);
        with_rows(&mut store, entries.clone(), choices);
        let refreshed = SyncEntry { remote: entries[0].local, status: SyncStatus::Same, ..entries[0].clone() };

        let steps = vec![step("a", SyncAction::Upload)];
        let effects = store.run_sync(steps.clone(), SyncRunMode::Selected, &settings(), Some("/project"), &[], R);
        assert_eq!(effects, [SyncEffect::Sync { steps, server: "one".to_owned(), request: R }]);
        assert_eq!(store.synced(R, Ok(report(1, &[])), &mut i18n), [compare("/project/a")]);
        let effects = store.compared(R, Ok(vec![refreshed.clone()]), &mut i18n);
        assert!(matches!(effects.last(), Some(SyncEffect::Finished(_))));
        let sync = sync_state(&store);
        assert!(!sync.running);
        assert_eq!(sync.request, None);
        assert_eq!(sync.choices, SyncChoices::from_pairs(&[("a", SyncChoice::Skip), ("b", SyncChoice::DeleteRemote)]));
        assert_eq!(sync.entries, Some(vec![refreshed, entries[1].clone()]));

        let steps = vec![step("b", SyncAction::DeleteRemote)];
        store.run_sync(steps, SyncRunMode::Selected, &settings(), Some("/project"), &[], R);
        store.synced(R, Ok(report(1, &[])), &mut i18n);
        let local_only = SyncEntry { remote: None, status: SyncStatus::LocalOnly, ..entries[1].clone() };
        store.compared(R, Ok(vec![local_only]), &mut i18n);
        let sync = sync_state(&store);
        assert_eq!(sync.choices, SyncChoices::from_pairs(&[("a", SyncChoice::Skip), ("b", SyncChoice::Skip)]));
        assert_eq!(sync.entries.as_ref().map(Vec::len), Some(2));
    }

    // deployment eligibility › retains failed selected operations for retry and does not change
    // pending files when refresh fails
    #[test]
    fn retains_failed_selected_operations_for_retry_and_does_not_change_pending_files_when_refresh_fails() {
        let mut i18n = I18n::default();
        let mut store = opened(&mut i18n);
        let entries = rows(SyncStatus::LocalOnly, None);
        let both = SyncChoices::from_pairs(&[("a", SyncChoice::Upload), ("b", SyncChoice::Upload)]);
        with_rows(&mut store, entries.clone(), both.clone());

        let steps = vec![step("a", SyncAction::Upload)];
        store.run_sync(steps.clone(), SyncRunMode::Selected, &settings(), Some("/project"), &[], R);
        store.synced(R, Ok(report(0, &[("a", "Permission denied")])), &mut i18n);
        store.compared(R, Ok(vec![entries[0].clone()]), &mut i18n);
        let sync = sync_state(&store);
        assert!(!sync.running);
        assert_eq!(sync.choices, both);
        assert_eq!(sync.error.as_deref(), Some("a: Permission denied"));

        store.run_sync(steps, SyncRunMode::Selected, &settings(), Some("/project"), &[], R);
        store.synced(R, Ok(report(1, &[])), &mut i18n);
        store.compared(R, Err(IdeError::new(ErrorKind::Io, "Connection lost")), &mut i18n);
        let sync = sync_state(&store);
        assert!(!sync.running);
        assert_eq!(sync.request, None);
        assert_eq!(sync.choices, SyncChoices::from_pairs(&[("a", SyncChoice::Skip), ("b", SyncChoice::Upload)]));
        assert_eq!(sync.entries.as_ref(), Some(&entries));
        let error = sync.error.as_deref().unwrap_or_default();
        assert!(error.contains("Could not refresh comparison: a: Connection lost"), "{error}");
    }

    // Not from a spec: Synchronize All closes the dialog and counts the uncovered paths as skipped;
    // closing cancels the comparison in flight and drops its late answer; a new SSH prompt
    // replaces the open one.
    #[test]
    fn closes_after_synchronizing_all_and_drops_late_answers() {
        let mut i18n = I18n::default();
        let mut store = opened(&mut i18n);
        store.run_sync(vec![step("a", SyncAction::Upload)], SyncRunMode::All, &settings(), None, &[], R);
        let effects = store.synced(R, Ok(report(1, &[])), &mut i18n);
        assert!(store.sync.is_none());
        assert!(matches!(effects.last(), Some(SyncEffect::Finished(done)) if done.skipped == 1));

        let paths = vec!["/project/a".to_owned()];
        store.open_sync(&server("one"), paths, &coverage(&["/project/a"]), RequestId(2), &mut i18n);
        assert_eq!(store.close_sync(), [SyncEffect::Cancel(RequestId(2))]);
        assert!(store.compared(RequestId(2), Ok(Vec::new()), &mut i18n).is_empty());

        assert_eq!(store.prompt_secret("ssh", "SSH password required"), None);
        let replaced = store.prompt_secret("ssh", "again").map(|prompt| prompt.message);
        assert_eq!(replaced.as_deref(), Some("SSH password required"));
        let mut form = SecretForm { secret: "pw".to_owned(), save: true };
        assert_eq!(form.submit(), Some(SecretAnswer { secret: "pw".to_owned(), save: true }));
        assert_eq!(form.submit(), None);
    }
}
