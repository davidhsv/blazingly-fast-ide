//! The editing commands that read the document's structure from the backend: Move Statement Up /
//! Down, Complete Current Statement, and the comment commands when a block comment holds its own
//! closing token.
//!
//! Such a command waits for the backend, so the presses of one editor run in order (a quick ⇧⌘↑
//! ⇧⌘↑ moves twice), each against the text and selection of its turn. While a command waits,
//! typing goes on: when the text or the selection moved meanwhile, the answer is stale and the
//! command does nothing. A command whose language has no syntax tree, or whose question the
//! service cannot answer, does what the text alone tells (Move Statement moves the line,
//! Complete Current Statement finishes the line).

use super::{Lookup, StructureService, Support, chain_tree, has_syntax_tree, parses_itself};
use crate::backend::Backend;
use crate::editor::history::EditKind;
use crate::editor::view::Editor;
use crate::editor_commands::tree::SyntaxTree;
use crate::editor_commands::{Edit, EditState, IndentContext, SelectionExt, comments, editing, structural};
use crate::editor_lsp::positions::offset_to_pos;
use gpui_kit::{Context, EntityId};
use ide_core::{DocId, IdeError, IdeResult, LanguageId, Pos, Range};
use ide_host::Host;
use ide_syntax::{NodeAt, StructuralRanges, StructureSupport, SyntaxErrors};
use std::path::PathBuf;
use std::sync::Arc;

/// A structure command waiting for its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    MoveStatement {
        up: bool,
    },
    CompleteStatement,
    /// Comment with Block Comment over a comment holding its own closing token.
    CommentBlock,
    /// Comment with Line Comment in a language without line comments, over such a comment;
    /// `repeated`: the press follows the previous one where it left the editor.
    CommentLine {
        repeated: bool,
    },
}

/// What the backend is asked for a command to go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    Support,
    Ranges(Pos),
    Errors,
    Node(Pos),
}

/// The backend's answer to a [`Need`]; the support comes with the document's editing mode.
enum Answer {
    Support(IdeResult<StructureSupport>, Option<String>),
    /// Boxed: the ranges are much larger than the other answers.
    Ranges(Pos, Box<IdeResult<StructuralRanges>>),
    Errors(IdeResult<SyntaxErrors>),
    Node(Pos, IdeResult<Option<NodeAt>>),
}

impl Answer {
    /// The answer of a question the backend could not take.
    fn failed(need: Need, error: IdeError) -> Self {
        match need {
            Need::Support => Self::Support(Err(error), None),
            Need::Ranges(pos) => Self::Ranges(pos, Box::new(Err(error))),
            Need::Errors => Self::Errors(Err(error)),
            Need::Node(pos) => Self::Node(pos, Err(error)),
        }
    }
}

/// What one turn of a command came to.
enum Step {
    Done,
    Ask(Need),
}

