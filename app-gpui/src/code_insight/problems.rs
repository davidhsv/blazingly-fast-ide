//! Project-wide problems for the Problems tool window, the error badge and the file decorations.
//!
//! The backend publishes full replacements per (file, producer), and the IDE's own inspections
//! (typos, file references) publish the same way: the store keeps one list per producer and merges
//! them per file. It holds the problems as reported and as the inspection profile shows them; the
//! views read the latter.

use crate::editor_lsp::diagnostics::severity_rank;
use crate::find::store::StorageWrite;
use crate::find::window_store::HeaderMenuItem;
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform_more::inspection_controls::{ControlItem, InspectionCommand, problem_controls};
use crate::platform_more::inspections::{InspectionRef, StyledDiagnostic, apply_profile, diagnostic_inspection};
use crate::settings_model::inspections_model::ProfileOverrides;
use crate::vcs_ops::menu::MenuItem;
use ide_core::{Diagnostic, Severity};
use serde_json::{Value, json};
use std::cmp::Ordering;

/// The problems of one file: one list per producer, in the order the producers first reported.
pub type FileProblems = Vec<(String, Vec<StyledDiagnostic>)>;

/// The problems of every file, in the order the files first reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemsMap {
    files: Vec<(String, FileProblems)>,
}

impl ProblemsMap {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// The paths with problems, in the order they first reported.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(path, _)| path.as_str())
    }

    /// The producers' lists of `path`.
    pub fn file(&self, path: &str) -> Option<&FileProblems> {
        self.files.iter().find(|(own, _)| own == path).map(|(_, sources)| sources)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.file(path).is_some()
    }

    /// Every file with its producers' lists.
    pub fn files(&self) -> impl Iterator<Item = (&str, &FileProblems)> {
        self.files.iter().map(|(path, sources)| (path.as_str(), sources))
    }

    /// Every problem of every file.
    pub fn all(&self) -> impl Iterator<Item = &StyledDiagnostic> {
        self.files.iter().flat_map(|(_, sources)| sources.iter().flat_map(|(_, items)| items.iter()))
    }
}

/// Problems per severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeverityCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
    pub hint: usize,
}

impl SeverityCounts {
    fn add(&mut self, severity: Severity) {
        match severity {
            Severity::Error => self.error += 1,
            Severity::Warning => self.warning += 1,
            Severity::Info => self.info += 1,
            Severity::Hint => self.hint += 1,
        }
    }
}

/// Problems as reported, without a profile applied.
pub fn plain(items: Vec<Diagnostic>) -> Vec<StyledDiagnostic> {
    items.into_iter().map(|diagnostic| StyledDiagnostic { diagnostic, typo: false }).collect()
}

/// Replaces `source`'s problems of `path` with `items`; an item without a source of its own gets
/// `source`. A file whose producers all cleared goes. Returns whether `map` changed.
pub fn merge_diagnostics(map: &mut ProblemsMap, path: &str, source: &str, items: Vec<StyledDiagnostic>) -> bool {
    let at = map.files.iter().position(|(own, _)| own == path);
    let known = at.is_some_and(|at| map.files[at].1.iter().any(|(own, _)| own == source));
    if items.is_empty() && !known {
        return false;
    }
    let at = at.unwrap_or_else(|| {
        map.files.push((path.to_owned(), Vec::new()));
        map.files.len() - 1
    });
    let sources = &mut map.files[at].1;
    if items.is_empty() {
        sources.retain(|(own, _)| own != source);
    } else {
        let items: Vec<StyledDiagnostic> = items
            .into_iter()
            .map(|mut item| {
                if item.diagnostic.source.is_none() {
                    item.diagnostic.source = Some(source.to_owned());
                }
                item
            })
            .collect();
        match sources.iter_mut().find(|(own, _)| own == source) {
            Some((_, own)) => *own = items,
            None => sources.push((source.to_owned(), items)),
        }
    }
    if sources.is_empty() {
        map.files.remove(at);
    }
    true
}

