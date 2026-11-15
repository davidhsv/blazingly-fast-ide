//! The Refactor menu's refactorings the language server carries out (Introduce Variable /
//! Constant / Field / Parameter, Extract Method, Inline, Move, Change Signature), Refactor This
//! (⌃T), Safe Delete (⌘⌦) and Navigate › Super Method (⌘U).
//!
//! The flows are the models of [`crate::editor_refactor`]: they say what to ask the backend and
//! what to show; this module asks it off the main thread, feeds the answers back and shows the
//! popups and dialogs. One run of each flow is in flight per window: a newer one supersedes it
//! and cancels its request.

use super::Workbench;
use super::branches::run_action_later;
use super::editor_language::{self, This, apply_code_action};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::editor::buffer;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::view::Editor;
use crate::editor_commands::{CommentTokens, Selection};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::intentions_view::{ListOptions, ListPopup, ListPopupEvent, ListRow};
use crate::editor_lsp::lsp::{error_hint, is_cancelled};
use crate::editor_lsp::positions::offset_to_pos;
use crate::editor_refactor::catalog::{RefactoringId, fallback_language_name};
use crate::editor_refactor::refactor_this::{
    ActionsStep, FactAnswer, FactRequest, MenuStep, NOTHING_HERE, RefactorItem, RefactorMenu, RefactorQuery,
    RefactorRun, RefactorThis, RefactorThisStep, actions_step,
};
use crate::editor_refactor::safe_delete::{
    SafeDeleteAnswer, SafeDeleteFlow, SafeDeleteRequest, SafeDeleteSetup, SafeDeleteStep, c_like_comments,
};
use crate::editor_refactor::server_introduce::{DOCUMENT_CHANGED, ExpressionChoice};
use crate::editor_refactor::structure_nav::{SUPER_DEPTH, SuperMethodSearch, SuperStep, base_name, member_at};
use crate::platform::actions::ActionRegistry;
use crate::platform::context::evaluate_when;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input::Keymap;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::project_view::operations::UsagesAnswer;
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, OK, PromptDialog};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, ConfirmRequest};
use gpui_kit::{App, AppContext, AsyncWindowContext, Context, Entity, Focusable, Window};
use ide_core::ipc_path::to_ipc;
use ide_core::{DocId, IdeError, IdeResult, LanguageId, Location, OutlineItem, Pos, Severity};
use ide_host::Host;
use ide_lsp::types::{CodeAction, GotoKind, HierarchyDirection, HierarchyItem, HierarchyRefactoring};
use ide_syntax::Locals;
use ropey::Rope;
use serde_json::json;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

/// When the editor actions apply: the active text editor, unless a tool window or popup has the
/// keyboard. Refactor This checks it itself, since outside an editor it says "Nothing here".
const EDITOR_WHEN: &str = "textEditorActive && (editorFocus || popupOpen || !toolWindowFocus)";

/// The Refactor menu actions that ask the server for code actions of their kind.
const MENU_REFACTORINGS: [(&str, RefactoringId); 8] = [
    ("refactor.extractVariable", RefactoringId::ExtractVariable),
    ("refactor.extractMethod", RefactoringId::ExtractMethod),
    ("refactor.extractConstant", RefactoringId::ExtractConstant),
    ("refactor.extractField", RefactoringId::ExtractField),
    ("refactor.extractParameter", RefactoringId::ExtractParameter),
    ("refactor.move", RefactoringId::Move),
    ("refactor.changeSignature", RefactoringId::ChangeSignature),
    ("refactor.inline", RefactoringId::Inline),
];

const NO_STRUCTURE: &str = "No structure information for this file";
const NO_SUPER_METHOD: &str = "No super method found";
const NO_SUPER_CLASS: &str = "No super class found";

/// The refactoring flows of one window.
pub(super) struct RefactorActions {
    menu: RefactorMenu,
    this: RefactorThis,
    safe_delete: Option<SafeDeleteFlow>,
    /// Safe Delete's "Search in comments and strings" as last confirmed (on at first, like IDEA).
    search_in_comments: bool,
}

impl Default for RefactorActions {
    fn default() -> Self {
        Self {
            menu: RefactorMenu::default(),
            this: RefactorThis::default(),
            safe_delete: None,
            search_in_comments: true,
        }
    }
}

/// What a flow starts from: the editor's document, its version and main selection.
#[derive(Clone)]
struct Caret {
    doc: DocId,
    rope: Rope,
    version: u64,
    selection: Selection,
    language: String,
    read_only: bool,
}

impl Caret {
    fn of(editor: &Editor) -> Self {
        let caret = editor.caret();
        Self {
            doc: editor.doc(),
            rope: editor.rope().clone(),
            version: u64::from(editor.version()),
            selection: editor.selections().get(editor.main_index()).copied().unwrap_or((caret, caret)),
            language: editor.language().as_str().to_owned(),
            read_only: editor.is_read_only(),
        }
    }
}

/// Runs `work` on the backend for the window's project, keeping the backend's error.
async fn call<T, F>(
    this: &This,
    cx: &mut AsyncWindowContext,
    work: impl FnOnce(Arc<Host>, String) -> F + Send + 'static,
) -> anyhow::Result<IdeResult<T>>
where
    T: Send + 'static,
    F: Future<Output = IdeResult<T>> + Send + 'static,
{
    let call = this.update(cx, |workbench, cx| {
        let label = workbench.label.clone();
        cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)))
    })?;
    let Some(call) = call else { return Ok(Err(IdeError::unavailable("The backend is not running"))) };
    Ok(call.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string()))))
}

/// The end of a refactoring's background work: it only fails when the window went away.
fn finished(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::debug!("a refactoring ended with its window: {error:#}");
    }
}

/// Shows `text` (English source text) as the hint at the caret of `editor`.
fn hint(editor: &Entity<Editor>, text: impl Into<String>, tone: Tone, cx: &mut App) {
    editor_language::hint(editor, text, None, tone, None, cx);
}

/// The hint of a failed request the user asked for; nothing for a cancelled one.
fn report(editor: &Entity<Editor>, error: &IdeError, cx: &mut App) {
    if let Some(text) = error_hint(error, true) {
        hint(editor, text, Tone::Info, cx);
    }
}

/// The shortcut bound to action `id`, as menus show it.
fn shortcut(cx: &App, id: &str) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(id, crate::platform::keymap::is_mac_host()))
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn cancel_request(request: ide_core::RequestId, cx: &App) {
    if let Some(backend) = cx.try_global::<Backend>() {
        backend.host.cancel_request(request);
    }
}

/// The display name of a language for the reasons a refactoring gives ("Rust", "Mock").
fn language_name(id: &str, cx: &App) -> String {
    cx.try_global::<Backend>()
        .and_then(|backend| backend.host.languages.get(&LanguageId::new(id)))
        .map_or_else(|| fallback_language_name(id), |language| language.name)
}

/// The first error the editor shows on the selected lines: servers offer nothing over broken code.
fn line_error(editor: &Entity<Editor>, cx: &App) -> Option<String> {
    let intelligence = EditorIntelligence::of(editor, cx)?;
    let e = editor.read(cx);
    let rope = e.rope();
    let selection = e.selection();
    let first = buffer::line_col(rope, selection.start).0;
    let last = buffer::line_col(rope, selection.end).0;
    let (from, to) = (buffer::line_range(rope, first).start, buffer::line_range(rope, last).end);
    intelligence
        .read(cx)
        .problems()
        .iter()
        .find(|d| d.severity == Severity::Error && d.from <= to && d.to >= from)
        .map(|d| d.message.clone())
}

/// Shows `dialog` in the dialog layer and waits for its answer; a dialog dropped without one
/// answers Cancel.
async fn ask(
    this: &This,
    cx: &mut AsyncWindowContext,
    dialog: impl FnOnce(&mut Window, &mut Context<PromptDialog>) -> PromptDialog + 'static,
) -> anyhow::Result<DialogAnswer> {
    let answer = this.update_in(cx, |workbench, window, cx| workbench.show_prompt_dialog(dialog, window, cx))?;
    Ok(answer.await.unwrap_or_else(|_| DialogAnswer { button: CANCEL.to_owned(), ..DialogAnswer::default() }))
}

