//! The language actions of the editor that go through the language servers: Rename, Reformat
//! Code, Optimize Imports, Code Cleanup, Next / Previous Error with its description, Apply Quick
//! Fix, Highlight Usages in File, Quick (Type) Definition, Generate / Implement / Override
//! Methods, Go to Test, Next / Previous Method.
//!
//! Every action applies to the window's active editor. The backend work runs off the main thread
//! and comes back to the window; what a server answers is turned into edits, hints, popups and
//! dialogs by the models of [`crate::editor_lsp`] and [`crate::editor_refactor`]. A refactoring the
//! backend applied across files (`WorkspaceEditApplied`) is remembered so that it is undone as one
//! step: from the notification it raises, or by Edit › Undo in an editor of one of its files once
//! that editor has nothing of its own left to undo.

use super::Workbench;
use super::branches::run_action_later;
use super::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::code_insight::workspace_undo::{NOTICE_KEY, UndoAction, UndoNotice, UndoStep, WorkspaceUndo, failed_notice};
use crate::editor::buffer::{self, Assoc};
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::marks::{Mark, MarkKind, MarkLayer};
use crate::editor::view::Editor;
use crate::editor_lsp::definition_view::{DefinitionEvent, DefinitionPopup, DefinitionTarget, MAX_TARGETS};
use crate::editor_lsp::diagnostics::{
    DESCRIPTION_HINT_TIMEOUT_MS, GOTO_HINT_TIMEOUT_MS, LintDiagnostic, NO_PROBLEMS_AT_CARET, NO_PROBLEMS_IN_FILE,
    error_description, goto_diagnostic,
};
use crate::editor_lsp::format::{
    FormatAsk, FormatFailure, FormatStart, IndentStyle, format_answer, format_failure, format_options, formatted_text,
    start_format,
};
use crate::editor_lsp::format_guard::FormatGuardOptions;
use crate::editor_lsp::generate::{is_generate_action, is_implement_title, is_override_title};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::intentions::{
    IntentionItem, IntentionRun, KindOutcome, action_items, is_quick_fix, kind_outcome, quick_fix_icon,
    quick_fix_order, silent_action, sort_code_actions,
};
use crate::editor_lsp::intentions_view::{ListOptions, ListPopup, ListPopupEvent, ListRow, intention_rows};
use crate::editor_lsp::lsp::{error_hint, incomplete_note, is_cancelled, is_no_server};
use crate::editor_lsp::navigation::usages_status_text;
use crate::editor_lsp::positions::{offset_to_pos, offsets_to_range, pos_to_offset, range_to_offsets};
use crate::editor_lsp::workspace_edit::{describe_summary, needs_preview, summarize_workspace_edit};
use crate::editor_refactor::goto_test::{GotoTest, GotoTestStep, TestChoice, TestRun};
use crate::editor_refactor::structure_nav::method_stop;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input::{self, Keymap};
use crate::platform::project_roots::{FolderRole, RootFolder};
use crate::platform::virtual_docs::basename;
use crate::theme::icons::file_icon_name;
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, FieldRule, PromptDialog};
use crate::vcs_ops::dialogs::{ConfirmRequest, PromptRequest};
use gpui_kit::{App, AppContext, AsyncWindowContext, Context, Entity, EntityId, Focusable, WeakEntity, Window};
use ide_core::error::ErrorKind;
use ide_core::ipc_path::to_ipc;
use ide_core::settings::ExternalFormatter;
use ide_core::{DocId, IdeError, IdeEvent, IdeResult, Location, Pos, RequestId, Severity, TextChange, WorkspaceEdit};
use ide_host::Host;
use ide_lsp::types::{CodeAction, FormatOptions, GotoKind};
use ide_workspace::RootKind;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The command of the refactoring notification's Undo / Redo button.
pub(super) const WORKSPACE_UNDO_COMMAND: &str = "refactor.workspaceEdit.history";

/// How many refactorings stay undoable from the editors.
const REMEMBERED_EDITS: usize = 20;
/// How long an action waits for the editor of a file it opened, in steps of [`MOUNT_STEP`].
const MOUNT_STEPS: u32 = 250;
const MOUNT_STEP: Duration = Duration::from_millis(16);
/// How long an action that changed the document waits for the editor to show the change.
const EDIT_WAIT_STEPS: u32 = 25;
const EDIT_WAIT_STEP: Duration = Duration::from_millis(20);

const READ_ONLY: &str = "The file is read-only";
const CANNOT_RENAME: &str = "This element can't be renamed";
const NOTHING_TO_RENAME: &str = "Nothing to rename";
const NO_QUICK_FIXES: &str = "No quick fixes available";
const NO_USAGES_IN_FILE: &str = "No usages found in file";
const NO_DEFINITION: &str = "No definition found";
const NO_TYPE_DEFINITION: &str = "No type definition found";
const UNREADABLE_DEFINITION: &str = "Cannot read the definition's file";
const NO_STRUCTURE: &str = "No structure information for this file";
const RUST_TEST_MODULE: &str =
    "\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn it_works() {\n        \n    }\n}\n";

/// A refactoring the backend applied over several files, undoable as one step.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedEdit {
    id: u32,
    /// The files and folders it changed.
    paths: Vec<String>,
    undone: bool,
}

impl AppliedEdit {
    fn affects(&self, path: &str) -> bool {
        self.paths
            .iter()
            .any(|root| path == root || path.strip_prefix(root.as_str()).is_some_and(|r| r.starts_with('/')))
    }
}

/// The language actions' state of one window.
#[derive(Default)]
pub(super) struct EditorLanguage {
    /// The backend listener of applied workspace edits, once an editor opened.
    listener: Option<u64>,
    applied: Vec<AppliedEdit>,
    undo: WorkspaceUndo,
    /// What the refactoring notification's button does.
    undo_action: Option<UndoAction>,
    /// The Escape claim that removes an editor's highlighted usages, per editor.
    usage_claims: HashMap<EntityId, u64>,
    /// The status bar text while usages are highlighted.
    usage_status: Option<String>,
    /// The rename waiting for the server's edit: a newer one cancels it.
    rename_request: Option<RequestId>,
}

/// What a kind of code action (Optimize Imports, Generate…) asks the server for and applies.
#[derive(Clone, Copy)]
struct KindRun {
    /// A `CodeActionKind` prefix; empty for any kind.
    kind: &'static str,
    keep: fn(&CodeAction) -> bool,
    /// Ask for the whole file instead of the selection.
    whole: bool,
    /// The hint when nothing applies.
    none: &'static str,
    /// The title of the chooser of several.
    label: &'static str,
}

fn any_action(_: &CodeAction) -> bool {
    true
}

fn generates(action: &CodeAction) -> bool {
    is_generate_action(action)
}

fn implements(action: &CodeAction) -> bool {
    is_implement_title(&action.title)
}

fn overrides(action: &CodeAction) -> bool {
    is_override_title(&action.title)
}

const OPTIMIZE_IMPORTS: KindRun = KindRun {
    kind: "source.organizeImports",
    keep: any_action,
    whole: true,
    none: "Optimize imports is not available",
    label: "Optimize Imports",
};
const GENERATE: KindRun =
    KindRun { kind: "", keep: generates, whole: false, none: "Nothing to generate here", label: "Generate" };
const IMPLEMENT: KindRun =
    KindRun { kind: "", keep: implements, whole: false, none: "Nothing to implement", label: "Implement Members" };
const OVERRIDE: KindRun =
    KindRun { kind: "", keep: overrides, whole: false, none: "Nothing to override", label: "Override Members" };

/// How a reformat was asked for.
#[derive(Debug, Clone, Copy)]
struct FormatRun {
    explicit: bool,
    selection_only: bool,
    tool: Option<ExternalFormatter>,
}

const REFORMAT: FormatRun = FormatRun { explicit: true, selection_only: true, tool: None };

/// The steps of Code Cleanup in File; all of them unless the argument says otherwise.
#[derive(Debug, Clone, Copy)]
struct Cleanup {
    fix_all: bool,
    optimize_imports: bool,
    reformat: bool,
}

impl Cleanup {
    fn of(arg: Option<&Value>) -> Self {
        let step = |key: &str| arg.and_then(|arg| arg.get(key)).and_then(Value::as_bool) != Some(false);
        Self { fix_all: step("fixAll"), optimize_imports: step("optimizeImports"), reformat: step("reformat") }
    }
}

type This = WeakEntity<Workbench>;

/// The file an action is given: `"path"` or `{ path }`.
fn path_arg(arg: Option<&Value>) -> Option<PathBuf> {
    let arg = arg?;
    arg.as_str().or_else(|| arg.get("path").and_then(Value::as_str)).map(PathBuf::from)
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

/// The end of an action's background work: it only fails when the window went away.
fn finished(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::debug!("an editor language action ended with its window: {error:#}");
    }
}

/// Shows `text` (English source text) as the hint of `editor`, at `pos` (the caret when `None`).
/// It is shown once the current effects are delivered: a caret move or an edit made just before
/// would hide it again.
fn hint(
    editor: &Entity<Editor>,
    text: impl Into<String>,
    pos: Option<u32>,
    tone: Tone,
    timeout_ms: Option<u64>,
    cx: &mut App,
) {
    let (editor, text) = (editor.downgrade(), text.into());
    cx.defer(move |cx| {
        let Some(editor) = editor.upgrade() else { return };
        if let Some(intelligence) = EditorIntelligence::of(&editor, cx) {
            intelligence.update(cx, |this, cx| this.show_hint_at(&text, pos, tone, timeout_ms, cx));
        }
    });
}

/// The hint of a failed request the user asked for; nothing for a cancelled one.
fn report(editor: &Entity<Editor>, error: &IdeError, cx: &mut App) {
    if let Some(text) = error_hint(error, true) {
        hint(editor, text, None, Tone::Info, None, cx);
    }
}

/// The problems the editor shows.
fn problems(editor: &Entity<Editor>, cx: &App) -> Vec<LintDiagnostic> {
    EditorIntelligence::of(editor, cx).map(|intelligence| intelligence.read(cx).problems().to_vec()).unwrap_or_default()
}

/// The editor's selections after `changes`, which are in the coordinates of its current text.
fn mapped_selections(editor: &Editor, changes: &[TextChange]) -> Vec<(u32, u32)> {
    let map = |offset| buffer::map_offset(offset, changes, Assoc::After);
    editor.selections().iter().map(|&(anchor, head)| (map(anchor), map(head))).collect()
}

/// The lines the selection touches, as offsets.
fn selected_lines(editor: &Editor) -> std::ops::Range<u32> {
    let rope = editor.rope();
    let selection = editor.selection();
    let first = buffer::line_col(rope, selection.start).0;
    let last = buffer::line_col(rope, selection.end).0;
    buffer::line_range(rope, first).start..buffer::line_range(rope, last).end
}