fn position_order(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    (a.range.start.line, a.range.start.col).cmp(&(b.range.start.line, b.range.start.col))
}

/// Every problem of `path` across producers, by position, then severity.
pub fn file_diagnostics<'a>(map: &'a ProblemsMap, path: &str) -> Vec<&'a StyledDiagnostic> {
    let mut items: Vec<&StyledDiagnostic> =
        map.file(path).into_iter().flat_map(|sources| sources.iter().flat_map(|(_, items)| items.iter())).collect();
    items.sort_by(|a, b| {
        position_order(&a.diagnostic, &b.diagnostic)
            .then_with(|| severity_rank(a.diagnostic.severity).cmp(&severity_rank(b.diagnostic.severity)))
    });
    items
}

/// The producer (inspection) that reported `diagnostic`, one of `path`'s problems in `map`.
pub fn producer_for<'a>(map: &'a ProblemsMap, path: &str, diagnostic: &Diagnostic) -> Option<&'a str> {
    map.file(path)?
        .iter()
        .find(|(_, items)| items.iter().any(|item| &item.diagnostic == diagnostic))
        .map(|(source, _)| source.as_str())
}

/// The inspection that reported `diagnostic` (for Disable Inspection / Change Severity).
pub fn diagnostic_inspection_of(map: &ProblemsMap, path: &str, diagnostic: &Diagnostic) -> Option<InspectionRef> {
    let producer = producer_for(map, path, diagnostic)
        .map(str::to_owned)
        .unwrap_or_else(|| crate::platform_more::inspections::producer_of(diagnostic, None).to_owned());
    diagnostic_inspection(&producer, diagnostic.code.as_deref())
}

/// How the Problems view orders a file's problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProblemOrder {
    /// Errors before warnings before infos (IDEA's default).
    pub by_severity: bool,
    /// By message text, then position; otherwise by position, then text.
    pub by_name: bool,
}

impl Default for ProblemOrder {
    fn default() -> Self {
        Self { by_severity: true, by_name: false }
    }
}

/// A file's problems in the Problems view order (Sort by Severity / Name).
pub fn sort_problems<'a>(items: &[&'a StyledDiagnostic], order: ProblemOrder) -> Vec<&'a StyledDiagnostic> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| {
        let (a, b) = (&a.diagnostic, &b.diagnostic);
        let severity =
            if order.by_severity { severity_rank(a.severity).cmp(&severity_rank(b.severity)) } else { Ordering::Equal };
        severity.then_with(|| {
            if order.by_name {
                collate::compare_natural(&a.message, &b.message).then_with(|| position_order(a, b))
            } else {
                position_order(a, b).then_with(|| collate::compare_natural(&a.message, &b.message))
            }
        })
    });
    sorted
}

pub fn count_severities<'a>(items: impl IntoIterator<Item = &'a StyledDiagnostic>) -> SeverityCounts {
    let mut counts = SeverityCounts::default();
    for item in items {
        counts.add(item.diagnostic.severity);
    }
    counts
}

pub fn project_counts(map: &ProblemsMap) -> SeverityCounts {
    count_severities(map.all())
}

/// The most severe of `items`; `None` for none.
pub fn worst_severity<'a>(items: impl IntoIterator<Item = &'a StyledDiagnostic>) -> Option<Severity> {
    items.into_iter().map(|item| item.diagnostic.severity).min_by_key(|severity| severity_rank(*severity))
}

/// `raw` under `profile`: disabled inspections dropped, severities remapped.
pub fn profiled_problems(raw: &ProblemsMap, profile: &ProfileOverrides) -> ProblemsMap {
    let mut out = ProblemsMap::default();
    for (path, sources) in &raw.files {
        for (source, items) in sources {
            merge_diagnostics(&mut out, path, source, profiled(profile, source, items));
        }
    }
    out
}