// -------------------------------------------------------------------------------------------------
// The Refactor menu actions

/// Starts refactoring `id` at the selection of `editor`: the expression is chosen first when only
/// a caret is there (in a language the structure service parses), then the server is asked.
async fn refactor_menu(
    this: &This,
    editor: &Entity<Editor>,
    id: RefactoringId,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let language = this.update(cx, |_, cx| editor.read(cx).language().clone())?;
    let support =
        call(
            this,
            cx,
            move |host, label| async move { ide_host::ops::syntax::syntax_support(&host, &label, language) },
        )
        .await?;
    let tree = support.is_ok_and(|support| support.tree);
    let request = Backend::next_request_id();
    let step = this.update(cx, |workbench, cx| {
        let at = Caret::of(editor.read(cx));
        let (cancel, step) =
            workbench.refactorings.menu.start(id, request, &at.rope, at.version, at.selection, at.read_only, tree);
        if let Some(cancel) = cancel {
            cancel_request(cancel, cx);
        }
        step
    })?;
    carry_out_menu(this, editor, id, step, cx).await
}

/// Carries out the steps of a Refactor menu action until it shows something or ends.
async fn carry_out_menu(
    this: &This,
    editor: &Entity<Editor>,
    id: RefactoringId,
    mut step: MenuStep,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    loop {
        step = match step {
            MenuStep::Nothing => return Ok(()),
            MenuStep::Hint(text) => return this.update(cx, |_, cx| hint(editor, text, Tone::Info, cx)),
            // The syntax-based Introduce runs only where the structure service parses the
            // language and no server answers; without it the server's reason is shown.
            MenuStep::IntroduceFallback { otherwise, .. } => {
                return this.update(cx, |_, cx| hint(editor, otherwise, Tone::Info, cx));
            }
            MenuStep::ExpressionAt { ticket, range } => {
                let doc = this.update(cx, |_, cx| editor.read(cx).doc())?;
                let result = call(this, cx, move |host, label| async move {
                    ide_host::ops::syntax::syntax_expression_at(&host, &label, doc, range, None).await
                })
                .await?;
                this.update(cx, |workbench, cx| {
                    let at = Caret::of(editor.read(cx));
                    workbench.refactorings.menu.expression_answered(ticket, result, &at.rope, at.version, at.selection)
                })?
            }
            MenuStep::ChooseExpression(choice) => {
                return this.update_in(cx, |workbench, window, cx| {
                    workbench.choose_expression(editor, id, choice, window, cx);
                });
            }
            MenuStep::Query(query) => {
                let doc = this.update(cx, |_, cx| editor.read(cx).doc())?;
                let RefactorQuery { id: request, range, only } = query;
                let result = call(this, cx, move |host, label| async move {
                    ide_host::ops::lsp::lsp_code_action_query(&host, &label, request, doc, range, only).await
                })
                .await?;
                this.update(cx, |workbench, cx| {
                    let at = Caret::of(editor.read(cx));
                    let language = language_name(&at.language, cx);
                    let error = line_error(editor, cx);
                    let i18n = &mut cx.default_global::<Localization>().0;
                    workbench.refactorings.menu.query_answered(
                        request,
                        result,
                        i18n,
                        &at.rope,
                        at.version,
                        at.selection,
                        &language,
                        error.as_deref(),
                    )
                })?
            }
            MenuStep::RunActions(actions) => {
                return this.update_in(cx, |workbench, window, cx| {
                    workbench.run_actions_step(editor, actions, window, cx);
                });
            }
        };
    }
}

// -------------------------------------------------------------------------------------------------
// Refactor This

/// Refactor This at the selection of `editor`: one query for every `refactor*` action, then what
/// Rename and the structure service find at the caret.
async fn refactor_this(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let request = Backend::next_request_id();
    let (doc, query) = this.update(cx, |workbench, cx| {
        let at = Caret::of(editor.read(cx));
        let started =
            workbench.refactorings.this.start(request, &at.rope, at.version, at.selection, &at.language, at.read_only);
        if let Some(cancel) = started.cancel {
            cancel_request(cancel, cx);
        }
        (at.doc, started.query)
    })?;
    let RefactorQuery { id, range, only } = query;
    let result = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_code_action_query(&host, &label, id, doc, range, only).await
    })
    .await?;
    let mut step = this.update(cx, |workbench, cx| {
        let at = Caret::of(editor.read(cx));
        workbench.refactorings.this.query_answered(id, result, at.version, at.selection)
    })?;
    loop {
        match step {
            RefactorThisStep::Nothing => return Ok(()),
            RefactorThisStep::Requests(requests) => {
                let mut next = RefactorThisStep::Nothing;
                for request in requests {
                    let (ticket, answer) = fact(this, doc, request, cx).await?;
                    let answered = this.update(cx, |workbench, cx| {
                        let at = Caret::of(editor.read(cx));
                        workbench.refactorings.this.fact_answered(ticket, answer, at.version, at.selection)
                    })?;
                    if !matches!(answered, RefactorThisStep::Nothing) {
                        next = answered;
                    }
                }
                step = next;
            }
            RefactorThisStep::Popup { title, items, numbered } => {
                return this.update_in(cx, |workbench, window, cx| {
                    workbench.show_refactor_this(editor, title, items, numbered, window, cx);
                });
            }
            RefactorThisStep::NothingHere(_) => {
                return this.update_in(cx, |workbench, window, cx| workbench.show_nothing_here(window, cx));
            }
        }
    }
}

