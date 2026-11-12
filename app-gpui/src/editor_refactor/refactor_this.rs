//! Refactor This (⌃T) and the Refactor menu actions. One cancellable `refactor*` code-action
//! query per invocation; server actions map to IDEA's refactorings ([`super::catalog`]), and the
//! refactorings done without a server action (Rename, Safe Delete, the syntax-based Introduce
//! Variable / Constant, the Java class-hierarchy ones) are offered when they apply. Menu items
//! always run, and say why with IDEA's "Cannot perform refactoring. <reason>" when nothing
//! applies.
//!
//! A newer invocation supersedes the one in flight (its request is returned to cancel), and an
//! answer that arrives after the text changed or the caret moved is dropped.

use super::catalog::{
    REFACTORINGS, Refactoring, RefactoringId, cannot_perform, classify_action, is_hierarchy_refactoring,
    kind_advertised, refactoring,
};
use super::run_kind::FallbackReason;
use super::server_introduce::{
    ChoiceRow, ExpressionChoice, OccurrencesChoice, expression_choice, introduces_name, is_expression_refactoring,
    occurrence_variants, occurrences_choice,
};
use crate::editor_commands::{Selection, SelectionExt};
use crate::editor_lsp::lsp::{Inflight, is_cancelled, is_unavailable};
use crate::editor_lsp::positions::{offset_to_pos, offsets_to_range};
use crate::editor_lsp::rename::Span;
use crate::platform::i18n::I18n;
use ide_core::{IdeError, Pos, Range, RequestId};
use ide_lsp::types::{CodeAction, CodeActionQuery, HierarchyRefactoring};
use ide_syntax::ExpressionAt;
use ropey::Rope;

/// The languages whose server runs the class-hierarchy refactorings (the Java extension).
const HIERARCHY_LANGUAGES: [&str; 1] = ["java"];

/// The server's answer to the refactoring query, or why there is none.
#[derive(Debug, Clone)]
pub enum ServerAnswer {
    Ok(CodeActionQuery),
    NotReady,
    Error(String),
}

impl ServerAnswer {
    /// The answer from the query's result; `None` when it was cancelled. Servers may ignore the
    /// kinds asked for, so only `refactor*` actions are kept.
    pub fn from_result(result: Result<CodeActionQuery, IdeError>) -> Option<Self> {
        match result {
            Ok(mut query) => {
                query.actions.retain(|action| action.kind.as_deref().is_some_and(|kind| kind.starts_with("refactor")));
                Some(Self::Ok(query))
            }
            Err(error) if is_cancelled(&error) => None,
            Err(error) if is_unavailable(&error) => Some(Self::NotReady),
            Err(error) => Some(Self::Error(error.message)),
        }
    }

    pub fn actions(&self) -> &[CodeAction] {
        match self {
            Self::Ok(query) => &query.actions,
            _ => &[],
        }
    }

    fn usable(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Why a syntax-based fallback runs for this answer.
    pub fn fallback_reason(&self) -> FallbackReason {
        match self {
            Self::Ok(_) => FallbackReason::Empty,
            Self::NotReady => FallbackReason::NotReady,
            Self::Error(_) => FallbackReason::Error,
        }
    }
}

/// The enabled server actions per IDEA refactoring, and the other refactorings (by title).
#[derive(Debug, Clone, Default)]
pub struct Grouped {
    pub mapped: Vec<(RefactoringId, Vec<CodeAction>)>,
    pub other: Vec<CodeAction>,
}

impl Grouped {
    pub fn get(&self, id: RefactoringId) -> &[CodeAction] {
        self.mapped.iter().find(|(own, _)| *own == id).map_or(&[], |(_, actions)| actions.as_slice())
    }
}

pub fn group_actions(actions: &[CodeAction]) -> Grouped {
    let mut grouped = Grouped::default();
    for action in actions.iter().filter(|action| action.disabled_reason.is_none()) {
        match classify_action(action.kind.as_deref(), &action.title) {
            None => grouped.other.push(action.clone()),
            Some(id) => match grouped.mapped.iter_mut().find(|(own, _)| *own == id) {
                Some((_, list)) => list.push(action.clone()),
                None => grouped.mapped.push((id, vec![action.clone()])),
            },
        }
    }
    grouped
}

fn short_label(refactoring: &Refactoring) -> String {
    refactoring.label.strip_suffix('…').unwrap_or(refactoring.label).to_owned()
}

/// IDEA's reason for `refactoring` being unavailable, given the server's answer (English).
/// `selection`: something is selected; `line_error`: the first error diagnostic on the selected
/// lines (servers offer nothing over broken code).
pub fn unavailable_reason(
    refactoring: &Refactoring,
    answer: &ServerAnswer,
    language_name: &str,
    selection: bool,
    line_error: Option<&str>,
) -> String {
    let query = match answer {
        ServerAnswer::NotReady => return format!("No language server for {language_name} is running"),
        ServerAnswer::Error(message) => return message.clone(),
        ServerAnswer::Ok(query) => query,
    };
    let disabled = query
        .actions
        .iter()
        .find(|action| {
            action.disabled_reason.is_some()
                && classify_action(action.kind.as_deref(), &action.title) == Some(refactoring.id)
        })
        .and_then(|action| action.disabled_reason.clone());
    if let Some(disabled) = disabled {
        return disabled;
    }
    if let Some(kind) = refactoring.kind
        && !kind_advertised(query.advertised_kinds.as_deref(), kind)
    {
        return format!("The language server for {language_name} doesn't support {}", short_label(refactoring));
    }
    let reason = refactoring.reason(selection);
    match line_error {
        Some(error) => format!("{reason} (fix the error first: {error})"),
        None => reason.to_owned(),
    }
}

/// The two refactorings the syntax-only Introduce does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntroduceKind {
    Variable,
    Constant,
}