fn profiled(profile: &ProfileOverrides, source: &str, items: &[StyledDiagnostic]) -> Vec<StyledDiagnostic> {
    let diagnostics: Vec<Diagnostic> = items.iter().map(|item| item.diagnostic.clone()).collect();
    apply_profile(profile, source, &diagnostics).unwrap_or_else(|| items.to_vec())
}

/// The problems as reported and as the inspection profile shows them.
#[derive(Debug, Clone, Default)]
pub struct ProblemsStore {
    raw: ProblemsMap,
    shown: ProblemsMap,
    profile: ProfileOverrides,
}

impl ProblemsStore {
    /// As reported, before the inspection profile.
    pub fn raw(&self) -> &ProblemsMap {
        &self.raw
    }

    /// What the views show.
    pub fn map(&self) -> &ProblemsMap {
        &self.shown
    }

    /// A publication of `source` for `path` (a backend `diagnostics` event, or one of the IDE's own
    /// inspections). Returns whether what the views show may have changed.
    pub fn apply(&mut self, path: &str, source: &str, items: Vec<Diagnostic>) -> bool {
        let items = plain(items);
        let shown = profiled(&self.profile, source, &items);
        if !merge_diagnostics(&mut self.raw, path, source, items) {
            return false;
        }
        merge_diagnostics(&mut self.shown, path, source, shown);
        true
    }

    /// The inspection profile changed: every problem is shown under the new one.
    pub fn set_profile(&mut self, profile: ProfileOverrides) {
        self.shown = profiled_problems(&self.raw, &profile);
        self.profile = profile;
    }

    /// The window switched projects: the previous one's problems go.
    pub fn clear(&mut self) {
        self.raw = ProblemsMap::default();
        self.shown = ProblemsMap::default();
    }
}

/// The decoration of a file with errors: a red underline in the project view, its folders too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemDecoration {
    pub error: bool,
    pub propagate: bool,
    pub tooltip: String,
}

/// The "problems" file decorations: files with errors, with a tooltip counting them.
pub fn problem_decorations(map: &ProblemsMap, i18n: &mut I18n) -> Vec<(String, ProblemDecoration)> {
    let mut out = Vec::new();
    for path in map.paths() {
        let counts = count_severities(file_diagnostics(map, path));
        if counts.error == 0 {
            continue;
        }
        let errors = counted(i18n, counts.error, " error", " errors");
        let tooltip = match counts.warning {
            0 => errors,
            n => format!("{errors}, {}", counted(i18n, n, " warning", " warnings")),
        };
        out.push((path.to_owned(), ProblemDecoration { error: true, propagate: true, tooltip }));
    }
    out
}

fn counted(i18n: &mut I18n, n: usize, one: &str, many: &str) -> String {
    i18n.tx_built(&["", if n == 1 { one } else { many }], &[Param::from(n)])
}

/// The Problems stripe button's badge: the project's errors, `None` without any.
pub fn error_badge(map: &ProblemsMap) -> Option<usize> {
    Some(project_counts(map).error).filter(|n| *n > 0)
}

// ---------------------------------------------------------------------------
// The Problems tool window.

/// The scope tabs: the current editor's file, or the whole project's errors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProblemsTab {
    #[default]
    File,
    Project,
}

/// The severity groups the toolbar shows or hides; hints go with infos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityGroup {
    Error,
    Warning,
    Info,
}

impl SeverityGroup {
    pub fn of(severity: Severity) -> Self {
        match severity {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warning,
            Severity::Info | Severity::Hint => Self::Info,
        }
    }
}

/// Which severity groups are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeverityFilter {
    pub error: bool,
    pub warning: bool,
    pub info: bool,
}

impl Default for SeverityFilter {
    fn default() -> Self {
        Self { error: true, warning: true, info: true }
    }
}

impl SeverityFilter {
    pub fn shows(self, severity: Severity) -> bool {
        match SeverityGroup::of(severity) {
            SeverityGroup::Error => self.error,
            SeverityGroup::Warning => self.warning,
            SeverityGroup::Info => self.info,
        }
    }

