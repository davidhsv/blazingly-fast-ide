//! Dependency versions in manifests: in `Cargo.toml` a weak warning on each requirement a newer version
//! is not allowed by, with a quick fix that rewrites only the version; in `Cargo.toml` and `go.mod`
//! version completion at a parsed dependency's version.
//!
//! Nothing here calls the backend. A [`DependencyCheck`] per editor says when to ask for the versions
//! (500 ms after the last edit) and takes the answer; an answer that an edit, a project switch or the
//! editor's close made stale is dropped, and the request is handed back to be cancelled. Request ids,
//! times and the document's revision are passed in.

use crate::editor::buffer;
use crate::editor_lsp::intentions::{IntentionItem, IntentionRun};
use crate::editor_lsp::positions::offsets_to_range;
use crate::platform::i18n::{I18n, Param};
use crate::settings_model::inspections_model::{ProfileOverrides, is_enabled};
use ide_core::{Diagnostic, RequestId, Severity, TextChange};
use ide_workspace::dependencies::DependencyVersions;
use ropey::Rope;
use std::collections::HashMap;

/// The inspection that reports newer versions.
pub const DEPENDENCY_INSPECTION: &str = "builtin:dependency-versions";
/// The diagnostics producer of that inspection.
pub const DEPENDENCY_PRODUCER: &str = "dependency-versions";
/// Quiet time after an edit before the versions are checked again.
pub const CHECK_DELAY_MS: u64 = 500;

/// A manifest the feature serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manifest {
    Cargo,
    GoMod,
}

impl Manifest {
    /// Whether the editor checks for newer versions (and offers fixes), besides completing versions.
    pub fn checks_versions(self) -> bool {
        self == Self::Cargo
    }
}

/// The manifest an editor of `path` shows; `None` for other files and for virtual or read-only editors.
pub fn manifest_of(path: &str, virtual_doc: bool, read_only: bool) -> Option<Manifest> {
    if virtual_doc || read_only {
        return None;
    }
    let named = |name: &str| path == name || path.strip_suffix(name).is_some_and(|dir| dir.ends_with('/'));
    if named("Cargo.toml") {
        Some(Manifest::Cargo)
    } else if named("go.mod") {
        Some(Manifest::GoMod)
    } else {
        None
    }
}

/// Whether the inspection is on in `profile`.
pub fn inspection_enabled(profile: &ProfileOverrides) -> bool {
    is_enabled(profile, DEPENDENCY_INSPECTION)
}

/// What the editor does besides asking for versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckEffect {
    /// Cancel a request whose answer is no longer wanted.
    Cancel(RequestId),
    /// Publish no diagnostics of [`DEPENDENCY_PRODUCER`] for the path.
    ClearDiagnostics,
    /// The editor's intentions changed (recompute the bulb).
    IntentionsChanged,
}

/// Ask the backend for the dependency versions of the editor's document; `pos` is the caret for a
/// completion, `None` for the whole manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionsRequest {
    pub id: RequestId,
    pub pos: Option<u32>,
}

/// A request in flight and what it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    id: RequestId,
    sequence: u64,
    revision: u64,
    root: Option<String>,
}

/// "Update to <version>" on a dependency: replaces its version, unless the document changed since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyFix {
    pub id: String,
    /// Translated label.
    pub label: String,
    /// The document revision the fix was offered for.
    pub revision: u64,
    pub change: TextChange,
}

/// The change of `fix`, when the document is still at the revision it was offered for.
pub fn apply_fix(fix: &DependencyFix, revision: u64) -> Option<TextChange> {
    (fix.revision == revision).then(|| fix.change.clone())
}

/// The rows of the intentions list (and quick fixes) for `fixes`; each runs [`IntentionRun::Local`]
/// with the fix's id.
pub fn intention_items(fixes: &[DependencyFix]) -> Vec<IntentionItem> {
    fixes
        .iter()
        .map(|fix| IntentionItem {
            icon: Some("bulb"),
            quick_fix: Some(true),
            ..IntentionItem::new(&fix.id, &fix.label, IntentionRun::Local(fix.id.clone()))
        })
        .collect()
}