pub fn introduce_kind(id: RefactoringId) -> Option<IntroduceKind> {
    match id {
        RefactoringId::ExtractVariable => Some(IntroduceKind::Variable),
        RefactoringId::ExtractConstant => Some(IntroduceKind::Constant),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Running server actions

/// What running a refactoring's server actions comes to.
#[derive(Debug, Clone)]
pub enum ActionsStep {
    /// "Multiple occurrences found": the first row applies `one`, the second `all`.
    Occurrences { choice: OccurrencesChoice, one: Box<CodeAction>, all: Box<CodeAction> },
    /// Apply the action; `introduces` then renames the introduced name in place
    /// ([`super::server_introduce::introduced_name`]).
    Apply { action: Box<CodeAction>, introduces: bool },
    /// Offer the choice, each row applying its action (as [`ActionsStep::Apply`] would).
    Choose { title: String, actions: Vec<CodeAction>, introduces: bool },
}

/// Applies the single action, or lets the user pick one; expression refactorings with both
/// occurrence variants get IDEA's occurrences choice. `actions` is not empty.
pub fn actions_step(
    i18n: &mut I18n,
    doc: &Rope,
    selection: Selection,
    title: &str,
    actions: Vec<CodeAction>,
    id: Option<RefactoringId>,
) -> ActionsStep {
    if id.is_some_and(is_expression_refactoring)
        && let Some(variants) = occurrence_variants(&actions)
    {
        let selected = Span { from: selection.start(), to: selection.end() };
        let (one, all) = (Box::new(variants.one.clone()), Box::new(variants.all.clone()));
        return ActionsStep::Occurrences { choice: occurrences_choice(i18n, doc, selected), one, all };
    }
    let introduces = introduces_name(id);
    match <[CodeAction; 1]>::try_from(actions) {
        Ok([action]) => ActionsStep::Apply { action: Box::new(action), introduces },
        Err(actions) => ActionsStep::Choose { title: title.to_owned(), actions, introduces },
    }
}

// ---------------------------------------------------------------------------
// Refactor This

/// What a row of the Refactor This list runs.
#[derive(Debug, Clone)]
pub enum RefactorRun {
    /// The Refactor menu action of the refactoring ([`RefactorMenu::start`]).
    Menu(RefactoringId),
    /// These server actions ([`actions_step`]).
    Actions {
        title: String,
        actions: Vec<CodeAction>,
        id: Option<RefactoringId>,
    },
    Rename,
    SafeDelete,
    Hierarchy(HierarchyRefactoring),
    /// The syntax-based Introduce Variable / Constant.
    IntroduceFallback {
        kind: IntroduceKind,
        reason: FallbackReason,
    },
    /// One of the server's other refactorings.
    Apply(Box<CodeAction>),
}

/// A row of the Refactor This list.
#[derive(Debug, Clone)]
pub struct RefactorItem {
    /// English source text.
    pub label: String,
    /// The action whose shortcut the row shows.
    pub shortcut_action: Option<&'static str>,
    pub icon: Option<&'static str>,
    pub emphasis: bool,
    pub run: RefactorRun,
}

/// What is known at the caret besides the server's answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    /// Rename has a symbol to act on.
    pub symbol: bool,
    /// The structure service finds an expression at the selection.
    pub expression: bool,
    /// The class-hierarchy refactorings exist for the file.
    pub hierarchy: bool,
    pub read_only: bool,
    /// Nothing is selected.
    pub caret_only: bool,
}

/// The rows of Refactor This, in IDEA's order: the applicable IDEA refactorings, then the
/// server's others.
pub fn refactor_this_items(answer: &ServerAnswer, facts: &Facts) -> Vec<RefactorItem> {
    let grouped = group_actions(answer.actions());
    let usable = answer.usable();
    let mut items = Vec::new();
    for r in &REFACTORINGS {
        let actions = grouped.get(r.id);
        let run = if is_expression_refactoring(r.id)
            && facts.caret_only
            && usable
            && (!actions.is_empty() || facts.expression)
        {
            // With only a caret the expression is chosen first, then the server asked for it.
            RefactorRun::Menu(r.id)
        } else if !actions.is_empty() {
            RefactorRun::Actions { title: short_label(r), actions: actions.to_vec(), id: Some(r.id) }
        } else if r.id == RefactoringId::Rename && facts.symbol {
            RefactorRun::Rename
        } else if r.id == RefactoringId::SafeDelete && facts.symbol && !facts.read_only {
            RefactorRun::SafeDelete
        } else if is_hierarchy_refactoring(r.id) && facts.hierarchy && usable {
            RefactorRun::Hierarchy(if r.id == RefactoringId::PullMembersUp {
                HierarchyRefactoring::PullUp
            } else {
                HierarchyRefactoring::PushDown
            })
        } else if let Some(kind) = introduce_kind(r.id).filter(|_| facts.expression && !usable) {
            RefactorRun::IntroduceFallback { kind, reason: answer.fallback_reason() }
        } else {
            continue;
        };
        items.push(RefactorItem {
            label: r.label.to_owned(),
            shortcut_action: Some(r.action),
            icon: None,
            emphasis: false,
            run,
        });
    }
    items.extend(grouped.other.into_iter().map(|action| RefactorItem {
        label: action.title.clone(),
        shortcut_action: None,
        icon: Some("bulb"),
        emphasis: action.is_preferred,
        run: RefactorRun::Apply(Box::new(action)),
    }));
    items
}

/// IDEA's "Nothing here" popup: Refactor This where no refactoring applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NothingHere {
    pub title: &'static str,
    pub label: &'static str,
    /// Why the only row is disabled.
    pub disabled: &'static str,
}