/// The shortcut bound to action `id`, as menus show it.
fn shortcut(cx: &App, id: &str) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(id, crate::platform::keymap::is_mac_host()))
}

fn folder_role(kind: RootKind) -> FolderRole {
    match kind {
        RootKind::Source => FolderRole::Source,
        RootKind::Test => FolderRole::Test,
        RootKind::Resources => FolderRole::Resources,
        RootKind::TestResources => FolderRole::TestResources,
        RootKind::Excluded => FolderRole::Excluded,
    }
}

/// The editor of `path` once it shows, opening it first; the active editor without a path.
async fn editor_for(
    this: &This,
    path: Option<PathBuf>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<Option<Entity<Editor>>> {
    let Some(path) = path else { return this.update(cx, |workbench, _| workbench.editor().cloned()) };
    let shown = |workbench: &Workbench, cx: &App| workbench.editor().filter(|e| e.read(cx).path() == path).cloned();
    let open = this.update_in(cx, |workbench, window, cx| {
        let open = shown(workbench, cx);
        if open.is_none() {
            workbench.open_project_file(path.clone(), window, cx);
        }
        open
    })?;
    if open.is_some() {
        return Ok(open);
    }
    for _ in 0..MOUNT_STEPS {
        cx.background_executor().timer(MOUNT_STEP).await;
        let open = this.update(cx, |workbench, cx| shown(workbench, cx))?;
        if open.is_some() {
            return Ok(open);
        }
    }
    tracing::info!("{} did not open in an editor", path.display());
    Ok(None)
}

/// Waits until `editor` is past `version`: the backend's edit reached it.
async fn edited_since(
    this: &This,
    editor: &Entity<Editor>,
    version: u32,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    for _ in 0..EDIT_WAIT_STEPS {
        if this.update(cx, |_, cx| editor.read(cx).version() != version)? {
            break;
        }
        cx.background_executor().timer(EDIT_WAIT_STEP).await;
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Reformat Code

/// What a reformat asks the backend.
struct FormatQuestion {
    doc: DocId,
    version: u32,
    rope: ropey::Rope,
    path: String,
    range: Option<ide_core::Range>,
    options: FormatOptions,
    guard: FormatGuardOptions,
}

/// The reformat of `editor` as `run` asks, from the settings in effect: nothing (with the hint to
/// show), or the question for the backend.
fn format_question(
    workbench: &Workbench,
    editor: &Entity<Editor>,
    run: FormatRun,
    cx: &App,
) -> Result<FormatQuestion, Option<String>> {
    let e = editor.read(cx);
    let settings = crate::platform::settings::effective(&workbench.label, cx);
    let guard = FormatGuardOptions::from(&settings.editor.code_style);
    let path = to_ipc(e.path());
    let root = workbench.project_root(cx).map(|root| to_ipc(&root));
    let ask = FormatAsk {
        explicit: run.explicit,
        on_save: false,
        tool: run.tool.is_some(),
        path: &path,
        root: root.as_deref(),
        read_only: e.is_read_only(),
        contributed: false,
        format_on_save: settings.editor.format_on_save,
    };
    if let FormatStart::Nothing(text) = start_format(&ask, &guard) {
        return Err(text.map(str::to_owned));
    }
    let facts = e.facts();
    let tabs = facts.indent_unit.contains('\t');
    let editor_indent = IndentStyle {
        tab_size: facts.tab_size,
        insert_spaces: !tabs,
        indent_size: (!tabs).then(|| facts.indent_unit.chars().count() as u32),
    };
    let indent = format_options(editor_indent, None);
    let selection = e.selection();
    let range = (run.selection_only && !selection.is_empty())
        .then(|| offsets_to_range(e.rope(), selection.start, selection.end));
    Ok(FormatQuestion {
        doc: e.doc(),
        version: e.version(),
        rope: e.rope().clone(),
        path,
        range,
        options: FormatOptions {
            tab_size: indent.tab_size,
            insert_spaces: indent.insert_spaces,
            on_save: false,
            tool: run.tool,
        },
        guard,
    })
}

/// Reformats `editor`'s selection (or the whole file) through `lsp_format`, as one undo step.
/// Answers whether the text changed.
async fn format_document(
    this: &This,
    editor: &Entity<Editor>,
    run: FormatRun,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<bool> {
    let question = match this.update(cx, |workbench, cx| format_question(workbench, editor, run, cx))? {
        Ok(question) => question,
        Err(text) => {
            if let Some(text) = text {
                this.update(cx, |_, cx| hint(editor, text, None, Tone::Info, None, cx))?;
            }
            return Ok(false);
        }
    };
    let FormatQuestion { doc, version, rope, path, range, options, guard } = question;
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_format(&host, &label, doc, range, options).await
    })
    .await?;
    let edits = match answer {
        Ok(edits) => edits,
        Err(error) => {
            this.update_in(cx, |workbench, window, cx| {
                workbench.format_failed(editor, &error, &path, run.explicit, window, cx);
            })?;
            return Ok(false);
        }
    };
    this.update(cx, |_, cx| {
        let unchanged = editor.read(cx).version() == version;
        let formatted = format_answer(&rope, unchanged, &edits, &guard, run.explicit);
        if let Some(text) = formatted.hint.clone() {
            hint(editor, text, None, Tone::Info, None, cx);
        }
        if formatted.changes.is_empty() {
            return false;
        }
        let applied = editor.update(cx, |e, cx| {
            let selections = mapped_selections(e, &formatted.changes);
            let main = e.main_index();
            e.apply_edit(&formatted.changes, selections, main, cx)
        });
        if applied && run.explicit {
            let text = formatted_text(&cx.default_global::<Localization>().0, formatted.lines);
            hint(editor, text, None, Tone::Info, None, cx);
        }
        applied
    })
}

// -------------------------------------------------------------------------------------------------
// Code actions of a kind

/// The enabled action of `kind` that runs without asking (preferred first) applied to the whole
/// file, as Code Cleanup does; answers whether one was applied.
async fn apply_silently(
    this: &This,
    editor: &Entity<Editor>,
    kind: &'static str,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<bool> {
    let (doc, range, version, read_only) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), offsets_to_range(e.rope(), 0, buffer::len_utf16(e.rope())), e.version(), e.is_read_only())
    })?;
    if read_only {
        return Ok(false);
    }
    let only = vec![kind.to_owned()];
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_code_actions(&host, &label, doc, range, only).await
    })
    .await?;
    let Some(action) = answer.ok().and_then(|actions| silent_action(&actions, kind).cloned()) else { return Ok(false) };
    let applied = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_apply_code_action(&host, &label, doc, action).await
    })
    .await?;
    if let Err(error) = applied {
        tracing::debug!("{kind} was not applied: {}", error.message);
        return Ok(false);
    }
    edited_since(this, editor, version, cx).await?;
    Ok(true)
}

/// Applies a chosen code action through the backend; a disabled one says why.
async fn apply_action(
    this: &This,
    editor: &Entity<Editor>,
    doc: DocId,
    action: CodeAction,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    if let Some(reason) = action.disabled_reason.clone() {
        return this.update(cx, |_, cx| hint(editor, reason, None, Tone::Info, None, cx));
    }
    let applied = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_apply_code_action(&host, &label, doc, action).await
    })
    .await?;
    if let Err(error) = applied {
        this.update(cx, |_, cx| report(editor, &error, cx))?;
    }
    Ok(())
}

/// The actions of `run`'s kind at the selection (or in the file): one is applied, several are
/// offered, none says why.
async fn run_kind(
    this: &This,
    editor: &Entity<Editor>,
    run: KindRun,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let (doc, range, line_error, fix_icon) = this.update(cx, |_, cx| {
        let lint = problems(editor, cx);
        let e = editor.read(cx);
        let rope = e.rope();
        let selection = e.selection();
        let range = if run.whole {
            offsets_to_range(rope, 0, buffer::len_utf16(rope))
        } else {
            offsets_to_range(rope, selection.start, selection.end)
        };
        let lines = selected_lines(e);
        let on_lines = |d: &&LintDiagnostic| d.from <= lines.end && d.to >= lines.start;
        let line_error =
            lint.iter().filter(|d| d.severity == Severity::Error).find(on_lines).map(|d| d.message.clone());
        (e.doc(), range, line_error, quick_fix_icon(&lint, lines.start, lines.end))
    })?;
    let only = if run.kind.is_empty() { Vec::new() } else { vec![run.kind.to_owned()] };
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_code_actions(&host, &label, doc, range, only).await
    })
    .await?;
    let actions = match answer {
        Ok(actions) => actions,
        Err(error) => return this.update(cx, |_, cx| report(editor, &error, cx)),
    };
    match kind_outcome(&actions, run.kind, &run.keep, run.none, line_error.as_deref()) {
        KindOutcome::None(text) => this.update(cx, |_, cx| hint(editor, text, None, Tone::Info, None, cx)),
        KindOutcome::Apply(action) => apply_action(this, editor, doc, action, cx).await,
        KindOutcome::Choose(actions) => this.update_in(cx, |workbench, window, cx| {
            workbench.choose_action(editor, doc, run.label, actions, fix_icon, window, cx);
        }),
    }
}

// -------------------------------------------------------------------------------------------------
// Rename

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

/// Rename at the caret of `editor`: the server says what can be renamed, the new name is asked,
/// then the rename's edit is applied (shown first when it changes several files).
async fn rename(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (doc, pos, version, read_only) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), offset_to_pos(e.rope(), e.caret()), e.version(), e.is_read_only())
    })?;
    if read_only {
        return this.update(cx, |_, cx| hint(editor, READ_ONLY, None, Tone::Info, None, cx));
    }
    let prepared = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_prepare_rename(&host, &label, doc, pos).await
    })
    .await?;
    let current = |cx: &mut AsyncWindowContext| this.update(cx, |_, cx| editor.read(cx).version() == version);
    let range = match prepared {
        Ok(range) => range,
        Err(error) if is_cancelled(&error) => return Ok(()),
        Err(error) if matches!(error.kind, ErrorKind::OutOfSync | ErrorKind::InvalidInput) => {
            return this.update(cx, |_, cx| hint(editor, error.message, None, Tone::Info, None, cx));
        }
        Err(error) => {
            // No server covers the file (a plain text file): Rename renames the file instead.
            // The action reaches for the workbench: it runs outside the workbench's update.
            let file_renamed = is_no_server(&error)
                && cx.update(|window, cx| {
                    crate::platform::actions::execute_action_from(
                        crate::platform::actions::ActionSource::Other,
                        "project.rename",
                        None,
                        window,
                        cx,
                    )
                })?;
            if !file_renamed {
                this.update(cx, |_, cx| report(editor, &error, cx))?;
            }
            return Ok(());
        }
    };
    if !current(cx)? {
        return Ok(());
    }
    let Some(range) = range else {
        return this.update(cx, |_, cx| hint(editor, CANNOT_RENAME, None, Tone::Info, None, cx));
    };
    let (old_name, title) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        let offsets = range_to_offsets(e.rope(), range);
        let old_name = buffer::slice(e.rope(), offsets.start, offsets.end);
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx_built(&["Rename '", "'"], &[Param::from(old_name.as_str())]);
        (old_name, title)
    })?;
    let request = PromptRequest { title, initial: old_name.clone(), mono: true, ..PromptRequest::default() };
    let rule = FieldRule::NonEmpty("Enter a name".to_owned());
    let answer = ask(this, cx, move |window, cx| PromptDialog::prompt(request, rule, window, cx)).await?;
    let Some(chosen) = answer.prompt_result() else { return Ok(()) };
    finish_rename(this, editor, doc, pos, &old_name, chosen.value.trim(), cx).await
}