    pub fn toggle(&mut self, group: SeverityGroup) {
        let flag = match group {
            SeverityGroup::Error => &mut self.error,
            SeverityGroup::Warning => &mut self.warning,
            SeverityGroup::Info => &mut self.info,
        };
        *flag = !*flag;
    }
}

/// A file of the Problems tree with its shown problems, in view order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemFile {
    pub path: String,
    pub items: Vec<StyledDiagnostic>,
}

/// The files of `paths` with the problems `show` lets through, files with errors first, then by
/// path; files left without problems are not listed.
pub fn build_problem_nodes(
    map: &ProblemsMap,
    paths: &[&str],
    show: SeverityFilter,
    order: ProblemOrder,
) -> Vec<ProblemFile> {
    let mut files: Vec<ProblemFile> = paths
        .iter()
        .filter_map(|path| {
            let shown: Vec<&StyledDiagnostic> =
                file_diagnostics(map, path).into_iter().filter(|item| show.shows(item.diagnostic.severity)).collect();
            let items: Vec<StyledDiagnostic> = sort_problems(&shown, order).into_iter().cloned().collect();
            (!items.is_empty()).then(|| ProblemFile { path: (*path).to_owned(), items })
        })
        .collect();
    let rank =
        |file: &ProblemFile| usize::from(!file.items.iter().any(|item| item.diagnostic.severity == Severity::Error));
    files.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| collate::compare(&a.path, &b.path)));
    files
}

/// The count beside the File tab: the current file's errors and warnings, `None` for none.
pub fn file_tab_count(map: &ProblemsMap, current: Option<&str>) -> Option<usize> {
    let counts = count_severities(file_diagnostics(map, current?));
    Some(counts.error + counts.warning).filter(|n| *n > 0)
}

/// The count beside the Project Errors tab, `None` for none.
pub fn project_error_count(map: &ProblemsMap) -> Option<usize> {
    error_badge(map)
}

/// The summary of a file row: "2 errors, 1 warning, 3 infos" (hints count as infos).
pub fn file_summary(items: &[StyledDiagnostic], i18n: &mut I18n) -> String {
    let counts = count_severities(items);
    let parts: Vec<String> = [
        (counts.error, " error", " errors"),
        (counts.warning, " warning", " warnings"),
        (counts.info + counts.hint, " info", " infos"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| counted(i18n, n, one, many))
    .collect();
    parts.join(", ")
}

/// A problem as Copy Problem Description puts it: the message, then its producer and code.
pub fn problem_text(diagnostic: &Diagnostic) -> String {
    match (&diagnostic.source, &diagnostic.code) {
        (Some(source), Some(code)) => format!("{} ({source}({code}))", diagnostic.message),
        (Some(source), None) => format!("{} ({source})", diagnostic.message),
        (None, _) => diagnostic.message.clone(),
    }
}

/// What the empty Problems view says.
pub fn empty_text(tab: ProblemsTab, current: Option<&str>) -> &'static str {
    match (tab, current) {
        (ProblemsTab::File, None) => "No file is open",
        (ProblemsTab::File, Some(_)) => "No problems in this file",
        (ProblemsTab::Project, _) => "No problems found",
    }
}

/// The key the view options are kept under.
pub const OPTIONS_KEY: &str = "openidea.problems.view.v1";

/// The view options of the ⋮ menu (IDEA's defaults: no autoscroll, no preview, by severity).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProblemsOptions {
    pub order: ProblemOrder,
    /// Selecting a problem opens it in the editor, the focus staying in the list.
    pub autoscroll: bool,
    /// Source preview beside the list.
    pub preview: bool,
}

const BY_SEVERITY: &str = "bySeverity";
const BY_NAME: &str = "byName";
const PREVIEW: &str = "preview";
const AUTOSCROLL: &str = "autoscroll";

