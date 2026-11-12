//! Code actions: the bulb on the caret line, the ⌥↵ intentions list (quick fixes first, preferred
//! ones emphasised, an inspection's controls in a fix's → menu), its preview of what an action
//! changes, the quick fix of ⌥⇧↵, actions of one kind (Optimize Imports, Generate) and the code
//! an action generated, brought to the file's indentation.

use super::diagnostics::{LintDiagnostic, severity_rank};
use super::workspace_edit::{DiffLine, edit_preview_lines};
use crate::editor::buffer;
use crate::platform::i18n::{I18n, Param};
use crate::platform_more::inspection_controls::{ControlItem, InspectionCommand, intention_controls};
use crate::platform_more::inspections::InspectionRef;
use crate::settings_model::inspections_model::SPELLING_INSPECTION;
use ide_core::ipc_path::to_ipc;
use ide_core::{FileEdit, IdeError, Severity, TextChange};
use ide_lsp::types::CodeAction;
use regex::Regex;
use ropey::Rope;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::LazyLock;

pub fn is_quick_fix(action: &CodeAction) -> bool {
    action.kind.as_deref().is_some_and(|kind| kind == "quickfix" || kind.starts_with("quickfix."))
}

/// IDEA's order: quick fixes first, preferred before others, disabled last; stable otherwise.
pub fn sort_code_actions(actions: &[CodeAction]) -> Vec<CodeAction> {
    let rank = |action: &CodeAction| {
        u8::from(action.disabled_reason.is_some()) * 4
            + u8::from(!is_quick_fix(action)) * 2
            + u8::from(!action.is_preferred)
    };
    let mut sorted = actions.to_vec();
    sorted.sort_by_key(rank);
    sorted
}

/// Sorted, one action per title: a server may report the same action under two kinds.
pub fn listed_code_actions(actions: &[CodeAction]) -> Vec<CodeAction> {
    let mut seen = HashSet::new();
    sort_code_actions(actions).into_iter().filter(|action| seen.insert(action.title.clone())).collect()
}

/// A problem placed in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedProblem {
    pub from: u32,
    pub to: u32,
    pub severity: Severity,
}

/// As in IDEA, when no error touches the caret the fixes of an error elsewhere on the caret line
/// are offered too: the errors to ask about, in line order; none when an error is at the caret.
pub fn line_errors_away_from_caret(
    problems: &[PlacedProblem],
    line_from: u32,
    line_to: u32,
    caret: u32,
) -> Vec<PlacedProblem> {
    let errors = problems.iter().filter(|problem| problem.severity == Severity::Error);
    if errors.clone().any(|problem| problem.from <= caret && caret <= problem.to) {
        return Vec::new();
    }
    let mut away: Vec<PlacedProblem> =
        errors.filter(|problem| problem.from >= line_from && problem.from <= line_to).copied().collect();
    away.sort_by_key(|problem| problem.from);
    away
}

/// The quick fixes of an error elsewhere on the caret line that join the list: enabled, and not
/// listed already.
pub fn extra_line_fixes(listed: &[CodeAction], fixes: &[CodeAction]) -> Vec<CodeAction> {
    let titles: HashSet<&str> = listed.iter().map(|action| action.title.as_str()).collect();
    fixes
        .iter()
        .filter(|fix| is_quick_fix(fix) && fix.disabled_reason.is_none() && !titles.contains(fix.title.as_str()))
        .cloned()
        .collect()
}

/// The bulb icon of fixes on the caret line: the red one is reserved for errors.
pub fn quick_fix_icon(lint: &[LintDiagnostic], line_from: u32, line_to: u32) -> &'static str {
    let error = lint.iter().any(|d| d.severity == Severity::Error && d.from <= line_to && d.to >= line_from);
    if error { "bulb-fix" } else { "bulb" }
}

/// What a row of the intentions list runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentionRun {
    /// The code action at this index of [`listed_code_actions`].
    CodeAction(usize),
    /// An intention of the editor itself (spelling, a language's own), by its id.
    Local(String),
    /// Open the inspection's settings.
    Inspection(InspectionCommand),
}