/// Renames the symbol at `pos` from `old_name` to `new_name` through the server; the edit is
/// applied at once when it changes this file only, and shown first otherwise.
async fn finish_rename(
    this: &This,
    editor: &Entity<Editor>,
    doc: DocId,
    pos: Pos,
    old_name: &str,
    new_name: &str,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    if new_name.is_empty() || new_name == old_name {
        return Ok(());
    }
    let request = Backend::next_request_id();
    this.update(cx, |workbench, cx| {
        if let Some(previous) = workbench.language.rename_request.replace(request) {
            cx.global::<Backend>().host.cancel_request(previous);
        }
    })?;
    let name = new_name.to_owned();
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_rename(&host, &label, request, doc, pos, name).await
    })
    .await?;
    let latest = this.update(cx, |workbench, _| {
        let latest = workbench.language.rename_request == Some(request);
        if latest {
            workbench.language.rename_request = None;
        }
        latest
    })?;
    if !latest {
        return Ok(());
    }
    let renamed = match answer {
        Ok(renamed) => renamed,
        Err(error) => return this.update(cx, |_, cx| report(editor, &error, cx)),
    };
    let summary = summarize_workspace_edit(&renamed.edit);
    let note = this.update(cx, |_, cx| {
        let i18n = &mut cx.default_global::<Localization>().0;
        renamed.incomplete.map(|reason| incomplete_note(i18n, reason, Some(old_name)))
    })?;
    if summary.files == 0 {
        let (text, tone) = match note {
            Some(note) => (note, Tone::Warning),
            None => (NOTHING_TO_RENAME.to_owned(), Tone::Info),
        };
        return this.update(cx, |_, cx| hint(editor, text, None, tone, None, cx));
    }
    // An edit that may leave usages behind is never applied unseen.
    if !renamed.preview && note.is_none() && !needs_preview(&summary) {
        return apply_workspace_edit(this, editor, renamed.edit, cx).await;
    }
    let confirm = this.update(cx, |workbench, cx| {
        let root = workbench.project_root(cx).map(|root| to_ipc(&root)).unwrap_or_default();
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx_built(&["Rename '", "' to '", "'"], &[Param::from(old_name), Param::from(new_name)]);
        let mut lines: Vec<String> = note.iter().cloned().collect();
        lines.push(describe_summary(&summary));
        for entry in &summary.entries {
            let relative =
                entry.path.strip_prefix(&root).map_or(entry.path.as_str(), |rest| rest.trim_start_matches('/'));
            lines.push(format!("{}  {relative}", basename(&entry.path)));
        }
        let ok_label = if note.is_some() { "Rename Anyway" } else { "Do Refactor" };
        ConfirmRequest { title, message: lines.join("\n"), ok_label: ok_label.to_owned(), danger: false }
    })?;
    let answer = ask(this, cx, move |window, cx| PromptDialog::confirm(confirm, window, cx)).await?;
    if answer.confirmed() {
        apply_workspace_edit(this, editor, renamed.edit, cx).await?;
    }
    Ok(())
}

async fn apply_workspace_edit(
    this: &This,
    editor: &Entity<Editor>,
    edit: WorkspaceEdit,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let applied = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::apply_workspace_edit(&host, &label, edit).await
    })
    .await?;
    if let Err(error) = applied {
        this.update(cx, |_, cx| report(editor, &error, cx))?;
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Highlight Usages in File

/// Highlights the usages of the symbol at the caret that the server reports, with the status text.
async fn highlight_usages(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (doc, head, version) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), e.caret(), e.version())
    })?;
    let pos = this.update(cx, |_, cx| offset_to_pos(editor.read(cx).rope(), head))?;
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_document_highlights(&host, &label, doc, pos).await
    })
    .await?;
    this.update(cx, |workbench, cx| {
        let stale = {
            let e = editor.read(cx);
            e.version() != version || e.caret() != head
        };
        if stale {
            return;
        }
        let highlights = match answer {
            Ok(highlights) => highlights,
            Err(error) => {
                report(editor, &error, cx);
                return;
            }
        };
        let (marks, name) = {
            let rope = editor.read(cx).rope();
            let marks: Vec<Mark> = highlights
                .iter()
                .map(|highlight| range_to_offsets(rope, highlight.range))
                .filter(|range| range.start < range.end)
                .map(|range| Mark::new(range, MarkKind::FindOccurrence))
                .collect();
            let at =
                marks.iter().find(|mark| mark.range.start <= head && head <= mark.range.end).map(|m| m.range.clone());
            let at = at.unwrap_or_else(|| buffer::word_at(rope, head));
            (marks, buffer::slice(rope, at.start, at.end))
        };
        if marks.is_empty() {
            hint(editor, NO_USAGES_IN_FILE, None, Tone::Info, None, cx);
            workbench.clear_usages(editor, cx);
            return;
        }
        let name = if name.is_empty() { "the element".to_owned() } else { format!("'{name}'") };
        let status = usages_status_text(marks.len(), &name, shortcut(cx, "find.highlightUsages").as_deref());
        editor.update(cx, |e, cx| e.set_marks(MarkLayer::Usages, marks, cx));
        workbench.language.usage_status = Some(status);
        workbench.claim_usage_escape(editor, cx);
        cx.notify();
    })
}

// -------------------------------------------------------------------------------------------------
// Quick Definition, Go to Test, Next / Previous Method

/// The definition (or type definition) at the caret in a popup.
async fn quick_definition(
    this: &This,
    editor: &Entity<Editor>,
    kind: GotoKind,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let (doc, pos) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), offset_to_pos(e.rope(), e.caret()))
    })?;
    let answer =
        call(
            this,
            cx,
            move |host, label| async move { ide_host::ops::lsp::lsp_goto(&host, &label, doc, pos, kind).await },
        )
        .await?;
    let found = match answer {
        Ok(found) => found,
        Err(error) => return this.update(cx, |_, cx| report(editor, &error, cx)),
    };
    let mut locations: Vec<Location> = Vec::new();
    for location in found {
        let start = location.range.start;
        let seen = locations.iter().any(|known| known.path == location.path && known.range.start == start);
        if !seen {
            locations.push(location);
        }
    }
    if locations.is_empty() {
        let text = if matches!(kind, GotoKind::TypeDefinition) { NO_TYPE_DEFINITION } else { NO_DEFINITION };
        return this.update(cx, |_, cx| hint(editor, text, None, Tone::Info, None, cx));
    }
    locations.truncate(MAX_TARGETS);
    let read = call(this, cx, move |host, label| async move {
        let project = host.project(&label)?;
        Ok(locations
            .into_iter()
            .filter_map(|location| {
                let text = project.text_of(&location.path.0).ok()?;
                DefinitionTarget::read(location, &text)
            })
            .collect::<Vec<_>>())
    })
    .await?;
    match read {
        Ok(targets) if !targets.is_empty() => {
            this.update_in(cx, |workbench, window, cx| workbench.show_definition(targets, window, cx))
        }
        Ok(_) => this.update(cx, |_, cx| hint(editor, UNREADABLE_DEFINITION, None, Tone::Info, None, cx)),
        Err(error) => this.update(cx, |_, cx| report(editor, &error, cx)),
    }
}

/// Go to Test / Test Subject from the file of `editor`.
async fn goto_test(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (path, text, root) = this.update(cx, |workbench, cx| {
        let e = editor.read(cx);
        (to_ipc(e.path()), e.text(), workbench.project_root(cx).map(|root| to_ipc(&root)))
    })?;
    let roots = call(this, cx, move |host, label| async move { ide_host::ops::project::roots(&host, &label) }).await?;
    let roots: Vec<RootFolder> = roots
        .map(|roots| roots.roots)
        .unwrap_or_default()
        .into_iter()
        .map(|root| RootFolder { path: root.path, kind: folder_role(root.kind), detected_from: root.detected_from })
        .collect();
    let flow = GotoTest::new(&path, root.as_deref(), roots);
    let names = flow.names();
    let found = call(this, cx, move |host, label| async move {
        let mut files = Vec::new();
        for name in names {
            match ide_host::ops::nav::find_files(&host, &label, name.clone(), 50) {
                Ok(matches) => files.extend(
                    matches.into_iter().map(|found| to_ipc(&found.path.0)).filter(|found| basename(found) == name),
                ),
                Err(error) => tracing::debug!("no files named {name}: {}", error.message),
            }
        }
        Ok(files)
    })
    .await?;
    let files = match found {
        Ok(files) => files,
        Err(error) => return this.update(cx, |_, cx| hint(editor, error.message, None, Tone::Error, None, cx)),
    };
    this.update_in(cx, |workbench, window, cx| {
        let step = flow.found(files, &text, &mut cx.default_global::<Localization>().0);
        match step {
            GotoTestStep::Run(run) => workbench.run_test_choice(editor, run, window, cx),
            GotoTestStep::Hint(text) => hint(editor, text, None, Tone::Info, None, cx),
            GotoTestStep::Choose { title, items } => workbench.choose_test(editor, &title, items, window, cx),
        }
    })
}

/// Next (`forward`) or Previous Method: the caret goes to the next stop of the file's outline.
async fn goto_method(
    this: &This,
    editor: &Entity<Editor>,
    forward: bool,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let (doc, caret) = this.update(cx, |_, cx| {
        let e = editor.read(cx);
        (e.doc(), offset_to_pos(e.rope(), e.caret()))
    })?;
    let outline =
        call(this, cx, move |host, label| async move { ide_host::ops::doc::doc_outline(&host, &label, doc).await })
            .await?;
    this.update(cx, |_, cx| match outline {
        Ok(outline) => {
            if let Some(stop) = method_stop(&outline, caret, forward) {
                editor.update(cx, |e, cx| {
                    let offset = pos_to_offset(e.rope(), stop);
                    e.select(offset, offset, cx);
                });
            }
        }
        Err(error) if is_cancelled(&error) => {}
        Err(_) => hint(editor, NO_STRUCTURE, None, Tone::Info, None, cx),
    })
}

// -------------------------------------------------------------------------------------------------
// Apply Quick Fix