/// The version check of one `Cargo.toml` editor.
#[derive(Debug, Clone, Default)]
pub struct DependencyCheck {
    /// The last answer, for the document revision it was computed on; empty after an edit.
    versions: Vec<DependencyVersions>,
    versions_revision: u64,
    /// Bumped by every reschedule: an older request's answer is void.
    sequence: u64,
    pending: Option<Pending>,
    run_at: Option<u64>,
    disposed: bool,
}

impl DependencyCheck {
    /// An editor opened: the first check is scheduled.
    pub fn open(now: u64) -> (Self, Vec<CheckEffect>) {
        let mut check = Self::default();
        let effects = check.schedule(now);
        (check, effects)
    }

    fn cancel(&mut self) -> Vec<CheckEffect> {
        self.sequence += 1;
        self.pending.take().map(|pending| CheckEffect::Cancel(pending.id)).into_iter().collect()
    }

    /// Drops what is in flight and the published diagnostics, and checks again after [`CHECK_DELAY_MS`].
    pub fn schedule(&mut self, now: u64) -> Vec<CheckEffect> {
        let mut effects = self.cancel();
        effects.push(CheckEffect::ClearDiagnostics);
        self.run_at = (!self.disposed).then_some(now + CHECK_DELAY_MS);
        effects
    }

    /// The document changed: the fixes are gone at once, and the check runs again.
    pub fn doc_changed(&mut self, now: u64) -> Vec<CheckEffect> {
        self.versions.clear();
        self.schedule(now)
    }

    /// The inspection profile changed.
    pub fn profile_changed(&mut self, now: u64) -> Vec<CheckEffect> {
        let mut effects = self.schedule(now);
        effects.push(CheckEffect::IntentionsChanged);
        effects
    }

    /// The window switched projects: the editor checks no more.
    pub fn project_changed(&mut self) -> Vec<CheckEffect> {
        self.disposed = true;
        self.run_at = None;
        let mut effects = self.cancel();
        effects.push(CheckEffect::ClearDiagnostics);
        effects
    }

    /// The editor closed: what is in flight is cancelled. Whether to clear the diagnostics is
    /// [`DependencyViewers::close`]'s answer.
    pub fn close(&mut self) -> Vec<CheckEffect> {
        self.disposed = true;
        self.run_at = None;
        self.cancel()
    }

    /// When the next check is due (for the caller's timer).
    pub fn next_run(&self) -> Option<u64> {
        self.run_at
    }

    /// The check due by `now`, if any: the request to send once the document is synced with the backend.
    /// Nothing is asked while the inspection is off.
    pub fn due(
        &mut self,
        now: u64,
        enabled: bool,
        revision: u64,
        root: Option<&str>,
        id: RequestId,
    ) -> Option<VersionsRequest> {
        if self.run_at.is_none_or(|at| at > now) {
            return None;
        }
        self.run_at = None;
        if self.disposed || !enabled {
            return None;
        }
        self.pending = Some(Pending { id, sequence: self.sequence, revision, root: root.map(str::to_owned) });
        Some(VersionsRequest { id, pos: None })
    }

    /// Whether request `id` is still wanted (checked again after the document synced).
    pub fn is_current(&self, id: RequestId) -> bool {
        !self.disposed && self.pending.as_ref().is_some_and(|p| p.id == id && p.sequence == self.sequence)
    }