/// A row of the intentions list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentionItem {
    pub id: String,
    /// English source text, or a translated label.
    pub label: String,
    pub icon: Option<&'static str>,
    /// An inspection's fix, whatever icon its severity gives it.
    pub quick_fix: Option<bool>,
    pub emphasis: bool,
    /// Why the row cannot run; it shows, but is not activatable.
    pub disabled: Option<String>,
    pub run: IntentionRun,
    /// The row's → menu (an inspection's controls).
    pub submenu: Option<Vec<ControlItem>>,
}

impl IntentionItem {
    pub fn new(id: &str, label: &str, run: IntentionRun) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            icon: None,
            quick_fix: None,
            emphasis: false,
            disabled: None,
            run,
            submenu: None,
        }
    }

    fn is_fix(&self) -> bool {
        self.quick_fix.unwrap_or(self.icon == Some("bulb-fix"))
    }
}

/// The rows of language-server actions (already [`listed_code_actions`]).
pub fn action_items(actions: &[CodeAction], fix_icon: &'static str) -> Vec<IntentionItem> {
    actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let fix = is_quick_fix(action);
            IntentionItem {
                icon: Some(if fix { fix_icon } else { "bulb" }),
                quick_fix: Some(fix),
                emphasis: action.is_preferred,
                disabled: action.disabled_reason.clone(),
                ..IntentionItem::new(&format!("lsp:{i}"), &action.title, IntentionRun::CodeAction(i))
            }
        })
        .collect()
}

/// The editor's own fixes and the server's share icons and one stable order: fixes first,
/// emphasised before others, disabled last.
pub fn listed_intentions(items: Vec<IntentionItem>, fix_icon: &'static str) -> Vec<IntentionItem> {
    let rank = |item: &IntentionItem| {
        u8::from(item.disabled.is_some()) * 4 + u8::from(!item.is_fix()) * 2 + u8::from(!item.emphasis)
    };
    let mut items: Vec<IntentionItem> =
        items
            .into_iter()
            .map(|item| {
                if item.is_fix() { IntentionItem { quick_fix: Some(true), icon: Some(fix_icon), ..item } } else { item }
            })
            .collect();
    items.sort_by_key(rank);
    items
}

/// IDEA's → menu on inspection fixes: with one inspection reporting at the caret its fixes get
/// its controls (Edit inspection profile setting, Disable inspection); otherwise each inspection
/// there gets a row of its own. Typo fixes carry theirs already.
pub fn with_inspection_controls(
    items: Vec<IntentionItem>,
    inspections: &[InspectionRef],
    i18n: &mut I18n,
) -> Vec<IntentionItem> {
    let inspections: Vec<&InspectionRef> =
        inspections.iter().filter(|inspection| inspection.id != SPELLING_INSPECTION).collect();
    if inspections.is_empty() {
        return items;
    }
    let fixes = items.iter().filter(|item| item.is_fix() && item.submenu.is_none()).count();
    if let [inspection] = inspections.as_slice()
        && fixes > 0
    {
        let submenu = intention_controls(inspection);
        return items
            .into_iter()
            .map(|item| {
                if item.is_fix() && item.submenu.is_none() {
                    IntentionItem { submenu: Some(submenu.clone()), ..item }
                } else {
                    item
                }
            })
            .collect();
    }
    let mut items = items;
    for inspection in inspections {
        let label = i18n.tx_built(&["Inspection '", "' options"], &[Param::from(inspection.name.as_str())]);
        items.push(IntentionItem {
            icon: Some("settings"),
            submenu: Some(intention_controls(inspection)),
            ..IntentionItem::new(
                &format!("inspection:{}", inspection.id),
                &label,
                IntentionRun::Inspection(InspectionCommand::EditSettings(inspection.id.clone())),
            )
        });
    }
    items
}

/// An action a plugin computes only when invoked (it may ask the user): not resolved for a
/// preview, not run silently.
pub fn is_lazy_plugin_action(action: &CodeAction) -> bool {
    action.server.0 == 0 && action.edit.is_none() && action.command.is_none()
}