impl ProblemsOptions {
    /// The options stored under [`OPTIONS_KEY`]; whatever is missing or malformed keeps its default.
    pub fn restored(stored: Option<&str>) -> Self {
        let value = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok()).unwrap_or(Value::Null);
        let defaults = Self::default();
        let flag = |key: &str, default: bool| value.get(key).and_then(Value::as_bool).unwrap_or(default);
        Self {
            order: ProblemOrder {
                by_severity: flag(BY_SEVERITY, defaults.order.by_severity),
                by_name: flag(BY_NAME, defaults.order.by_name),
            },
            autoscroll: flag(AUTOSCROLL, defaults.autoscroll),
            preview: flag(PREVIEW, defaults.preview),
        }
    }

    pub fn stored(self) -> StorageWrite {
        let value = json!({
            "bySeverity": self.order.by_severity,
            "byName": self.order.by_name,
            "autoscroll": self.autoscroll,
            "preview": self.preview,
        });
        StorageWrite { key: OPTIONS_KEY, value: value.to_string() }
    }

    /// The entries of the tool window's ⋮ menu.
    pub fn header_menu(self) -> [HeaderMenuItem; 6] {
        [
            HeaderMenuItem::Header("Sort By"),
            HeaderMenuItem::Check { id: BY_SEVERITY, label: "Severity", checked: self.order.by_severity },
            HeaderMenuItem::Check { id: BY_NAME, label: "Name", checked: self.order.by_name },
            HeaderMenuItem::Separator,
            HeaderMenuItem::Check { id: PREVIEW, label: "Show Preview", checked: self.preview },
            HeaderMenuItem::Check { id: AUTOSCROLL, label: "Autoscroll to Source", checked: self.autoscroll },
        ]
    }

    /// Toggles the option a menu entry stands for; returns what to store, `None` for an unknown id.
    pub fn toggle(&mut self, id: &str) -> Option<StorageWrite> {
        let flag = match id {
            BY_SEVERITY => &mut self.order.by_severity,
            BY_NAME => &mut self.order.by_name,
            PREVIEW => &mut self.preview,
            AUTOSCROLL => &mut self.autoscroll,
            _ => return None,
        };
        *flag = !*flag;
        Some(self.stored())
    }
}

/// A problem a context menu row acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemTarget {
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// What a row of the Problems context menu does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemCommand {
    /// Opens the problem and shows its quick fixes (the editor's context actions).
    ShowQuickFixes(ProblemTarget),
    /// Opens the problem in the editor, focused.
    JumpToSource(ProblemTarget),
    /// Puts the problem's description on the clipboard.
    Copy(String),
    /// Changes the problem's inspection in the profile.
    Inspection(InspectionCommand),
}

/// The context menu of a Problems row: the problem (or a file's first problem) as `target`;
/// `diagnostic` is set on a problem row, which can be copied and whose inspection can be changed.
pub fn problem_menu(
    target: ProblemTarget,
    diagnostic: Option<&Diagnostic>,
    inspection: Option<&InspectionRef>,
    profile: &ProfileOverrides,
    shortcuts: &ProblemShortcuts,
    i18n: &mut I18n,
) -> Vec<MenuItem<ProblemCommand>> {
    let mut quick_fixes =
        MenuItem::item("Show Quick Fixes", ProblemCommand::ShowQuickFixes(target.clone())).icon("bulb");
    if let Some(keys) = &shortcuts.quick_fixes {
        quick_fixes = quick_fixes.shortcut(keys.clone());
    }
    let copied = diagnostic.map(|d| ProblemCommand::Copy(problem_text(d)));
    let mut copy = MenuItem::entry("Copy Problem Description", copied).icon("copy");
    if let Some(keys) = &shortcuts.copy {
        copy = copy.shortcut(keys.clone());
    }
    let mut menu = vec![
        quick_fixes,
        MenuItem::item("Jump to Source", ProblemCommand::JumpToSource(target)).shortcut("F4"),
        MenuItem::Separator,
        copy,
    ];
    if let (Some(inspection), Some(_)) = (inspection, diagnostic) {
        menu.push(MenuItem::Separator);
        menu.extend(problem_controls(inspection, profile, i18n).into_iter().map(control_row));
    }
    menu
}

