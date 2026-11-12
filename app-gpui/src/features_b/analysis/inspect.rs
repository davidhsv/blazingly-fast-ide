//! Inspect Code: batch language-server diagnostics over a scope, grouped like IDEA's Inspection Results
//! (severity → inspection → file → problem), and the results of the run in progress or done.
//!
//! The backend streams the results; [`InspectionStore`] takes its events for the run in flight only and
//! keeps the servers' own diagnostics, so the results follow edits to the inspection profile without
//! running again. The caller passes the overrides of the run's profile as they are now, request ids and
//! times.

use super::scope::ScopeChoice;
use crate::platform::collate;
use crate::platform_more::inspections::{SeenCodes, apply_profile_mixed, producer_of};
use crate::settings_model::inspections_model::ProfileOverrides;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::ipc_path::to_ipc;
use ide_core::{Diagnostic, RequestId, Severity};
use ide_host::ops::analysis::{InspectEvent, InspectSummary, InspectedFile};
use ide_search::SearchScope;
use std::collections::BTreeSet;

/// Severity groups of the results; hints count as weak warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SeverityKey {
    Error,
    Warning,
    Info,
}

impl SeverityKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

pub fn severity_key(d: &Diagnostic) -> SeverityKey {
    match d.severity {
        Severity::Error => SeverityKey::Error,
        Severity::Warning => SeverityKey::Warning,
        Severity::Info | Severity::Hint => SeverityKey::Info,
    }
}

/// The inspection a problem is listed under: `source` or `source(code)`, e.g. `eslint(no-unused-vars)`.
pub fn kind_of(d: &Diagnostic) -> String {
    let source = d.source.as_deref().filter(|s| !s.is_empty()).unwrap_or("Language server");
    match d.code.as_deref().filter(|c| !c.is_empty()) {
        Some(code) => format!("{source}({code})"),
        None => source.to_owned(),
    }
}