/// The language's indentation, as the commands ask for it.
fn indent(editor: &Editor) -> impl Fn(&IndentContext<'_>) -> Option<u32> + '_ {
    |context| editor.facts().indent(context)
}

/// Applies `edit`, computed for the text at `version`, as one undo step; nothing when the text
/// moved on before the edit reached it.
fn apply(editor: &mut Editor, version: u32, edit: Edit, cx: &mut Context<Editor>) -> bool {
    editor.edit_with(EditKind::Other, cx, move |editor| (editor.version() == version).then(|| edit.clone()))
}

fn move_line(editor: &mut Editor, up: bool, cx: &mut Context<Editor>) {
    editor.edit_with(EditKind::Other, cx, |editor| editing::move_line(&editor.edit_state(), up));
}

fn complete_with_text(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.edit_with(EditKind::Other, cx, |editor| {
        let facts = &editor.facts().language;
        let state = editor.edit_state();
        editing::complete_statement(&state, &facts.id, facts.comments.line.as_deref(), Some(&indent(editor)))
    });
}

// --- entry points -------------------------------------------------------------------------------

/// Move Statement Up / Down: with one caret the statement at the caret line swaps with its sibling
/// as the backend's ranges say; with several ranges or a selection the lines move.
pub fn move_statement(editor: &mut Editor, up: bool, cx: &mut Context<Editor>) {
    let state = editor.edit_state();
    if state.selections.len() > 1 || !state.main().is_caret() || !cx.has_global::<Backend>() {
        editor.edit_with(EditKind::Other, cx, |editor| {
            editing::move_statement(&editor.edit_state(), &SyntaxTree::default(), Some(&indent(editor)), up)
        });
        return;
    }
    enqueue(editor, Command::MoveStatement { up }, cx);
}

/// Complete Current Statement: the backend's syntax errors and ranges tell what is missing where,
/// then the text rules finish the line; with several carets the text rules alone.
pub fn complete_statement(editor: &mut Editor, cx: &mut Context<Editor>) {
    if editor.edit_state().selections.len() > 1 || !cx.has_global::<Backend>() {
        complete_with_text(editor, cx);
        return;
    }
    enqueue(editor, Command::CompleteStatement, cx);
}

/// The comment commands answered `edit` from the text alone and it changes nothing: the comment
/// around a range holds its own closing token, which only the parser can confirm is one comment.
fn blocked(state: &EditState, edit: Option<&Edit>) -> bool {
    edit.is_some_and(|edit| edit.changes.is_empty() && edit.selections == state.selections && edit.main == state.main)
}

/// Comment with Block Comment: from the text, unless a comment holds its own closing token; then
/// the comment's node in the syntax tree decides.
pub fn comment_block(editor: &mut Editor, cx: &mut Context<Editor>) {
    let state = editor.edit_state();
    let comments_at = |pos: u32| editor.facts().comments_at(pos);
    let text_only = comments::comment_block(&state, &comments_at, &SyntaxTree::default());
    if blocked(&state, text_only.as_ref()) && cx.has_global::<Backend>() {
        enqueue(editor, Command::CommentBlock, cx);
        return;
    }
    // Asked again inside the edit: a text the backend moved on gets its own answer.
    editor.edit_with(EditKind::Other, cx, |editor| {
        let comments_at = |pos: u32| editor.facts().comments_at(pos);
        comments::comment_block(&editor.edit_state(), &comments_at, &SyntaxTree::default())
    });
}

/// Comment with Line Comment: from the text, unless a language without line comments has a block
/// comment holding its own closing token on the lines; then the syntax tree decides.
pub fn comment_line(editor: &mut Editor, cx: &mut Context<Editor>) {
    let repeated = editor.memory.comment.as_ref() == Some(&editor.stamp());
    let state = editor.edit_state();
    let text_only = {
        let facts = editor.facts();
        let comments_at = |pos: u32| facts.comments_at(pos);
        comments::comment_line(&state, &comments_at, facts.comment_style, &SyntaxTree::default(), repeated)
    };
    if blocked(&state, text_only.as_ref()) && cx.has_global::<Backend>() {
        enqueue(editor, Command::CommentLine { repeated }, cx);
        return;
    }
    let done = editor.edit_with(EditKind::Other, cx, |editor| {
        let facts = editor.facts();
        let comments_at = |pos: u32| facts.comments_at(pos);
        let state = editor.edit_state();
        comments::comment_line(&state, &comments_at, facts.comment_style, &SyntaxTree::default(), repeated)
    });
    if done {
        editor.memory.comment = Some(editor.stamp());
    }
}

// --- the queue ----------------------------------------------------------------------------------

fn enqueue(editor: &mut Editor, command: Command, cx: &mut Context<Editor>) {
    let id = cx.entity_id();
    let queue = cx.default_global::<StructureService>().queues.entry(id).or_default();
    queue.pending.push_back(command);
    // A command already waiting drives the queue: this one runs after it.
    if queue.pending.len() == 1 {
        drain(editor, cx);
    }
}

/// Runs the waiting commands of `editor` until one has to ask the backend.
fn drain(editor: &mut Editor, cx: &mut Context<Editor>) {
    let id = cx.entity_id();
    loop {
        let front = cx.default_global::<StructureService>().queues.get(&id).and_then(|q| q.pending.front().copied());
        let Some(command) = front else {
            cx.default_global::<StructureService>().queues.remove(&id);
            return;
        };
        match attempt(editor, command, cx) {
            Step::Done => pop(id, cx),
            Step::Ask(need) => {
                ask(editor, need, cx);
                return;
            }
        }
    }
}

fn pop(id: EntityId, cx: &mut Context<Editor>) {
    if let Some(queue) = cx.default_global::<StructureService>().queues.get_mut(&id) {
        queue.pending.pop_front();
    }
}

/// Asks the backend for `need` off the window's thread; the answer is kept for the version it
/// was asked at, and the waiting command runs again unless the editor moved on meanwhile.
fn ask(editor: &Editor, need: Need, cx: &mut Context<Editor>) {
    let id = cx.entity_id();
    let backend = cx.global::<Backend>();
    let host = backend.host.clone();
    let (label, doc, language) = (editor.window_label().to_owned(), editor.doc(), editor.language().clone());
    let (version, asked, asked_language) = (editor.version(), editor.stamp(), language.clone());
    let path = editor.path().to_path_buf();
    let work = backend.call(async move { answer(host, label, doc, path, language, need).await });
    cx.spawn(async move |editor, cx| {
        let answer = work.await.unwrap_or_else(|error| Answer::failed(need, IdeError::internal(error.to_string())));
        let resumed = editor.update(cx, |editor: &mut Editor, cx| {
            let unsupported = matches!(answer, Answer::Support(Err(_), _));
            store(cx.default_global::<StructureService>(), doc, asked_language, version, answer);
            let queues = &cx.default_global::<StructureService>().queues;
            let front = queues.get(&id).and_then(|queue| queue.pending.front().copied());
            if editor.stamp() != asked {
                pop(id, cx);
            } else if let Some(command) = front.filter(|_| unsupported) {
                // The question failed: this press goes on without a tree, the next one asks again.
                turn(editor, command, Support::default(), cx);
                pop(id, cx);
            }
            drain(editor, cx);
        });
        if resumed.is_err() {
            cx.update(|cx| cx.default_global::<StructureService>().queues.remove(&id));
        }
    })
    .detach();
}

/// Calls the backend's structure query `need` stands for, as the IPC commands do.
async fn answer(host: Arc<Host>, label: String, doc: DocId, path: PathBuf, language: LanguageId, need: Need) -> Answer {
    use ide_host::ops::syntax;
    match need {
        Need::Support => {
            let framework = ide_host::ops::doc::doc_ui_mode(&host, &label, path).await.unwrap_or_else(|error| {
                tracing::debug!("no framework editing mode: {}", error.message);
                None
            });
            // A framework's mode (Angular's) replaces the language's own.
            let mode = framework.or_else(|| host.languages.get(&language).and_then(|config| config.ui_mode));
            let support = syntax::syntax_support(&host, &label, language);
            Answer::Support(support, mode)
        }
        Need::Ranges(pos) => {
            Answer::Ranges(pos, Box::new(syntax::syntax_structural_ranges(&host, &label, doc, pos, None).await))
        }
        Need::Errors => Answer::Errors(syntax::syntax_errors(&host, &label, doc, None).await),
        Need::Node(pos) => {
            let range = Range { start: pos, end: pos };
            Answer::Node(pos, syntax::syntax_node_at(&host, &label, doc, range, None).await)
        }
    }
}

fn store(service: &mut StructureService, doc: DocId, language: LanguageId, version: u32, answer: Answer) {
    let cache = service.cache(doc);
    match answer {
        Answer::Support(Ok(support), mode) => {
            let support = Support { tree: has_syntax_tree(&Ok(support)), parsed_mode: parses_itself(mode.as_deref()) };
            cache.set_support(language, support);
        }
        // Not kept: the next press asks again.
        Answer::Support(Err(error), _) => {
            tracing::debug!("no syntax structure for {language:?}: {}", error.message);
        }
        Answer::Ranges(pos, ranges) => cache.set_ranges(version, pos, *ranges),
        Answer::Errors(errors) => cache.set_errors(version, errors),
        Answer::Node(pos, node) => cache.set_node(version, pos, node),
    }
}

// --- one turn of a command ----------------------------------------------------------------------

/// The cached answer to `$lookup`, or the step that asks the backend for `$need`.
macro_rules! known {
    ($lookup:expr, $need:expr) => {
        match $lookup {
            Lookup::Known(answer) => answer,
            Lookup::Ask => return Step::Ask($need),
        }
    };
}

fn attempt(editor: &mut Editor, command: Command, cx: &mut Context<Editor>) -> Step {
    let (doc, language) = (editor.doc(), editor.language().clone());
    let support = known!(cx.default_global::<StructureService>().cache(doc).support(&language), Need::Support);
    turn(editor, command, support, cx)
}

/// One turn of `command` with what the language offers known.
fn turn(editor: &mut Editor, command: Command, support: Support, cx: &mut Context<Editor>) -> Step {
    let version = editor.version();
    match command {
        Command::MoveStatement { up } => move_turn(editor, up, support.tree, version, cx),
        Command::CompleteStatement => complete_turn(editor, support, version, cx),
        Command::CommentBlock | Command::CommentLine { .. } if !support.tree => Step::Done,
        Command::CommentBlock => comment_turn(editor, None, version, cx),
        Command::CommentLine { repeated } => comment_turn(editor, Some(repeated), version, cx),
    }
}

fn move_turn(editor: &mut Editor, up: bool, has_tree: bool, version: u32, cx: &mut Context<Editor>) -> Step {
    let state = editor.edit_state();
    let query = structural::move_statement_query(&state);
    let (Some(pos), true) = (query, has_tree) else {
        move_line(editor, up, cx);
        return Step::Done;
    };
    let cache = cx.default_global::<StructureService>().cache(editor.doc());
    let ranges = known!(cache.ranges(version, pos), Need::Ranges(pos));
    let errors = match &ranges {
        Some(ranges) if structural::move_statement_needs_errors(ranges) => known!(cache.errors(version), Need::Errors),
        _ => None,
    };
    let plan = {
        let comments_at = |pos: u32| editor.facts().comments_at(pos);
        structural::plan_move_statement(&state, &comments_at, ranges.as_ref(), up, errors.as_ref())
    };
    match plan {
        structural::MovePlan::Move(edit) => {
            apply(editor, version, edit, cx);
        }
        structural::MovePlan::Noop => {}
        structural::MovePlan::Fallback => move_line(editor, up, cx),
    }
    Step::Done
}

fn complete_turn(editor: &mut Editor, support: Support, version: u32, cx: &mut Context<Editor>) -> Step {
    if editor.is_read_only() {
        return Step::Done;
    }
    // A mode with its own parser completes from the text, except Java's, whose `;` the backend's
    // tree places better.
    let own_parser = support.parsed_mode && editor.facts().language.id != "java";
    if !support.tree || own_parser {
        complete_with_text(editor, cx);
        return Step::Done;
    }
    let state = editor.edit_state();
    let pos = offset_to_pos(&state.rope, state.main().head());
    let cache = cx.default_global::<StructureService>().cache(editor.doc());
    let ranges = known!(cache.ranges(version, pos), Need::Ranges(pos));
    let errors = known!(cache.errors(version), Need::Errors);
    let edit = {
        let facts = &editor.facts().language;
        let comments_at = |pos: u32| editor.facts().comments_at(pos);
        let indenter = indent(editor);
        structural::complete_statement_structural(
            &state,
            &facts.id,
            &comments_at,
            Some(&indenter),
            ranges.as_ref(),
            errors.as_ref(),
        )
    };
    match edit {
        Some(edit) => {
            apply(editor, version, edit, cx);
        }
        None => complete_with_text(editor, cx),
    }
    Step::Done
}

/// Where the comment around each range is looked for: the first character of code at or after
/// the range's start on its line, which lies in the comment when the range is in or over one.
fn probe_positions(state: &EditState) -> Vec<Pos> {
    let mut positions: Vec<Pos> = state
        .selections
        .iter()
        .map(|range| {
            let line = state.line_at(range.start());
            let rest = state.slice(range.start(), line.to);
            let blank = crate::editor_commands::utf16_len(&rest[..rest.len() - rest.trim_start().len()]);
            let at = if blank as usize == rest.encode_utf16().count() { range.start() } else { range.start() + blank };
            offset_to_pos(&state.rope, at)
        })
        .collect();
    positions.sort();
    positions.dedup();
    positions
}

/// Comment with Block Comment (`repeated` is `None`) or with Line Comment, with the tree of the
/// nodes around the ranges.
fn comment_turn(editor: &mut Editor, repeated: Option<bool>, version: u32, cx: &mut Context<Editor>) -> Step {
    let state = editor.edit_state();
    let cache = cx.default_global::<StructureService>().cache(editor.doc());
    let mut chains = Vec::new();
    for pos in probe_positions(&state) {
        chains.extend(known!(cache.node(version, pos), Need::Node(pos)));
    }
    let tree = chain_tree(&chains, &state.rope);
    let edit = {
        let facts = editor.facts();
        let comments_at = |pos: u32| facts.comments_at(pos);
        match repeated {
            None => comments::comment_block(&state, &comments_at, &tree),
            Some(repeated) => comments::comment_line(&state, &comments_at, facts.comment_style, &tree, repeated),
        }
    };
    // Still blocked: the parser says the tokens belong to separate comments; nothing changes.
    let Some(edit) = edit.filter(|edit| !edit.changes.is_empty()) else { return Step::Done };
    if apply(editor, version, edit, cx) && repeated.is_some() {
        editor.memory.comment = Some(editor.stamp());
    }
    Step::Done
}