/// The shortcuts the Problems context menu shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemShortcuts {
    /// Show Context Actions (`code.showIntentions`).
    pub quick_fixes: Option<String>,
    /// Copy (⌘C / Ctrl+C).
    pub copy: Option<String>,
}

fn control_row(control: ControlItem) -> MenuItem<ProblemCommand> {
    let mut row = if control.submenu.is_empty() {
        MenuItem::entry(control.label, control.run.map(ProblemCommand::Inspection))
    } else {
        MenuItem::submenu(control.label, control.submenu.into_iter().map(control_row).collect())
    };
    if control.checked == Some(true) {
        row = row.icon("check");
    } else if let Some(icon) = control.icon {
        row = row.icon(icon);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::diagnostics::DiagnosticStore;
    use crate::platform_more::inspections::producer_of;
    use ide_core::settings::{InspectionLevel, InspectionOverride};
    use ide_core::{Pos, Range};

    fn d(line: u32, severity: Severity, source: Option<&str>) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(line, 0), Pos::new(line, 1)),
            severity,
            message: format!("line {line}"),
            source: source.map(str::to_owned),
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn error(line: u32) -> Diagnostic {
        d(line, Severity::Error, None)
    }

    fn lines_and_sources(map: &ProblemsMap, path: &str) -> Vec<(u32, Option<String>)> {
        file_diagnostics(map, path)
            .into_iter()
            .map(|item| (item.diagnostic.range.start.line, item.diagnostic.source.clone()))
            .collect()
    }

    #[test]
    fn replaces_per_file_source_and_keeps_other_sources() {
        let mut map = ProblemsMap::default();
        merge_diagnostics(&mut map, "/a", "ra", plain(vec![error(3), error(1)]));
        merge_diagnostics(&mut map, "/a", "clippy", plain(vec![d(2, Severity::Warning, None)]));
        let ra = || Some("ra".to_owned());
        assert_eq!(lines_and_sources(&map, "/a"), [(1, ra()), (2, Some("clippy".to_owned())), (3, ra())]);
        merge_diagnostics(&mut map, "/a", "ra", plain(vec![d(5, Severity::Info, None)]));
        let lines: Vec<u32> = lines_and_sources(&map, "/a").into_iter().map(|(line, _)| line).collect();
        assert_eq!(lines, [2, 5]);
    }

    #[test]
    fn drops_files_whose_sources_are_all_empty_and_is_a_no_op_for_unknown_clears() {
        let mut map = ProblemsMap::default();
        merge_diagnostics(&mut map, "/a", "ra", plain(vec![error(1)]));
        let before = map.clone();
        assert!(!merge_diagnostics(&mut map, "/a", "other", Vec::new()));
        assert!(!merge_diagnostics(&mut map, "/b", "ra", Vec::new()));
        assert_eq!(map, before);
        assert!(merge_diagnostics(&mut map, "/a", "ra", Vec::new()));
        assert!(!map.contains("/a"));
    }

    #[test]
    fn keeps_an_items_own_source() {
        let mut map = ProblemsMap::default();
        merge_diagnostics(&mut map, "/a", "eslint", plain(vec![d(1, Severity::Error, Some("eslint/no-undef"))]));
        assert_eq!(file_diagnostics(&map, "/a")[0].diagnostic.source.as_deref(), Some("eslint/no-undef"));
    }

    #[test]
    fn counts_per_severity() {
        let mut map = ProblemsMap::default();
        merge_diagnostics(&mut map, "/a", "ra", plain(vec![error(1), d(2, Severity::Warning, None)]));
        merge_diagnostics(&mut map, "/b", "ra", plain(vec![d(1, Severity::Warning, None), d(2, Severity::Hint, None)]));
        assert_eq!(project_counts(&map), SeverityCounts { error: 1, warning: 2, info: 0, hint: 1 });
        assert_eq!(
            count_severities(file_diagnostics(&map, "/b")),
            SeverityCounts { error: 0, warning: 1, info: 0, hint: 1 }
        );
        assert_eq!(worst_severity(file_diagnostics(&map, "/b")), Some(Severity::Warning));
        assert_eq!(worst_severity(Vec::new()), None);
    }

    #[test]
    fn applies_events_and_decorates_only_files_with_errors() {
        let mut store = ProblemsStore::default();
        store.apply("/p/src/a.rs", "ra", vec![error(1), d(2, Severity::Warning, None)]);
        store.apply("/p/src/b.rs", "ra", vec![d(1, Severity::Warning, None)]);
        let decorations = problem_decorations(store.map(), &mut I18n::default());
        assert_eq!(
            decorations,
            [(
                "/p/src/a.rs".to_owned(),
                ProblemDecoration { error: true, propagate: true, tooltip: "1 error, 1 warning".to_owned() }
            )]
        );
    }

    #[test]
    fn applies_the_inspection_profile_per_producer() {
        let mut raw = ProblemsMap::default();
        merge_diagnostics(&mut raw, "/a", "ra", plain(vec![error(1), d(2, Severity::Warning, None)]));
        merge_diagnostics(&mut raw, "/b", "eslint", plain(vec![error(3)]));
        let mut profile = ProfileOverrides::new();
        profile.insert("lsp:eslint".to_owned(), InspectionOverride { enabled: Some(false), ..Default::default() });
        profile.insert(
            "lsp:ra".to_owned(),
            InspectionOverride { severity: Some(InspectionLevel::WeakWarning), ..Default::default() },
        );
        let out = profiled_problems(&raw, &profile);
        assert_eq!(out.paths().collect::<Vec<_>>(), ["/a"]);
        let severities: Vec<Severity> =
            file_diagnostics(&out, "/a").into_iter().map(|item| item.diagnostic.severity).collect();
        assert_eq!(severities, [Severity::Info, Severity::Info]);
        assert_eq!(file_diagnostics(&profiled_problems(&raw, &ProfileOverrides::new()), "/b").len(), 1);
    }

    #[test]
    fn reach_problems_and_clear_there_too_with_their_inspection_known() {
        let mut editor = DiagnosticStore::default();
        let mut store = ProblemsStore::default();
        let typo = Diagnostic {
            message: "Typo: In word 'mesage'".to_owned(),
            source: Some("Spelling".to_owned()),
            ..d(0, Severity::Info, None)
        };
        let (_, publish) = editor.set_file_diagnostics("/p/a.py", "spelling", vec![typo.clone()]);
        assert!(publish);
        store.apply("/p/a.py", "spelling", vec![typo.clone()]);
        let shown = file_diagnostics(store.map(), "/p/a.py");
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].diagnostic.message, "Typo: In word 'mesage'");
        assert!(shown[0].typo);
        let producer = producer_for(store.map(), "/p/a.py", &shown[0].diagnostic).map(str::to_owned);
        assert_eq!(producer.as_deref(), Some("spelling"));
        let inspection = diagnostic_inspection(producer.as_deref().unwrap_or_else(|| producer_of(&typo, None)), None);
        assert_eq!(inspection.map(|inspection| inspection.id).as_deref(), Some("builtin:spelling"));

        let (_, publish) = editor.set_file_diagnostics("/p/a.py", "spelling", Vec::new());
        assert!(publish);
        store.apply("/p/a.py", "spelling", Vec::new());
        assert!(store.map().is_empty());
    }

    fn problem(severity: Severity, line: u32, message: &str) -> StyledDiagnostic {
        let diagnostic = Diagnostic { message: message.to_owned(), ..d(line, severity, None) };
        StyledDiagnostic { diagnostic, typo: false }
    }

    #[test]
    fn lists_files_with_errors_first_and_applies_the_severity_filter() {
        let mut map = ProblemsMap::default();
        merge_diagnostics(&mut map, "/a", "s", vec![problem(Severity::Warning, 0, "warning")]);
        merge_diagnostics(
            &mut map,
            "/b",
            "s",
            vec![problem(Severity::Error, 0, "error"), problem(Severity::Hint, 0, "hint")],
        );
        let all = build_problem_nodes(&map, &["/a", "/b"], SeverityFilter::default(), ProblemOrder::default());
        assert_eq!(all.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["/b", "/a"]);
        let errors = SeverityFilter { error: true, warning: false, info: false };
        let errors_only = build_problem_nodes(&map, &["/a", "/b"], errors, ProblemOrder::default());
        assert_eq!(errors_only.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["/b"]);
        assert_eq!(errors_only[0].items.len(), 1);
    }

    fn order_lines(order: ProblemOrder) -> Vec<u32> {
        let items = [
            problem(Severity::Warning, 1, "b"),
            problem(Severity::Error, 5, "c"),
            problem(Severity::Warning, 0, "z"),
            problem(Severity::Error, 2, "a"),
        ];
        let refs: Vec<&StyledDiagnostic> = items.iter().collect();
        sort_problems(&refs, order).into_iter().map(|item| item.diagnostic.range.start.line).collect()
    }

    #[test]
    fn orders_by_severity_then_position_ideas_default() {
        assert_eq!(order_lines(ProblemOrder { by_severity: true, by_name: false }), [2, 5, 0, 1]);
    }

    #[test]
    fn orders_by_position_or_by_name_when_asked() {
        assert_eq!(order_lines(ProblemOrder { by_severity: false, by_name: false }), [0, 1, 2, 5]);
        assert_eq!(order_lines(ProblemOrder { by_severity: false, by_name: true }), [2, 1, 5, 0]);
    }

    // Not from a spec: the tab counts, the file summary, the copied text and the stored options.
    #[test]
    fn tabs_count_and_rows_describe_their_problems() {
        let mut store = ProblemsStore::default();
        store.apply("/p/a.rs", "ra", vec![error(1), d(2, Severity::Warning, None), d(3, Severity::Hint, None)]);
        store.apply("/p/b.rs", "ra", vec![error(4)]);
        assert_eq!(file_tab_count(store.map(), Some("/p/a.rs")), Some(2));
        assert_eq!(file_tab_count(store.map(), Some("/p/c.rs")), None);
        assert_eq!(file_tab_count(store.map(), None), None);
        assert_eq!(project_error_count(store.map()), Some(2));
        assert_eq!(error_badge(&ProblemsMap::default()), None);
        let items: Vec<StyledDiagnostic> = file_diagnostics(store.map(), "/p/a.rs").into_iter().cloned().collect();
        assert_eq!(file_summary(&items, &mut I18n::default()), "1 error, 1 warning, 1 info");
        let coded = Diagnostic { code: Some("E0308".to_owned()), ..d(1, Severity::Error, Some("rustc")) };
        assert_eq!(problem_text(&coded), "line 1 (rustc(E0308))");
        assert_eq!(empty_text(ProblemsTab::File, None), "No file is open");

        let mut options = ProblemsOptions::restored(Some(r#"{"preview":true,"byName":"x"}"#));
        assert!(options.preview && !options.order.by_name && options.order.by_severity);
        let write = options.toggle(BY_NAME).expect("a known option");
        assert_eq!(ProblemsOptions::restored(Some(&write.value)), options);
        assert_eq!(options.toggle("other"), None);

        let mut clears = store.clone();
        clears.clear();
        assert!(clears.map().is_empty() && clears.raw().is_empty());
    }
}