    /// The backend's answer to request `id`, with the document's revision and the project root now. Returns
    /// the diagnostics to publish under [`DEPENDENCY_PRODUCER`] (the intentions changed too), or `None`
    /// when the answer is stale or failed: offline registries and cancelled checks never interrupt editing.
    pub fn answered(
        &mut self,
        id: RequestId,
        answer: Result<Vec<DependencyVersions>, String>,
        revision: u64,
        root: Option<&str>,
        doc: &Rope,
        i18n: &mut I18n,
    ) -> Option<Vec<Diagnostic>> {
        if self.pending.as_ref().is_none_or(|p| p.id != id) {
            return None;
        }
        let pending = self.pending.take()?;
        let current = !self.disposed
            && pending.sequence == self.sequence
            && pending.revision == revision
            && pending.root.as_deref() == root;
        let items = answer.ok().filter(|_| current)?;
        let diagnostics = items
            .iter()
            .filter_map(|item| {
                let update = item.update.as_ref()?;
                let latest = Param::from(update.latest.as_str());
                let message = i18n.tx_built(&["A newer version is available: ", ""], &[latest]);
                Some(Diagnostic {
                    range: offsets_to_range(doc, item.dependency.from, item.dependency.to),
                    severity: Severity::Info,
                    message,
                    source: Some("Cargo".to_owned()),
                    code: None,
                    unnecessary: false,
                    deprecated: false,
                    server: None,
                })
            })
            .collect();
        self.versions = items;
        self.versions_revision = revision;
        Some(diagnostics)
    }

    /// The fixes for the selection `from..to` (or a problem's range) at document `revision`.
    pub fn fixes(
        &self,
        from: u32,
        to: u32,
        revision: u64,
        enabled: bool,
        read_only: bool,
        i18n: &mut I18n,
    ) -> Vec<DependencyFix> {
        if !enabled || read_only || revision != self.versions_revision {
            return Vec::new();
        }
        self.versions
            .iter()
            .filter_map(|item| {
                let update = item.update.as_ref()?;
                let dependency = &item.dependency;
                if from > dependency.to || to < dependency.from {
                    return None;
                }
                Some(DependencyFix {
                    id: format!("dependency.update:{}", dependency.from),
                    label: i18n.tx_built(&["Update to ", ""], &[Param::from(update.replacement.as_str())]),
                    revision,
                    change: TextChange { from: dependency.from, to: dependency.to, insert: update.replacement.clone() },
                })
            })
            .collect()
    }
}

/// Editors open per manifest path: split editors share the path's diagnostics, which go when the last
/// of them closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyViewers {
    counts: HashMap<String, usize>,
}

impl DependencyViewers {
    pub fn open(&mut self, path: &str) {
        *self.counts.entry(path.to_owned()).or_default() += 1;
    }

    /// An editor of `path` closed; returns whether it was the last one (clear the path's diagnostics).
    pub fn close(&mut self, path: &str) -> bool {
        match self.counts.get_mut(path) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            _ => {
                self.counts.remove(path);
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Completion.

/// A version offered at a dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOption {
    pub label: String,
    /// The package.
    pub detail: String,
    /// The text that replaces the whole version: the typed requirement operator, then the version.
    pub apply: String,
}

/// Versions to complete, replacing `from..to` (the editor does not filter them again).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCompletion {
    pub from: u32,
    pub to: u32,
    pub options: Vec<VersionOption>,
}

/// The dependency whose version holds `pos`.
pub fn completion_item(items: &[DependencyVersions], pos: u32) -> Option<&DependencyVersions> {
    items.iter().find(|item| item.dependency.from <= pos && pos <= item.dependency.to)
}

/// The versions of `item` in the backend's order (newest first). `typed` is the text from the version's
/// start to the caret: a leading requirement operator (`^`, `~`, `>=`…) is kept, and unless completion
/// was invoked explicitly only versions starting with the rest are offered.
pub fn version_completions(item: &DependencyVersions, typed: &str, explicit: bool) -> VersionCompletion {
    let filter = typed.trim_start_matches(|c: char| c.is_whitespace() || "^~<>=".contains(c));
    let prefix = &typed[..typed.len() - filter.len()];
    VersionCompletion {
        from: item.dependency.from,
        to: item.dependency.to,
        options: item
            .versions
            .iter()
            .filter(|version| explicit || version.starts_with(filter))
            .map(|version| VersionOption {
                label: version.clone(),
                detail: item.dependency.package.clone(),
                apply: format!("{prefix}{version}"),
            })
            .collect(),
    }
}

/// A completion request in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    id: RequestId,
    revision: u64,
    root: Option<String>,
    pos: u32,
    explicit: bool,
}

