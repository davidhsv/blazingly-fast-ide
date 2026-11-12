//! Code vision above declarations: "N usages" (IDEA's usages inlay), the declaration's last
//! author from VCS blame ("Thomas +2"), and the lenses of language servers and plugins.
//!
//! Usage counts are cheap by construction: only declarations in and near the viewport are
//! counted, in one debounced pass after edits or scrolling, a few reference requests at a time,
//! each in the server's current scope (only an explicit Find Usages widens it), and cached per
//! document text. A count that came back while the server was indexing is not shown, and is
//! asked for again once the server settles or asks for a refresh.

use super::availability::FeatureBackoff;
use super::lsp::is_unavailable;
use super::positions::pos_to_offset;
use ide_core::{DocId, IdeError, OutlineItem, Pos, RequestId, SymbolKind};
use ide_lsp::types::{CodeLens, Command, References};
use ide_vcs::BlameLine;
use ropey::Rope;
use std::collections::{HashMap, HashSet, VecDeque};

/// A lens drawn above a line.
#[derive(Debug, Clone)]
pub struct CodeLensView {
    pub pos: u32,
    pub title: String,
    pub tooltip: Option<String>,
    pub icon: Option<&'static str>,
    /// Lenses of one line are drawn by this order.
    pub order: u8,
    pub run: Option<LensRun>,
}

impl CodeLensView {
    fn same(&self, other: &Self) -> bool {
        self.pos == other.pos && self.title == other.title && self.tooltip == other.tooltip
    }
}

/// What clicking a lens does.
#[derive(Debug, Clone)]
pub enum LensRun {
    /// Put the caret at this offset and run Show Usages (`find.showUsages`).
    ShowUsages(u32),
    /// Run the server's (or plugin's) command.
    ExecuteCommand { doc: DocId, command: Command },
}

const LENS_KINDS: [SymbolKind; 10] = [
    SymbolKind::Class,
    SymbolKind::Interface,
    SymbolKind::Struct,
    SymbolKind::Enum,
    SymbolKind::Trait,
    SymbolKind::Function,
    SymbolKind::Method,
    SymbolKind::Constructor,
    SymbolKind::TypeAlias,
    SymbolKind::Macro,
];

/// A declaration worth a usages lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensTarget {
    pub name: String,
    /// The identifier.
    pub pos: Pos,
    /// The lines of the whole declaration.
    pub start_line: u32,
    pub end_line: u32,
}

impl LensTarget {
    fn key(&self) -> String {
        format!("{}:{}:{}", self.pos.line, self.pos.col, self.name)
    }
}

/// Servers name callbacks and anonymous functions (`<function>`, `map() callback`); they have no
/// usages.
fn anonymous(name: &str) -> bool {
    (name.starts_with('<') && name.ends_with('>')) || name.ends_with(" callback")
}

/// The declarations of an outline worth a usages lens, in document order, one per line.
pub fn lens_targets(items: &[OutlineItem]) -> Vec<LensTarget> {
    fn walk(items: &[OutlineItem], out: &mut Vec<LensTarget>) {
        for item in items {
            if LENS_KINDS.contains(&item.kind) && !anonymous(&item.name) {
                out.push(LensTarget {
                    name: item.name.clone(),
                    pos: item.selection_range.start,
                    start_line: item.range.start.line,
                    end_line: item.range.end.line,
                });
            }
            walk(&item.children, out);
        }
    }
    let mut out = Vec::new();
    walk(items, &mut out);
    out.sort_by_key(|target| target.pos);
    let mut lines = HashSet::new();
    out.retain(|target| lines.insert(target.pos.line));
    out
}

pub fn usages_label(count: usize) -> String {
    match count {
        0 => "no usages".into(),
        1 => "1 usage".into(),
        n => format!("{n} usages"),
    }
}

// ---------------------------------------------------------------------------
// Author

/// Who changed a declaration's lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorSummary {
    pub author: String,
    /// Other distinct authors of the lines.
    pub others: usize,
    /// Every distinct author, latest change first.
    pub authors: Vec<String>,
}