pub const NOTHING_HERE: NothingHere =
    NothingHere { title: "Refactor This", label: "Nothing here", disabled: "No refactoring applies here" };

/// A code-action query for the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefactorQuery {
    pub id: RequestId,
    pub range: Range,
    /// The kinds asked for.
    pub only: Vec<String>,
}

/// What Refactor This asks besides the query; the answer carries the ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactRequest {
    /// `lsp_prepare_rename` at `pos`.
    PrepareRename { ticket: u64, pos: Pos },
    /// Whether the syntax fallback can rename the local at `pos`.
    LocalRename { ticket: u64, pos: Pos },
    /// The structure service's expression at the selection.
    ExpressionAt { ticket: u64, range: Range },
}

#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one per server answer, consumed at once")]
pub enum FactAnswer {
    PrepareRename(Result<Option<Range>, IdeError>),
    LocalRename(Result<bool, IdeError>),
    ExpressionAt(Result<ExpressionAt, IdeError>),
}

#[derive(Debug, Clone)]
pub enum RefactorThisStep {
    /// Nothing (stale, cancelled, or waiting for more answers).
    Nothing,
    Requests(Vec<FactRequest>),
    /// The numbered list (1-9 run a row directly).
    Popup {
        title: &'static str,
        items: Vec<RefactorItem>,
        numbered: bool,
    },
    NothingHere(NothingHere),
}

#[derive(Debug, Clone)]
struct Running {
    version: u64,
    selection: Selection,
    pos: Pos,
    range: Range,
    facts: Facts,
    answer: Option<ServerAnswer>,
    symbol: Option<bool>,
    expression: Option<bool>,
}

/// Refactor This in one editor: at most one runs.
#[derive(Debug, Clone, Default)]
pub struct RefactorThis {
    inflight: Inflight,
    ticket: u64,
    running: Option<Running>,
}

/// What starting an invocation sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The superseded request, to cancel in the backend.
    pub cancel: Option<RequestId>,
    pub query: RefactorQuery,
}

impl RefactorThis {
    /// ⌃T at `selection` of `doc` (version `version`), once the editor's edits reached the
    /// backend.
    pub fn start(
        &mut self,
        id: RequestId,
        doc: &Rope,
        version: u64,
        selection: Selection,
        language: &str,
        read_only: bool,
    ) -> Started {
        let cancel = self.inflight.begin(id);
        self.ticket += 1;
        let range = offsets_to_range(doc, selection.start(), selection.end());
        self.running = Some(Running {
            version,
            selection,
            pos: offset_to_pos(doc, selection.head()),
            range,
            facts: Facts {
                hierarchy: HIERARCHY_LANGUAGES.contains(&language) && !read_only,
                read_only,
                caret_only: selection.is_caret(),
                ..Facts::default()
            },
            answer: None,
            symbol: None,
            expression: None,
        });
        Started { cancel, query: RefactorQuery { id, range, only: vec!["refactor".to_owned()] } }
    }

    fn stale(&self, version: u64, selection: Selection) -> bool {
        self.running.as_ref().is_none_or(|running| running.version != version || running.selection != selection)
    }