/// ⌥⇧↵: the first quick fix (preferred first) of the worst problem at the caret, else of one
/// elsewhere on its line.
async fn apply_quick_fix(this: &This, editor: &Entity<Editor>, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    let (doc, version, ranges) = this.update(cx, |_, cx| {
        let lint = problems(editor, cx);
        let e = editor.read(cx);
        let line = buffer::line_range(e.rope(), buffer::line_col(e.rope(), e.caret()).0);
        let order = quick_fix_order(&lint, e.caret(), line.start, line.end);
        let ranges: Vec<ide_core::Range> =
            order.iter().map(|problem| offsets_to_range(e.rope(), problem.from, problem.to)).collect();
        (e.doc(), e.version(), ranges)
    })?;
    for range in ranges {
        let answer = call(this, cx, move |host, label| async move {
            ide_host::ops::lsp::lsp_code_actions(&host, &label, doc, range, vec!["quickfix".to_owned()]).await
        })
        .await?;
        let actions = match answer {
            Ok(actions) => actions,
            Err(error) => return this.update(cx, |_, cx| report(editor, &error, cx)),
        };
        if this.update(cx, |_, cx| editor.read(cx).version() != version)? {
            return Ok(());
        }
        let fixes: Vec<CodeAction> =
            actions.into_iter().filter(|action| is_quick_fix(action) && action.disabled_reason.is_none()).collect();
        if let Some(fix) = sort_code_actions(&fixes).into_iter().next() {
            return apply_action(this, editor, doc, fix, cx).await;
        }
    }
    this.update(cx, |_, cx| hint(editor, NO_QUICK_FIXES, None, Tone::Info, None, cx))
}

impl Workbench {
    /// The popup that is open, when it is a `V`.
    pub fn language_popup<V: 'static>(&self) -> Option<Entity<V>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// The status bar text of the highlighted usages, while some are highlighted.
    pub fn usages_status(&self) -> Option<&str> {
        self.language.usage_status.as_deref()
    }