fn uncommitted(line: &BlameLine) -> bool {
    line.commit.bytes().all(|b| b == b'0')
}

/// The authors of 0-based lines `from..=to`; `None` when none of them is committed.
pub fn author_summary(blame: &[BlameLine], from: u32, to: u32) -> Option<AuthorSummary> {
    let mut latest: Vec<(&str, f64)> = Vec::new();
    for line in blame.iter().take(to as usize + 1).skip(from as usize) {
        if uncommitted(line) || line.author.is_empty() {
            continue;
        }
        match latest.iter_mut().find(|(author, _)| *author == line.author) {
            Some((_, time)) => *time = time.max(line.time_ms),
            None => latest.push((&line.author, line.time_ms)),
        }
    }
    latest.sort_by(|a, b| b.1.total_cmp(&a.1));
    let authors: Vec<String> = latest.into_iter().map(|(author, _)| author.to_owned()).collect();
    let author = authors.first()?.clone();
    Some(AuthorSummary { author, others: authors.len() - 1, authors })
}

pub fn author_label(summary: &AuthorSummary) -> String {
    if summary.others > 0 { format!("{} +{}", summary.author, summary.others) } else { summary.author.clone() }
}

pub fn author_tooltip(summary: &AuthorSummary) -> String {
    if summary.others > 0 {
        format!("Last changed by {}; also {}", summary.author, summary.authors[1..].join(", "))
    } else {
        format!("Last changed by {}", summary.author)
    }
}

// ---------------------------------------------------------------------------
// Usages

/// Declarations counted per pass.
const MAX_PER_PASS: usize = 40;
/// Reference requests in flight at once.
const CONCURRENCY: usize = 4;
/// Lines above and below the viewport counted too, so scrolling feels instant.
const MARGIN_LINES: u32 = 10;
/// The pause after edits or scrolling before a pass.
pub const PASS_DELAY_MS: u64 = 900;

/// What the caller does.
#[derive(Debug, Clone)]
pub enum LensEffect {
    /// Start (or restart) the pass timer; report [`UsagesLens::run`] with the ticket.
    Schedule {
        ticket: u64,
        ms: u64,
    },
    CancelRequest(RequestId),
    /// Wait for the editors' edits to reach the backend; report [`UsagesLens::synced`].
    Sync {
        ticket: u64,
    },
    /// Ask for the document outline; report [`UsagesLens::outline_answer`].
    AskOutline {
        ticket: u64,
    },
    /// Ask for the references at `pos` in the server's current scope (no declaration, no
    /// libraries, no widening); report [`UsagesLens::references_answer`].
    AskReferences {
        id: RequestId,
        pos: Pos,
    },
    /// The lenses to draw now.
    Show(Vec<CodeLensView>),
}

/// What a pass sees of the editor.
#[derive(Debug, Clone)]
pub struct LensPass<'a> {
    pub doc: &'a Rope,
    /// Bumped by every edit of the document.
    pub version: u64,
    /// The first and last visible lines (0-based).
    pub viewport: (u32, u32),
    /// Code vision › Usages.
    pub usages: bool,
    /// Code vision › Author, with the file's blame when it is under version control.
    pub blame: Option<&'a [BlameLine]>,
    pub now: u64,
}

#[derive(Debug, Clone)]
struct Pass {
    doc: Rope,
    version: u64,
    viewport: (u32, u32),
    usages: bool,
    blame: Option<Vec<BlameLine>>,
    now: u64,
    visible: Vec<LensTarget>,
    todo: VecDeque<LensTarget>,
    /// Requests of the batch in flight.
    batch: HashMap<RequestId, String>,
    unavailable: bool,
}

/// The usages and author lenses of one editor.
#[derive(Debug, Clone)]
pub struct UsagesLens {
    doc: DocId,
    generation: u64,
    pass: Option<Pass>,
    cache: HashMap<String, usize>,
    cache_version: Option<u64>,
    outline: Option<(u64, Vec<LensTarget>)>,
    incomplete: HashSet<String>,
    backoff: FeatureBackoff,
    shown: Vec<CodeLensView>,
    destroyed: bool,
}