    /// The query answered; `version` and `selection` are the editor's now.
    pub fn query_answered(
        &mut self,
        id: RequestId,
        result: Result<CodeActionQuery, IdeError>,
        version: u64,
        selection: Selection,
    ) -> RefactorThisStep {
        if !self.inflight.finish(id) {
            return RefactorThisStep::Nothing;
        }
        let Some(answer) = ServerAnswer::from_result(result) else { return self.end() };
        if self.stale(version, selection) {
            return self.end();
        }
        let ticket = self.ticket;
        let Some(running) = self.running.as_mut() else { return RefactorThisStep::Nothing };
        let symbol = if matches!(answer, ServerAnswer::NotReady) {
            FactRequest::LocalRename { ticket, pos: running.pos }
        } else {
            FactRequest::PrepareRename { ticket, pos: running.pos }
        };
        let expression = FactRequest::ExpressionAt { ticket, range: running.range };
        running.answer = Some(answer);
        RefactorThisStep::Requests(vec![symbol, expression])
    }

    fn end(&mut self) -> RefactorThisStep {
        self.running = None;
        RefactorThisStep::Nothing
    }

    /// A fact answered.
    pub fn fact_answered(
        &mut self,
        ticket: u64,
        answer: FactAnswer,
        version: u64,
        selection: Selection,
    ) -> RefactorThisStep {
        if ticket != self.ticket || self.running.as_ref().is_none_or(|running| running.answer.is_none()) {
            return RefactorThisStep::Nothing;
        }
        let cancelled = match &answer {
            FactAnswer::PrepareRename(Err(error)) | FactAnswer::LocalRename(Err(error)) => is_cancelled(error),
            FactAnswer::ExpressionAt(Err(error)) => is_cancelled(error),
            _ => false,
        };
        if cancelled {
            return self.end();
        }
        let Some(running) = self.running.as_mut() else { return RefactorThisStep::Nothing };
        match answer {
            FactAnswer::PrepareRename(Ok(Some(_))) => running.symbol = Some(true),
            FactAnswer::PrepareRename(_) => {
                return RefactorThisStep::Requests(vec![FactRequest::LocalRename { ticket, pos: running.pos }]);
            }
            FactAnswer::LocalRename(result) => running.symbol = Some(result.unwrap_or(false)),
            FactAnswer::ExpressionAt(result) => {
                running.expression = Some(result.is_ok_and(|at| at.expression.is_some()));
            }
        }
        let (Some(symbol), Some(expression)) = (running.symbol, running.expression) else {
            return RefactorThisStep::Nothing;
        };
        if self.stale(version, selection) {
            return self.end();
        }
        let Some(running) = self.running.take() else { return RefactorThisStep::Nothing };
        let facts = Facts { symbol, expression, ..running.facts };
        let items = running.answer.map(|answer| refactor_this_items(&answer, &facts)).unwrap_or_default();
        if items.is_empty() {
            return RefactorThisStep::NothingHere(NOTHING_HERE);
        }
        RefactorThisStep::Popup { title: "Refactor This", items, numbered: true }
    }
}

// ---------------------------------------------------------------------------
// Refactor menu actions

#[derive(Debug, Clone)]
pub enum MenuStep {
    Nothing,
    /// Show this hint at the caret (English source text).
    Hint(String),
    /// Ask the structure service for the expression at the selection (`range`), answering with
    /// [`RefactorMenu::expression_answered`]: IDEA picks the expression first.
    ExpressionAt {
        ticket: u64,
        range: Range,
    },
    /// Select the chosen expression (or ask which, see [`ExpressionChoice`]) and start the
    /// action again with that selection.
    ChooseExpression(ExpressionChoice),
    /// Send the query; answer with [`RefactorMenu::query_answered`].
    Query(RefactorQuery),
    /// Run the server's actions.
    RunActions(ActionsStep),
    /// Run the syntax-based Introduce; when it declines, show `otherwise`.
    IntroduceFallback {
        kind: IntroduceKind,
        reason: FallbackReason,
        otherwise: String,
    },
}

#[derive(Debug, Clone)]
struct MenuRun {
    id: RefactoringId,
    request: RequestId,
    version: u64,
    selection: Selection,
    range: Range,
    ticket: u64,
}

/// The Refactor menu actions of one editor (Move…, Inline…, Extract Method…): runs the
/// refactoring or says why it cannot.
#[derive(Debug, Clone, Default)]
pub struct RefactorMenu {
    inflight: Inflight,
    ticket: u64,
    running: Option<MenuRun>,
}