/// Answers one of Refactor This's questions about the caret.
async fn fact(
    this: &This,
    doc: DocId,
    request: FactRequest,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<(u64, FactAnswer)> {
    Ok(match request {
        FactRequest::PrepareRename { ticket, pos } => {
            let prepared = call(this, cx, move |host, label| async move {
                ide_host::ops::lsp::lsp_prepare_rename(&host, &label, doc, pos).await
            })
            .await?;
            (ticket, FactAnswer::PrepareRename(prepared))
        }
        // Without a server Rename renames a local the syntax tree resolves.
        FactRequest::LocalRename { ticket, pos } => {
            let locals = call(this, cx, move |host, label| async move {
                ide_host::ops::syntax::syntax_locals(&host, &label, doc, pos, None).await
            })
            .await?;
            (ticket, FactAnswer::LocalRename(locals.map(|locals| matches!(locals, Locals::Local { .. }))))
        }
        FactRequest::ExpressionAt { ticket, range } => {
            let at = call(this, cx, move |host, label| async move {
                ide_host::ops::syntax::syntax_expression_at(&host, &label, doc, range, None).await
            })
            .await?;
            (ticket, FactAnswer::ExpressionAt(at))
        }
    })
}

// -------------------------------------------------------------------------------------------------
// Safe Delete

/// Safe Delete at the caret of `editor`.
async fn safe_delete(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (doc, step) = this.update(cx, |workbench, cx| {
        let at = Caret::of(editor.read(cx));
        let e = editor.read(cx);
        let own = e.facts().language.comments.clone();
        let comments: CommentTokens = if own.line.is_none() && own.block.is_none() { c_like_comments() } else { own };
        let setup = SafeDeleteSetup {
            path: to_ipc(e.path()),
            language_name: language_name(&at.language, cx),
            comments,
            search_in_comments: workbench.refactorings.search_in_comments,
        };
        let (flow, step) = SafeDeleteFlow::start(&at.rope, at.version, at.selection, at.read_only, setup);
        workbench.refactorings.safe_delete = Some(flow);
        (at.doc, step)
    })?;
    carry_out_safe_delete(this, editor, doc, step, cx).await
}

/// The step of the running Safe Delete, or nothing when another one replaced it.
fn safe_delete_step(
    workbench: &mut Workbench,
    cx: &mut Context<Workbench>,
    next: impl FnOnce(&mut SafeDeleteFlow, &mut crate::platform::i18n::I18n) -> SafeDeleteStep,
) -> SafeDeleteStep {
    let i18n = &mut cx.default_global::<Localization>().0;
    workbench.refactorings.safe_delete.as_mut().map_or(SafeDeleteStep::Nothing, |flow| next(flow, i18n))
}

/// Carries out the steps of Safe Delete until it deletes, says why not, or is cancelled.
async fn carry_out_safe_delete(
    this: &This,
    editor: &Entity<Editor>,
    doc: DocId,
    mut step: SafeDeleteStep,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    loop {
        step = match step {
            SafeDeleteStep::Nothing => return Ok(()),
            SafeDeleteStep::Requests(queries) => {
                let mut next = SafeDeleteStep::Nothing;
                for query in queries {
                    let answer = safe_delete_answer(this, doc, query.request, cx).await?;
                    let answered = this.update(cx, |workbench, cx| {
                        let version = u64::from(editor.read(cx).version());
                        safe_delete_step(workbench, cx, |flow, i18n| flow.answered(query.ticket, answer, version, i18n))
                    })?;
                    if !matches!(answered, SafeDeleteStep::Nothing) {
                        next = answered;
                    }
                }
                next
            }
            SafeDeleteStep::Hint { text, tone } => return this.update(cx, |_, cx| hint(editor, text, tone, cx)),
            SafeDeleteStep::Confirm(question) => {
                let checked = question.checked;
                let request = AskRequest {
                    title: question.title.to_owned(),
                    body: question.message,
                    checkbox: Some(question.checkbox.to_owned()),
                    buttons: vec![
                        AskButton { id: OK, label: question.ok_label, primary: true },
                        AskButton { id: CANCEL, label: "Cancel", primary: false },
                    ],
                    ..AskRequest::default()
                };
                let answer =
                    ask(this, cx, move |window, cx| PromptDialog::ask(request, window, cx).with_checked(checked))
                        .await?;
                let confirmed = (answer.button == OK).then_some(answer.checked);
                this.update(cx, |workbench, cx| {
                    if let Some(on) = confirmed {
                        workbench.refactorings.search_in_comments = on;
                    }
                    let at = Caret::of(editor.read(cx));
                    safe_delete_step(workbench, cx, |flow, i18n| {
                        flow.confirmed(confirmed, at.version, at.selection, i18n)
                    })
                })?
            }
            SafeDeleteStep::Focus => {
                return this.update_in(cx, |_, window, cx| {
                    let focus = editor.focus_handle(cx);
                    window.focus(&focus, cx);
                });
            }
            SafeDeleteStep::UsagesDetected(question) => {
                let request = AskRequest {
                    title: question.title.to_owned(),
                    body: question.body,
                    warning: true,
                    buttons: vec![
                        AskButton { id: "view", label: question.view_label, primary: true },
                        AskButton { id: "delete", label: question.delete_label, primary: false },
                    ],
                    ..AskRequest::default()
                };
                let answer = ask(this, cx, move |window, cx| PromptDialog::ask(request, window, cx)).await?;
                let reply = match answer.button.as_str() {
                    "view" => UsagesAnswer::View,
                    "delete" => UsagesAnswer::Delete,
                    _ => UsagesAnswer::Cancel,
                };
                this.update(cx, |workbench, cx| {
                    let version = u64::from(editor.read(cx).version());
                    safe_delete_step(workbench, cx, |flow, _| flow.usages_answered(reply, version))
                })?
            }
            SafeDeleteStep::ViewUsages { action, symbol, locations } => {
                let arg = json!({ "symbol": symbol, "locations": locations });
                return cx.update(|window, cx| run_action_later(action.to_owned(), Some(arg), window, cx));
            }
            SafeDeleteStep::Preview(preview) => {
                let message = this.update(cx, |_, cx| {
                    let mut lines: Vec<String> = preview.warnings.iter().map(|warning| tx(cx, warning)).collect();
                    lines.extend(preview.occurrences.iter().map(|o| format!("{}: {}", o.line + 1, o.text.trim())));
                    lines.join("\n")
                })?;
                let request = ConfirmRequest {
                    title: preview.title,
                    message,
                    ok_label: preview.confirm_label.to_owned(),
                    danger: true,
                };
                let answer = ask(this, cx, move |window, cx| PromptDialog::confirm(request, window, cx)).await?;
                if !answer.confirmed() {
                    return this.update(cx, |workbench, _| workbench.refactorings.safe_delete = None);
                }
                this.update(cx, |workbench, cx| {
                    let version = u64::from(editor.read(cx).version());
                    safe_delete_step(workbench, cx, |flow, _| flow.preview_confirmed(version))
                })?
            }
            SafeDeleteStep::Delete(edit) => {
                return this.update(cx, |_, cx| {
                    editor.update(cx, |e, cx| e.apply_edit(&edit.changes, edit.selections.clone(), edit.main, cx));
                });
            }
        };
    }
}

/// Asks the backend what Safe Delete needs to know.
async fn safe_delete_answer(
    this: &This,
    doc: DocId,
    request: SafeDeleteRequest,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<SafeDeleteAnswer> {
    Ok(match request {
        SafeDeleteRequest::Definition { pos } => SafeDeleteAnswer::Definition(
            call(this, cx, move |host, label| async move {
                ide_host::ops::lsp::lsp_goto(&host, &label, doc, pos, GotoKind::Definition).await
            })
            .await?,
        ),
        SafeDeleteRequest::Outline => SafeDeleteAnswer::Outline(
            call(this, cx, move |host, label| async move { ide_host::ops::doc::doc_outline(&host, &label, doc).await })
                .await?,
        ),
        SafeDeleteRequest::Locals { pos } => SafeDeleteAnswer::Locals(
            call(this, cx, move |host, label| async move {
                ide_host::ops::syntax::syntax_locals(&host, &label, doc, pos, None).await
            })
            .await?,
        ),
        SafeDeleteRequest::StructuralRanges { pos } => SafeDeleteAnswer::StructuralRanges(
            call(this, cx, move |host, label| async move {
                ide_host::ops::syntax::syntax_structural_ranges(&host, &label, doc, pos, None).await
            })
            .await?,
        ),
        SafeDeleteRequest::References { pos } => {
            let request = Backend::next_request_id();
            SafeDeleteAnswer::References(
                call(this, cx, move |host, label| async move {
                    let partial = ide_host::ops::Channel::new(|_| {});
                    // Project-wide, without the declaration and without the libraries.
                    ide_host::ops::lsp::lsp_references(&host, &label, request, doc, pos, false, true, false, partial)
                        .await
                        .map(|references| references.locations)
                })
                .await?,
            )
        }
        SafeDeleteRequest::Identifiers { name } => SafeDeleteAnswer::Identifiers(
            call(this, cx, move |host, label| async move {
                ide_host::ops::syntax::syntax_identifiers(&host, &label, doc, name, None).await
            })
            .await?,
        ),
    })
}

// -------------------------------------------------------------------------------------------------
// Super Method

/// The document of `path`: the open one, else opened for the search (`true`: close it after).
async fn document_of(host: &Host, label: &str, path: &str) -> IdeResult<(DocId, bool)> {
    let file = PathBuf::from(path);
    if let Some(doc) = host.project(label)?.documents.by_path(&file) {
        return Ok((doc, false));
    }
    // A library class the server returned (`jdt://…`) is opened from its virtual document.
    let opened = if is_virtual_uri(path) {
        ide_host::ops::doc::doc_open_virtual(host, label, path.to_owned()).await
    } else {
        ide_host::ops::doc::open(host, label, file).await
    };
    opened.map(|info| (info.id, true))
}

fn release(host: &Host, label: &str, (doc, opened): (DocId, bool)) {
    if opened && let Err(error) = ide_host::ops::doc::close(host, label, doc) {
        tracing::debug!("super method: cannot close a document: {}", error.message);
    }
}

async fn supertypes_of(host: &Host, label: &str, path: &str, pos: Pos) -> IdeResult<Vec<HierarchyItem>> {
    let document = document_of(host, label, path).await?;
    let incoming = HierarchyDirection::Incoming;
    let found = ide_host::ops::lsp::lsp_type_hierarchy(host, label, document.0, pos, incoming).await;
    release(host, label, document);
    found
}

async fn outline_of(host: &Host, label: &str, path: &str) -> IdeResult<Vec<OutlineItem>> {
    let document = document_of(host, label, path).await?;
    let found = ide_host::ops::doc::doc_outline(host, label, document.0).await;
    release(host, label, document);
    found
}

/// The super methods of `method` (or the supertypes of `r#type` outside a method), searched up the
/// type hierarchy; for languages whose servers have none, the declaration (Rust) or the
/// implementation (Go) the server answers instead.
async fn super_locations(
    host: &Host,
    label: &str,
    doc: DocId,
    path: &str,
    language: &str,
    method: Option<&OutlineItem>,
    r#type: Option<&OutlineItem>,
) -> Vec<Location> {
    let mut found: Vec<Location> = Vec::new();
    if let (Some(method), Some(owner)) = (method, r#type) {
        let at = owner.selection_range.start;
        let (mut search, mut step) = SuperMethodSearch::start(path, at, &method.name, SUPER_DEPTH);
        loop {
            let next = match step {
                SuperStep::Done(supers) => {
                    found = supers.into_iter().map(|s| s.location).collect();
                    break;
                }
                SuperStep::Supertypes { ticket, path, pos } => {
                    search.supertypes(ticket, supertypes_of(host, label, &path, pos).await)
                }
                SuperStep::Outline { ticket, path } => search.outline(ticket, outline_of(host, label, &path).await),
            };
            let Some(next) = next else { break };
            step = next;
        }
    } else if let Some(owner) = r#type {
        let supers = supertypes_of(host, label, path, owner.selection_range.start).await.unwrap_or_default();
        found = supers.into_iter().map(|s| Location { path: s.path, range: s.selection_range, uri: s.uri }).collect();
    }
    if found.is_empty()
        && let Some(method) = method
    {
        let kind = match language {
            "go" => Some(GotoKind::Implementation),
            "rust" => Some(GotoKind::Declaration),
            _ => None,
        };
        if let Some(kind) = kind {
            let at = method.selection_range.start;
            let targets = ide_host::ops::lsp::lsp_goto(host, label, doc, at, kind).await.unwrap_or_default();
            found = targets
                .into_iter()
                .filter(|target| !(to_ipc(&target.path.0) == path && target.range.start.line == at.line))
                .collect();
        }
    }
    let mut unique: Vec<Location> = Vec::new();
    for location in found {
        let seen = unique.iter().any(|known| known.path == location.path && known.range.start == location.range.start);
        if !seen {
            unique.push(location);
        }
    }
    unique
}

/// Super Method from the caret of `editor`: one super method opens at once, several are offered.
async fn super_method(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (doc, path, caret, language) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), to_ipc(e.path()), offset_to_pos(e.rope(), e.caret()), e.language().as_str().to_owned())
    })?;
    let outline =
        call(this, cx, move |host, label| async move { ide_host::ops::doc::doc_outline(&host, &label, doc).await })
            .await?;
    let outline = match outline {
        Ok(outline) => outline,
        Err(error) if is_cancelled(&error) => return Ok(()),
        Err(_) => return this.update(cx, |_, cx| hint(editor, NO_STRUCTURE, Tone::Info, cx)),
    };
    let member = member_at(&outline, caret);
    let (method, owner) = (member.method.cloned(), member.r#type.cloned());
    let in_method = method.is_some();
    let method_name = method.as_ref().map(|method| base_name(&method.name));
    let found = call(this, cx, move |host, label| async move {
        Ok(super_locations(&host, &label, doc, &path, &language, method.as_ref(), owner.as_ref()).await)
    })
    .await?;
    let locations = match found {
        Ok(locations) => locations,
        Err(error) => return this.update(cx, |_, cx| report(editor, &error, cx)),
    };
    this.update_in(cx, |workbench, window, cx| match locations.as_slice() {
        [] => hint(editor, if in_method { NO_SUPER_METHOD } else { NO_SUPER_CLASS }, Tone::Info, cx),
        [one] => {
            let start = one.range.start;
            workbench.open_project_file_at(one.path.0.clone(), start.line, start.col, window, cx);
        }
        several => {
            let i18n = &mut cx.default_global::<Localization>().0;
            let title = match &method_name {
                Some(name) => i18n.tx_built(
                    &["Choose Super Method of ", " (", " found)"],
                    &[Param::from(name.as_str()), Param::from(several.len())],
                ),
                None => i18n.tx_built(&["Choose Super Class or Interface (", " found)"], &[Param::from(several.len())]),
            };
            workbench.choose_location(&title, several.to_vec(), window, cx);
        }
    })
}