    /// Runs `work` in the background for the active editor, or for the editor of the file `path`
    /// opened first.
    fn on_language_editor<F>(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(This, Entity<Editor>, AsyncWindowContext) -> F + 'static,
    ) where
        F: Future<Output = anyhow::Result<()>> + 'static,
    {
        cx.spawn_in(window, async move |this, cx| {
            let result = async {
                let Some(editor) = editor_for(&this, path, cx).await? else { return Ok(()) };
                work(this.clone(), editor, cx.clone()).await
            };
            finished(result.await);
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Refactorings applied over several files

    /// Listens to the workspace edits the backend applies in this window's project, once.
    pub(super) fn watch_workspace_edits(&mut self, cx: &mut Context<Self>) {
        if self.language.listener.is_some() || !cx.has_global::<Backend>() {
            return;
        }
        let (this, label) = (cx.weak_entity(), self.label.clone());
        let listener = cx.global_mut::<Backend>().listen(move |event, cx| {
            if let IdeEvent::WorkspaceEditApplied { id, paths } = &event.event
                && event.window.as_deref().is_none_or(|window| window == label)
            {
                let paths: Vec<String> = paths.iter().map(|path| to_ipc(&path.0)).collect();
                let id = *id;
                let _ = this.update(cx, |workbench, cx| workbench.workspace_edit_applied(id, paths, cx));
            }
        });
        self.language.listener = Some(listener);
        cx.on_release(|workbench, cx| {
            if let Some(listener) = workbench.language.listener.take()
                && cx.has_global::<Backend>()
            {
                cx.global_mut::<Backend>().unlisten(listener);
            }
            for (_, claim) in workbench.language.usage_claims.drain() {
                input::release_keys(claim, cx);
            }
        })
        .detach();
    }

    /// The project the window shows, as the refactoring history knows it.
    fn undo_project(&self, cx: &App) -> Option<String> {
        self.project_root(cx).map(|root| to_ipc(&root))
    }

    fn workspace_edit_applied(&mut self, id: u32, paths: Vec<String>, cx: &mut Context<Self>) {
        let applied = &mut self.language.applied;
        // A new edit drops the redo of an undone one it overlaps.
        applied.retain(|edit| !(edit.undone && paths.iter().any(|path| edit.affects(path))));
        applied.push(AppliedEdit { id, paths, undone: false });
        if applied.len() > REMEMBERED_EDITS {
            applied.remove(0);
        }
        let project = self.undo_project(cx);
        let notice = self.language.undo.applied(id, project.as_deref());
        self.raise_undo_notice(notice, cx);
    }

    fn raise_undo_notice(&mut self, notice: UndoNotice, cx: &mut Context<Self>) {
        let mut request = NotificationRequest::new(notice.level, notice.title);
        request.body = notice.body;
        request.key = notice.key.map(str::to_owned);
        if let Some(action) = &notice.action {
            request.actions =
                vec![NotificationAction { label: action.label.to_owned(), command: WORKSPACE_UNDO_COMMAND.to_owned() }];
        }
        if notice.key == Some(NOTICE_KEY) {
            self.language.undo_action = notice.action;
        }
        self.notify(request, cx);
    }

    /// The refactoring notification's button: undoes or redoes the refactoring it reports.
    /// Answers whether `command` was that button's.
    pub(super) fn run_language_notification(&mut self, command: &str, cx: &mut Context<Self>) -> bool {
        if command != WORKSPACE_UNDO_COMMAND {
            return false;
        }
        let Some(action) = self.language.undo_action.clone() else { return true };
        // The editors' edits reach the backend as they are made: nothing is pending.
        let project = self.undo_project(cx);
        match self.language.undo.synced(&action, project.as_deref()) {
            UndoStep::Undo(id) => self.step_refactoring(id, false, cx),
            UndoStep::Redo(id) => self.step_refactoring(id, true, cx),
            UndoStep::Refused(notice) => self.raise_undo_notice(notice, cx),
            UndoStep::WaitForSync | UndoStep::Nothing => {}
        }
        true
    }

    /// Undoes (or with `redo`, redoes) refactoring `id` on the backend; the notification follows.
    fn step_refactoring(&mut self, id: u32, redo: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let done = this
                .update(cx, |workbench, cx| {
                    let label = workbench.label.clone();
                    cx.try_global::<Backend>().map(|backend| {
                        backend.with_host(move |host| async move {
                            if redo {
                                ide_host::ops::lsp::redo_workspace_edit(&host, &label, id).await
                            } else {
                                ide_host::ops::lsp::undo_workspace_edit(&host, &label, id).await
                            }
                        })
                    })
                })
                .ok()
                .flatten();
            let Some(done) = done else { return };
            let done = done.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
            let _ = this.update(cx, |workbench, cx| match done {
                Ok(()) => {
                    if let Some(edit) = workbench.language.applied.iter_mut().find(|edit| edit.id == id) {
                        edit.undone = !redo;
                    }
                    let project = workbench.undo_project(cx);
                    if let Some(notice) = workbench.language.undo.history(id, !redo, project.as_deref()) {
                        workbench.raise_undo_notice(notice, cx);
                    }
                }
                Err(error) => workbench.raise_undo_notice(failed_notice(&error.message), cx),
            });
        })
        .detach();
    }

    /// Edit › Undo (`redo` false) or Redo in `editor` once its own history has nothing left in
    /// that direction: the latest refactoring of its file, as one step. Answers whether there was
    /// one.
    fn refactoring_history(&mut self, editor: &Entity<Editor>, redo: bool, cx: &mut Context<Self>) -> bool {
        let e = editor.read(cx);
        let own_history = if redo { e.can_redo() } else { e.can_undo() };
        if own_history {
            return false;
        }
        let path = to_ipc(e.path());
        let Some(id) = self
            .language
            .applied
            .iter()
            .rev()
            .find(|edit| edit.undone == redo && edit.affects(&path))
            .map(|edit| edit.id)
        else {
            return false;
        };
        self.step_refactoring(id, redo, cx);
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Popups

    /// Offers `actions` (several of one kind) titled `label`; the chosen one is applied.
    #[expect(clippy::too_many_arguments, reason = "the chooser carries the request it answers")]
    fn choose_action(
        &mut self,
        editor: &Entity<Editor>,
        doc: DocId,
        label: &str,
        actions: Vec<CodeAction>,
        fix_icon: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items: Vec<IntentionItem> = action_items(&actions, fix_icon);
        let rows = intention_rows(&items, cx);
        let title = cx.default_global::<Localization>().0.tx(label);
        let list = cx.new(|cx| ListPopup::new(&title, rows, ListOptions::default(), cx));
        let editor = editor.downgrade();
        let events =
            cx.subscribe_in(&list, window, move |workbench, _, event: &ListPopupEvent, window, cx| match event {
                ListPopupEvent::Activated(id) => {
                    workbench.close_popup(window, cx);
                    let chosen = items.iter().find(|item| item.id == *id).and_then(|item| match &item.run {
                        IntentionRun::CodeAction(index) => actions.get(*index).cloned(),
                        IntentionRun::Local(_) | IntentionRun::Inspection(_) => None,
                    });
                    if let (Some(action), Some(editor)) = (chosen, editor.upgrade()) {
                        cx.spawn_in(window, async move |this, cx| {
                            finished(apply_action(&this, &editor, doc, action, cx).await);
                        })
                        .detach();
                    }
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
                    workbench.close_popup(window, cx);
                }
                ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_) => {}
            });
        self.open_popup(&list, events, window, cx);
        list.update(cx, |list, cx| list.opened(cx));
    }

    /// The definition popup over `targets`; F4 opens the shown one.
    fn show_definition(&mut self, targets: Vec<DefinitionTarget>, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.project_root(cx).map(|root| to_ipc(&root)).unwrap_or_default();
        let popup = cx.new(|cx| DefinitionPopup::new(targets, &root, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, _, event: &DefinitionEvent, window, cx| {
            workbench.close_popup(window, cx);
            if let DefinitionEvent::Open(location) = event {
                let start = location.range.start;
                workbench.open_project_file_at(location.path.0.clone(), start.line, start.col, window, cx);
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    /// The Go to Test chooser: Create New Test… and the files found.
    fn choose_test(
        &mut self,
        editor: &Entity<Editor>,
        title: &str,
        items: Vec<TestChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows: Vec<ListRow> = items
            .iter()
            .enumerate()
            .map(|(index, item)| ListRow {
                id: index.to_string(),
                label: cx.default_global::<Localization>().0.tx(&item.label),
                detail: item.detail.clone(),
                emphasis: item.emphasis,
                icon: Some(if item.emphasis { "plus".to_owned() } else { file_icon_name(&item.label).to_owned() }),
                ..ListRow::default()
            })
            .collect();
        let list = cx.new(|cx| ListPopup::new(title, rows, ListOptions::default(), cx));
        let editor = editor.downgrade();
        let events =
            cx.subscribe_in(&list, window, move |workbench, _, event: &ListPopupEvent, window, cx| match event {
                ListPopupEvent::Activated(id) => {
                    workbench.close_popup(window, cx);
                    let run = id.parse::<usize>().ok().and_then(|index| items.get(index)).map(|item| item.run.clone());
                    if let (Some(run), Some(editor)) = (run, editor.upgrade()) {
                        workbench.run_test_choice(&editor, run, window, cx);
                    }
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
                    workbench.close_popup(window, cx);
                }
                ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_) => {}
            });
        self.open_popup(&list, events, window, cx);
        list.update(cx, |list, cx| list.opened(cx));
    }

    /// Carries out what Go to Test chose: opens the file, creates the test, or adds the Rust test
    /// module to the file.
    fn run_test_choice(&mut self, editor: &Entity<Editor>, run: TestRun, window: &mut Window, cx: &mut Context<Self>) {
        match run {
            TestRun::Open { path, offset } => {
                let own = to_ipc(editor.read(cx).path()) == path;
                match offset {
                    Some(offset) if own => editor.update(cx, |e, cx| e.select(offset, offset, cx)),
                    _ => self.open_project_file(PathBuf::from(path), window, cx),
                }
            }
            TestRun::CreateTest(create) => {
                let arg = json!({
                    "directory": create.directory,
                    "name": create.name,
                    "extension": create.extension,
                    "kindId": "test",
                });
                run_action_later("templates.newFromTemplate".to_owned(), Some(arg), window, cx);
            }
            TestRun::CreateRustTestModule => editor.update(cx, |e, cx| {
                if e.is_read_only() {
                    return;
                }
                let end = buffer::len_utf16(e.rope());
                let newline = buffer::slice(e.rope(), end.saturating_sub(1), end) == "\n";
                let insert = format!("{}{RUST_TEST_MODULE}", if newline { "" } else { "\n" });
                // The caret goes inside the test function's body.
                let body = "fn it_works() {\n        ";
                let before = insert.find(body).map_or(0, |at| at + body.len());
                let caret = end + insert[..before].encode_utf16().count() as u32;
                e.apply_edit(&[TextChange { from: end, to: end, insert }], vec![(caret, caret)], 0, cx);
            }),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Highlighted usages

    /// Removes the highlighted usages of `editor`; answers whether there were some.
    fn clear_usages(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) -> bool {
        if editor.read(cx).marks(MarkLayer::Usages).is_empty() {
            return false;
        }
        editor.update(cx, |e, cx| e.set_marks(MarkLayer::Usages, Vec::new(), cx));
        self.language.usage_status = None;
        cx.notify();
        true
    }

    /// Escape in `editor` removes its highlighted usages, while it has some.
    fn claim_usage_escape(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        if self.language.usage_claims.contains_key(&id) {
            return;
        }
        let (this, target) = (cx.weak_entity(), editor.downgrade());
        let claim = input::claim_keys(
            editor.focus_handle(cx),
            move |keystroke, _, cx| {
                if keystroke.key != "escape" || keystroke.modifiers.modified() {
                    return false;
                }
                let Some(editor) = target.upgrade() else { return false };
                this.update(cx, |workbench, cx| workbench.clear_usages(&editor, cx)).unwrap_or(false)
            },
            cx,
        );
        self.language.usage_claims.insert(id, claim);
        cx.observe_release(editor, move |workbench, _, cx| {
            if let Some(claim) = workbench.language.usage_claims.remove(&id) {
                input::release_keys(claim, cx);
            }
        })
        .detach();
    }

    /// ⌘⇧F7: highlights the usages at the caret; again on a highlighted usage removes them.
    fn toggle_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let on_usage = {
            let e = editor.read(cx);
            let head = e.caret();
            e.marks(MarkLayer::Usages).iter().any(|mark| mark.range.start <= head && head <= mark.range.end)
        };
        if on_usage && self.clear_usages(&editor, cx) {
            return;
        }
        self.on_language_editor(None, window, cx, |this, editor, mut cx| async move {
            highlight_usages(&this, &editor, &mut cx).await
        });
    }

    /// ⌃⌥↓ / ⌃⌥↑: the next (`forward`) or previous highlighted usage, the caret at the same place
    /// in it. Without two highlighted usages around the caret it is Next / Previous Occurrence.
    fn goto_usage(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let target = {
            let e = editor.read(cx);
            let head = e.caret();
            let marks = e.marks(MarkLayer::Usages);
            let current = marks.iter().position(|mark| mark.range.start <= head && head <= mark.range.end);
            current.filter(|_| marks.len() > 1).map(|current| {
                let next =
                    if forward { (current + 1) % marks.len() } else { (current + marks.len() - 1) % marks.len() };
                let (from, to) = (&marks[current].range, &marks[next].range);
                (to.start + (head - from.start)).min(to.end)
            })
        };
        match target {
            Some(offset) => editor.update(cx, |e, cx| e.select(offset, offset, cx)),
            None => {
                let id = if forward { "navigate.nextOccurrence" } else { "navigate.prevOccurrence" };
                run_action_later(id.to_owned(), None, window, cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Problems

    /// F2 / ⇧F2: the caret goes to the next (`forward`) or previous problem, whose message shows.
    fn goto_problem(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let settings = crate::platform::settings::effective(&self.label, cx);
        let errors_only = settings.editor.code_editing.next_error_errors_only;
        let lint = problems(&editor, cx);
        let head = editor.read(cx).caret();
        match goto_diagnostic(&lint, head, forward, errors_only) {
            None => hint(&editor, NO_PROBLEMS_IN_FILE, None, Tone::Info, None, cx),
            Some(jump) => {
                editor.update(cx, |e, cx| e.select(jump.offset, jump.offset, cx));
                hint(&editor, jump.message, Some(jump.offset), jump.tone, Some(GOTO_HINT_TIMEOUT_MS), cx);
            }
        }
    }

    /// ⌘F1: the messages of the problems at the caret.
    fn describe_problem(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let lint = problems(&editor, cx);
        match error_description(&lint, editor.read(cx).caret()) {
            None => hint(&editor, NO_PROBLEMS_AT_CARET, None, Tone::Info, None, cx),
            Some((text, tone)) => hint(&editor, text, None, tone, Some(DESCRIPTION_HINT_TIMEOUT_MS), cx),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Reformat failures

    fn format_failed(
        &mut self,
        editor: &Entity<Editor>,
        error: &IdeError,
        path: &str,
        explicit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let failure = format_failure(&cx.default_global::<Localization>().0, error, path, explicit, false);
        match failure {
            FormatFailure::Silent => {}
            FormatFailure::PromptInstall { tool, sticky } => {
                let arg = json!({ "name": tool, "sticky": sticky, "formatter": true });
                run_action_later("tools.promptInstall".to_owned(), Some(arg), window, cx);
            }
            FormatFailure::Notify { title, body, key } => {
                let mut request = NotificationRequest::new(NotificationLevel::Error, title);
                request.body = Some(body);
                request.key = Some(key);
                request.group = Some("tools".to_owned());
                request.sticky = Some(false);
                request.verbatim = true;
                self.notify(request, cx);
            }
            FormatFailure::Hint(text) => hint(editor, text, None, Tone::Info, None, cx),
        }
    }
}

/// Edit › Undo in an editor with nothing of its own to undo: the latest refactoring of its file,
/// as one step. Answers whether there was one.
pub(crate) fn undo_refactoring(window: &mut Window, cx: &mut App) -> bool {
    refactoring_history(window, cx, false)
}

/// Edit › Redo in an editor with nothing of its own to redo: the refactoring of its file undone
/// last. Answers whether there was one.
pub(crate) fn redo_refactoring(window: &mut Window, cx: &mut App) -> bool {
    refactoring_history(window, cx, true)
}

fn refactoring_history(window: &mut Window, cx: &mut App, redo: bool) -> bool {
    let Some(workbench) = super::for_window(window, cx) else { return false };
    workbench.update(cx, |workbench, cx| {
        let Some(editor) = workbench.editor().cloned() else { return false };
        workbench.refactoring_history(&editor, redo, cx)
    })
}

/// Registers the language actions the window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    type Formatting = (&'static str, ExternalFormatter);
    registry.handle("code.reformat", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.on_language_editor(None, window, cx, |this, editor, mut cx| async move {
                format_document(&this, &editor, REFORMAT, &mut cx).await.map(drop)
            });
        })
    });
    let tools: [Formatting; 3] = [
        ("code.reformatWithPrettier", ExternalFormatter::Prettier),
        ("code.reformatWithOxfmt", ExternalFormatter::Oxfmt),
        ("code.reformatWithRuff", ExternalFormatter::Ruff),
    ];
    for (id, tool) in tools {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.on_language_editor(None, window, cx, move |this, editor, mut cx| async move {
                    let run = FormatRun { tool: Some(tool), ..REFORMAT };
                    format_document(&this, &editor, run, &mut cx).await.map(drop)
                });
            })
        });
    }
    registry.handle("code.reformatFile", |call, window, cx| {
        let path = path_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.on_language_editor(path, window, cx, |this, editor, mut cx| async move {
                format_document(&this, &editor, REFORMAT, &mut cx).await.map(drop)
            });
        })
    });
    for (id, by_path) in [("code.optimizeImports", false), ("code.optimizeImportsFile", true)] {
        registry.handle(id, move |call, window, cx| {
            let path = by_path.then(|| path_arg(call.arg)).flatten();
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.on_language_editor(path, window, cx, |this, editor, mut cx| async move {
                    run_kind(&this, &editor, OPTIMIZE_IMPORTS, &mut cx).await
                });
            })
        });
    }
    registry.handle("code.cleanupFile", |call, window, cx| {
        let (path, steps) = (path_arg(call.arg), Cleanup::of(call.arg));
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.on_language_editor(path, window, cx, move |this, editor, mut cx| async move {
                if steps.fix_all {
                    apply_silently(&this, &editor, "source.fixAll", &mut cx).await?;
                }
                if steps.optimize_imports {
                    apply_silently(&this, &editor, "source.organizeImports", &mut cx).await?;
                }
                if steps.reformat {
                    let run = FormatRun { explicit: false, ..REFORMAT };
                    format_document(&this, &editor, run, &mut cx).await?;
                }
                Ok(())
            });
        })
    });
    for (id, run) in
        [("code.generate", GENERATE), ("code.implementMethods", IMPLEMENT), ("code.overrideMethods", OVERRIDE)]
    {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.on_language_editor(None, window, cx, move |this, editor, mut cx| async move {
                    run_kind(&this, &editor, run, &mut cx).await
                });
            })
        });
    }
    registry.handle("refactor.rename", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.on_language_editor(None, window, cx, |this, editor, mut cx| async move {
                rename(&this, &editor, &mut cx).await
            });
        })
    });
    registry.handle("code.applyQuickFix", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.on_language_editor(None, window, cx, |this, editor, mut cx| async move {
                apply_quick_fix(&this, &editor, &mut cx).await
            });
        })
    });
    for (id, kind) in
        [("code.quickDefinition", GotoKind::Definition), ("code.quickTypeDefinition", GotoKind::TypeDefinition)]
    {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.on_language_editor(None, window, cx, move |this, editor, mut cx| async move {
                    quick_definition(&this, &editor, kind, &mut cx).await
                });
            })
        });
    }
    registry.handle("navigate.gotoTest", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.on_language_editor(None, window, cx, |this, editor, mut cx| async move {
                goto_test(&this, &editor, &mut cx).await
            });
        })
    });
    for (id, forward) in [("navigate.nextMethod", true), ("navigate.previousMethod", false)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.on_language_editor(None, window, cx, move |this, editor, mut cx| async move {
                    goto_method(&this, &editor, forward, &mut cx).await
                });
            })
        });
    }
    for (id, forward) in [("navigate.nextError", true), ("navigate.prevError", false)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| workbench.goto_problem(forward, cx))
        });
    }
    registry.handle("code.showErrorDescription", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| workbench.describe_problem(cx))
    });
    registry.handle("find.highlightUsages", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.toggle_usages(window, cx))
    });
    for (id, forward) in [("navigate.nextHighlightedUsage", true), ("navigate.prevHighlightedUsage", false)] {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, window, cx| workbench.goto_usage(forward, window, cx))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::editor::intelligence::fake_server::FakeServer;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use crate::workbench;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::path::Path;

    mod proofs3_editor_keys;
    mod proofs5_editor;

    /// A trusted project of `files` in a window, `open` open in the editor with the keyboard. With
    /// `answers` the IDE is in Full mode and `*.mock` files have the fake language server, which
    /// offers the capabilities of `answers["capabilities"]` and has the opened file before this
    /// returns; without, the IDE is in Light mode and no server runs. `settings` are set globally
    /// first.
    struct Fixture {
        workbench: Entity<Workbench>,
        root: PathBuf,
        server: Option<FakeServer>,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
        open: &str,
        answers: Option<Value>,
        settings: &[(&str, Value)],
    ) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let dirs = AppDirs::under(home.path());
        let server = answers.as_ref().map(|answers| {
            let server = FakeServer::install(&dirs.config, home.path());
            server.answer(answers.clone());
            server
        });
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a folder")).expect("the folder is made");
            std::fs::write(file, text).expect("the file is written");
        }
        let mode = if server.is_some() { "full" } else { "light" };
        let settings: Vec<(String, Value)> =
            settings.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect();
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
            host.global_settings.set(global, "ide.mode", json!(mode)).expect("the mode is set");
            for (key, value) in settings {
                host.global_settings.set(global, &key, value).expect("the setting is set");
            }
            host.trust.decide(&root, true, false).expect("the project is trusted");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let file = root.join(open);
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
        let fixture = Fixture { workbench, root, server, _folders: (home, folder) };
        settle(cx, |cx| {
            let editor = fixture.editor(cx);
            cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
        });
        if fixture.server.is_some() {
            fixture.wait_opened(cx, open);
        }
        (fixture, cx)
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..400 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within four seconds");
    }

    fn range(line: u32, from: u32, to: u32) -> Value {
        json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
    }

    fn uri(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    fn edit(path: &Path, edits: Vec<Value>) -> Value {
        json!({ "changes": { uri(path): edits } })
    }

    fn text_edit(range: Value, new_text: &str) -> Value {
        json!({ "range": range, "newText": new_text })
    }

    /// Replaces the text of `dialog`'s field by typing `text`; answers the text it held before.
    fn retype(dialog: &Entity<PromptDialog>, text: &str, cx: &mut VisualTestContext) -> String {
        let field = dialog.read_with(cx, |d, _| d.field().cloned()).expect("the dialog has a field");
        let before = field.read_with(cx, |field, _| field.text().to_owned());
        for _ in 0..before.chars().count() {
            cx.simulate_keystrokes("backspace");
        }
        cx.simulate_input(text);
        cx.run_until_parked();
        before
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
            settle(cx, |cx| self.hint(cx).as_deref() == Some(text));
        }

        fn server(&self) -> &FakeServer {
            self.server.as_ref().expect("the fake server runs")
        }

        fn answer(&self, answers: Value) {
            self.server().answer(answers);
        }

        fn requests(&self, method: &str) -> Vec<Value> {
            self.server().received().into_iter().filter(|message| message["method"] == method).collect()
        }

        fn wait_opened(&self, cx: &mut VisualTestContext, name: &str) {
            let name = format!("/{name}");
            settle(cx, |_| {
                let opened = self.requests("textDocument/didOpen");
                let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
                opened.iter().filter_map(uri).any(|uri| uri.ends_with(&name))
            });
        }

        fn put_caret(&self, cx: &mut VisualTestContext, offset: u32) {
            let editor = self.active(cx);
            editor.update(cx, |editor, cx| editor.select(offset, offset, cx));
            cx.run_until_parked();
        }

        fn run(&self, cx: &mut VisualTestContext, id: &str) {
            self.run_with(cx, id, None);
        }

        fn run_with(&self, cx: &mut VisualTestContext, id: &str, arg: Option<Value>) {
            cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx))
                .unwrap_or_else(|error| panic!("{id}: {error}"));
            cx.run_until_parked();
        }

        /// The text of `path` as the project has it: its open document's, else the file's on disk.
        fn project_text(&self, cx: &mut VisualTestContext, path: &Path) -> String {
            cx.update(|_, cx| {
                let project = cx.global::<Backend>().host.project("w1").expect("the project is open");
                project.documents.text_of(path)
            })
            .unwrap_or_else(|| std::fs::read_to_string(path).expect("the file is read"))
        }

        fn notices(&self, cx: &mut VisualTestContext) -> Vec<(String, Vec<String>)> {
            self.workbench.read_with(cx, |w, _| {
                let items = w.notifications().items();
                items.iter().map(|n| (n.title.clone(), n.actions.iter().map(|a| a.label.clone()).collect())).collect()
            })
        }

        fn prompt(&self, cx: &mut VisualTestContext) -> Entity<PromptDialog> {
            settle(cx, |cx| self.workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
            self.workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the dialog is open")
        }

        fn list(&self, cx: &mut VisualTestContext) -> Entity<ListPopup> {
            settle(cx, |cx| self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_some()));
            self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>()).expect("the list is open")
        }

        fn close(&self, cx: &mut VisualTestContext) {
            cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
        }
    }

    /// Rename asks the server what is renamed and asks the new name; an edit of two files is shown
    /// first, then applied to both; one Undo in the editor restores both files, the notification's
    /// Redo renames them again; a single-file rename applies at once; an element the server cannot
    /// rename says so.
    #[gpui_kit::test]
    async fn rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each(cx: &mut TestAppContext) {
        let files = [("a.mock", "let foo = 1\nfoo + foo\n"), ("b.mock", "use foo\n")];
        let capabilities = json!({ "renameProvider": { "prepareProvider": true } });
        let (f, cx) = open(cx, &files, "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
        let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
        let a_edits =
            vec![text_edit(range(0, 4, 7), "bar"), text_edit(range(1, 0, 3), "bar"), text_edit(range(1, 6, 9), "bar")];
        let both = json!({ "changes": {
            uri(&a): a_edits.clone(),
            uri(&b): [text_edit(range(0, 4, 7), "bar")],
        } });
        f.answer(json!({
            "capabilities": capabilities.clone(),
            "textDocument/prepareRename": range(0, 4, 7),
            "textDocument/rename": both,
        }));
        f.put_caret(cx, 5);
        f.run(cx, "refactor.rename");

        let dialog = f.prompt(cx);
        assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "Rename 'foo'");
        assert_eq!(retype(&dialog, "bar", cx), "foo");
        let prepared = f.requests("textDocument/prepareRename");
        assert_eq!(prepared[0]["params"]["position"], json!({ "line": 0, "character": 5 }));
        cx.simulate_keystrokes("enter");

        // Two files change: the preview asks first.
        let previewing = |cx: &mut VisualTestContext| {
            f.workbench.read_with(cx, |w, cx| w.vcs_prompt().is_some_and(|d| d.read(cx).title() != "Rename 'foo'"))
        };
        settle(cx, previewing);
        let preview = f.prompt(cx);
        assert_eq!(preview.read_with(cx, |d, _| d.title().to_owned()), "Rename 'foo' to 'bar'");
        let buttons: Vec<String> = preview.read_with(cx, |d, _| d.buttons().iter().map(|b| b.label.clone()).collect());
        assert_eq!(buttons, ["Do Refactor", "Cancel"]);
        assert_eq!(f.requests("textDocument/rename")[0]["params"]["newName"], "bar");
        assert_eq!(f.text(cx), "let foo = 1\nfoo + foo\n", "nothing changes before the preview is confirmed");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| f.text(cx) == "let bar = 1\nbar + bar\n");
        settle(cx, |cx| f.project_text(cx, &b) == "use bar\n");
        settle(cx, |cx| f.notices(cx).contains(&("Refactoring applied".to_owned(), vec!["Undo".to_owned()])));

        // One Undo in the editor takes the whole rename back.
        f.run(cx, "edit.undo");
        settle(cx, |cx| f.text(cx) == "let foo = 1\nfoo + foo\n");
        settle(cx, |cx| f.project_text(cx, &b) == "use foo\n");
        settle(cx, |cx| f.notices(cx).contains(&("Refactoring undone".to_owned(), vec!["Redo".to_owned()])));
        // The notification's Redo renames both again.
        f.workbench.update(cx, |w, cx| assert!(w.run_language_notification(WORKSPACE_UNDO_COMMAND, cx)));
        settle(cx, |cx| f.text(cx) == "let bar = 1\nbar + bar\n");
        settle(cx, |cx| f.project_text(cx, &b) == "use bar\n");

        // A rename inside this file only is applied without a preview.
        let a_edits: Vec<Value> =
            [range(0, 4, 7), range(1, 0, 3), range(1, 6, 9)].into_iter().map(|at| text_edit(at, "baz")).collect();
        let local = json!({ "changes": { uri(&a): a_edits } });
        f.answer(json!({
            "capabilities": capabilities.clone(),
            "textDocument/prepareRename": range(0, 4, 7),
            "textDocument/rename": local,
        }));
        f.put_caret(cx, 5);
        f.run(cx, "refactor.rename");
        let dialog = f.prompt(cx);
        assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "Rename 'bar'");
        assert_eq!(retype(&dialog, "baz", cx), "bar");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| f.text(cx) == "let baz = 1\nbaz + baz\n");
        assert!(f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "no preview for one file");

        // Nothing renamable at the caret.
        f.answer(json!({ "capabilities": capabilities, "textDocument/prepareRename": null }));
        f.run(cx, "refactor.rename");
        f.wait_hint(cx, CANNOT_RENAME);
        assert!(f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
        f.close(cx);
    }

    /// Reformat Code applies the server's edits as one undo step and says how many lines changed;
    /// with nothing to change it says so; Reformat File opens the file it is given and formats it.
    #[gpui_kit::test]
    async fn reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines(cx: &mut TestAppContext) {
        let files = [("a.mock", "let  x=1\nok\n"), ("b.mock", "fn  f\n")];
        let capabilities = json!({ "documentFormattingProvider": true, "documentRangeFormattingProvider": true });
        let (f, cx) = open(cx, &files, "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
        f.answer(json!({
            "capabilities": capabilities.clone(),
            "textDocument/formatting": [text_edit(range(0, 3, 5), " "), text_edit(range(0, 6, 7), " = ")],
        }));
        f.run(cx, "code.reformat");
        settle(cx, |cx| f.text(cx) == "let x = 1\nok\n");
        f.wait_hint(cx, "Formatted 1 line");
        let asked = f.requests("textDocument/formatting");
        assert_eq!(asked.len(), 1);
        assert!(asked[0]["params"]["options"]["tabSize"].is_number(), "{asked:?}");
        // One undo step takes the whole reformat back.
        f.run(cx, "edit.undo");
        assert_eq!(f.text(cx), "let  x=1\nok\n");
        f.run(cx, "edit.redo");
        assert_eq!(f.text(cx), "let x = 1\nok\n");

        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/formatting": [] }));
        f.run(cx, "code.reformat");
        f.wait_hint(cx, "No lines changed: content is already properly formatted");

        // Reformat File on a file that is not open: it opens, then is formatted.
        let b = f.root.join("b.mock");
        f.answer(json!({ "capabilities": capabilities, "textDocument/formatting": [text_edit(range(0, 2, 4), " ")] }));
        f.run_with(cx, "code.reformatFile", Some(json!({ "path": b.to_string_lossy() })));
        settle(cx, |cx| f.editing(cx, &b));
        settle(cx, |cx| f.text(cx) == "fn f\n");
        f.close(cx);
    }

    /// Reformat with Prettier, oxfmt or Ruff runs the formatter of that name (the program Settings
    /// name for it), whatever the settings select, and takes the text it prints.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn reformat_with_a_named_formatter_runs_that_formatter(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt;
        let tools = tempfile::tempdir().expect("a folder for the formatters");
        let fake = |name: &str, output: &str| {
            let path = tools.path().join(name);
            let script = format!("#!/bin/sh\ncat >/dev/null\nprintf '{output}\\n'\n");
            std::fs::write(&path, script).expect("the formatter is written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the formatter runs");
            path.to_string_lossy().into_owned()
        };
        let settings = [
            ("languages.prettier.path", json!(fake("prettier", "# by prettier"))),
            ("languages.oxfmt.path", json!(fake("oxfmt", "# by oxfmt"))),
            ("languages.ruff.path", json!(fake("ruff", "x = 1  # by ruff"))),
        ];
        let files = [("README.md", "#   Title\n"), ("a.py", "x=1\n")];
        let (f, cx) = open(cx, &files, "README.md", None, &settings);
        f.run(cx, "code.reformatWithPrettier");
        settle(cx, |cx| f.text(cx) == "# by prettier\n");
        f.run(cx, "code.reformatWithOxfmt");
        settle(cx, |cx| f.text(cx) == "# by oxfmt\n");
        let py = f.root.join("a.py");
        f.workbench.update_in(cx, |w, window, cx| w.open_project_file(py.clone(), window, cx));
        settle(cx, |cx| f.editing(cx, &py));
        f.run(cx, "code.reformatWithRuff");
        settle(cx, |cx| f.text(cx) == "x = 1  # by ruff\n");
        f.close(cx);
    }

    /// Optimize Imports applies the organize-imports action the server offers for the whole file;
    /// without one it says so. Optimize Imports in File and Code Cleanup in File open the file they
    /// are given; Code Cleanup fixes all, organizes the imports and reformats.
    #[gpui_kit::test]
    async fn optimize_imports_and_code_cleanup_apply_the_servers_source_actions(cx: &mut TestAppContext) {
        let files = [
            ("a.mock", "import b\nimport a\n"),
            ("b.mock", "import y\nimport x\n"),
            ("c.mock", "import d\nimport c\nlet x = 1;;\n"),
        ];
        let capabilities = json!({ "documentFormattingProvider": true });
        let (f, cx) = open(cx, &files, "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
        let (a, b, c) = (f.root.join("a.mock"), f.root.join("b.mock"), f.root.join("c.mock"));
        let sort = |path: &Path, first: &str, second: &str| {
            let whole = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 8 } });
            json!({
                "title": "Organize imports",
                "kind": "source.organizeImports",
                "edit": edit(path, vec![text_edit(whole, &format!("import {first}\nimport {second}"))]),
            })
        };
        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/codeAction": [sort(&a, "a", "b")] }));
        f.run(cx, "code.optimizeImports");
        settle(cx, |cx| f.text(cx) == "import a\nimport b\n");
        let asked = f.requests("textDocument/codeAction");
        assert_eq!(asked[0]["params"]["context"]["only"], json!(["source.organizeImports"]));
        assert_eq!(asked[0]["params"]["range"]["start"], json!({ "line": 0, "character": 0 }));
        assert_eq!(asked[0]["params"]["range"]["end"], json!({ "line": 2, "character": 0 }));

        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/codeAction": [] }));
        f.run(cx, "code.optimizeImports");
        f.wait_hint(cx, "Optimize imports is not available");

        // Optimize Imports in File on a file that is not open: it opens, then its imports are sorted.
        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/codeAction": [sort(&b, "x", "y")] }));
        f.run_with(cx, "code.optimizeImportsFile", Some(json!({ "path": b.to_string_lossy() })));
        settle(cx, |cx| f.editing(cx, &b));
        settle(cx, |cx| f.text(cx) == "import x\nimport y\n");

        let fix_all = json!({
            "title": "Fix all",
            "kind": "source.fixAll",
            "edit": edit(&c, vec![text_edit(range(2, 9, 11), ";")]),
        });
        f.answer(json!({
            "capabilities": capabilities,
            "textDocument/codeAction": [sort(&c, "c", "d"), fix_all],
            "textDocument/formatting": [text_edit(range(2, 0, 3), "var")],
        }));
        f.run_with(cx, "code.cleanupFile", Some(json!({ "path": c.to_string_lossy() })));
        settle(cx, |cx| f.editing(cx, &c));
        settle(cx, |cx| f.text(cx) == "import c\nimport d\nvar x = 1;\n");
        f.close(cx);
    }

    /// F2 moves the caret to the next error and shows its message there, wrapping, and skips the
    /// warnings; ⇧F2 goes back; Error Description shows the problems at the caret, or that there
    /// are none.
    #[gpui_kit::test]
    async fn next_error_moves_the_caret_to_the_next_problem_and_shows_its_message(cx: &mut TestAppContext) {
        let problem = |line: u32, from: u32, to: u32, severity: u32, message: &str| {
            let range = range(line, from, to);
            json!({ "range": range, "message": message, "severity": severity, "source": "mock" })
        };
        let diagnostics = json!([
            problem(0, 4, 7, 1, "unknown foo"),
            problem(1, 0, 3, 2, "unused bar"),
            problem(2, 2, 5, 1, "unknown baz"),
        ]);
        let text = "let foo = 1\nbar\n  baz\n";
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "diagnostics": diagnostics })), &[]);
        let editor = f.active(cx);
        settle(cx, |cx| cx.update(|_, cx| problems(&editor, cx).len()) == 3);

        f.put_caret(cx, 0);
        cx.simulate_keystrokes("f2");
        settle(cx, |cx| f.caret(cx) == 4);
        f.wait_hint(cx, "unknown foo");
        f.run(cx, "navigate.nextError");
        settle(cx, |cx| f.caret(cx) == 18);
        f.wait_hint(cx, "unknown baz");
        f.run(cx, "navigate.nextError");
        settle(cx, |cx| f.caret(cx) == 4);
        f.run(cx, "navigate.prevError");
        settle(cx, |cx| f.caret(cx) == 18);

        f.run(cx, "code.showErrorDescription");
        f.wait_hint(cx, "unknown baz");
        f.put_caret(cx, 10);
        f.run(cx, "code.showErrorDescription");
        f.wait_hint(cx, NO_PROBLEMS_AT_CARET);
        f.close(cx);
    }

    /// Apply Quick Fix applies the first quick fix the server offers for the problem at the caret,
    /// asking for quick fixes only; with none it says so.
    #[gpui_kit::test]
    async fn apply_quick_fix_applies_the_fix_of_the_problem_at_the_caret(cx: &mut TestAppContext) {
        let problem = json!({ "range": range(0, 4, 7), "message": "unknown foo", "severity": 1, "source": "mock" });
        let answers = json!({ "diagnostics": [problem.clone()] });
        let (f, cx) = open(cx, &[("a.mock", "let foo = 1\n")], "a.mock", Some(answers), &[]);
        let a = f.root.join("a.mock");
        let editor = f.active(cx);
        settle(cx, |cx| cx.update(|_, cx| !problems(&editor, cx).is_empty()));
        let fix = |title: &str, name: &str, preferred: bool| {
            json!({
                "title": title,
                "kind": "quickfix",
                "isPreferred": preferred,
                "edit": edit(&a, vec![text_edit(range(0, 4, 7), name)]),
            })
        };
        let refactoring = json!({ "title": "Extract", "kind": "refactor.extract", "edit": edit(&a, vec![]) });
        let actions = [refactoring, fix("Rename to fob", "fob", false), fix("Rename to foe", "foe", true)];
        f.answer(json!({ "diagnostics": [problem.clone()], "textDocument/codeAction": actions }));
        f.put_caret(cx, 5);
        f.run(cx, "code.applyQuickFix");
        settle(cx, |cx| f.text(cx) == "let foe = 1\n");
        let asked = f.requests("textDocument/codeAction");
        assert_eq!(asked[0]["params"]["context"]["only"], json!(["quickfix"]));

        f.answer(json!({ "textDocument/codeAction": [] }));
        f.put_caret(cx, 0);
        f.run(cx, "code.applyQuickFix");
        f.wait_hint(cx, NO_QUICK_FIXES);
        f.close(cx);
    }

    /// Highlight Usages in File marks the occurrences the server reports and says how many there
    /// are; Next / Previous Highlighted Usage move the caret through them at the same place in the
    /// name, wrapping; the action again on a usage removes them, and so does Escape; with no usages
    /// it says so.
    #[gpui_kit::test]
    async fn highlight_usages_marks_the_occurrences_and_the_caret_moves_through_them(cx: &mut TestAppContext) {
        let capabilities = json!({ "documentHighlightProvider": true });
        let text = "let foo = 1\nfoo + foo\n";
        let answers = json!({ "capabilities": capabilities.clone() });
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(answers), &[]);
        let highlights = json!([
            { "range": range(0, 4, 7), "kind": 3 },
            { "range": range(1, 0, 3), "kind": 2 },
            { "range": range(1, 6, 9), "kind": 2 },
        ]);
        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/documentHighlight": highlights }));
        let editor = f.active(cx);
        let marks = |cx: &mut VisualTestContext| {
            editor.read_with(cx, |e, _| e.marks(MarkLayer::Usages).iter().map(|m| m.range.clone()).collect::<Vec<_>>())
        };
        f.put_caret(cx, 5);
        f.run(cx, "find.highlightUsages");
        settle(cx, |cx| !marks(cx).is_empty());
        assert_eq!(marks(cx), [4..7, 12..15, 18..21]);
        let status = f.workbench.read_with(cx, |w, _| w.usages_status().map(str::to_owned)).expect("the status text");
        assert!(status.starts_with("3 usages of 'foo' found ("), "{status}");
        assert!(status.ends_with("Escape to remove all highlighting)"), "{status}");

        f.run(cx, "navigate.nextHighlightedUsage");
        assert_eq!(f.caret(cx), 13);
        f.run(cx, "navigate.nextHighlightedUsage");
        assert_eq!(f.caret(cx), 19);
        f.run(cx, "navigate.nextHighlightedUsage");
        assert_eq!(f.caret(cx), 5, "it wraps");
        f.run(cx, "navigate.prevHighlightedUsage");
        assert_eq!(f.caret(cx), 19);

        // Again on a usage: the highlighting goes.
        f.run(cx, "find.highlightUsages");
        assert!(marks(cx).is_empty());
        assert!(f.workbench.read_with(cx, |w, _| w.usages_status().is_none()));
        // Escape removes it too.
        f.run(cx, "find.highlightUsages");
        settle(cx, |cx| !marks(cx).is_empty());
        cx.simulate_keystrokes("escape");
        assert!(marks(cx).is_empty());

        f.answer(json!({ "capabilities": capabilities, "textDocument/documentHighlight": [] }));
        f.run(cx, "find.highlightUsages");
        f.wait_hint(cx, NO_USAGES_IN_FILE);
        assert!(marks(cx).is_empty());
        f.close(cx);
    }

    /// Quick Definition shows the lines of the definition at the caret in a popup, and F4 opens it
    /// with the caret on it; Quick Type Definition with no type definition says so.
    #[gpui_kit::test]
    async fn quick_definition_shows_the_definitions_lines_and_f4_opens_it(cx: &mut TestAppContext) {
        let files = [("a.mock", "call target here\n"), ("b.mock", "zero\none\n  target\nthree\n")];
        let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
        let b = f.root.join("b.mock");
        let at = json!({ "line": 2, "character": 2 });
        f.answer(json!({ "textDocument/definition": [{ "uri": uri(&b), "range": { "start": at, "end": at } }] }));
        f.put_caret(cx, 7);
        f.run(cx, "code.quickDefinition");
        settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some()));
        let popup = f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>()).expect("the popup");
        assert_eq!(popup.read_with(cx, |p, _| p.header()), ["b.mock", "b.mock:3"]);
        assert_eq!(popup.read_with(cx, |p, _| p.lines()), ["2  one", "3    target", "4  three"]);
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the popup has the keyboard");
        cx.simulate_keystrokes("f4");
        settle(cx, |cx| f.editing(cx, &b));
        let opened = f.active(cx);
        settle(cx, |cx| opened.read_with(cx, |e, _| e.caret_position()) == (2, 2));
        assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_none()));

        f.answer(json!({ "textDocument/typeDefinition": [] }));
        f.run(cx, "code.quickTypeDefinition");
        f.wait_hint(cx, NO_TYPE_DEFINITION);
        assert_eq!(f.requests("textDocument/typeDefinition").len(), 1);
        f.close(cx);
    }

    /// Generate lists the server's generate actions at the caret and applies the one chosen;
    /// Implement Methods applies the only implement action at once; Override Methods with none
    /// says so.
    #[gpui_kit::test]
    async fn generate_lists_the_generate_actions_and_applies_the_chosen_one(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.mock", "class A {\n}\n")], "a.mock", Some(json!({})), &[]);
        let a = f.root.join("a.mock");
        let action = |title: &str, kind: &str, text: &str| {
            let edit = edit(&a, vec![text_edit(range(1, 0, 0), text)]);
            json!({ "title": title, "kind": kind, "edit": edit })
        };
        f.answer(json!({ "textDocument/codeAction": [
            action("Generate constructor", "source.generate.constructor", "  A() {}\n"),
            action("Generate toString()", "source.generate.toString", "  toString() {}\n"),
            action("Inline variable", "refactor.inline", ""),
            action("Implement missing members", "quickfix", "  run() {}\n"),
        ] }));
        f.put_caret(cx, 6);
        f.run(cx, "code.generate");
        let list = f.list(cx);
        assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Generate");
        let lines = list.read_with(cx, |l, _| l.lines());
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("Generate toString()")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("Inline variable")), "{lines:?}");
        let chosen = list.read_with(cx, |l, _| l.lines()).iter().position(|line| line.contains("Generate toString()"));
        for _ in 0..chosen.expect("listed") {
            cx.simulate_keystrokes("down");
        }
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| f.text(cx) == "class A {\n  toString() {}\n}\n");

        f.answer(json!({ "textDocument/codeAction": [
            action("Implement missing members", "quickfix", "  run() {}\n"),
            action("Generate constructor", "source.generate.constructor", "  A() {}\n"),
        ] }));
        f.run(cx, "code.implementMethods");
        settle(cx, |cx| f.text(cx) == "class A {\n  run() {}\n  toString() {}\n}\n");

        f.run(cx, "code.overrideMethods");
        f.wait_hint(cx, "Nothing to override");
        f.close(cx);
    }

    /// ⌘N in a Markdown file opens the New popup: Generate yields the key outside the languages
    /// whose servers generate code.
    #[gpui_kit::test]
    async fn cmd_n_in_a_markdown_file_opens_the_new_popup(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("README.md", "# Title\n")], "README.md", None, &[]);
        cx.simulate_keystrokes("cmd-n");
        settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.new_popup().is_some()));
        assert_eq!(f.text(cx), "# Title\n");
        f.close(cx);
    }

    /// Go to Test opens the test of the file, and from the test Go to Test Subject opens the file
    /// it tests.
    #[gpui_kit::test]
    async fn opens_the_only_test_of_a_class(cx: &mut TestAppContext) {
        let files = [
            ("src/calc.py", "def add(a, b):\n    return a + b\n"),
            ("tests/test_calc.py", "def test_add():\n    pass\n"),
        ];
        let (f, cx) = open(cx, &files, "src/calc.py", None, &[]);
        let (subject, test) = (f.root.join("src/calc.py"), f.root.join("tests/test_calc.py"));
        let indexed = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let host = cx.global::<Backend>().host.clone();
                let found = ide_host::ops::nav::find_files(&host, "w1", "test_calc.py".to_owned(), 50);
                found.is_ok_and(|found| !found.is_empty())
            })
        };
        settle(cx, indexed);
        f.run(cx, "navigate.gotoTest");
        settle(cx, |cx| f.editing(cx, &test));
        f.run(cx, "navigate.gotoTest");
        settle(cx, |cx| f.editing(cx, &subject));
        f.close(cx);
    }

    /// Next Method moves the caret to the next method or type of the file's outline by line, and
    /// Previous Method back.
    #[gpui_kit::test]
    async fn moves_by_line(cx: &mut TestAppContext) {
        let capabilities = json!({ "documentSymbolProvider": true });
        let text = "fn one() {\n}\n\nfn two() {\n}\n";
        let answers = json!({ "capabilities": capabilities.clone() });
        let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(answers), &[]);
        let symbol = |name: &str, line: u32| {
            let whole = json!({
                "start": { "line": line, "character": 0 },
                "end": { "line": line + 1, "character": 1 },
            });
            json!({ "name": name, "kind": 12, "range": whole, "selectionRange": range(line, 3, 6) })
        };
        let symbols = [symbol("one", 0), symbol("two", 3)];
        f.answer(json!({ "capabilities": capabilities, "textDocument/documentSymbol": symbols }));
        f.put_caret(cx, 0);
        f.run(cx, "navigate.nextMethod");
        settle(cx, |cx| f.caret(cx) == 17);
        f.run(cx, "navigate.previousMethod");
        settle(cx, |cx| f.caret(cx) == 3);
        f.close(cx);
    }

    /// The language actions' keys need the editor (`textEditorActive && (editorFocus || popupOpen ||
    /// !toolWindowFocus)`): with the Project view focused F2, ⇧F2, ⌘F1, ⌘⇧F7, ⌃⇧↓, ⌃⇧↑, ⌃⌥↓, ⌃⌥↑,
    /// ⌃⌥O, ⌥⇧↵, ⌥⌘L, ⌃I, ⌃O, ⌥Space, ⌘Y, ⇧⌘T, ⇧F6 and ⌘N do nothing to the editor; back in the
    /// editor F2 says the file has no problems.
    #[gpui_kit::test]
    async fn the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard(cx: &mut TestAppContext) {
        let text = "let  x=1\n";
        let (f, cx) = open(cx, &[("a.py", text)], "a.py", None, &[]);
        let editor = f.active(cx);
        f.put_caret(cx, 4);
        let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a project tree");
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f2 cmd-f1 cmd-shift-f7 ctrl-shift-down ctrl-alt-down ctrl-alt-o");
        cx.simulate_keystrokes("alt-shift-enter cmd-alt-l shift-f2 ctrl-shift-up ctrl-alt-up");
        cx.simulate_keystrokes("ctrl-i ctrl-o alt-space cmd-y cmd-shift-t");
        cx.run_until_parked();
        let unchanged = |cx: &mut VisualTestContext| {
            assert!(f.editor(cx).is_some_and(|active| active == editor), "the same editor is active");
            assert_eq!(f.hint(cx), None);
            assert_eq!(editor.read_with(cx, |e, _| (e.text(), e.caret())), (text.to_owned(), 4));
            assert!(editor.read_with(cx, |e, _| e.marks(MarkLayer::Usages).is_empty()));
            assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_none()));
        };
        unchanged(cx);
        // Rename (⇧F6) and Generate (⌘N) last: the Project view may answer them with its own dialog.
        cx.simulate_keystrokes("shift-f6");
        cx.run_until_parked();
        unchanged(cx);
        cx.simulate_keystrokes("escape cmd-n");
        cx.run_until_parked();
        unchanged(cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f2");
        f.wait_hint(cx, NO_PROBLEMS_IN_FILE);
        f.close(cx);
    }
}
