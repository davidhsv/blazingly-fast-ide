//! Diagnostics in the editor. The backend publishes full replacements per file and producer; each
//! editor shows the union for its path, under the inspection profile in effect, as underlined
//! ranges and marks in the scrollbar. F2 / ⇧F2 walk the problems, ⌘F1 describes the one at the
//! caret.

use super::hint::Tone;
use super::positions::range_to_offsets;
use crate::editor::buffer;
use crate::platform_more::inspections::{InspectionRef, StyledDiagnostic, apply_profile, diagnostic_inspection};
use crate::settings_model::inspections_model::ProfileOverrides;
use ide_core::{Diagnostic, Severity};
use ropey::Rope;
use std::collections::{HashMap, HashSet};

/// A replacement the backend published for one producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPublication {
    pub source: String,
    pub items: Vec<Diagnostic>,
}

/// The diagnostics of a path changed; `publication` is set for a fresh backend publication (an
/// empty replacement included), not for the IDE's own inspections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsChanged {
    pub path: String,
    pub publication: Option<DiagnosticPublication>,
}

/// The diagnostics of every file: one list per producer, in the order producers first reported.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticStore {
    by_path: HashMap<String, Vec<(String, Vec<Diagnostic>)>>,
}

impl DiagnosticStore {
    /// A backend publication for `path`.
    pub fn published(&mut self, path: &str, source: &str, items: Vec<Diagnostic>) -> Option<DiagnosticsChanged> {
        self.store(path, source, items, true)
    }

    /// Diagnostics an IDE-side inspection computed for `path`. Returns the change, and whether to
    /// publish them for the Problems view too (they exist now or did before).
    pub fn set_file_diagnostics(
        &mut self,
        path: &str,
        source: &str,
        items: Vec<Diagnostic>,
    ) -> (Option<DiagnosticsChanged>, bool) {
        let had = self.by_path.get(path).is_some_and(|sources| sources.iter().any(|(own, _)| own == source));
        let publish = !items.is_empty() || had;
        (self.store(path, source, items, false), publish)
    }

    fn store(
        &mut self,
        path: &str,
        source: &str,
        items: Vec<Diagnostic>,
        published: bool,
    ) -> Option<DiagnosticsChanged> {
        let publication = published.then(|| DiagnosticPublication { source: source.to_owned(), items: items.clone() });
        if items.is_empty() {
            let removed = self.by_path.get_mut(path).is_some_and(|sources| {
                let before = sources.len();
                sources.retain(|(own, _)| own != source);
                sources.len() != before
            });
            if !removed && !published {
                return None;
            }
            if self.by_path.get(path).is_some_and(Vec::is_empty) {
                self.by_path.remove(path);
            }
        } else {
            let items: Vec<Diagnostic> = items
                .into_iter()
                .map(|item| Diagnostic { source: item.source.clone().or_else(|| Some(source.to_owned())), ..item })
                .collect();
            let sources = self.by_path.entry(path.to_owned()).or_default();
            match sources.iter_mut().find(|(own, _)| own == source) {
                Some((_, own)) => *own = items,
                None => sources.push((source.to_owned(), items)),
            }
        }
        Some(DiagnosticsChanged { path: path.to_owned(), publication })
    }

    /// The producers of `path`, in order.
    pub fn sources(&self, path: &str) -> Vec<&str> {
        self.by_path.get(path).map(|sources| sources.iter().map(|(own, _)| own.as_str()).collect()).unwrap_or_default()
    }

    /// The diagnostics of `path` under `profile`.
    pub fn diagnostics_for(&self, path: &str, profile: &ProfileOverrides) -> Vec<StyledDiagnostic> {
        let Some(sources) = self.by_path.get(path) else { return Vec::new() };
        sources
            .iter()
            .flat_map(|(source, items)| {
                apply_profile(profile, source, items).unwrap_or_else(|| {
                    items.iter().map(|item| StyledDiagnostic { diagnostic: item.clone(), typo: false }).collect()
                })
            })
            .collect()
    }