// -------------------------------------------------------------------------------------------------
// The window's side

impl Workbench {
    /// Starts refactoring `id` in `editor` (a Refactor menu action, or a Refactor This row).
    fn start_refactoring(
        &mut self,
        editor: Entity<Editor>,
        id: RefactoringId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| finished(refactor_menu(&this, &editor, id, cx).await)).detach();
    }

    fn start_refactor_this(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| finished(refactor_this(&this, &editor, cx).await)).detach();
    }

    fn start_safe_delete(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| finished(safe_delete(&this, &editor, cx).await)).detach();
    }

    fn start_super_method(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| finished(super_method(&this, &editor, cx).await)).detach();
    }

    /// Applies a refactoring's code action as the code-action chooser does.
    fn apply_refactoring(
        &mut self,
        editor: &Entity<Editor>,
        action: CodeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (editor, doc) = (editor.clone(), editor.read(cx).doc());
        cx.spawn_in(window, async move |this, cx| {
            finished(apply_code_action(&this, &editor, doc, action, cx).await);
        })
        .detach();
    }

    /// A list popup titled `title` (in the UI language); choosing a row calls `chosen` with its id
    /// once the popup is gone.
    fn open_refactor_list(
        &mut self,
        title: &str,
        rows: Vec<ListRow>,
        options: ListOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, &str, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let list = cx.new(|cx| ListPopup::new(title, rows, options, cx));
        let events =
            cx.subscribe_in(&list, window, move |workbench, _, event: &ListPopupEvent, window, cx| match event {
                ListPopupEvent::Activated(id) => {
                    workbench.close_popup(window, cx);
                    chosen(workbench, id, window, cx);
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
                    workbench.close_popup(window, cx);
                }
                ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_) => {}
            });
        self.open_popup(&list, events, window, cx);
        list.update(cx, |list, cx| list.opened(cx));
    }

    /// Runs a refactoring's server actions: the single one is applied, several are offered, and
    /// the two occurrence variants get "Multiple occurrences found".
    fn run_actions_step(
        &mut self,
        editor: &Entity<Editor>,
        step: ActionsStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = editor.downgrade();
        let plain = ListOptions::default;
        match step {
            ActionsStep::Apply { action, .. } => self.apply_refactoring(editor, *action, window, cx),
            ActionsStep::Choose { title, actions, .. } => {
                let rows = actions
                    .iter()
                    .enumerate()
                    .map(|(index, action)| ListRow {
                        id: index.to_string(),
                        label: action.title.clone(),
                        icon: Some("bulb".to_owned()),
                        emphasis: action.is_preferred,
                        ..ListRow::default()
                    })
                    .collect();
                let title = tx(cx, &title);
                self.open_refactor_list(&title, rows, plain(), window, cx, move |workbench, id, window, cx| {
                    let chosen = id.parse::<usize>().ok().and_then(|index| actions.get(index).cloned());
                    if let (Some(action), Some(editor)) = (chosen, weak.upgrade()) {
                        workbench.apply_refactoring(&editor, action, window, cx);
                    }
                });
            }
            ActionsStep::Occurrences { choice, one, all } => {
                let rows = vec![
                    ListRow { id: "one".to_owned(), label: choice.one.label.clone(), ..ListRow::default() },
                    ListRow { id: "all".to_owned(), label: choice.all.label.clone(), ..ListRow::default() },
                ];
                let title = tx(cx, choice.title);
                self.open_refactor_list(&title, rows, plain(), window, cx, move |workbench, id, window, cx| {
                    let action = if id == "all" { (*all).clone() } else { (*one).clone() };
                    if let Some(editor) = weak.upgrade() {
                        workbench.apply_refactoring(&editor, action, window, cx);
                    }
                });
            }
        }
    }

    /// With only a caret, the expression is chosen first: it is selected and the refactoring runs
    /// again on it, so the server extracts exactly that expression.
    fn choose_expression(
        &mut self,
        editor: &Entity<Editor>,
        id: RefactoringId,
        choice: ExpressionChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match choice {
            ExpressionChoice::None => {}
            ExpressionChoice::Select(span) => {
                editor.update(cx, |e, cx| e.select(span.from, span.to, cx));
                self.start_refactoring(editor.clone(), id, window, cx);
            }
            ExpressionChoice::Choose { title, rows, spans } => {
                let version = editor.read(cx).version();
                let rows = rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, row)| ListRow { id: index.to_string(), label: row.label, ..ListRow::default() })
                    .collect();
                let weak = editor.downgrade();
                let title = tx(cx, title);
                let options = ListOptions::default();
                self.open_refactor_list(&title, rows, options, window, cx, move |workbench, row, window, cx| {
                    let Some(editor) = weak.upgrade() else { return };
                    let span = row.parse::<usize>().ok().and_then(|index| spans.get(index).copied());
                    let Some(span) = span else { return };
                    if editor.read(cx).version() != version {
                        hint(&editor, DOCUMENT_CHANGED, Tone::Info, cx);
                        return;
                    }
                    editor.update(cx, |e, cx| e.select(span.from, span.to, cx));
                    workbench.start_refactoring(editor, id, window, cx);
                });
            }
        }
    }

    /// The Refactor This list, numbered (1–9 run a row). Rows whose refactoring this window does
    /// not carry out are left out: the syntax-based Introduce and, without their actions, the
    /// class-hierarchy refactorings.
    fn show_refactor_this(
        &mut self,
        editor: &Entity<Editor>,
        title: &str,
        items: Vec<RefactorItem>,
        numbered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let registry = cx.global::<ActionRegistry>();
        let items: Vec<RefactorItem> = items
            .into_iter()
            .filter(|item| match &item.run {
                RefactorRun::IntroduceFallback { .. } => false,
                RefactorRun::Hierarchy(kind) => registry.contains(hierarchy_action(*kind)),
                _ => true,
            })
            .collect();
        if items.is_empty() {
            return self.show_nothing_here(window, cx);
        }
        let rows = items
            .iter()
            .enumerate()
            .map(|(index, item)| ListRow {
                id: index.to_string(),
                label: tx(cx, &item.label),
                icon: item.icon.map(str::to_owned),
                emphasis: item.emphasis,
                shortcut: item.shortcut_action.and_then(|action| shortcut(cx, action)),
                ..ListRow::default()
            })
            .collect();
        let title = tx(cx, title);
        let weak = editor.downgrade();
        let options = ListOptions { numbered, ..ListOptions::default() };
        self.open_refactor_list(&title, rows, options, window, cx, move |workbench, id, window, cx| {
            let chosen = id.parse::<usize>().ok().and_then(|index| items.get(index)).map(|item| item.run.clone());
            if let (Some(run), Some(editor)) = (chosen, weak.upgrade()) {
                workbench.run_refactor_item(editor, run, window, cx);
            }
        });
    }

    fn run_refactor_item(
        &mut self,
        editor: Entity<Editor>,
        run: RefactorRun,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match run {
            RefactorRun::Menu(id) => self.start_refactoring(editor, id, window, cx),
            RefactorRun::Actions { title, actions, id } => {
                let at = Caret::of(editor.read(cx));
                let i18n = &mut cx.default_global::<Localization>().0;
                let step = actions_step(i18n, &at.rope, at.selection, &title, actions, id);
                self.run_actions_step(&editor, step, window, cx);
            }
            RefactorRun::Rename => run_action_later("refactor.rename".to_owned(), None, window, cx),
            RefactorRun::SafeDelete => self.start_safe_delete(editor, window, cx),
            RefactorRun::Hierarchy(kind) => run_action_later(hierarchy_action(kind).to_owned(), None, window, cx),
            // Never listed: show_refactor_this leaves these rows out.
            RefactorRun::IntroduceFallback { .. } => {}
            RefactorRun::Apply(action) => self.apply_refactoring(&editor, *action, window, cx),
        }
    }

    /// IDEA's "Nothing here": Refactor This where no refactoring applies, or outside an editor.
    fn show_nothing_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = vec![ListRow {
            id: "nothing".to_owned(),
            label: tx(cx, NOTHING_HERE.label),
            disabled: Some(tx(cx, NOTHING_HERE.disabled)),
            ..ListRow::default()
        }];
        let title = tx(cx, NOTHING_HERE.title);
        self.open_refactor_list(&title, rows, ListOptions::default(), window, cx, |_, _, _, _| {});
    }

    /// Several super methods or supertypes: the chosen one opens.
    fn choose_location(&mut self, title: &str, locations: Vec<Location>, window: &mut Window, cx: &mut Context<Self>) {
        let rows = locations
            .iter()
            .enumerate()
            .map(|(index, location)| {
                let file = location.path.0.file_name();
                let name = file.map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                ListRow {
                    id: index.to_string(),
                    label: format!("{name}:{}", location.range.start.line + 1),
                    detail: location.path.0.parent().map(|parent| parent.display().to_string()),
                    ..ListRow::default()
                }
            })
            .collect();
        self.open_refactor_list(title, rows, ListOptions::default(), window, cx, move |workbench, id, window, cx| {
            if let Some(location) = id.parse::<usize>().ok().and_then(|index| locations.get(index)) {
                let start = location.range.start;
                workbench.open_project_file_at(location.path.0.clone(), start.line, start.col, window, cx);
            }
        });
    }
}