/// The side panel of the intentions list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPreview {
    Diff { lines: Vec<DiffLine>, note: Option<String> },
    Text(String),
}

pub const NO_PREVIEW: &str = "Preview isn't available for this action";
pub const NO_CHANGES_HERE: &str = "No changes in this file";

/// The preview of a resolved action for the file at `path` showing `doc`: the diff it makes here,
/// and how many other files it changes.
pub fn intention_preview(action: &CodeAction, path: &str, doc: &Rope) -> ListPreview {
    let files = action.edit.as_ref().map_or(&[][..], |edit| edit.files.as_slice());
    let mut others = HashSet::new();
    let mut here = None;
    for file in files {
        let touched = match file {
            FileEdit::Edit { path: own, edits, .. } => {
                let own = to_ipc(own);
                if own == path && here.is_none() {
                    here = Some(edits);
                }
                own
            }
            FileEdit::Create { path, .. } | FileEdit::Delete { path, .. } => to_ipc(path),
            FileEdit::Rename { from, .. } => to_ipc(from),
        };
        if touched != path {
            others.insert(touched);
        }
    }
    let note = match others.len() {
        0 => None,
        1 => Some("Also changes 1 other file".to_owned()),
        n => Some(format!("Also changes {n} other files")),
    };
    if let Some(edits) = here {
        let lines = edit_preview_lines(doc, edits, 1, 60);
        if !lines.is_empty() {
            return ListPreview::Diff { lines, note };
        }
    }
    if let Some(note) = note {
        return ListPreview::Text(note);
    }
    let text = if action.command.is_some() && action.edit.is_none() { NO_PREVIEW } else { NO_CHANGES_HERE };
    ListPreview::Text(text.into())
}

/// What the preview of a row needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewStep {
    Show(ListPreview),
    /// Resolve the action (`codeAction/resolve`), hand the answer to [`ResolveCache::resolved`],
    /// then ask again.
    Resolve,
    /// The resolve is under way.
    Waiting,
}

/// The actions of one intentions list, each resolved once: a server may compute an edit lazily,
/// and the edit the preview resolved is the one applied.
#[derive(Debug, Clone)]
pub struct ResolveCache<K> {
    resolved: HashMap<K, CodeAction>,
    asked: HashSet<K>,
}

impl<K> Default for ResolveCache<K> {
    fn default() -> Self {
        Self { resolved: HashMap::new(), asked: HashSet::new() }
    }
}

impl<K: Eq + Hash + Clone> ResolveCache<K> {
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty() && self.asked.is_empty()
    }

    /// The preview of `action` (row `key`) for the file at `path` showing `doc`.
    pub fn preview(&mut self, key: &K, action: &CodeAction, path: &str, doc: &Rope) -> PreviewStep {
        if is_lazy_plugin_action(action) {
            return PreviewStep::Show(ListPreview::Text(NO_PREVIEW.into()));
        }
        if action.edit.is_some() || action.command.is_some() {
            return PreviewStep::Show(intention_preview(action, path, doc));
        }
        if let Some(full) = self.resolved.get(key) {
            return PreviewStep::Show(intention_preview(full, path, doc));
        }
        if self.asked.insert(key.clone()) { PreviewStep::Resolve } else { PreviewStep::Waiting }
    }

    /// The resolve of row `key` answered; a failure lets a later preview ask again.
    pub fn resolved(&mut self, key: &K, answer: Result<CodeAction, IdeError>) {
        self.asked.remove(key);
        if let Ok(action) = answer {
            self.resolved.insert(key.clone(), action);
        }
    }

    /// The action to apply for row `key`: the resolved one when the preview resolved it.
    pub fn action_to_apply<'a>(&'a self, key: &K, listed: &'a CodeAction) -> &'a CodeAction {
        self.resolved.get(key).unwrap_or(listed)
    }
}