impl RefactorMenu {
    /// Starts refactoring `id` at `selection`; `request` is the id the query will have, `tree`
    /// whether the structure service parses the language. Returns the superseded request to
    /// cancel, and the step.
    #[expect(clippy::too_many_arguments, reason = "the editor's state the refactoring starts from")]
    pub fn start(
        &mut self,
        id: RefactoringId,
        request: RequestId,
        doc: &Rope,
        version: u64,
        selection: Selection,
        read_only: bool,
        tree: bool,
    ) -> (Option<RequestId>, MenuStep) {
        if read_only {
            return (None, MenuStep::Hint(cannot_perform("The file is read-only")));
        }
        let cancel = self.inflight.cancel();
        self.ticket += 1;
        let range = offsets_to_range(doc, selection.start(), selection.end());
        self.running = Some(MenuRun { id, request, version, selection, range, ticket: self.ticket });
        if is_expression_refactoring(id) && selection.is_caret() && tree {
            return (cancel, MenuStep::ExpressionAt { ticket: self.ticket, range });
        }
        (cancel, self.query())
    }

    fn query(&mut self) -> MenuStep {
        let Some(run) = self.running.as_ref() else { return MenuStep::Nothing };
        let only =
            refactoring(run.id).kind.map(|kind| vec![kind.to_owned()]).unwrap_or_else(|| vec!["refactor".to_owned()]);
        let query = RefactorQuery { id: run.request, range: run.range, only };
        self.inflight.begin(run.request);
        MenuStep::Query(query)
    }

    /// The expression at the caret: one or several to choose from, or the query when there is
    /// none (or the structure service failed).
    pub fn expression_answered(
        &mut self,
        ticket: u64,
        result: Result<ExpressionAt, IdeError>,
        doc: &Rope,
        version: u64,
        selection: Selection,
    ) -> MenuStep {
        let Some(run) = self.running.as_ref().filter(|run| run.ticket == ticket) else { return MenuStep::Nothing };
        let (run_version, run_selection) = (run.version, run.selection);
        match result {
            Err(error) if is_cancelled(&error) => {
                self.running = None;
                MenuStep::Nothing
            }
            Err(_) => self.query(),
            Ok(_) if run_version != version || run_selection != selection => {
                self.running = None;
                MenuStep::Nothing
            }
            Ok(at) => match expression_choice(doc, &at) {
                ExpressionChoice::None => self.query(),
                choice => {
                    self.running = None;
                    MenuStep::ChooseExpression(choice)
                }
            },
        }
    }

    /// The query answered. `language_name` names the language in reasons, `line_error` is the
    /// first error diagnostic on the selected lines.
    #[expect(clippy::too_many_arguments, reason = "the editor's state the answer is judged against")]
    pub fn query_answered(
        &mut self,
        id: RequestId,
        result: Result<CodeActionQuery, IdeError>,
        i18n: &mut I18n,
        doc: &Rope,
        version: u64,
        selection: Selection,
        language_name: &str,
        line_error: Option<&str>,
    ) -> MenuStep {
        if !self.inflight.finish(id) {
            return MenuStep::Nothing;
        }
        let Some(run) = self.running.take() else { return MenuStep::Nothing };
        let Some(answer) = ServerAnswer::from_result(result) else { return MenuStep::Nothing };
        if run.version != version || run.selection != selection {
            return MenuStep::Nothing;
        }
        let r = refactoring(run.id);
        let actions = group_actions(answer.actions()).get(run.id).to_vec();
        if !actions.is_empty() {
            return MenuStep::RunActions(actions_step(i18n, doc, selection, &short_label(r), actions, Some(run.id)));
        }
        let reason = cannot_perform(&unavailable_reason(r, &answer, language_name, !selection.is_caret(), line_error));
        match introduce_kind(run.id) {
            Some(kind) => MenuStep::IntroduceFallback { kind, reason: answer.fallback_reason(), otherwise: reason },
            None => MenuStep::Hint(reason),
        }
    }
}