fn hierarchy_action(kind: HierarchyRefactoring) -> &'static str {
    match kind {
        HierarchyRefactoring::PullUp => "refactor.pullMembersUp",
        HierarchyRefactoring::PushDown => "refactor.pushMembersDown",
    }
}

/// Registers the refactorings of this module.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for (action, id) in MENU_REFACTORINGS {
        registry.handle(action, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| {
                if let Some(editor) = workbench.editor().cloned() {
                    workbench.start_refactoring(editor, id, window, cx);
                }
            })
        });
    }
    registry.handle("refactor.thisMenu", |call, window, cx| {
        let in_editor = evaluate_when(Some(EDITOR_WHEN), call.ctx).unwrap_or(false);
        on_workbench(window, cx, move |workbench, window, cx| match workbench.editor().cloned().filter(|_| in_editor) {
            Some(editor) => workbench.start_refactor_this(editor, window, cx),
            None => workbench.show_nothing_here(window, cx),
        })
    });
    registry.handle("refactor.safeDelete", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some(editor) = workbench.editor().cloned() {
                workbench.start_safe_delete(editor, window, cx);
            }
        })
    });
    registry.handle("navigate.superMethod", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            if let Some(editor) = workbench.editor().cloned() {
                workbench.start_super_method(editor, window, cx);
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::context_is_on;
    use super::*;
    use crate::backend::AppDirs;
    use crate::editor::intelligence::fake_server::FakeServer;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use crate::workbench;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::Value;
    use std::path::Path;
    use std::time::Duration;

    /// A trusted project of `files` in a window, in Full mode with the fake server answering
    /// `*.mock` files with the capabilities `capabilities` (read when it starts), `open` open in the
    /// editor with the keyboard and known to the server.
    struct Fixture {
        workbench: Entity<Workbench>,
        root: PathBuf,
        server: FakeServer,
        capabilities: Value,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
        open: &str,
        capabilities: Value,
    ) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let dirs = AppDirs::under(home.path());
        let server = FakeServer::install(&dirs.config, home.path());
        server.answer(json!({ "capabilities": capabilities.clone() }));
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            std::fs::write(root.join(path), text).expect("the file is written");
        }
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            workbench::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(dirs, cx).expect("the backend starts");
            workbench::menu::init(cx);
            let host = cx.global::<Backend>().host.clone();
            let global = ide_workspace::settings::SettingsScope::Global;
            host.global_settings.set(global, "ide.mode", json!("full")).expect("the mode is set");
            host.trust.decide(&root, true, false).expect("the project is trusted");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, "the project tree", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let file = root.join(open);
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
        let fixture = Fixture { workbench, root, server, capabilities, _folders: (home, folder) };
        settle(cx, "the editor takes the keyboard", |cx| {
            let editor = fixture.editor(cx);
            cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
        });
        fixture.wait_opened(cx, open);
        (fixture, cx)
    }

    /// Waits for `done`, named `what`, while the backend's work comes back, moving the clock so
    /// debounced work runs.
    #[track_caller]
    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what} did not happen within thirty seconds");
    }

    fn range(line: u32, from: u32, to: u32) -> Value {
        json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
    }

    fn span(from: (u32, u32), to: (u32, u32)) -> Value {
        json!({ "start": { "line": from.0, "character": from.1 }, "end": { "line": to.0, "character": to.1 } })
    }

    fn uri(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    /// A code action titled `title` of `kind` that inserts `text` at the start of `path`.
    fn inserting(path: &Path, title: &str, kind: &str, text: &str) -> Value {
        let edit = json!({ "changes": { uri(path): [{ "range": range(0, 0, 0), "newText": text }] } });
        json!({ "title": title, "kind": kind, "edit": edit })
    }

    impl Fixture {
        fn editor(&self, cx: &mut VisualTestContext) -> Option<Entity<Editor>> {
            self.workbench.read_with(cx, |workbench, _| workbench.editor().cloned())
        }

        fn active(&self, cx: &mut VisualTestContext) -> Entity<Editor> {
            self.editor(cx).expect("an editor")
        }

        fn text(&self, cx: &mut VisualTestContext) -> String {
            self.active(cx).read_with(cx, |editor, _| editor.text())
        }

        fn caret(&self, cx: &mut VisualTestContext) -> u32 {
            self.active(cx).read_with(cx, |editor, _| editor.caret())
        }

        fn editing(&self, cx: &mut VisualTestContext, path: &Path) -> bool {
            self.editor(cx).is_some_and(|editor| editor.read_with(cx, |e, _| e.path() == path))
        }

        fn hint(&self, cx: &mut VisualTestContext) -> Option<String> {
            let editor = self.active(cx);
            let intelligence =
                cx.update(|_, cx| EditorIntelligence::of(&editor, cx)).expect("the editor's intelligence");
            intelligence.read_with(cx, |this, _| this.hint().map(str::to_owned))
        }

        fn wait_hint(&self, cx: &mut VisualTestContext, text: &str) {
            settle(cx, text, |cx| self.hint(cx).as_deref() == Some(text));
        }

        /// The server answers `answers`, with the fixture's capabilities.
        fn answer(&self, mut answers: Value) {
            answers["capabilities"] = self.capabilities.clone();
            self.server.answer(answers);
        }

        fn requests(&self, method: &str) -> Vec<Value> {
            self.server.received().into_iter().filter(|message| message["method"] == method).collect()
        }

        fn wait_opened(&self, cx: &mut VisualTestContext, name: &str) {
            let name = format!("/{name}");
            settle(cx, "the server hears the opened document", |_| {
                let opened = self.requests("textDocument/didOpen");
                let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
                opened.iter().filter_map(uri).any(|uri| uri.ends_with(&name))
            });
        }

        fn select(&self, cx: &mut VisualTestContext, from: u32, to: u32) {
            let editor = self.active(cx);
            editor.update(cx, |editor, cx| editor.select(from, to, cx));
            cx.run_until_parked();
        }

        fn run(&self, cx: &mut VisualTestContext, id: &str) {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
                .unwrap_or_else(|error| panic!("{id}: {error}"));
            cx.run_until_parked();
        }

        /// Whether the action `id` ran (its `when` clause held).
        fn ran(&self, cx: &mut VisualTestContext, id: &str) -> bool {
            let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).is_ok();
            cx.run_until_parked();
            ran
        }

        fn list(&self, cx: &mut VisualTestContext) -> Entity<ListPopup> {
            settle(cx, "the list popup", |cx| {
                self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_some())
            });
            self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>()).expect("the list is open")
        }

        fn no_list(&self, cx: &mut VisualTestContext) -> bool {
            self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none())
        }

        fn prompt(&self, cx: &mut VisualTestContext) -> Entity<PromptDialog> {
            settle(cx, "the dialog", |cx| self.workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
            self.workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the dialog is open")
        }

        /// How many code-action requests asked only for `kinds` (the fake server ignores `only`, so
        /// a test counts them before and after an action).
        fn asked_only(&self, kinds: &[&str]) -> usize {
            let only: Vec<Value> = kinds.iter().map(|kind| json!(kind)).collect();
            let only = json!(only);
            self.requests("textDocument/codeAction").iter().filter(|r| r["params"]["context"]["only"] == only).count()
        }

        fn close(&self, cx: &mut VisualTestContext) {
            cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
        }
    }

    /// Each Refactor menu action and the code actions it applies: its kind asked of the server,
    /// the code action's title it maps from, and the kinds it asks for.
    const MENU_CASES: [(&str, &str, &str, &str); 8] = [
        ("refactor.extractVariable", "Extract into variable", "refactor.extract.variable", "refactor.extract"),
        ("refactor.extractMethod", "Extract into function", "refactor.extract.function", "refactor.extract"),
        ("refactor.extractConstant", "Extract into constant", "refactor.extract.constant", "refactor.extract"),
        ("refactor.extractField", "Extract into field", "refactor.extract.field", "refactor.extract"),
        ("refactor.extractParameter", "Extract into parameter", "refactor.extract.parameter", "refactor.extract"),
        ("refactor.move", "Move to a new file", "refactor.move.newFile", "refactor.move"),
        ("refactor.changeSignature", "Change signature", "refactor.rewrite.signature", "refactor.rewrite"),
        ("refactor.inline", "Inline variable", "refactor.inline", "refactor.inline"),
    ];

    /// Every Refactor menu action asks the server for the code actions of its kind at the
    /// selection and applies the one that implements it: its edit lands in the document. (The
    /// choice between several is in the next test.)
    #[gpui_kit::test]
    async fn apply_the_single_matching_action_or_offer_a_choice(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", "let total = price * 2\n")], "a.mock", json!({}));
        let a = f.root.join("a.mock");
        let actions: Vec<Value> = MENU_CASES
            .iter()
            .map(|(action, title, kind, _)| inserting(&a, title, kind, &format!("// {action}\n")))
            .collect();
        f.answer(json!({ "textDocument/codeAction": actions }));
        let mut expected = "let total = price * 2\n".to_owned();
        for (action, _, _, only) in MENU_CASES {
            let asked = f.asked_only(&[only]);
            f.select(cx, 0, 0);
            f.run(cx, action);
            expected = format!("// {action}\n{expected}");
            settle(cx, action, |cx| f.text(cx) == expected);
            assert_eq!(f.asked_only(&[only]), asked + 1, "{action} asks the server for {only}");
        }
        f.close(cx);
    }

    /// Several actions of the refactoring are offered in a list titled after it and the chosen one
    /// is applied; a single-occurrence and an all-occurrences variant get "Multiple occurrences
    /// found".
    #[gpui_kit::test]
    async fn offer_idea_s_occurrences_choice_when_the_server_has_both_variants(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", "g(price)\nh(price)\n")], "a.mock", json!({}));
        let a = f.root.join("a.mock");
        f.answer(json!({ "textDocument/codeAction": [
            inserting(&a, "Inline variable", "refactor.inline", "// variable\n"),
            inserting(&a, "Inline call", "refactor.inline.call", "// call\n"),
            inserting(&a, "Convert to raw string", "refactor.rewrite", "// raw\n"),
        ] }));
        f.select(cx, 0, 0);
        f.run(cx, "refactor.inline");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Inline");
        assert_eq!(list.read_with(cx, |l, _| l.lines()), ["Inline variable", "Inline call"]);
        cx.simulate_keystrokes("down enter");
        settle(cx, "the chosen action's edit", |cx| f.text(cx) == "// call\ng(price)\nh(price)\n");

        let all = "Extract to local variable (replace all occurrences)";
        f.answer(json!({ "textDocument/codeAction": [
            inserting(&a, "Extract to local variable", "refactor.extract.variable", "// one\n"),
            inserting(&a, all, "refactor.extract.variable", "// all\n"),
        ] }));
        let from = "// call\ng(".encode_utf16().count() as u32;
        f.select(cx, from, from + 5);
        f.run(cx, "refactor.extractVariable");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Multiple occurrences found");
        assert_eq!(list.read_with(cx, |l, _| l.lines()), ["Replace this occurrence only", "Replace all 2 occurrences"]);
        cx.simulate_keystrokes("down enter");
        settle(cx, "the all-occurrences edit", |cx| f.text(cx).starts_with("// all\n"));
        f.close(cx);
    }

    /// Without a matching action each Refactor menu action says why with IDEA's "Cannot perform
    /// refactoring." and the reason for a caret, or the server's reason for a disabled action;
    /// unrelated code actions are never applied. (No running server is the model's test: the fake
    /// server always runs here.)
    #[gpui_kit::test]
    async fn give_idea_s_caret_reason_the_server_s_disabled_reason_or_the_missing_server(cx: &mut TestAppContext) {
        let text = "let total = price * 2\n";
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", json!({}));
        let a = f.root.join("a.mock");
        let unrelated = inserting(&a, "Generate constructor", "source.generate", "x\n");
        f.answer(json!({ "textDocument/codeAction": [unrelated] }));
        let at_name = "Caret should be positioned at the name of the element to be refactored";
        let at_function = "Caret should be positioned at the name of the function or method to be refactored";
        // No two neighbours give the same reason: each hint is a new one.
        let cases = [
            ("refactor.inline", at_name),
            ("refactor.extractVariable", "Caret should be inside an expression"),
            ("refactor.move", at_name),
            ("refactor.extractConstant", "Caret should be inside an expression"),
            ("refactor.changeSignature", at_function),
            ("refactor.extractField", "Caret should be inside an expression in a class"),
            ("refactor.extractMethod", "Select the code to extract"),
            ("refactor.extractParameter", "Caret should be inside an expression in a function"),
        ];
        f.select(cx, 4, 4);
        for (action, reason) in cases {
            f.run(cx, action);
            f.wait_hint(cx, &format!("Cannot perform refactoring. {reason}"));
        }
        let disabled = json!({ "title": "Extract into function", "kind": "refactor.extract.function",
                               "disabled": { "reason": "Selection is not a statement" } });
        f.answer(json!({ "textDocument/codeAction": [disabled] }));
        f.run(cx, "refactor.extractMethod");
        f.wait_hint(cx, "Cannot perform refactoring. Selection is not a statement");
        assert_eq!(f.text(cx), text);
        f.close(cx);
    }

    /// A server whose advertised kinds cannot produce the refactoring is said not to support it.
    #[gpui_kit::test]
    async fn say_the_server_doesn_t_support_it_when_its_advertised_kinds_can_t_produce_it(cx: &mut TestAppContext) {
        let capabilities = json!({ "codeActionProvider": {
            "resolveProvider": true,
            "codeActionKinds": ["quickfix", "refactor.extract", "refactor.inline"],
        } });
        let (f, cx) = open(cx, &[("a.mock", "let total = price\n")], "a.mock", capabilities);
        f.answer(json!({ "textDocument/codeAction": [] }));
        f.run(cx, "refactor.move");
        f.wait_hint(cx, "Cannot perform refactoring. The language server for Mock doesn't support Move");
        f.close(cx);
    }

    fn refactor_this_capabilities() -> Value {
        json!({ "renameProvider": { "prepareProvider": true } })
    }

    /// Refactor This lists, numbered and in IDEA's order, the refactorings that apply at the caret
    /// and then the server's other refactorings; choosing Inline… applies the server's action.
    #[gpui_kit::test]
    async fn lists_only_the_refactorings_applicable_at_the_caret_in_idea_order(cx: &mut TestAppContext) {
        let text = "let total = price\n";
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", refactor_this_capabilities());
        let a = f.root.join("a.mock");
        f.answer(json!({
            "textDocument/prepareRename": range(0, 4, 9),
            "textDocument/codeAction": [
                inserting(&a, "Inline variable", "refactor.inline", "// inlined\n"),
                inserting(&a, "Convert to raw string", "refactor.rewrite", "// raw\n"),
                json!({ "title": "Extract into function", "kind": "refactor.extract",
                        "disabled": { "reason": "Can't extract here" } }),
            ],
        }));
        f.select(cx, 5, 5);
        f.run(cx, "refactor.thisMenu");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Refactor This");
        assert_eq!(
            list.read_with(cx, |l, _| l.lines()),
            ["1 Rename…", "2 Safe Delete…", "3 Inline…", "4 Convert to raw string"]
        );
        assert!(f.asked_only(&["refactor"]) >= 1, "Refactor This asks for every refactor* action");
        cx.simulate_keystrokes("down down enter");
        settle(cx, "the inline edit", |cx| f.text(cx) == format!("// inlined\n{text}"));
        f.close(cx);
    }

    /// Refactor This where nothing applies, in the editor and outside it (a tool window has the
    /// keyboard), shows IDEA's "Nothing here".
    #[gpui_kit::test]
    async fn says_so_when_nothing_applies(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", "let total = price\n")], "a.mock", refactor_this_capabilities());
        f.answer(json!({ "textDocument/prepareRename": null, "textDocument/codeAction": [] }));
        f.run(cx, "refactor.thisMenu");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Refactor This");
        assert_eq!(list.read_with(cx, |l, _| l.lines()), ["Nothing here"]);
        cx.simulate_keystrokes("escape");
        settle(cx, "the popup closes", |cx| f.no_list(cx));

        let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a project tree");
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        let asked = f.asked_only(&["refactor"]);
        f.run(cx, "refactor.thisMenu");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.lines()), ["Nothing here"]);
        assert_eq!(f.asked_only(&["refactor"]), asked, "outside the editor the server is not asked");
        f.close(cx);
    }

    /// Refactor This (`projectOpen`) runs while a project is open, and once the window has closed its
    /// project the registry refuses it and no popup opens.
    #[gpui_kit::test]
    async fn refactor_this_is_not_available_once_the_project_is_closed(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", "let total = price\n")], "a.mock", refactor_this_capabilities());
        f.answer(json!({ "textDocument/prepareRename": null, "textDocument/codeAction": [] }));
        assert!(context_is_on(cx, "projectOpen"));
        assert!(f.ran(cx, "refactor.thisMenu"));
        f.list(cx);
        cx.simulate_keystrokes("escape");
        settle(cx, "the popup closes", |cx| f.no_list(cx));

        f.workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
        settle(cx, "the project closes", |cx| !context_is_on(cx, "projectOpen"));
        assert!(!f.ran(cx, "refactor.thisMenu"), "Refactor This needs an open project");
        assert!(f.no_list(cx));
    }

    /// The editor refactorings need the editor (`textEditorActive && (editorFocus || popupOpen ||
    /// !toolWindowFocus)`): with the Project view focused none of them runs, and the text stays;
    /// back in the editor they run.
    #[gpui_kit::test]
    async fn the_editor_refactorings_do_nothing_while_a_tool_window_has_the_keyboard(cx: &mut TestAppContext) {
        let text = "let total = price\n";
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", json!({}));
        let a = f.root.join("a.mock");
        f.answer(json!({ "textDocument/codeAction": [inserting(&a, "Inline variable", "refactor.inline", "// x\n")] }));
        let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a project tree");
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        let ids: Vec<&str> = MENU_CASES
            .iter()
            .map(|(action, ..)| *action)
            .chain(["refactor.safeDelete", "navigate.superMethod"])
            .collect();
        for id in &ids {
            assert!(!f.ran(cx, id), "{id} runs only in the editor");
        }
        cx.run_until_parked();
        assert_eq!(f.text(cx), text);
        assert!(f.no_list(cx));
        assert!(f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));

        let editor = f.active(cx);
        cx.update(|window, cx| {
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert!(f.ran(cx, "refactor.inline"));
        settle(cx, "the inline edit", |cx| f.text(cx) == format!("// x\n{text}"));
        f.close(cx);
    }

    fn safe_delete_capabilities() -> Value {
        json!({ "referencesProvider": true, "documentSymbolProvider": true })
    }

    fn function(name: &str, line: u32, end: u32) -> Value {
        let at = 3 + name.encode_utf16().count() as u32;
        json!({ "name": name, "kind": 12, "range": span((line, 0), (line, end)), "selectionRange": range(line, 3, at) })
    }

    /// The text of the Safe Delete tests:
    /// `unused` calls itself.
    const RECURSIVE: &str = "fn unused() { unused() }\nfn main() {}\n";

    fn recursive_answers(a: &Path) -> Value {
        json!({
            "textDocument/definition": [{ "uri": uri(a), "range": range(0, 3, 9) }],
            "textDocument/documentSymbol": [function("unused", 0, 24), function("main", 1, 12)],
            "textDocument/references": [{ "uri": uri(a), "range": range(0, 14, 20) }],
        })
    }

    /// Safe Delete asks "Delete Function 'unused'?" first; Cancel searches nothing and changes
    /// nothing.
    #[gpui_kit::test]
    async fn confirms_first_delete_function_and_searches_nothing_when_cancelled(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", RECURSIVE)], "a.mock", safe_delete_capabilities());
        f.answer(recursive_answers(&f.root.join("a.mock")));
        f.select(cx, 5, 5);
        f.run(cx, "refactor.safeDelete");
        let dialog = f.prompt(cx);
        assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "Safe Delete");
        let message = dialog.read_with(cx, |d, _| d.message_text().map(str::to_owned));
        assert_eq!(message.as_deref(), Some("Delete Function 'unused'?"));
        assert_eq!(dialog.read_with(cx, |d, _| d.checked()), Some(true), "Search in comments and strings is on");
        cx.simulate_keystrokes("escape");
        settle(cx, "the dialog closes", |cx| f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
        assert!(f.requests("textDocument/references").is_empty(), "nothing is searched");
        assert_eq!(f.text(cx), RECURSIVE);
        f.close(cx);
    }

    /// Confirmed, Safe Delete finds no usage outside the declaration (its recursive call does not
    /// count) and deletes it with its line; one Undo brings it back.
    #[gpui_kit::test]
    async fn deletes_an_unused_declaration_as_one_undo_step_ignoring_recursive_calls(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", RECURSIVE)], "a.mock", safe_delete_capabilities());
        f.answer(recursive_answers(&f.root.join("a.mock")));
        f.select(cx, 5, 5);
        f.run(cx, "refactor.safeDelete");
        f.prompt(cx);
        cx.simulate_keystrokes("enter");
        settle(cx, "the declaration is deleted", |cx| f.text(cx) == "fn main() {}\n");
        let asked = f.requests("textDocument/references");
        let declaration = asked.last().map(|r| r["params"]["context"]["includeDeclaration"].clone());
        assert_eq!(declaration, Some(json!(false)));
        f.run(cx, "edit.undo");
        settle(cx, "one undo restores it", |cx| f.text(cx) == RECURSIVE);
        f.close(cx);
    }

    /// A declaration still used elsewhere gets IDEA's "Usages Detected" with View Usages and Delete
    /// Anyway; cancelling it changes nothing.
    #[gpui_kit::test]
    async fn asks_when_usages_exist_view_usages_delete_anyway_cancel(cx: &mut TestAppContext) {
        let text = "fn used() {}\nfn main() {}\n";
        let files = [("a.mock", text), ("b.mock", "used()\n")];
        let (f, cx) = open(cx, &files, "a.mock", safe_delete_capabilities());
        let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
        f.answer(json!({
            "textDocument/definition": [{ "uri": uri(&a), "range": range(0, 3, 7) }],
            "textDocument/documentSymbol": [function("used", 0, 12), function("main", 1, 12)],
            "textDocument/references": [{ "uri": uri(&b), "range": range(0, 0, 4) }],
        }));
        f.select(cx, 4, 4);
        f.run(cx, "refactor.safeDelete");
        let confirm = f.prompt(cx);
        assert_eq!(confirm.read_with(cx, |d, _| d.title().to_owned()), "Safe Delete");
        cx.simulate_keystrokes("enter");
        settle(cx, "Usages Detected", |cx| {
            f.workbench.read_with(cx, |w, cx| w.vcs_prompt().is_some_and(|d| d.read(cx).title() == "Usages Detected"))
        });
        let usages = f.prompt(cx);
        let buttons: Vec<String> = usages.read_with(cx, |d, _| d.buttons().iter().map(|b| b.label.clone()).collect());
        assert_eq!(buttons, ["View Usages", "Delete Anyway"]);
        assert!(usages.read_with(cx, |d, _| d.message_text().is_some_and(|m| m.starts_with("1 usage found"))));
        cx.simulate_keystrokes("escape");
        settle(cx, "the dialog closes", |cx| f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
        assert_eq!(f.text(cx), text);
        assert!(f.editing(cx, &a));
        f.close(cx);
    }

    const CLASSES: &str = "class B {\n  run() {}\n}\nclass A extends B {\n  run() {}\n}\n";

    fn super_capabilities() -> Value {
        json!({ "typeHierarchyProvider": true, "documentSymbolProvider": true })
    }

    /// The outline of [`CLASSES`]: `B` with `run` on line 1, `A` with `run` on line 4.
    fn classes_outline() -> Value {
        let class = |name: &str, line: u32, method: u32| {
            json!({
                "name": name, "kind": 5,
                "range": span((line, 0), (line + 2, 1)),
                "selectionRange": range(line, 6, 7),
                "children": [{
                    "name": "run", "kind": 6,
                    "range": span((method, 2), (method, 10)),
                    "selectionRange": range(method, 2, 5),
                }],
            })
        };
        json!([class("B", 0, 1), class("A", 3, 4)])
    }

    fn type_item(name: &str, path: &Path, line: u32) -> Value {
        json!({
            "name": name, "kind": 5, "uri": uri(path),
            "range": span((line, 0), (line + 2, 1)), "selectionRange": range(line, 6, 7),
        })
    }

    /// The offset of line 4, column 3 of [`CLASSES`]: inside `A.run`.
    const IN_A_RUN: u32 = 46;
    /// The offset of line 1, column 2 of [`CLASSES`]: `B.run`'s name.
    const B_RUN: u32 = 12;

    /// Super Method moves the caret to the method the supertype declares, in the same file.
    #[gpui_kit::test]
    async fn looks_for_the_method_in_supertypes_level_by_level(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", CLASSES)], "a.mock", super_capabilities());
        let a = f.root.join("a.mock");
        f.answer(json!({
            "textDocument/documentSymbol": classes_outline(),
            "textDocument/prepareTypeHierarchy": [type_item("A", &a, 3)],
            "typeHierarchy/supertypes": [type_item("B", &a, 0)],
        }));
        f.select(cx, IN_A_RUN, IN_A_RUN);
        f.run(cx, "navigate.superMethod");
        settle(cx, "the caret on B.run", |cx| f.caret(cx) == B_RUN);
        assert!(f.editing(cx, &a));
        let prepared = f.requests("textDocument/prepareTypeHierarchy");
        let asked_at = prepared.last().map(|r| r["params"]["position"].clone());
        assert_eq!(asked_at, Some(json!({ "line": 3, "character": 6 })));
        f.close(cx);
    }

    /// Super Method opens the file of the supertype declaring the method, at its name; without a
    /// supertype declaring it, it says so.
    #[gpui_kit::test]
    async fn opens_the_super_method_in_another_file_or_says_there_is_none(cx: &mut TestAppContext) {
        let files = [("a.mock", CLASSES), ("b.mock", CLASSES)];
        let (f, cx) = open(cx, &files, "a.mock", super_capabilities());
        let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
        f.answer(json!({
            "textDocument/documentSymbol": classes_outline(),
            "textDocument/prepareTypeHierarchy": [type_item("A", &a, 3)],
            "typeHierarchy/supertypes": [],
        }));
        f.select(cx, IN_A_RUN, IN_A_RUN);
        f.run(cx, "navigate.superMethod");
        f.wait_hint(cx, NO_SUPER_METHOD);

        f.answer(json!({
            "textDocument/documentSymbol": classes_outline(),
            "textDocument/prepareTypeHierarchy": [type_item("A", &a, 3)],
            "typeHierarchy/supertypes": [type_item("B", &b, 0)],
        }));
        f.run(cx, "navigate.superMethod");
        settle(cx, "the editor of b.mock", |cx| f.editing(cx, &b));
        settle(cx, "the caret on B.run", |cx| f.caret(cx) == B_RUN);
        f.close(cx);
    }
}