/// The action Actions on Save applies for `kind` without UI: the first enabled one (preferred
/// first) that will not prompt.
pub fn silent_action<'a>(actions: &'a [CodeAction], kind: &str) -> Option<&'a CodeAction> {
    let mut candidates = actions.iter().filter(|action| {
        action.kind.as_deref().is_some_and(|own| own.starts_with(kind))
            && action.disabled_reason.is_none()
            && !is_lazy_plugin_action(action)
    });
    let first = candidates.clone().next();
    candidates.find(|action| action.is_preferred).or(first)
}

/// The problems ⌥⇧↵ tries, in order: worst first, those at the caret before others on its line;
/// hints never.
pub fn quick_fix_order(lint: &[LintDiagnostic], caret: u32, line_from: u32, line_to: u32) -> Vec<PlacedProblem> {
    let mut problems: Vec<(u8, PlacedProblem)> = lint
        .iter()
        .filter(|d| d.severity != Severity::Hint && d.to >= line_from && d.from <= line_to)
        .map(|d| {
            let at_caret = d.from <= caret && caret <= d.to;
            let rank = severity_rank(d.severity).min(2) + if at_caret { 0 } else { 3 };
            (rank, PlacedProblem { from: d.from, to: d.to, severity: d.severity })
        })
        .collect();
    problems.sort_by_key(|(rank, _)| *rank);
    problems.into_iter().map(|(_, problem)| problem).collect()
}

/// Whether an applied action's generated code is reformatted afterwards: not for organizing
/// imports or fixing the whole file.
pub fn reformats_generated(action: &CodeAction) -> bool {
    let kind = action.kind.as_deref().unwrap_or_default();
    !kind.starts_with("source.organizeImports") && !kind.starts_with("source.fixAll")
}

/// What an action of a kind (Optimize Imports, Generate…) does once the server answered.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one per server answer, consumed at once")]
pub enum KindOutcome {
    /// Nothing to apply: the hint to show.
    None(String),
    Apply(CodeAction),
    /// Several apply: offer the choice.
    Choose(Vec<CodeAction>),
}

/// The actions of `kind` (a prefix; empty for any) that `keep` accepts. `none` is the hint when
/// none applies; `line_error` the message of an error on the selected lines, which is why a
/// server offers no refactoring there.
pub fn kind_outcome(
    actions: &[CodeAction],
    kind: &str,
    keep: &dyn Fn(&CodeAction) -> bool,
    none: &str,
    line_error: Option<&str>,
) -> KindOutcome {
    let matching: Vec<&CodeAction> = actions
        .iter()
        .filter(|action| kind.is_empty() || action.kind.as_deref().is_some_and(|own| own.starts_with(kind)))
        .filter(|action| keep(action))
        .collect();
    let enabled: Vec<CodeAction> =
        matching.iter().filter(|action| action.disabled_reason.is_none()).map(|action| (*action).clone()).collect();
    match enabled.as_slice() {
        [] => {
            KindOutcome::None(matching.first().and_then(|action| action.disabled_reason.clone()).unwrap_or_else(|| {
                match line_error {
                    Some(error) => format!("{none}: fix the error first ({error})"),
                    None => none.to_owned(),
                }
            }))
        }
        [one] => KindOutcome::Apply(one.clone()),
        _ => KindOutcome::Choose(listed_code_actions(&enabled)),
    }
}

// ---------------------------------------------------------------------------
// Generated code

/// Lines (0-based) of `after` that an edit changed, top to bottom; an added import and the code
/// using it are separate spans, so the user's code between them is left alone.
pub fn changed_lines(before: &Rope, after: &Rope) -> Vec<(u32, u32)> {
    if before == after {
        return Vec::new();
    }
    let old: Vec<String> = (0..buffer::line_count(before)).map(|line| buffer::line_text(before, line)).collect();
    let new: Vec<String> = (0..buffer::line_count(after)).map(|line| buffer::line_text(after, line)).collect();
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut push = |from: u32, to: u32| match spans.last_mut() {
        Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
        _ => spans.push((from, to)),
    };
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            i += 1;
            j += 1;
            continue;
        }
        // A hunk: removed lines of `before` and added lines of `after`.
        let start = j;
        let mut removed = false;
        while (i < n || j < m) && !(i < n && j < m && old[i] == new[j]) {
            if i < n && (j >= m || lcs[i + 1][j] >= lcs[i][j + 1]) {
                i += 1;
                removed = true;
            } else {
                j += 1;
            }
        }
        if j > start {
            push(start as u32, j as u32 - 1);
        } else if removed {
            let at = (start as u32).min(m.saturating_sub(1) as u32);
            push(at, at);
        }
    }
    spans
}