/// The labels of a chooser's rows.
pub fn row_labels(rows: &[ChoiceRow]) -> Vec<&str> {
    rows.iter().map(|row| row.label.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::super::server_introduce::{introduced_name, introduced_rename};
    use super::super::testing::{code_action as action, offset_of, range_of, unavailable};
    use super::*;
    use crate::editor_commands::{apply_changes, caret, change};
    use ide_core::TextChange;
    use ide_syntax::SyntaxNode;

    const TEXT: &str = "fn main() { let total = price * 2; }";

    fn query(actions: Vec<CodeAction>, advertised: Option<&[&str]>) -> Result<CodeActionQuery, IdeError> {
        Ok(CodeActionQuery {
            actions,
            advertised_kinds: advertised.map(|kinds| kinds.iter().map(|kind| (*kind).to_owned()).collect()),
        })
    }

    fn at_price() -> Selection {
        caret(offset_of(TEXT, "price") + 1)
    }

    /// Runs Refactor This with the answers given; `symbol` is what renaming finds at the caret.
    fn refactor_this(
        language: &str,
        answer: Result<CodeActionQuery, IdeError>,
        symbol: bool,
        expression: bool,
    ) -> RefactorThisStep {
        let doc = Rope::from_str(TEXT);
        let mut flow = RefactorThis::default();
        let started = flow.start(RequestId(1), &doc, 1, at_price(), language, false);
        assert_eq!(started.query.only, ["refactor"]);
        let mut step = flow.query_answered(RequestId(1), answer, 1, at_price());
        loop {
            let RefactorThisStep::Requests(requests) = step else { return step };
            step = RefactorThisStep::Nothing;
            for request in requests {
                let (ticket, answer) = match request {
                    FactRequest::PrepareRename { ticket, .. } => {
                        (ticket, FactAnswer::PrepareRename(Ok(symbol.then(Range::default))))
                    }
                    FactRequest::LocalRename { ticket, .. } => (ticket, FactAnswer::LocalRename(Ok(false))),
                    FactRequest::ExpressionAt { ticket, .. } => {
                        let found = expression.then(|| node("call_expression", "price"));
                        (
                            ticket,
                            FactAnswer::ExpressionAt(Ok(ExpressionAt { expression: found, ..ExpressionAt::default() })),
                        )
                    }
                };
                let next = flow.fact_answered(ticket, answer, 1, at_price());
                if !matches!(next, RefactorThisStep::Nothing) {
                    step = next;
                }
            }
        }
    }

    fn labels(step: &RefactorThisStep) -> Vec<String> {
        match step {
            RefactorThisStep::Popup { items, .. } => items.iter().map(|item| item.label.clone()).collect(),
            RefactorThisStep::NothingHere(nothing) => vec![nothing.label.to_owned()],
            other => panic!("expected a popup, got {other:?}"),
        }
    }

    fn node(kind: &str, needle: &str) -> SyntaxNode {
        let from = offset_of(TEXT, needle);
        SyntaxNode {
            kind: kind.to_owned(),
            range: range_of(TEXT, from, from + needle.encode_utf16().count() as u32),
            field: None,
            named: true,
            error: false,
            missing: false,
            has_error: false,
        }
    }

    fn item<'a>(step: &'a RefactorThisStep, label: &str) -> &'a RefactorItem {
        let RefactorThisStep::Popup { items, .. } = step else { panic!("expected a popup") };
        items.iter().find(|item| item.label == label).expect("the row is listed")
    }

    #[test]
    fn lists_only_the_refactorings_applicable_at_the_caret_in_idea_order() {
        let answer = query(
            vec![
                action("Inline variable", "refactor.inline", None),
                action("Extract into variable", "refactor.extract", None),
                action("Convert to raw string", "refactor.rewrite", None),
                action("Extract into function", "refactor.extract", Some("Can't extract here")),
            ],
            None,
        );
        let step = refactor_this("rust", answer, true, false);
        assert_eq!(
            labels(&step),
            ["Rename…", "Safe Delete…", "Introduce Variable…", "Inline…", "Convert to raw string"]
        );
        assert!(matches!(step, RefactorThisStep::Popup { numbered: true, .. }));
    }

    #[test]
    fn offers_pull_members_up_push_members_down_in_java_files_after_idea_s_other_refactorings() {
        let convert = || query(vec![action("Convert to switch expression", "refactor.rewrite", None)], None);
        let step = refactor_this("java", convert(), false, false);
        assert_eq!(labels(&step), ["Pull Members Up…", "Push Members Down…", "Convert to switch expression"]);
        assert!(matches!(
            item(&step, "Push Members Down…").run,
            RefactorRun::Hierarchy(HierarchyRefactoring::PushDown)
        ));
        // Other languages have no server for them; nor does Java while its server is down.
        assert!(!labels(&refactor_this("rust", convert(), false, false)).contains(&"Pull Members Up…".to_owned()));
        assert_eq!(labels(&refactor_this("java", Err(unavailable()), false, false)), ["Nothing here"]);
    }

    #[test]
    fn runs_the_mapped_server_action_rename_and_safe_delete() {
        let step =
            refactor_this("rust", query(vec![action("Inline variable", "refactor.inline", None)], None), true, false);
        let RefactorRun::Actions { title, actions, id } = &item(&step, "Inline…").run else {
            panic!("expected actions")
        };
        let mut i18n = I18n::default();
        let doc = Rope::from_str(TEXT);
        let run = actions_step(&mut i18n, &doc, at_price(), title, actions.clone(), *id);
        assert!(matches!(run, ActionsStep::Apply { ref action, .. } if action.title == "Inline variable"));
        assert!(matches!(item(&step, "Rename…").run, RefactorRun::Rename));
        assert!(matches!(item(&step, "Safe Delete…").run, RefactorRun::SafeDelete));
    }

    #[test]
    fn offers_the_syntax_based_introduce_variable_constant_when_the_server_is_down_and_an_expression_is_there() {
        let step = refactor_this("rust", Err(unavailable()), false, true);
        assert_eq!(labels(&step), ["Introduce Variable…", "Introduce Constant…"]);
        assert!(matches!(
            item(&step, "Introduce Constant…").run,
            RefactorRun::IntroduceFallback { kind: IntroduceKind::Constant, reason: FallbackReason::NotReady }
        ));
    }

    #[test]
    fn says_so_when_nothing_applies() {
        let step = refactor_this("rust", query(Vec::new(), None), false, false);
        let RefactorThisStep::NothingHere(nothing) = step else { panic!("expected Nothing here") };
        assert_eq!(nothing.label, "Nothing here");
        assert!(!nothing.disabled.is_empty());
    }

    #[test]
    fn cancels_a_superseded_query_and_drops_answers_after_the_caret_moved() {
        let doc = Rope::from_str(TEXT);
        let mut flow = RefactorThis::default();
        flow.start(RequestId(1), &doc, 1, at_price(), "rust", false);
        let moved = caret(2);
        let started = flow.start(RequestId(2), &doc, 1, moved, "rust", false);
        assert_eq!(started.cancel, Some(RequestId(1)));
        let cancelled = Err(super::super::testing::cancelled());
        assert!(matches!(flow.query_answered(RequestId(1), cancelled, 1, moved), RefactorThisStep::Nothing));
        let inline = || query(vec![action("Inline variable", "refactor.inline", None)], None);
        let RefactorThisStep::Requests(requests) = flow.query_answered(RequestId(2), inline(), 1, moved) else {
            panic!("expected the fact requests")
        };
        let mut last = RefactorThisStep::Nothing;
        for request in requests {
            let (ticket, answer) = match request {
                FactRequest::PrepareRename { ticket, .. } => (ticket, FactAnswer::PrepareRename(Ok(None))),
                FactRequest::LocalRename { ticket, .. } => (ticket, FactAnswer::LocalRename(Ok(false))),
                FactRequest::ExpressionAt { ticket, .. } => {
                    (ticket, FactAnswer::ExpressionAt(Ok(ExpressionAt::default())))
                }
            };
            let step = flow.fact_answered(ticket, answer, 1, moved);
            if let RefactorThisStep::Requests(more) = &step {
                for request in more {
                    let FactRequest::LocalRename { ticket, .. } = request else { continue };
                    last = flow.fact_answered(*ticket, FactAnswer::LocalRename(Ok(false)), 1, moved);
                }
            } else if !matches!(step, RefactorThisStep::Nothing) {
                last = step;
            }
        }
        assert_eq!(labels(&last), ["Inline…"]);

        // An answer after the caret moved shows nothing.
        flow.start(RequestId(3), &doc, 1, moved, "rust", false);
        assert!(matches!(flow.query_answered(RequestId(3), inline(), 1, caret(5)), RefactorThisStep::Nothing));
    }

    // --- the Refactor menu -------------------------------------------------------------------

    fn menu(
        id: RefactoringId,
        answer: Result<CodeActionQuery, IdeError>,
        selection: Selection,
    ) -> (MenuStep, Option<RefactorQuery>) {
        let doc = Rope::from_str(TEXT);
        let mut i18n = I18n::default();
        let mut flow = RefactorMenu::default();
        let (_, mut step) = flow.start(id, RequestId(7), &doc, 1, selection, false, true);
        if let MenuStep::ExpressionAt { ticket, .. } = step {
            step = flow.expression_answered(ticket, Ok(ExpressionAt::default()), &doc, 1, selection);
        }
        let MenuStep::Query(query) = step else { return (step, None) };
        let step = flow.query_answered(query.id, answer, &mut i18n, &doc, 1, selection, "Rust", None);
        (step, Some(query))
    }

    fn menu_hint(step: &MenuStep) -> &str {
        let MenuStep::Hint(hint) = step else { panic!("expected a hint, got {step:?}") };
        hint
    }

    #[test]
    fn apply_the_single_matching_action_or_offer_a_choice() {
        let (step, sent) = menu(
            RefactoringId::Move,
            query(vec![action("Move to a new file", "refactor.move.newFile", None)], None),
            at_price(),
        );
        assert!(matches!(step, MenuStep::RunActions(ActionsStep::Apply { .. })));
        assert_eq!(sent.map(|query| query.only), Some(vec!["refactor.move".to_owned()]));
        let two = vec![
            action("Inline variable", "refactor.inline", None),
            action("Inline call", "refactor.inline.call", None),
        ];
        let (step, _) = menu(RefactoringId::Inline, query(two, None), at_price());
        let MenuStep::RunActions(ActionsStep::Choose { actions, .. }) = step else { panic!("expected a choice") };
        let titles: Vec<&str> = actions.iter().map(|action| action.title.as_str()).collect();
        assert_eq!(titles, ["Inline variable", "Inline call"]);
    }

    #[test]
    fn say_the_server_doesn_t_support_it_when_its_advertised_kinds_can_t_produce_it() {
        let advertised: &[&str] = &["quickfix", "refactor.extract", "refactor.inline"];
        let (step, _) = menu(RefactoringId::Move, query(Vec::new(), Some(advertised)), at_price());
        assert_eq!(menu_hint(&step), "Cannot perform refactoring. The language server for Rust doesn't support Move");
    }

    #[test]
    fn give_idea_s_caret_reason_the_server_s_disabled_reason_or_the_missing_server() {
        let (step, _) = menu(RefactoringId::Inline, query(Vec::new(), None), at_price());
        assert_eq!(
            menu_hint(&step),
            "Cannot perform refactoring. Caret should be positioned at the name of the element to be refactored"
        );
        let disabled = vec![action("Extract into function", "refactor.extract", Some("Selection is not a statement"))];
        let (step, _) = menu(RefactoringId::ExtractMethod, query(disabled, None), at_price());
        assert_eq!(menu_hint(&step), "Cannot perform refactoring. Selection is not a statement");
        let (step, _) = menu(RefactoringId::ChangeSignature, Err(unavailable()), at_price());
        assert_eq!(menu_hint(&step), "Cannot perform refactoring. No language server for Rust is running");
    }

    #[test]
    fn introduce_variable_falls_back_to_the_syntax_refactoring() {
        let (step, _) = menu(RefactoringId::ExtractVariable, Err(unavailable()), at_price());
        assert!(matches!(
            step,
            MenuStep::IntroduceFallback { kind: IntroduceKind::Variable, reason: FallbackReason::NotReady, .. }
        ));
    }

    #[test]
    fn never_fall_back_to_unrelated_code_actions() {
        let (step, sent) = menu(RefactoringId::ExtractField, query(Vec::new(), None), at_price());
        assert_eq!(sent.map(|query| query.only), Some(vec!["refactor.extract".to_owned()]));
        assert!(menu_hint(&step).starts_with("Cannot perform refactoring."));
    }

    #[test]
    fn with_only_a_caret_choose_the_expression_first_highlighted_while_choosing_and_ask_the_server_for_it() {
        let doc = Rope::from_str(TEXT);
        let mut flow = RefactorMenu::default();
        let (_, step) = flow.start(RefactoringId::ExtractVariable, RequestId(7), &doc, 1, at_price(), false, true);
        let MenuStep::ExpressionAt { ticket, .. } = step else { panic!("expected the expression request") };
        let candidates = vec![node("e", "price"), node("e", "price * 2")];
        let at = ExpressionAt { candidates: candidates.clone(), ..ExpressionAt::default() };
        let step = flow.expression_answered(ticket, Ok(at), &doc, 1, at_price());
        let MenuStep::ChooseExpression(ExpressionChoice::Choose { title, rows, spans }) = step else {
            panic!("expected the chooser")
        };
        assert_eq!(title, "Expressions");
        assert_eq!(row_labels(&rows), ["price", "price * 2"]);
        let whole = Span { from: offset_of(TEXT, "price"), to: offset_of(TEXT, "2") + 1 };
        assert_eq!(rows[1].highlights, [whole]);
        // Picking the row selects it and runs the action again: the server is asked for exactly it.
        let selected = (spans[1].from, spans[1].to);
        let (_, step) = flow.start(RefactoringId::ExtractVariable, RequestId(8), &doc, 1, selected, false, true);
        let MenuStep::Query(query) = step else { panic!("expected the query") };
        assert_eq!(query.range, candidates[1].range);
    }

    #[test]
    fn offer_idea_s_occurrences_choice_when_the_server_has_both_variants() {
        let text = "fn f() { g(price); h(price); }";
        let doc = Rope::from_str(text);
        let from = super::super::testing::offset_of(text, "price");
        let mut i18n = I18n::default();
        let actions = vec![
            action("Extract to local variable", "refactor.extract.variable", None),
            action("Extract to local variable (replace all occurrences)", "refactor.extract.variable", None),
        ];
        let step = actions_step(
            &mut i18n,
            &doc,
            (from, from + 5),
            "Introduce Variable",
            actions,
            Some(RefactoringId::ExtractVariable),
        );
        let ActionsStep::Occurrences { choice, .. } = step else { panic!("expected the occurrences choice") };
        assert_eq!(
            [choice.one.label.as_str(), choice.all.label.as_str()],
            ["Replace this occurrence only", "Replace all 2 occurrences"]
        );
        assert_eq!(choice.all.highlights.len(), 2);
    }

    #[test]
    fn rename_the_introduced_name_in_place_after_the_server_s_edit() {
        let before = Rope::from_str(TEXT);
        let declaration = offset_of(TEXT, "let total");
        let (from, to) = (offset_of(TEXT, "price"), offset_of(TEXT, "2") + 1);
        let edit: Vec<TextChange> =
            vec![change(declaration, declaration, "let var_name = price * 2;\n    "), change(from, to, "var_name")];
        let after = apply_changes(&before, &edit).to_string();
        let found = introduced_name(TEXT, &after).expect("the server introduced a name");
        let mut i18n = I18n::default();
        let rename = introduced_rename(&mut i18n, found, 0, Vec::new(), None).expect("a rename starts");
        assert_eq!(rename.name, "var_name");
        assert_eq!(rename.others.len(), 1);
    }
}