/// The problems a server reported for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedFileResult {
    pub path: String,
    pub rel_path: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl From<InspectedFile> for InspectedFileResult {
    fn from(file: InspectedFile) -> Self {
        Self { path: to_ipc(&file.path), rel_path: file.rel_path, diagnostics: file.diagnostics }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionProblem {
    pub path: String,
    pub rel_path: String,
    pub d: Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionFileGroup {
    pub path: String,
    pub rel_path: String,
    pub problems: Vec<InspectionProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionKindGroup {
    /// See [`kind_of`].
    pub kind: String,
    pub count: usize,
    pub files: Vec<InspectionFileGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionSeverityGroup {
    pub severity: SeverityKey,
    pub count: usize,
    pub kinds: Vec<InspectionKindGroup>,
}

/// Problems of a file in document order.
pub(super) fn sort_problems(problems: &mut [InspectionProblem]) {
    problems.sort_by_key(|p| (p.d.range.start.line, p.d.range.start.col));
}

/// Groups results by severity (errors first), then inspection (most problems first, then by name), then
/// file (by relative path), problems in document order.
pub fn group_inspection(files: &[InspectedFileResult]) -> Vec<InspectionSeverityGroup> {
    type Kinds = Vec<(String, Vec<InspectionFileGroup>)>;
    let mut tree: Vec<(SeverityKey, Kinds)> = Vec::new();
    for file in files {
        for d in &file.diagnostics {
            let severity = severity_key(d);
            let si = tree.iter().position(|(s, _)| *s == severity).unwrap_or_else(|| {
                tree.push((severity, Vec::new()));
                tree.len() - 1
            });
            let kinds = &mut tree[si].1;
            let kind = kind_of(d);
            let ki = kinds.iter().position(|(k, _)| *k == kind).unwrap_or_else(|| {
                kinds.push((kind, Vec::new()));
                kinds.len() - 1
            });
            let by_file = &mut kinds[ki].1;
            let problem = InspectionProblem { path: file.path.clone(), rel_path: file.rel_path.clone(), d: d.clone() };
            match by_file.iter_mut().find(|g| g.path == file.path) {
                Some(group) => group.problems.push(problem),
                None => by_file.push(InspectionFileGroup {
                    path: file.path.clone(),
                    rel_path: file.rel_path.clone(),
                    problems: vec![problem],
                }),
            }
        }
    }
    tree.sort_by_key(|(severity, _)| *severity);
    tree.into_iter()
        .map(|(severity, kinds)| {
            let mut kinds: Vec<InspectionKindGroup> = kinds
                .into_iter()
                .map(|(kind, mut files)| {
                    files.sort_by(|a, b| collate::compare(&a.rel_path, &b.rel_path));
                    for group in &mut files {
                        sort_problems(&mut group.problems);
                    }
                    let count = files.iter().map(|g| g.problems.len()).sum();
                    InspectionKindGroup { kind, count, files }
                })
                .collect();
            kinds.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| collate::compare(&a.kind, &b.kind)));
            let count = kinds.iter().map(|k| k.count).sum();
            InspectionSeverityGroup { severity, count, kinds }
        })
        .collect()
}

/// Stable identity of a problem across regroupings.
pub fn problem_id(p: &InspectionProblem) -> String {
    let start = p.d.range.start;
    format!("{}\u{0}{}:{}\u{0}{}\u{0}{}", p.path, start.line, start.col, kind_of(&p.d), p.d.message)
}

/// The summary of a finished run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionSummary {
    /// Files inspected by a language server.
    pub files: u32,
    /// Problems the profile kept, not all the servers reported.
    pub problems: u32,
    /// Files in scope no language server serves.
    pub skipped: u32,
    /// Files whose server failed or never became ready.
    pub failed: u32,
    pub cancelled: bool,
    pub elapsed_ms: u32,
}

impl From<InspectSummary> for InspectionSummary {
    fn from(s: InspectSummary) -> Self {
        Self {
            files: s.files,
            problems: s.problems,
            skipped: s.skipped,
            failed: s.failed,
            cancelled: s.cancelled,
            elapsed_ms: s.elapsed_ms,
        }
    }
}

/// The files with problems under `profile`, and how many problems they have; rebuilt from the servers'
/// diagnostics, never from severities overridden earlier.
pub fn project_results(raw: &[InspectedFileResult], profile: &ProfileOverrides) -> (Vec<InspectedFileResult>, u32) {
    let files: Vec<InspectedFileResult> = raw
        .iter()
        .filter_map(|file| {
            let diagnostics: Vec<Diagnostic> =
                apply_profile_mixed(profile, &file.diagnostics).into_iter().map(|styled| styled.diagnostic).collect();
            (!diagnostics.is_empty()).then(|| InspectedFileResult { diagnostics, ..file.clone() })
        })
        .collect();
    let problems = files.iter().map(|f| f.diagnostics.len() as u32).sum();
    (files, problems)
}

/// Results stream into view at most this often.
pub const PUBLISH_DELAY_MS: u64 = 150;

/// What starting a run asks for.
#[derive(Debug, Clone)]
pub enum RunStart {
    /// The scope could not be resolved: tell the user.
    Rejected(NotificationRequest),
    /// Cancel `cancel` (the previous run), show the Inspection Results tool window and send Inspect Code
    /// over `scope` as request `id`.
    Started { id: RequestId, scope: SearchScope, cancel: Option<RequestId> },
}

/// Inspect Code's results in a project window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionStore {
    pub running: bool,
    /// Files about to be inspected, and those done.
    pub total: u32,
    pub done: u32,
    /// The servers' results as last published, kept so disabled inspections can be restored.
    pub raw_files: Vec<InspectedFileResult>,
    /// Files with problems under the run's profile, in the order the servers reported them.
    pub files: Vec<InspectedFileResult>,
    pub summary: Option<InspectionSummary>,
    pub scope_label: Option<String>,
    pub error: Option<String>,
    /// Scope of the last run (Rerun).
    pub last: Option<ScopeChoice>,
    /// Inspection profile of the last run.
    pub profile: Option<String>,
    /// Problems excluded from the results (⌫), by [`problem_id`].
    pub excluded: BTreeSet<String>,
    inflight: Option<RequestId>,
    /// Results received, not yet published.
    found: Vec<InspectedFileResult>,
    publish_at: Option<u64>,
}

fn upsert(files: &mut Vec<InspectedFileResult>, file: InspectedFileResult) {
    match files.iter_mut().find(|known| known.path == file.path) {
        Some(known) => *known = file,
        None => files.push(file),
    }
}

impl InspectionStore {
    /// Inspect Code over `choice` with `profile_id`: `resolved` is the scope with its label (see
    /// [`super::scope::describe_scope`]), or why there is none.
    pub fn run(
        &mut self,
        choice: ScopeChoice,
        profile_id: &str,
        resolved: Result<(SearchScope, String), String>,
        id: RequestId,
    ) -> RunStart {
        let (scope, label) = match resolved {
            Ok(resolved) => resolved,
            Err(message) => {
                let mut request = NotificationRequest::new(NotificationLevel::Warning, "Inspect Code");
                request.group = Some("analysis".to_owned());
                request.body = Some(message);
                return RunStart::Rejected(request);
            }
        };
        let cancel = self.inflight;
        *self = Self {
            running: true,
            scope_label: Some(label),
            last: Some(choice),
            profile: Some(profile_id.to_owned()),
            inflight: Some(id),
            ..Self::default()
        };
        RunStart::Started { id, scope, cancel }
    }

    /// An event of request `id`; `profile` holds the run's profile overrides now. Returns whether `seen`
    /// learned diagnostic codes (store them).
    pub fn event(
        &mut self,
        id: RequestId,
        event: InspectEvent,
        profile: &ProfileOverrides,
        seen: &mut SeenCodes,
        now: u64,
    ) -> bool {
        if self.inflight != Some(id) {
            return false;
        }
        let mut learned = false;
        match event {
            InspectEvent::Started { total } => self.total = total,
            InspectEvent::File { file, done } => {
                for d in file.diagnostics.iter().filter(|d| d.code.is_some()) {
                    learned |= seen.note(producer_of(d, None), std::slice::from_ref(d));
                }
                upsert(&mut self.found, file.into());
                self.done = done;
                self.publish_at.get_or_insert(now + PUBLISH_DELAY_MS);
            }
            InspectEvent::Done { summary } => {
                let (files, problems) = project_results(&self.found, profile);
                self.publish_at = None;
                self.inflight = None;
                self.running = false;
                self.raw_files = self.found.clone();
                self.files = files;
                self.summary = Some(InspectionSummary { problems, ..InspectionSummary::from(summary) });
            }
        }
        learned
    }

    /// When streamed results are due to be published (for the caller's timer).
    pub fn next_publish(&self) -> Option<u64> {
        self.publish_at
    }

    /// Publishes the streamed results once due.
    pub fn tick(&mut self, now: u64, profile: &ProfileOverrides) {
        if self.publish_at.is_none_or(|at| at > now) {
            return;
        }
        self.publish_at = None;
        if self.inflight.is_some() {
            self.raw_files = self.found.clone();
            self.files = project_results(&self.found, profile).0;
        }
    }

    /// Request `id` failed.
    pub fn failed(&mut self, id: RequestId, message: String) {
        if self.inflight == Some(id) {
            self.inflight = None;
            self.running = false;
            self.error = Some(message);
        }
    }

    /// The run's profile changed in content: completed and streaming results follow it.
    pub fn profile_changed(&mut self, profile: &ProfileOverrides) {
        if self.profile.is_none() {
            return;
        }
        let (files, problems) = project_results(&self.raw_files, profile);
        self.files = files;
        if let Some(summary) = &mut self.summary {
            summary.problems = problems;
        }
    }

    /// The request to cancel to stop the run (its results keep coming until it stops).
    pub fn cancel(&self) -> Option<RequestId> {
        self.inflight
    }

    /// The window switched projects: forgets the results; returns the run to cancel.
    pub fn reset(&mut self) -> Option<RequestId> {
        let cancel = self.inflight;
        *self = Self::default();
        cancel
    }

    /// The scope and profile to run again with (Rerun).
    pub fn rerun(&self) -> Option<(ScopeChoice, Option<String>)> {
        self.last.clone().map(|last| (last, self.profile.clone()))
    }

    /// Excludes `problems`, or includes them again when all are excluded already.
    pub fn toggle_excluded(&mut self, problems: &[InspectionProblem]) {
        let ids: Vec<String> = problems.iter().map(problem_id).collect();
        if ids.iter().all(|id| self.excluded.contains(id)) {
            for id in &ids {
                self.excluded.remove(id);
            }
        } else {
            self.excluded.extend(ids);
        }
    }

    /// The results grouped for the tool window.
    pub fn groups(&self) -> Vec<InspectionSeverityGroup> {
        group_inspection(&self.files)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::scope::ScopeKind;
    use super::*;
    use ide_core::{IpcPath, Pos, Range};

    pub fn diag(line: u32, severity: Severity, source: Option<&str>, code: Option<&str>) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(line, 2), Pos::new(line, 5)),
            severity,
            message: format!("problem at {line}"),
            source: source.map(str::to_owned),
            code: code.map(str::to_owned),
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn result(path: &str, rel_path: &str, diagnostics: Vec<Diagnostic>) -> InspectedFileResult {
        InspectedFileResult { path: path.to_owned(), rel_path: rel_path.to_owned(), diagnostics }
    }

    pub fn files() -> Vec<InspectedFileResult> {
        vec![
            result(
                "/p/b.ts",
                "b.ts",
                vec![
                    diag(9, Severity::Error, Some("ts"), Some("2304")),
                    diag(1, Severity::Error, Some("ts"), Some("2304")),
                ],
            ),
            result(
                "/p/a.ts",
                "a.ts",
                vec![
                    diag(3, Severity::Warning, Some("eslint"), Some("no-unused-vars")),
                    diag(4, Severity::Hint, None, None),
                ],
            ),
            result(
                "/p/c.ts",
                "c.ts",
                vec![
                    diag(0, Severity::Error, Some("ts"), Some("2304")),
                    diag(2, Severity::Error, Some("ts"), Some("7006")),
                ],
            ),
        ]
    }

    #[test]
    fn groups_by_severity_then_inspection_then_file_sorted() {
        let groups = group_inspection(&files());
        let shape: Vec<(SeverityKey, usize)> = groups.iter().map(|g| (g.severity, g.count)).collect();
        assert_eq!(shape, [(SeverityKey::Error, 4), (SeverityKey::Warning, 1), (SeverityKey::Info, 1)]);
        let kinds: Vec<(&str, usize)> = groups[0].kinds.iter().map(|k| (k.kind.as_str(), k.count)).collect();
        assert_eq!(kinds, [("ts(2304)", 3), ("ts(7006)", 1)]);
        let ts2304 = &groups[0].kinds[0];
        let paths: Vec<&str> = ts2304.files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(paths, ["b.ts", "c.ts"]);
        let lines: Vec<u32> = ts2304.files[0].problems.iter().map(|p| p.d.range.start.line).collect();
        assert_eq!(lines, [1, 9]);
        assert_eq!(groups[2].kinds[0].kind, "Language server");
    }

    #[test]
    fn names_inspections_by_source_and_code() {
        assert_eq!(kind_of(&diag(0, Severity::Error, Some("eslint"), Some("eqeqeq"))), "eslint(eqeqeq)");
        assert_eq!(kind_of(&diag(0, Severity::Error, Some("rustc"), None)), "rustc");
    }

    const PROFILE_KEY: &str = "lsp:typescript-language-server";

    fn profile(entry: serde_json::Value) -> ProfileOverrides {
        let mut overrides = serde_json::Map::new();
        overrides.insert(PROFILE_KEY.to_owned(), entry);
        serde_json::from_value(serde_json::Value::Object(overrides)).unwrap_or_default()
    }

    fn typed_error() -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(0, 0), Pos::new(0, 1)),
            severity: Severity::Error,
            message: "Wrong type".to_owned(),
            source: Some("ts".to_owned()),
            code: Some("2322".to_owned()),
            unnecessary: false,
            deprecated: false,
            server: Some("typescript-language-server".to_owned()),
        }
    }

    fn file_event(path: &str) -> InspectEvent {
        InspectEvent::File {
            file: InspectedFile {
                path: IpcPath::from(path),
                rel_path: path[3..].to_owned(),
                diagnostics: vec![typed_error()],
            },
            done: 1,
        }
    }

    fn done_event() -> InspectEvent {
        InspectEvent::Done { summary: InspectSummary { problems: 1, ..InspectSummary::default() } }
    }

    fn started(store: &mut InspectionStore) -> RequestId {
        let choice = ScopeChoice { kind: ScopeKind::Project, directory: String::new(), custom: None };
        let id = RequestId(7);
        let start = store.run(choice, "default", Ok((SearchScope::Project, "Project".to_owned())), id);
        assert!(matches!(start, RunStart::Started { id: RequestId(7), scope: SearchScope::Project, cancel: None }));
        id
    }

    fn severities(store: &InspectionStore) -> Vec<Severity> {
        store.files.iter().map(|f| f.diagnostics[0].severity).collect()
    }

    #[test]
    fn reprojects_completed_results_counts_and_original_severities_without_rerunning() {
        let mut store = InspectionStore::default();
        let mut seen = SeenCodes::default();
        let plain = profile(serde_json::json!({}));
        let id = started(&mut store);
        store.event(id, file_event("/p/a.ts"), &plain, &mut seen, 0);
        store.event(id, done_event(), &plain, &mut seen, 10);
        let excluded = BTreeSet::from(["excluded".to_owned()]);
        store.excluded = excluded.clone();
        store.profile_changed(&profile(serde_json::json!({ "severity": "warning" })));
        assert_eq!(severities(&store), [Severity::Warning]);
        store.profile_changed(&profile(serde_json::json!({ "enabled": false })));
        assert!(store.files.is_empty());
        assert_eq!(store.summary.as_ref().map(|s| s.problems), Some(0));
        store.profile_changed(&plain);
        assert_eq!(severities(&store), [Severity::Error]);
        assert_eq!(store.summary.as_ref().map(|s| s.problems), Some(1));
        assert_eq!(store.excluded, excluded);
        assert_eq!(store.reset(), None);
        assert!(store.raw_files.is_empty());
    }

    #[test]
    fn retains_initially_disabled_diagnostics_and_uses_edits_made_during_streaming() {
        let mut store = InspectionStore::default();
        let mut seen = SeenCodes::default();
        let disabled = profile(serde_json::json!({ "enabled": false }));
        let id = started(&mut store);
        store.event(id, file_event("/p/a.ts"), &disabled, &mut seen, 0);
        assert_eq!(store.next_publish(), Some(PUBLISH_DELAY_MS));
        store.tick(PUBLISH_DELAY_MS, &disabled);
        assert!(store.files.is_empty());
        let warning = profile(serde_json::json!({ "severity": "warning" }));
        store.profile_changed(&warning);
        assert_eq!(store.files.len(), 1);
        store.event(id, file_event("/p/b.ts"), &warning, &mut seen, PUBLISH_DELAY_MS);
        store.event(id, done_event(), &warning, &mut seen, PUBLISH_DELAY_MS);
        assert_eq!(severities(&store), [Severity::Warning, Severity::Warning]);
        assert_eq!(store.summary.as_ref().map(|s| s.problems), Some(2));
    }
}