    /// The inspections reporting a problem at `pos` of `path` (profile applied), most severe first.
    pub fn inspections_at(&self, path: &str, doc: &Rope, pos: u32, profile: &ProfileOverrides) -> Vec<InspectionRef> {
        let Some(sources) = self.by_path.get(path) else { return Vec::new() };
        let mut found: Vec<(InspectionRef, u8)> = Vec::new();
        for (producer, items) in sources {
            let shown = apply_profile(profile, producer, items)
                .map(|styled| styled.into_iter().map(|styled| styled.diagnostic).collect())
                .unwrap_or_else(|| items.clone());
            for item in shown {
                let range = range_to_offsets(doc, item.range);
                if pos < range.start || pos > range.end {
                    continue;
                }
                if let Some(inspection) = diagnostic_inspection(producer, item.code.as_deref())
                    && !found.iter().any(|(known, _)| known.id == inspection.id)
                {
                    found.push((inspection, severity_rank(item.severity)));
                }
            }
        }
        found.sort_by_key(|(_, rank)| *rank);
        found.into_iter().map(|(inspection, _)| inspection).collect()
    }
}

pub fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

/// How a diagnostic shows in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintDiagnostic {
    pub from: u32,
    pub to: u32,
    pub severity: Severity,
    pub message: String,
    /// The producer and code, as the hover shows them.
    pub source: Option<String>,
    /// `cm-lsp-unnecessary`, `cm-lsp-deprecated`, `cm-lsp-typo`, space-separated.
    pub mark_class: Option<String>,
}

/// Problem ids that mean nothing to a reader (`67109264`); short ones (`2322`) stay.
fn is_internal_code(code: &str) -> bool {
    code.len() >= 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// The diagnostics in `doc`, empty ranges widened to one character, in order.
pub fn to_lint_diagnostics(doc: &Rope, items: &[StyledDiagnostic]) -> Vec<LintDiagnostic> {
    let mut out: Vec<LintDiagnostic> = items
        .iter()
        .map(|styled| {
            let item = &styled.diagnostic;
            let mut range = range_to_offsets(doc, item.range);
            if range.start == range.end {
                let line = buffer::line_range(doc, buffer::line_col(doc, range.start).0);
                if range.end < line.end {
                    range.end += 1;
                } else if range.start > line.start {
                    range.start -= 1;
                }
            }
            let classes: Vec<&str> = [
                (item.unnecessary, "cm-lsp-unnecessary"),
                (item.deprecated, "cm-lsp-deprecated"),
                (styled.typo, "cm-lsp-typo"),
            ]
            .into_iter()
            .filter_map(|(on, class)| on.then_some(class))
            .collect();
            let code = item.code.as_deref().filter(|code| !code.is_empty() && !is_internal_code(code));
            let source: Vec<&str> =
                [item.source.as_deref().filter(|source| !source.is_empty()), code].into_iter().flatten().collect();
            LintDiagnostic {
                from: range.start,
                to: range.end,
                severity: item.severity,
                message: item.message.clone(),
                source: (!source.is_empty()).then(|| source.join(" ")),
                mark_class: (!classes.is_empty()).then(|| classes.join(" ")),
            }
        })
        .collect();
    out.sort_by_key(|lint| (lint.from, severity_rank(lint.severity)));
    out
}

/// A mark in the scrollbar's overview lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticMark {
    pub from: u32,
    pub to: u32,
    pub severity: Severity,
    pub tooltip: String,
    /// Errors draw over warnings over infos.
    pub priority: u8,
}

/// The scrollbar marks of diagnostics; unused code, typos and hints get none.
pub fn diagnostic_marks(lint: &[LintDiagnostic]) -> Vec<DiagnosticMark> {
    lint.iter()
        .filter(|d| d.severity != Severity::Hint)
        .filter(|d| {
            let class = d.mark_class.as_deref().unwrap_or_default();
            !class.contains("cm-lsp-unnecessary") && !class.contains("cm-lsp-typo")
        })
        .map(|d| DiagnosticMark {
            from: d.from,
            to: d.to,
            severity: d.severity,
            tooltip: d.message.clone(),
            priority: 3 - severity_rank(d.severity),
        })
        .collect()
}

/// The diagnostics touching `pos`, most severe first.
pub fn diagnostics_at(lint: &[LintDiagnostic], pos: u32) -> Vec<&LintDiagnostic> {
    let mut out: Vec<&LintDiagnostic> = lint.iter().filter(|d| d.from <= pos && pos <= d.to).collect();
    out.sort_by_key(|d| severity_rank(d.severity));
    out
}