impl UsagesLens {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            generation: 0,
            pass: None,
            cache: HashMap::new(),
            cache_version: None,
            outline: None,
            incomplete: HashSet::new(),
            backoff: FeatureBackoff::default(),
            shown: Vec::new(),
            destroyed: false,
        }
    }

    pub fn shown(&self) -> &[CodeLensView] {
        &self.shown
    }

    fn cancel(&mut self, effects: &mut Vec<LensEffect>) {
        self.generation += 1;
        if let Some(pass) = self.pass.take() {
            let mut ids: Vec<RequestId> = pass.batch.into_keys().collect();
            ids.sort();
            effects.extend(ids.into_iter().map(LensEffect::CancelRequest));
        }
    }

    /// Cancels what is under way and schedules a pass: after an edit, a scroll, a settings
    /// change, a VCS change.
    pub fn refresh(&mut self) -> Vec<LensEffect> {
        let mut effects = Vec::new();
        if self.destroyed {
            return effects;
        }
        self.cancel(&mut effects);
        effects.push(LensEffect::Schedule { ticket: self.generation, ms: PASS_DELAY_MS });
        effects
    }

    /// The server asked for fresh lenses: counts computed while it was indexing may be
    /// incomplete.
    pub fn server_refresh(&mut self) -> Vec<LensEffect> {
        self.cache.clear();
        self.incomplete.clear();
        self.refresh()
    }

    /// A server became ready after a busy spell: incomplete counts are asked for again.
    pub fn server_settled(&mut self) -> Vec<LensEffect> {
        if self.incomplete.is_empty() {
            return Vec::new();
        }
        self.incomplete.clear();
        self.refresh()
    }

    /// The pass timer of `ticket` elapsed.
    pub fn run(&mut self, ticket: u64, pass: &LensPass<'_>) -> Vec<LensEffect> {
        if ticket != self.generation || self.destroyed {
            return Vec::new();
        }
        let usages = pass.usages && !self.backoff.active(self.doc, pass.now);
        if !usages && pass.blame.is_none() {
            return self.show(Vec::new());
        }
        if self.cache_version != Some(pass.version) {
            self.cache.clear();
            self.incomplete.clear();
            self.cache_version = Some(pass.version);
        }
        self.pass = Some(Pass {
            doc: pass.doc.clone(),
            version: pass.version,
            viewport: pass.viewport,
            usages,
            blame: pass.blame.map(<[BlameLine]>::to_vec),
            now: pass.now,
            visible: Vec::new(),
            todo: VecDeque::new(),
            batch: HashMap::new(),
            unavailable: false,
        });
        vec![LensEffect::Sync { ticket }]
    }

    /// The edits reached the backend.
    pub fn synced(&mut self, ticket: u64) -> Vec<LensEffect> {
        let Some(pass) = self.pass.as_ref().filter(|_| ticket == self.generation) else { return Vec::new() };
        match &self.outline {
            Some((version, targets)) if *version == pass.version => {
                let targets = targets.clone();
                self.start(targets)
            }
            _ => vec![LensEffect::AskOutline { ticket }],
        }
    }

    pub fn outline_answer(&mut self, ticket: u64, answer: Result<Vec<OutlineItem>, IdeError>) -> Vec<LensEffect> {
        let Some(pass) = self.pass.as_ref().filter(|_| ticket == self.generation) else { return Vec::new() };
        match answer {
            Ok(items) => {
                let targets = lens_targets(&items);
                self.outline = Some((pass.version, targets.clone()));
                self.start(targets)
            }
            Err(error) => {
                if is_unavailable(&error) {
                    self.backoff.mark(self.doc, pass.now);
                }
                self.pass = None;
                Vec::new()
            }
        }
    }

    fn start(&mut self, targets: Vec<LensTarget>) -> Vec<LensEffect> {
        let Some(pass) = self.pass.as_mut() else { return Vec::new() };
        let first = pass.viewport.0.saturating_sub(MARGIN_LINES);
        let last = pass.viewport.1.saturating_add(MARGIN_LINES);
        pass.visible = targets
            .into_iter()
            .filter(|target| target.pos.line >= first && target.pos.line <= last)
            .take(MAX_PER_PASS)
            .collect();
        if pass.usages {
            pass.todo = pass
                .visible
                .iter()
                .filter(|target| !self.cache.contains_key(&target.key()) && !self.incomplete.contains(&target.key()))
                .cloned()
                .collect();
        }
        self.batch()
    }

    /// Sends the next batch of reference requests, or finishes the pass.
    fn batch(&mut self) -> Vec<LensEffect> {
        let Some(pass) = self.pass.as_mut() else { return Vec::new() };
        if pass.todo.is_empty() || pass.unavailable {
            return self.finish();
        }
        let mut effects = Vec::new();
        for _ in 0..CONCURRENCY {
            let Some(target) = pass.todo.pop_front() else { break };
            let id = RequestId::next();
            pass.batch.insert(id, target.key());
            effects.push(LensEffect::AskReferences { id, pos: target.pos });
        }
        effects
    }

    pub fn references_answer(&mut self, id: RequestId, answer: Result<References, IdeError>) -> Vec<LensEffect> {
        let Some(pass) = self.pass.as_mut() else { return Vec::new() };
        let Some(key) = pass.batch.remove(&id) else { return Vec::new() };
        match answer {
            Ok(references) if references.incomplete.is_some() => {
                self.incomplete.insert(key);
            }
            Ok(references) => {
                self.cache.insert(key, references.locations.len());
            }
            Err(error) => pass.unavailable |= is_unavailable(&error),
        }
        if self.pass.as_ref().is_some_and(|pass| pass.batch.is_empty()) { self.batch() } else { Vec::new() }
    }

    fn finish(&mut self) -> Vec<LensEffect> {
        let Some(pass) = self.pass.take() else { return Vec::new() };
        if pass.unavailable {
            self.backoff.mark(self.doc, pass.now);
        }
        let mut lenses = Vec::new();
        for target in &pass.visible {
            let offset = pos_to_offset(&pass.doc, target.pos);
            if let Some(who) =
                pass.blame.as_deref().and_then(|blame| author_summary(blame, target.start_line, target.end_line))
            {
                lenses.push(CodeLensView {
                    pos: offset,
                    title: author_label(&who),
                    tooltip: Some(author_tooltip(&who)),
                    icon: Some("user"),
                    order: 20,
                    run: None,
                });
            }
            let Some(&count) = self.cache.get(&target.key()).filter(|_| pass.usages) else { continue };
            lenses.push(CodeLensView {
                pos: offset,
                title: usages_label(count),
                tooltip: Some(format!("Show usages of {}", target.name)),
                icon: None,
                order: 10,
                run: Some(LensRun::ShowUsages(offset)),
            });
        }
        self.show(lenses)
    }

    fn show(&mut self, lenses: Vec<CodeLensView>) -> Vec<LensEffect> {
        let same = self.shown.len() == lenses.len() && self.shown.iter().zip(&lenses).all(|(a, b)| a.same(b));
        if same {
            return Vec::new();
        }
        self.shown.clone_from(&lenses);
        vec![LensEffect::Show(lenses)]
    }

    /// The editor went away: every request in flight is cancelled and no batch follows.
    pub fn destroy(&mut self) -> Vec<LensEffect> {
        let mut effects = Vec::new();
        self.cancel(&mut effects);
        self.destroyed = true;
        effects
    }
}