fn leading_whitespace(text: &str) -> &str {
    &text[..text.len() - text.trim_start().len()]
}

/// Generated lines right under a `{` line but not indented past it (a server that formats the
/// snippet on its own): the block moves one level in, keeping its own nesting. Never applies to
/// indentation-based languages, whose blocks do not open with `{`. Lines are 0-based.
pub fn indent_under_brace(doc: &Rope, from_line: u32, to_line: u32, unit: &str) -> Vec<TextChange> {
    if from_line < 1 {
        return Vec::new();
    }
    let previous = buffer::line_text(doc, from_line - 1);
    if !previous.trim_end().ends_with('{') {
        return Vec::new();
    }
    let lead = leading_whitespace(&previous);
    let target = format!("{lead}{unit}");
    let first = (from_line..=to_line).map(|line| buffer::line_text(doc, line)).find(|text| !text.trim().is_empty());
    let Some(first) = first.map(|text| leading_whitespace(&text).to_owned()) else { return Vec::new() };
    if first.len() > lead.len() || !target.starts_with(&first) {
        return Vec::new();
    }
    let extra = &target[first.len()..];
    (from_line..=to_line)
        .filter(|line| !buffer::line_text(doc, *line).trim().is_empty())
        .map(|line| {
            let at = buffer::line_range(doc, line).start;
            TextChange { from: at, to: at, insert: extra.to_owned() }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The bulb

/// Fixes and refactorings light the bulb; source actions (organize imports, generate…) and
/// server extras are offered on almost every line and only appear in the ⌥↵ list.
pub fn lights_bulb(action: &CodeAction) -> bool {
    static FIX_OR_REFACTOR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(quickfix|refactor)(\.|$)").expect("the pattern is valid"));
    let kind = action.kind.as_deref().unwrap_or_default();
    kind.is_empty() || kind == "quickassist" || FIX_OR_REFACTOR.is_match(kind)
}

/// The bulb on a line (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bulb {
    pub line: u32,
    /// The red bulb of a fix for an error.
    pub quick_fix: bool,
}

/// The bulb for the caret line once the lookup answered: lit by a fix or refactoring, or by the
/// editor's own intentions. `fix_icon_is_red` is [`quick_fix_icon`] being `bulb-fix`.
pub fn bulb_for(line: u32, actions: &[CodeAction], local: bool, own_fix: bool, fix_icon_is_red: bool) -> Option<Bulb> {
    (actions.iter().any(lights_bulb) || local)
        .then(|| Bulb { line, quick_fix: (own_fix || actions.iter().any(is_quick_fix)) && fix_icon_is_red })
}

/// The bulb after an edit or a caret move: it stays only while the caret stays on its line and
/// the text is unchanged.
pub fn bulb_after_update(bulb: Option<Bulb>, doc_changed: bool, caret_line: u32) -> Option<Bulb> {
    bulb.filter(|bulb| !doc_changed && bulb.line == caret_line)
}

/// Columns of free space the floating bulb needs at a line's start.
const BULB_COLUMNS: usize = 3;

/// Where the bulb floats: at the start of the text of the caret line, or of the line below or
/// above when the caret line's text starts at the left edge; in the gutter when none has room.
pub fn bulb_line(doc: &Rope, line: u32) -> (u32, bool) {
    let free = |line: u32| {
        if line >= buffer::line_count(doc) {
            return false;
        }
        let text = buffer::line_text(doc, line);
        text.trim().is_empty() || leading_whitespace(&text).chars().count() >= BULB_COLUMNS
    };
    [Some(line), line.checked_add(1), line.checked_sub(1)]
        .into_iter()
        .flatten()
        .find(|candidate| free(*candidate))
        .map_or((line, true), |candidate| (candidate, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::diagnostics::{DiagnosticStore, to_lint_diagnostics};
    use crate::settings_model::inspections_model::ProfileOverrides;
    use ide_core::{Diagnostic, Pos, Range, ServerId, TextEdit, WorkspaceEdit};

    fn action(title: &str, kind: Option<&str>, is_preferred: bool, disabled_reason: Option<&str>) -> CodeAction {
        CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            is_preferred,
            disabled_reason: disabled_reason.map(str::to_owned),
            edit: None,
            command: None,
            data: None,
            server: ServerId(1),
        }
    }

    /// "Introduce local variable", resolved: an edit of `/p/a.ts` and a new `/p/b.ts`.
    fn resolved() -> CodeAction {
        let insert = TextEdit { range: Range::new(Pos::new(0, 0), Pos::new(0, 0)), new_text: "const x = 1;\n".into() };
        CodeAction {
            edit: Some(WorkspaceEdit {
                files: vec![
                    FileEdit::Edit { path: "/p/a.ts".into(), edits: vec![insert], version: None },
                    FileEdit::Create { path: "/p/b.ts".into(), overwrite: false },
                ],
                preconditions: Vec::new(),
            }),
            ..action("Introduce local variable", Some("refactor.extract"), false, None)
        }
    }

    fn titles(actions: &[CodeAction]) -> Vec<&str> {
        actions.iter().map(|action| action.title.as_str()).collect()
    }

    #[test]
    fn puts_preferred_quick_fixes_first_and_disabled_actions_last() {
        let sorted = sort_code_actions(&[
            action("extract", Some("refactor.extract"), false, None),
            action("disabled fix", Some("quickfix"), false, Some("nope")),
            action("fix", Some("quickfix"), false, None),
            action("preferred fix", Some("quickfix.import"), true, None),
            action("preferred refactor", Some("refactor"), true, None),
        ]);
        assert_eq!(titles(&sorted), ["preferred fix", "fix", "preferred refactor", "extract", "disabled fix"]);
    }

    fn problem(from: u32, to: u32, severity: Severity) -> PlacedProblem {
        PlacedProblem { from, to, severity }
    }

    #[test]
    fn offers_errors_elsewhere_on_the_caret_line_when_none_is_at_the_caret() {
        let problems = [
            problem(30, 34, Severity::Error),
            problem(12, 15, Severity::Error),
            problem(50, 52, Severity::Error),
            problem(5, 6, Severity::Warning),
        ];
        assert_eq!(
            line_errors_away_from_caret(&problems, 0, 40, 2),
            [problem(12, 15, Severity::Error), problem(30, 34, Severity::Error)]
        );
    }

    #[test]
    fn offers_nothing_extra_when_an_error_touches_the_caret() {
        let problems = [problem(0, 3, Severity::Error), problem(12, 15, Severity::Error)];
        assert!(line_errors_away_from_caret(&problems, 0, 40, 3).is_empty());
    }

    // Inspection controls in ⌥↵.

    fn unused(severity: Severity) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(0, 7), Pos::new(0, 9)),
            severity,
            message: "`os` imported but unused".into(),
            source: Some("Ruff".into()),
            code: Some("F401".into()),
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    /// The inspections at the caret (offset 8 of `import os`) of a file with `items` from Ruff.
    fn inspections(items: Vec<Diagnostic>) -> Vec<InspectionRef> {
        let mut store = DiagnosticStore::default();
        store.set_file_diagnostics("/p/one.py", "ruff", items);
        store.inspections_at("/p/one.py", &Rope::from_str("import os\n"), 8, &ProfileOverrides::default())
    }

    fn local(id: &str, label: &str) -> IntentionItem {
        IntentionItem::new(id, label, IntentionRun::Local(id.into()))
    }

    fn submenu_labels(item: &IntentionItem) -> Option<Vec<String>> {
        item.submenu.as_ref().map(|rows| rows.iter().map(|row| row.label.clone()).collect())
    }

    #[test]
    fn gives_the_fixes_of_the_one_inspection_at_the_caret_its_menu() {
        let fix = IntentionItem { icon: Some("bulb-fix"), ..local("0", "Remove unused import") };
        let refactor = IntentionItem { icon: Some("bulb"), ..local("1", "Extract method") };
        let at_caret = inspections(vec![unused(Severity::Warning)]);
        let items = with_inspection_controls(vec![fix, refactor], &at_caret, &mut I18n::default());
        assert_eq!(
            submenu_labels(&items[0]),
            Some(vec!["Edit inspection profile setting".to_owned(), "Disable inspection".to_owned()])
        );
        assert_eq!(items[1].submenu, None);
    }

    #[test]
    fn offers_an_options_row_when_the_inspection_has_no_fix() {
        let mut i18n = I18n::default();
        let items = with_inspection_controls(Vec::new(), &inspections(vec![unused(Severity::Warning)]), &mut i18n);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Inspection 'ruff F401' options"]);
        assert_eq!(items[0].submenu.as_ref().map(Vec::len), Some(2));
        assert!(with_inspection_controls(Vec::new(), &inspections(Vec::new()), &mut i18n).is_empty());
    }

    #[test]
    fn uses_the_effective_severity_for_local_and_lsp_fixes() {
        for severity in [Severity::Info, Severity::Warning, Severity::Error] {
            let doc = Rope::from_str("import os\n");
            let lint = to_lint_diagnostics(
                &doc,
                &[crate::platform_more::inspections::StyledDiagnostic { diagnostic: unused(severity), typo: false }],
            );
            let icon = quick_fix_icon(&lint, 0, 9);
            let items = listed_intentions(
                vec![
                    IntentionItem { icon: Some("bulb"), ..local("refactor", "Extract method") },
                    IntentionItem { quick_fix: Some(true), ..local("local", "Local fix") },
                    IntentionItem { quick_fix: Some(true), emphasis: true, ..local("lsp", "LSP fix") },
                ],
                icon,
            );
            let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(ids, ["lsp", "local", "refactor"], "{severity:?}");
            let expected = if severity == Severity::Error { "bulb-fix" } else { "bulb" };
            assert!(items[..2].iter().all(|item| item.icon == Some(expected)), "{severity:?}");
            let controls = with_inspection_controls(items, &inspections(vec![unused(severity)]), &mut I18n::default());
            assert!(controls[..2].iter().all(|item| item.submenu.as_ref().map(Vec::len) == Some(2)), "{severity:?}");
            assert_eq!(controls[2].submenu, None, "{severity:?}");
        }
    }

    #[test]
    fn does_not_run_lazy_plugin_actions_that_could_prompt_during_actions_on_save() {
        let lazy = CodeAction { kind: Some("source.fixAll".into()), server: ServerId(0), edit: None, ..resolved() };
        assert_eq!(silent_action(std::slice::from_ref(&lazy), "source.fixAll").map(|a| a.title.clone()), None);
    }

    #[test]
    fn does_not_resolve_lazy_plugin_actions_while_previewing() {
        let doc = Rope::from_str("f(1);");
        let mut cache = ResolveCache::default();
        let lazy = CodeAction { server: ServerId(0), edit: None, ..resolved() };
        let no_preview = PreviewStep::Show(ListPreview::Text(NO_PREVIEW.into()));
        assert_eq!(cache.preview(&0, &lazy, "/p/a.ts", &doc), no_preview);
        assert!(cache.is_empty());
        let with_edit = CodeAction { server: ServerId(0), ..resolved() };
        assert!(matches!(cache.preview(&1, &with_edit, "/p/a.ts", &doc), PreviewStep::Show(ListPreview::Diff { .. })));
    }

    #[test]
    fn resolves_the_action_once_and_shows_its_diff_for_this_file() {
        let doc = Rope::from_str("f(1);");
        let mut cache = ResolveCache::default();
        let lazy = CodeAction { edit: None, ..resolved() };
        assert_eq!(cache.preview(&0, &lazy, "/p/a.ts", &doc), PreviewStep::Resolve);
        assert_eq!(cache.preview(&0, &lazy, "/p/a.ts", &doc), PreviewStep::Waiting);
        cache.resolved(&0, Ok(resolved()));
        let expected = PreviewStep::Show(ListPreview::Diff {
            lines: vec![DiffLine::Added("const x = 1;".into()), DiffLine::Context("f(1);".into())],
            note: Some("Also changes 1 other file".into()),
        });
        assert_eq!(cache.preview(&0, &lazy, "/p/a.ts", &doc), expected);
        assert_eq!(cache.preview(&0, &lazy, "/p/a.ts", &doc), expected);
    }

    #[test]
    fn lists_one_action_per_title_quick_fixes_first() {
        let list = listed_code_actions(&[
            action("Generate toString()", Some("quickassist"), false, None),
            action("Add unimplemented methods", Some("quickfix"), false, None),
            action("Generate toString()", Some("source.generate.toString"), false, None),
        ]);
        let pairs: Vec<(&str, Option<&str>)> = list.iter().map(|a| (a.title.as_str(), a.kind.as_deref())).collect();
        assert_eq!(
            pairs,
            [("Add unimplemented methods", Some("quickfix")), ("Generate toString()", Some("quickassist"))]
        );
    }

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    #[test]
    fn spans_the_lines_an_edit_inserted() {
        assert_eq!(changed_lines(&rope("class A {\n}\n"), &rope("class A {\n  f() {\n  }\n}\n")), [(1, 2)]);
    }

    #[test]
    fn keeps_separate_edits_apart() {
        let before = rope("package p;\n\nclass A {\n  int a;\n  int b;\n  void f() {\n  }\n}\n");
        let after = rope(
            "package p;\n\nimport java.util.List;\n\nclass A {\n  int a;\n  int b;\n  void f() {\n    List<String> x = \
             null;\n    x.size();\n  }\n}\n",
        );
        assert_eq!(changed_lines(&before, &after), [(2, 3), (8, 9)]);
    }

    #[test]
    fn is_empty_for_equal_texts() {
        assert!(changed_lines(&rope("a\nb"), &rope("a\nb")).is_empty());
    }

    #[test]
    fn prefers_an_indented_caret_line_then_the_lines_around_it() {
        assert_eq!(bulb_line(&rope("    x\ny"), 0), (0, false));
        assert_eq!(bulb_line(&rope("x\n\ny"), 0), (1, false));
    }

    #[test]
    fn falls_back_to_the_gutter_when_no_nearby_line_has_room() {
        assert_eq!(bulb_line(&rope("x\n  y\nz"), 0), (0, true));
    }

    #[test]
    fn lights_for_fixes_and_refactorings_only() {
        let lights = |kind: Option<&str>| lights_bulb(&CodeAction { kind: kind.map(str::to_owned), ..resolved() });
        assert!([None, Some("quickfix"), Some("refactor.extract"), Some("quickassist")].into_iter().all(lights));
        assert!(!["source.organizeImports", "source.doc", "gopls.doc.features"].into_iter().any(|k| lights(Some(k))));
    }

    #[test]
    fn moves_code_generated_at_the_braces_column_one_level_in() {
        let doc = rope("class A : B {\noverride fun f() {\n    g()\n}\n}");
        let lines: Vec<(u32, String)> = indent_under_brace(&doc, 1, 3, "    ")
            .into_iter()
            .map(|change| (buffer::line_col(&doc, change.from).0, change.insert))
            .collect();
        assert_eq!(lines, [(1, "    ".to_owned()), (2, "    ".to_owned()), (3, "    ".to_owned())]);
    }

    #[test]
    fn leaves_code_already_indented_or_not_under_a_brace() {
        assert!(indent_under_brace(&rope("class A {\n    f()\n}"), 1, 1, "    ").is_empty());
        assert!(indent_under_brace(&rope("class A:\npass\nx"), 1, 2, "    ").is_empty());
    }
}