fn tone_of(severity: Severity) -> Tone {
    match severity {
        Severity::Error => Tone::Error,
        Severity::Warning => Tone::Warning,
        Severity::Info | Severity::Hint => Tone::Info,
    }
}

pub const NO_PROBLEMS_IN_FILE: &str = "No problems in this file";
pub const NO_PROBLEMS_AT_CARET: &str = "No problems at the caret";
pub const GOTO_HINT_TIMEOUT_MS: u64 = 6000;
pub const DESCRIPTION_HINT_TIMEOUT_MS: u64 = 10_000;

/// Where F2 / ⇧F2 goes: the caret moves to `offset`, and the message shows there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemJump {
    pub offset: u32,
    pub message: String,
    pub tone: Tone,
}

/// The next problem after (`forward`) or before the caret at `head`, wrapping. With
/// `errors_only` (Next error action goes to high priority problems only) only the most severe
/// level the file has is visited, infos and hints counting as one level. `None` without problems:
/// show [`NO_PROBLEMS_IN_FILE`].
pub fn goto_diagnostic(lint: &[LintDiagnostic], head: u32, forward: bool, errors_only: bool) -> Option<ProblemJump> {
    let level = |d: &LintDiagnostic| severity_rank(d.severity).min(2);
    let best = lint.iter().map(level).min()?;
    let mut list: Vec<&LintDiagnostic> = lint.iter().filter(|d| !errors_only || level(d) == best).collect();
    list.sort_by_key(|d| d.from);
    let target = if forward {
        list.iter().find(|d| d.from > head).or(list.first())
    } else {
        list.iter().rev().find(|d| d.from < head && head > d.to).or(list.last())
    }?;
    Some(ProblemJump { offset: target.from, message: target.message.clone(), tone: tone_of(target.severity) })
}

/// ⌘F1 Show Error Description: the messages of the problems at the caret, worst first, and the
/// tone of the worst. `None`: show [`NO_PROBLEMS_AT_CARET`].
pub fn error_description(lint: &[LintDiagnostic], head: u32) -> Option<(String, Tone)> {
    let at = diagnostics_at(lint, head);
    let worst = at.first()?.severity;
    let mut seen = HashSet::new();
    let messages: Vec<&str> = at.iter().map(|d| d.message.as_str()).filter(|message| seen.insert(*message)).collect();
    Some((messages.join("\n"), tone_of(worst)))
}

/// One editor's copy of the diagnostics. A hidden editor skips updates and catches up when it is
/// shown again.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSync {
    skipped: bool,
}

impl DiagnosticsSync {
    /// The diagnostics of the editor's path changed: whether to recompute them now.
    pub fn changed(&mut self, visible: bool) -> bool {
        self.skipped = !visible;
        visible
    }

    /// The editor was shown again: whether it missed an update.
    pub fn shown(&mut self) -> bool {
        std::mem::take(&mut self.skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Pos, Range};

    fn d(line: u32, col: u32, end_col: u32, severity: Severity) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(line, col), Pos::new(line, end_col)),
            severity,
            message: format!("m{line}:{col}"),
            source: None,
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn styled(items: Vec<Diagnostic>) -> Vec<StyledDiagnostic> {
        items.into_iter().map(|diagnostic| StyledDiagnostic { diagnostic, typo: false }).collect()
    }

    #[test]
    fn identifies_fresh_backend_publications_including_empty_replacements_separately_from_local_inspections() {
        let mut store = DiagnosticStore::default();
        let items = vec![d(0, 0, 4, Severity::Error)];
        let (local, _) = store.set_file_diagnostics("/p/paste.java", "local", items.clone());
        assert_eq!(local.map(|change| change.publication), Some(None));
        let fresh = store.published("/p/paste.java", "jdtls", items.clone()).and_then(|change| change.publication);
        assert_eq!(fresh, Some(DiagnosticPublication { source: "jdtls".into(), items }));
        let empty = store.published("/p/paste.java", "other-server", Vec::new()).and_then(|change| change.publication);
        assert_eq!(empty, Some(DiagnosticPublication { source: "other-server".into(), items: Vec::new() }));
    }