/// Version completion of one editor (`Cargo.toml` or `go.mod`): one request at a time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionCompletionSource {
    asked: Option<Asked>,
}

impl VersionCompletionSource {
    /// Completion at `pos`: returns the request to send once the document is synced, and the earlier
    /// request to cancel.
    pub fn ask(
        &mut self,
        id: RequestId,
        pos: u32,
        explicit: bool,
        revision: u64,
        root: Option<&str>,
    ) -> (VersionsRequest, Option<RequestId>) {
        let previous = self.abort();
        self.asked = Some(Asked { id, revision, root: root.map(str::to_owned), pos, explicit });
        (VersionsRequest { id, pos: Some(pos) }, previous)
    }

    /// The completion was closed or the document changed: the request to cancel.
    pub fn abort(&mut self) -> Option<RequestId> {
        self.asked.take().map(|asked| asked.id)
    }

    /// The answer to request `id`; `None` when it is stale, failed or no dependency's version holds the caret.
    pub fn answered(
        &mut self,
        id: RequestId,
        answer: Result<Vec<DependencyVersions>, String>,
        revision: u64,
        root: Option<&str>,
        doc: &Rope,
    ) -> Option<VersionCompletion> {
        if self.asked.as_ref().is_none_or(|asked| asked.id != id) {
            return None;
        }
        let asked = self.asked.take()?;
        if asked.revision != revision || asked.root.as_deref() != root {
            return None;
        }
        let items = answer.ok()?;
        let item = completion_item(&items, asked.pos)?;
        let typed = buffer::slice(doc, item.dependency.from, asked.pos);
        Some(version_completions(item, &typed, asked.explicit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::diagnostics::DiagnosticStore;
    use crate::editor_lsp::intentions::{listed_intentions, with_inspection_controls};
    use crate::editor_lsp::positions::apply_changes;
    use crate::find::utf16;
    use crate::platform_more::inspections::diagnostic_inspection;
    use ide_workspace::dependencies::{DependencyUpdate, ManifestDependency};

    const TEXT: &str = "# 中文 😀\n[dependencies]\ntokio = { version = \"0.2\", features = [\"full\"] }\n";
    const PATH: &str = "/p/Cargo.toml";
    const ROOT: Option<&str> = Some("/p");

    fn version_from(text: &str, needle: &str) -> u32 {
        utf16::len(&text[..text.find(needle).unwrap_or_default()])
    }

    fn from() -> u32 {
        version_from(TEXT, "\"0.2\"") + 1
    }

    fn item_with(replacement: Option<(&str, &str)>) -> DependencyVersions {
        DependencyVersions {
            dependency: ManifestDependency {
                package: "tokio".to_owned(),
                requirement: "0.2".to_owned(),
                from: from(),
                to: from() + 3,
            },
            versions: ["2.0.0-beta.1", "1.10.0", "1.9.0", "0.2.1"].map(str::to_owned).to_vec(),
            update: replacement.map(|(latest, replacement)| DependencyUpdate {
                latest: latest.to_owned(),
                replacement: replacement.to_owned(),
            }),
        }
    }

    fn item() -> DependencyVersions {
        item_with(Some(("1.10.0", "1.10")))
    }

    fn no_profile() -> ProfileOverrides {
        ProfileOverrides::new()
    }

    /// A Cargo.toml editor at revision 0 whose first check was answered with `items`.
    fn settled(
        store: &mut DiagnosticStore,
        path: &str,
        items: Vec<DependencyVersions>,
        i18n: &mut I18n,
    ) -> DependencyCheck {
        let (mut check, _) = DependencyCheck::open(0);
        let request = check.due(CHECK_DELAY_MS, true, 0, ROOT, RequestId(1));
        assert_eq!(request, Some(VersionsRequest { id: RequestId(1), pos: None }));
        let diagnostics = check.answered(RequestId(1), Ok(items), 0, ROOT, &Rope::from_str(TEXT), i18n);
        store.set_file_diagnostics(path, DEPENDENCY_PRODUCER, diagnostics.unwrap_or_default());
        check
    }

    fn apply_effects(store: &mut DiagnosticStore, path: &str, effects: &[CheckEffect]) {
        if effects.contains(&CheckEffect::ClearDiagnostics) {
            store.set_file_diagnostics(path, DEPENDENCY_PRODUCER, Vec::new());
        }
    }

    #[test]
    fn publishes_a_weak_warning_and_fixes_only_the_version_through_context_actions() {
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let mut check = settled(&mut store, PATH, vec![item()], &mut i18n);
        let shown = store.diagnostics_for(PATH, &no_profile());
        assert_eq!(shown.len(), 1);
        let diagnostic = &shown[0].diagnostic;
        assert_eq!(diagnostic.severity, Severity::Info);
        assert_eq!(diagnostic.message, "A newer version is available: 1.10.0");
        assert_eq!((diagnostic.range.start.line, diagnostic.range.start.col), (2, 21));
        assert_eq!((diagnostic.range.end.line, diagnostic.range.end.col), (2, 24));
        let caret = from();
        let fixes = check.fixes(caret, caret, 0, true, false, &mut i18n);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].label, "Update to 1.10");
        let change = apply_fix(&fixes[0], 0);
        let doc = apply_changes(&Rope::from_str(TEXT), &change.into_iter().collect::<Vec<_>>());
        assert_eq!(doc.to_string(), TEXT.replace("\"0.2\"", "\"1.10\""));
        let effects = check.doc_changed(10);
        apply_effects(&mut store, PATH, &effects);
        assert!(store.diagnostics_for(PATH, &no_profile()).is_empty());
    }

    #[test]
    fn provides_direct_and_hover_quick_fixes_even_without_a_language_server() {
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let check = settled(&mut store, PATH, vec![item()], &mut i18n);
        let fixes = check.fixes(from(), from() + 3, 0, true, false, &mut i18n);
        assert_eq!(fixes.iter().map(|fix| fix.label.as_str()).collect::<Vec<_>>(), ["Update to 1.10"]);
        let changes: Vec<TextChange> = apply_fix(&fixes[0], 0).into_iter().collect();
        let doc = apply_changes(&Rope::from_str(TEXT), &changes);
        assert!(doc.to_string().contains("version = \"1.10\", features = [\"full\"]"));
    }

    #[test]
    fn shows_the_exact_replacement_with_weak_warning_inspection_controls() {
        for replacement in ["0.10", "^0.10", "=0.10.3"] {
            let mut i18n = I18n::default();
            let mut store = DiagnosticStore::default();
            let check = settled(&mut store, PATH, vec![item_with(Some(("0.10.3", replacement)))], &mut i18n);
            let fixes = check.fixes(from(), from(), 0, true, false, &mut i18n);
            let inspection = diagnostic_inspection(DEPENDENCY_PRODUCER, None);
            let items = with_inspection_controls(
                listed_intentions(intention_items(&fixes), "bulb"),
                &inspection.into_iter().collect::<Vec<_>>(),
                &mut i18n,
            );
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].label, format!("Update to {replacement}"));
            assert_eq!(items[0].icon, Some("bulb"));
            assert_eq!(items[0].quick_fix, Some(true));
            assert_eq!(items[0].submenu.as_ref().map(Vec::len), Some(2));
            assert_eq!(items[0].run, IntentionRun::Local(fixes[0].id.clone()));
            let changes: Vec<TextChange> = apply_fix(&fixes[0], 0).into_iter().collect();
            let doc = apply_changes(&Rope::from_str(TEXT), &changes);
            assert_eq!(doc.to_string(), TEXT.replace("\"0.2\"", &format!("\"{replacement}\"")));
        }
    }

    #[test]
    fn keeps_diagnostics_when_one_of_two_split_editors_closes() {
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let mut viewers = DependencyViewers::default();
        viewers.open(PATH);
        let _first = settled(&mut store, PATH, vec![item()], &mut i18n);
        viewers.open(PATH);
        let mut split = settled(&mut store, PATH, vec![item()], &mut i18n);
        let effects = split.close();
        apply_effects(&mut store, PATH, &effects);
        if viewers.close(PATH) {
            store.set_file_diagnostics(PATH, DEPENDENCY_PRODUCER, Vec::new());
        }
        assert_eq!(store.diagnostics_for(PATH, &no_profile()).len(), 1);
        assert!(viewers.close(PATH));
    }

    #[test]
    fn invalidates_fixes_immediately_after_edits_and_rejects_a_stale_fix_callback() {
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let mut check = settled(&mut store, PATH, vec![item()], &mut i18n);
        let fixes = check.fixes(from(), from(), 0, true, false, &mut i18n);
        check.doc_changed(10);
        assert!(check.fixes(from(), from(), 1, true, false, &mut i18n).is_empty());
        assert_eq!(apply_fix(&fixes[0], 1), None);
    }

    #[test]
    fn discards_delayed_responses_after_an_edit_project_switch_or_editor_close() {
        let mut i18n = I18n::default();
        let doc = Rope::from_str(TEXT);
        let (mut check, _) = DependencyCheck::open(0);
        assert!(check.due(CHECK_DELAY_MS, true, 0, ROOT, RequestId(1)).is_some());
        assert_eq!(check.doc_changed(600), [CheckEffect::Cancel(RequestId(1)), CheckEffect::ClearDiagnostics]);
        assert_eq!(check.answered(RequestId(1), Ok(vec![item()]), 1, ROOT, &doc, &mut i18n), None);

        assert!(check.due(1100, true, 1, ROOT, RequestId(2)).is_some());
        assert!(check.is_current(RequestId(2)));
        let effects = check.project_changed();
        assert!(effects.contains(&CheckEffect::Cancel(RequestId(2))));
        assert!(effects.contains(&CheckEffect::ClearDiagnostics));
        assert!(!check.is_current(RequestId(2)));
        assert_eq!(check.answered(RequestId(2), Ok(vec![item()]), 1, Some("/other"), &doc, &mut i18n), None);
        assert_eq!(check.next_run(), None);

        let (mut other, _) = DependencyCheck::open(2000);
        assert!(other.due(2500, true, 0, Some("/other"), RequestId(3)).is_some());
        assert_eq!(other.close(), [CheckEffect::Cancel(RequestId(3))]);
        assert_eq!(other.answered(RequestId(3), Ok(vec![item()]), 0, Some("/other"), &doc, &mut i18n), None);

        let (mut moved, _) = DependencyCheck::open(0);
        assert!(moved.due(CHECK_DELAY_MS, true, 0, ROOT, RequestId(4)).is_some());
        assert_eq!(moved.answered(RequestId(4), Ok(vec![item()]), 0, Some("/other"), &doc, &mut i18n), None);
    }

    #[test]
    fn stays_silent_offline_with_compatible_versions_and_for_non_cargo_or_readonly_files() {
        let mut i18n = I18n::default();
        let doc = Rope::from_str(TEXT);
        let (mut offline, _) = DependencyCheck::open(0);
        assert!(offline.due(CHECK_DELAY_MS, true, 0, ROOT, RequestId(1)).is_some());
        assert_eq!(offline.answered(RequestId(1), Err("offline".to_owned()), 0, ROOT, &doc, &mut i18n), None);

        let mut store = DiagnosticStore::default();
        settled(&mut store, "/p/sub/Cargo.toml", vec![item_with(None)], &mut i18n);
        assert!(store.diagnostics_for("/p/sub/Cargo.toml", &no_profile()).is_empty());

        assert_eq!(manifest_of("/p/other.toml", false, false), None);
        assert_eq!(manifest_of("/p/readonly/Cargo.toml", false, true), None);
        assert_eq!(manifest_of("/p/Cargo.toml", true, false), None);
        assert_eq!(manifest_of("/p/NotCargo.toml", false, false), None);
        assert_eq!(manifest_of("/p/sub/Cargo.toml", false, false), Some(Manifest::Cargo));
    }

    #[test]
    fn honours_the_inspection_profile_and_cleans_up_on_close() {
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let mut viewers = DependencyViewers::default();
        viewers.open(PATH);
        let mut check = settled(&mut store, PATH, vec![item()], &mut i18n);
        let off: ProfileOverrides =
            serde_json::from_value(serde_json::json!({ "builtin:dependency-versions": { "enabled": false } }))
                .unwrap_or_default();
        assert!(!inspection_enabled(&off));
        let effects = check.profile_changed(1000);
        assert!(effects.contains(&CheckEffect::IntentionsChanged));
        apply_effects(&mut store, PATH, &effects);
        assert!(store.diagnostics_for(PATH, &off).is_empty());
        assert!(check.fixes(from(), from(), 0, inspection_enabled(&off), false, &mut i18n).is_empty());
        assert_eq!(check.due(1500, inspection_enabled(&off), 0, ROOT, RequestId(2)), None);
        check.close();
        assert!(viewers.close(PATH));
        store.set_file_diagnostics(PATH, DEPENDENCY_PRODUCER, Vec::new());
        assert!(store.diagnostics_for(PATH, &no_profile()).is_empty());
    }

    #[test]
    fn completes_only_at_a_parsed_dependency_version_preserving_descending_source_order() {
        let doc = Rope::from_str(TEXT);
        let mut source = VersionCompletionSource::default();
        let (request, previous) = source.ask(RequestId(1), from() + 3, true, 0, ROOT);
        assert_eq!((request, previous), (VersionsRequest { id: RequestId(1), pos: Some(from() + 3) }, None));
        let completion = source.answered(RequestId(1), Ok(vec![item()]), 0, ROOT, &doc);
        let completion = completion.unwrap_or_else(|| VersionCompletion { from: 0, to: 0, options: Vec::new() });
        assert_eq!((completion.from, completion.to), (from(), from() + 3));
        let labels: Vec<&str> = completion.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, item().versions);
        source.ask(RequestId(2), 0, true, 0, ROOT);
        assert_eq!(source.answered(RequestId(2), Ok(Vec::new()), 0, ROOT, &doc), None);
    }

    #[test]
    fn replaces_the_whole_version_and_retains_a_simple_requirement_operator() {
        let mut item = item();
        item.dependency.from = 0;
        item.dependency.to = 4;
        let completion = version_completions(&item, "^1.2", true);
        assert_eq!(completion.options[0].label, "2.0.0-beta.1");
        assert_eq!(completion.options[0].apply, "^2.0.0-beta.1");
    }

    #[test]
    fn requests_versions_at_the_caret_and_replaces_the_entire_version_without_running_inspections() {
        let text = "module example.com/app\n\nrequire (\n\tgolang.org/x/text v0.1.0 // indirect\n)\n";
        let from = version_from(text, "v0.1.0");
        let to = from + 6;
        let manifest = manifest_of("/p/go.mod", false, false);
        assert_eq!(manifest, Some(Manifest::GoMod));
        assert!(!manifest.is_some_and(Manifest::checks_versions));
        let mut source = VersionCompletionSource::default();
        let (request, _) = source.ask(RequestId(1), from + 3, false, 0, ROOT);
        assert_eq!(request.pos, Some(from + 3));
        let answer = vec![DependencyVersions {
            dependency: ManifestDependency {
                package: "golang.org/x/text".to_owned(),
                requirement: "v0.1.0".to_owned(),
                from,
                to,
            },
            versions: ["v1.2.0", "v0.12.0", "v0.1.0"].map(str::to_owned).to_vec(),
            update: None,
        }];
        let completion = source.answered(RequestId(1), Ok(answer), 0, ROOT, &Rope::from_str(text));
        let completion = completion.unwrap_or_else(|| VersionCompletion { from: 0, to: 0, options: Vec::new() });
        assert_eq!((completion.from, completion.to), (from, to));
        let options: Vec<(&str, &str)> =
            completion.options.iter().map(|o| (o.label.as_str(), o.apply.as_str())).collect();
        assert_eq!(options, [("v0.12.0", "v0.12.0"), ("v0.1.0", "v0.1.0")]);
    }
}