/// The lenses of language servers and plugins with a command, at their range's start.
pub fn server_lenses(doc_id: DocId, doc: &Rope, lenses: &[CodeLens]) -> Vec<CodeLensView> {
    lenses
        .iter()
        .filter_map(|lens| {
            let command = lens.command.clone()?;
            Some(CodeLensView {
                pos: pos_to_offset(doc, lens.range.start),
                title: command.title.clone(),
                tooltip: None,
                icon: None,
                order: 0,
                run: Some(LensRun::ExecuteCommand { doc: doc_id, command }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Location, Range};
    use ide_lsp::types::ReferencesIncomplete;

    fn item(name: &str, kind: SymbolKind, line: u32, children: Vec<OutlineItem>) -> OutlineItem {
        let range = Range::new(Pos::new(line, 0), Pos::new(line, 1));
        OutlineItem { name: name.into(), detail: None, kind, range, selection_range: range, children, visibility: None }
    }

    #[test]
    fn skips_callbacks_and_anonymous_functions() {
        let callback = item("reduce() callback", SymbolKind::Function, 2, Vec::new());
        let count = item("count", SymbolKind::Method, 1, vec![callback]);
        let anonymous = item("<function>", SymbolKind::Function, 3, Vec::new());
        let items = [item("Cart", SymbolKind::Class, 0, vec![count, anonymous])];
        let names: Vec<String> = lens_targets(&items).into_iter().map(|target| target.name).collect();
        assert_eq!(names, ["Cart", "count"]);
    }

    fn refs(count: usize, incomplete: Option<ReferencesIncomplete>) -> Result<References, IdeError> {
        let location = Location { path: "/p/b.rs".into(), range: Range::default(), uri: None };
        Ok(References { locations: vec![location; count], incomplete, ..References::default() })
    }

    /// An editor of `fn f0() {}` … with the usages lens, its server and timers played by hand.
    struct Editor {
        lens: UsagesLens,
        doc: Rope,
        version: u64,
        usages: bool,
        outline: Vec<OutlineItem>,
        /// Reference answers, in order; then two usages each.
        answers: VecDeque<Result<References, IdeError>>,
        /// Answer reference requests as they come.
        answering: bool,
        open: Vec<RequestId>,
        asked: usize,
        outline_asked: usize,
        cancelled: Vec<RequestId>,
        schedule: Option<u64>,
        sync: Option<u64>,
        auto_sync: bool,
    }

    impl Editor {
        fn new(count: u32) -> Self {
            let text: Vec<String> = (0..count).map(|i| format!("fn f{i}() {{}}")).collect();
            let mut editor = Self {
                lens: UsagesLens::new(DocId(7)),
                doc: Rope::from_str(&text.join("\n")),
                version: 0,
                usages: true,
                outline: vec![item("f0", SymbolKind::Function, 0, Vec::new())],
                answers: VecDeque::new(),
                answering: true,
                open: Vec::new(),
                asked: 0,
                outline_asked: 0,
                cancelled: Vec::new(),
                schedule: None,
                sync: None,
                auto_sync: true,
            };
            let effects = editor.lens.refresh();
            editor.apply(effects);
            editor
        }

        fn apply(&mut self, effects: Vec<LensEffect>) {
            for effect in effects {
                match effect {
                    LensEffect::Schedule { ticket, .. } => self.schedule = Some(ticket),
                    LensEffect::CancelRequest(id) => self.cancelled.push(id),
                    LensEffect::Sync { ticket } if self.auto_sync => {
                        let effects = self.lens.synced(ticket);
                        self.apply(effects);
                    }
                    LensEffect::Sync { ticket } => self.sync = Some(ticket),
                    LensEffect::AskOutline { ticket } => {
                        self.outline_asked += 1;
                        let effects = self.lens.outline_answer(ticket, Ok(self.outline.clone()));
                        self.apply(effects);
                    }
                    LensEffect::AskReferences { id, .. } => {
                        self.asked += 1;
                        self.open.push(id);
                    }
                    LensEffect::Show(_) => {}
                }
            }
            while self.answering && !self.open.is_empty() {
                let id = self.open.remove(0);
                let answer = self.answers.pop_front().unwrap_or_else(|| refs(2, None));
                let effects = self.lens.references_answer(id, answer);
                self.apply(effects);
            }
        }

        /// The pass timer elapses.
        fn settle(&mut self) {
            if let Some(ticket) = self.schedule.take() {
                let pass = LensPass {
                    doc: &self.doc,
                    version: self.version,
                    viewport: (0, 40),
                    usages: self.usages,
                    blame: None,
                    now: 0,
                };
                let effects = self.lens.run(ticket, &pass);
                self.apply(effects);
            }
        }

        fn titles(&self) -> Vec<String> {
            self.lens.shown().iter().map(|lens| lens.title.clone()).collect()
        }

        fn edit(&mut self) {
            self.version += 1;
            let effects = self.lens.refresh();
            self.apply(effects);
        }
    }

    #[test]
    fn uses_the_servers_existing_scope_and_leaves_full_searches_to_the_explicit_action() {
        let mut editor = Editor::new(1);
        editor.settle();
        assert_eq!(editor.asked, 1);
        assert_eq!(editor.titles(), ["2 usages"]);
        assert!(matches!(editor.lens.shown()[0].run, Some(LensRun::ShowUsages(0))));
    }

    #[test]
    fn allows_a_full_search_even_when_the_servers_current_scope_has_no_usages() {
        let mut editor = Editor::new(1);
        editor.answers = VecDeque::from([refs(0, None)]);
        editor.settle();
        assert_eq!(editor.titles(), ["no usages"]);
        assert!(matches!(editor.lens.shown()[0].run, Some(LensRun::ShowUsages(_))));
    }

    #[test]
    fn does_not_cache_an_incomplete_count_while_indexing_and_retries_on_server_refresh() {
        let mut editor = Editor::new(1);
        editor.answers = VecDeque::from([refs(0, Some(ReferencesIncomplete::Indexing))]);
        editor.settle();
        assert!(editor.titles().is_empty());
        let effects = editor.lens.server_refresh();
        editor.apply(effects);
        editor.settle();
        assert_eq!(editor.asked, 2);
        assert_eq!(editor.titles(), ["2 usages"]);
    }

    #[test]
    fn cancels_obsolete_requests_immediately_on_edits_and_ignores_their_late_replies() {
        let mut editor = Editor { answering: false, ..Editor::new(1) };
        editor.settle();
        let old = editor.open.remove(0);
        editor.answering = true;
        editor.edit();
        assert_eq!(editor.cancelled, [old]);
        editor.settle();
        assert_eq!(editor.titles(), ["2 usages"]);
        let effects = editor.lens.references_answer(old, refs(99, None));
        editor.apply(effects);
        assert_eq!(editor.titles(), ["2 usages"]);
    }

    #[test]
    fn retries_incomplete_counts_when_indexing_ends_without_a_code_lens_refresh() {
        let mut editor = Editor::new(1);
        editor.answers = VecDeque::from([refs(0, Some(ReferencesIncomplete::Indexing))]);
        editor.settle();
        assert!(editor.titles().is_empty());
        let effects = editor.lens.server_settled();
        editor.apply(effects);
        editor.settle();
        assert_eq!(editor.asked, 2);
        assert_eq!(editor.titles(), ["2 usages"]);
    }

    #[test]
    fn does_not_repeat_complete_counts_when_the_server_finishes_other_work() {
        let mut editor = Editor::new(1);
        editor.settle();
        let effects = editor.lens.server_settled();
        editor.apply(effects);
        editor.settle();
        assert_eq!(editor.asked, 1);
        assert_eq!(editor.titles(), ["2 usages"]);
    }

    #[test]
    fn cancels_all_in_flight_requests_on_destroy_without_starting_the_next_batch() {
        let mut editor = Editor { answering: false, ..Editor::new(8) };
        editor.outline = (0..8).map(|i| item(&format!("f{i}"), SymbolKind::Function, i, Vec::new())).collect();
        editor.settle();
        assert_eq!(editor.asked, 4);
        let requests = editor.open.clone();
        let effects = editor.lens.destroy();
        editor.apply(effects);
        assert_eq!(editor.cancelled, requests);
        for id in requests {
            let effects = editor.lens.references_answer(id, refs(1, None));
            editor.apply(effects);
        }
        assert_eq!(editor.asked, 4);
    }

    #[test]
    fn cancels_pending_counts_when_the_feature_is_disabled() {
        let mut editor = Editor { answering: false, ..Editor::new(1) };
        editor.settle();
        let request = editor.open[0];
        editor.usages = false;
        let effects = editor.lens.refresh();
        editor.apply(effects);
        assert_eq!(editor.cancelled, [request]);
        let effects = editor.lens.references_answer(request, refs(2, None));
        editor.apply(effects);
        editor.settle();
        assert!(editor.titles().is_empty());
    }

    #[test]
    fn does_not_send_obsolete_requests_after_waiting_for_document_synchronization() {
        let mut editor = Editor { auto_sync: false, ..Editor::new(1) };
        editor.settle();
        assert_eq!(editor.outline_asked, 0);
        let ticket = editor.sync.take().expect("the pass waits for the sync");
        let effects = editor.lens.destroy();
        editor.apply(effects);
        let effects = editor.lens.synced(ticket);
        editor.apply(effects);
        assert_eq!((editor.outline_asked, editor.asked), (0, 0));
        assert!(editor.cancelled.is_empty());
    }

    #[test]
    fn renders_lsp_and_plugin_lens_commands_in_the_shared_facet_and_invokes_their_commands() {
        let command = Command {
            title: "Run fixture".into(),
            command: "openidea.plugin-language:p:run".into(),
            arguments: vec![serde_json::json!("\"lens\"")],
        };
        let lens = CodeLens { range: Range::new(Pos::new(0, 0), Pos::new(0, 1)), command: Some(command) };
        let views = server_lenses(DocId(1), &Rope::from_str("x"), &[lens]);
        assert_eq!(views.iter().map(|view| view.title.as_str()).collect::<Vec<_>>(), ["Run fixture"]);
        let Some(LensRun::ExecuteCommand { doc, command }) = &views[0].run else { panic!("the lens runs a command") };
        assert_eq!((*doc, command.command.as_str()), (DocId(1), "openidea.plugin-language:p:run"));
    }

    fn blame() -> Vec<BlameLine> {
        let line = |author: &str, time_ms: f64, commit: &str| BlameLine {
            commit: commit.into(),
            author: author.into(),
            time_ms,
            summary: String::new(),
        };
        let committed = "a".repeat(40);
        vec![
            line("Ann", 100.0, &committed),
            line("Thomas", 300.0, &committed),
            line("Bob", 200.0, &committed),
            line("Ann", 50.0, &committed),
            line("Not Committed Yet", 0.0, &"0".repeat(40)),
            line("Carol", 900.0, &committed),
        ]
    }

    #[test]
    fn names_the_latest_author_of_the_range_and_counts_the_others() {
        let summary = author_summary(&blame(), 0, 4).expect("committed lines");
        assert_eq!(summary.authors, ["Thomas", "Bob", "Ann"]);
        assert_eq!(author_label(&summary), "Thomas +2");
        assert_eq!(author_tooltip(&summary), "Last changed by Thomas; also Bob, Ann");
    }

    #[test]
    fn shows_a_single_author_without_a_count() {
        let summary = author_summary(&blame(), 3, 3).expect("a committed line");
        assert_eq!(author_label(&summary), "Ann");
        assert_eq!(author_tooltip(&summary), "Last changed by Ann");
    }

    #[test]
    fn ignores_uncommitted_lines_and_lines_past_the_blame() {
        assert_eq!(author_summary(&blame(), 4, 4), None);
        assert_eq!(author_summary(&blame(), 4, 99).map(|summary| author_label(&summary)).as_deref(), Some("Carol"));
        assert_eq!(author_summary(&blame(), 10, 12), None);
    }
}