    #[test]
    fn keeps_one_list_per_source_and_notifies_subscribers_of_the_path() {
        let mut store = DiagnosticStore::default();
        let mut calls = 0;
        let mut set = |store: &mut DiagnosticStore, path: &str, source: &str, items: Vec<Diagnostic>| {
            let (change, _) = store.set_file_diagnostics(path, source, items);
            if change.is_some_and(|change| change.path == "/p/x.rs") {
                calls += 1;
            }
        };
        set(&mut store, "/p/x.rs", "rust-analyzer", vec![d(0, 0, 1, Severity::Error)]);
        set(&mut store, "/p/x.rs", "cargo", vec![d(1, 0, 1, Severity::Warning)]);
        let profile = ProfileOverrides::default();
        let sources = |store: &DiagnosticStore| -> Vec<Option<String>> {
            store.diagnostics_for("/p/x.rs", &profile).into_iter().map(|styled| styled.diagnostic.source).collect()
        };
        assert_eq!(sources(&store), [Some("rust-analyzer".to_owned()), Some("cargo".to_owned())]);
        set(&mut store, "/p/x.rs", "rust-analyzer", Vec::new());
        assert_eq!(sources(&store), [Some("cargo".to_owned())]);
        set(&mut store, "/p/other.rs", "cargo", vec![d(0, 0, 1, Severity::Error)]);
        // Clearing an unknown source is a no-op.
        set(&mut store, "/p/x.rs", "unknown", Vec::new());
        assert_eq!(calls, 3);
    }

    fn lint_doc() -> Rope {
        Rope::from_str("fn main() {\n  let x = 1;\n}")
    }

    #[test]
    fn converts_ranges_sorts_and_adds_mark_classes() {
        let unused = Diagnostic {
            unnecessary: true,
            source: Some("rustc".into()),
            code: Some("unused".into()),
            ..d(1, 6, 7, Severity::Warning)
        };
        let deprecated = Diagnostic { deprecated: true, ..d(0, 3, 7, Severity::Error) };
        assert_eq!(
            to_lint_diagnostics(&lint_doc(), &styled(vec![unused, deprecated])),
            [
                LintDiagnostic {
                    from: 3,
                    to: 7,
                    severity: Severity::Error,
                    message: "m0:3".into(),
                    source: None,
                    mark_class: Some("cm-lsp-deprecated".into()),
                },
                LintDiagnostic {
                    from: 18,
                    to: 19,
                    severity: Severity::Warning,
                    message: "m1:6".into(),
                    source: Some("rustc unused".into()),
                    mark_class: Some("cm-lsp-unnecessary".into()),
                },
            ]
        );
    }

    #[test]
    fn widens_empty_ranges_to_one_character() {
        let doc = lint_doc();
        let lint = to_lint_diagnostics(&doc, &styled(vec![d(0, 2, 2, Severity::Error)]));
        assert_eq!((lint[0].from, lint[0].to), (2, 3));
        // At a line end the previous character is used.
        let lint = to_lint_diagnostics(&doc, &styled(vec![d(2, 1, 1, Severity::Error)]));
        let len = buffer::len_utf16(&doc);
        assert_eq!((lint[0].from, lint[0].to), (len - 1, len));
    }

    #[test]
    fn skips_a_hidden_view_and_catches_up_when_it_is_shown_again() {
        let mut store = DiagnosticStore::default();
        let mut sync = DiagnosticsSync::default();
        let doc = Rope::from_str("let x = 1;");
        let mut shown: Vec<LintDiagnostic> = Vec::new();
        let profile = ProfileOverrides::default();
        let change = store.published("/p/hidden.rs", "rust-analyzer", vec![d(0, 4, 5, Severity::Error)]);
        if change.is_some() && sync.changed(false) {
            shown = to_lint_diagnostics(&doc, &store.diagnostics_for("/p/hidden.rs", &profile));
        }
        assert!(diagnostics_at(&shown, 4).is_empty());
        if sync.shown() {
            shown = to_lint_diagnostics(&doc, &store.diagnostics_for("/p/hidden.rs", &profile));
        }
        assert_eq!(diagnostics_at(&shown, 4).len(), 1);
    }
}
