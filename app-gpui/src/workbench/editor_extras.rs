//! More of the editor's actions: Expand to Level N / Expand All to Level N, Context Info, Custom
//! Folding, Unwrap/Remove, Expression Type Info, Reformat File…, Rerun Search, the Rainbow Brackets
//! switch, and Next / Previous Occurrence with the status bar's message.
//!
//! Next / Previous Occurrence step through the results of the focused tool window, else of the last
//! active, the last used or a visible one: the first of them that can step (Usages, Problems, TODO). The step
//! shows in the status bar as "2 of 5" for a while.

use super::Workbench;
use super::editor_language::{FormatRun, REFORMAT, This, apply_silently, format_document};
use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::todo_window::TODO_TOOL_WINDOW;
use crate::backend::Backend;
use crate::code_insight::{PROBLEMS_TOOL_WINDOW, USAGES_TOOL_WINDOW};
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::view::Editor;
use crate::editor_commands::EditState;
use crate::editor_commands::unwrap::{ChainNode, UnwrapOption, unwrap_options};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::intentions_view::{ListOptions, ListPopup, ListPopupEvent, ListRow};
use crate::editor_lsp::lsp::error_hint;
use crate::editor_lsp::positions::{offset_to_pos, pos_to_offset, range_to_offsets};
use crate::editor_lsp::reformat_dialog::{LastReformatOptions, ReformatDialog, ReformatDialogEvent};
use crate::editor_lsp::type_info::{NO_TYPE_INFO, extract_type_info};
use crate::find::view::FindPanel;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::platform::i18n::{Localization, Param, format_message};
use crate::platform::settings;
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
use crate::project_tree::ProjectTree;
use crate::workbench::active_editor;
use gpui_kit::{App, AppContext as _, Context, Entity, Focusable, Window};
use ide_core::{DocId, IdeError, IdeResult, Pos};
use ide_syntax::NodeAt;
use ide_workspace::settings::SettingsScope;
use std::time::Duration;

/// When the editor's text commands apply: the editor has the keyboard, or a popup is open over it.
const TEXT_WHEN: &str = "editorTextFocus || textEditorActive && popupOpen";
/// The deepest level Expand to Level offers.
const FOLD_LEVELS: u32 = 5;
/// How long the status bar's message stays.
const STATUS_INFO_TIMEOUT: Duration = Duration::from_millis(8000);
/// The kind of the code action Reformat File… runs first when it optimizes imports.
const ORGANIZE_IMPORTS: &str = "source.organizeImports";

const NO_CONTEXT_INFO: &str = "No context information here";
const NO_CUSTOM_REGIONS: &str = "No custom folding regions in this file";
const NO_REGION_NAME: &str = "(no description)";
const CUSTOM_REGIONS_TITLE: &str = "Custom Folding Regions";
const UNWRAP_UNAVAILABLE: &str = "Unwrap/Remove is not available for this language";
const NOTHING_TO_UNWRAP: &str = "Nothing to unwrap here";
const UNWRAP_TITLE: &str = "Choose the Statement to Unwrap/Remove";
const READ_ONLY: &str = "The file is read-only";

/// What this module keeps on a window: the status bar's transient message.
#[derive(Debug, Default)]
pub(super) struct EditorExtras {
    /// The message and the number it was shown under: a newer message outlives an older one's timer.
    status_info: Option<(String, u64)>,
    shown: u64,
}

/// What the tree-sitter structure says at the caret: `None` when the language has no grammar.
type ChainAnswer = IdeResult<Option<Option<NodeAt>>>;

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for level in 1..=FOLD_LEVELS {
        for all in [false, true] {
            let (id, title) = if all {
                (format!("code.expandAllToLevel{level}"), format!("Expand All to Level {level}"))
            } else {
                (format!("code.expandToLevel{level}"), format!("Expand to Level {level}"))
            };
            // Their ids are built from the level, so they are not in the generated list of actions.
            let mut meta = ActionMeta::new(&id, &title).category("Folding").when(TEXT_WHEN);
            meta.keywords = vec!["unfold".to_owned(), "fold level".to_owned()];
            registry.register(Action::new(meta, move |_, window, cx| {
                let editor =
                    active_editor(window, cx).ok_or_else(|| anyhow::anyhow!("no editor is open in this window"))?;
                editor.update(cx, |editor, cx| {
                    editor.expand_to_level(level, all, cx);
                });
                Ok(())
            }));
        }
    }
    registry.handle("editor.contextInfo", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| workbench.show_context_info(cx))
    });
    registry.handle("navigate.customRegion", |_, window, cx| on_workbench(window, cx, Workbench::show_custom_regions));
    registry.handle("code.unwrap", |_, window, cx| on_workbench(window, cx, Workbench::unwrap_at_caret));
    registry
        .handle("code.expressionTypeInfo", |_, window, cx| on_workbench(window, cx, Workbench::expression_type_info));
    registry.handle("code.reformatDialog", |_, window, cx| on_workbench(window, cx, Workbench::show_reformat_dialog));
    registry.handle("find.rerun", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            if let Some(panel) = workbench.find_panel() {
                panel.update(cx, FindPanel::rerun);
            }
        })
    });
    for (id, direction) in
        [("navigate.nextOccurrence", Direction::Next), ("navigate.prevOccurrence", Direction::Previous)]
    {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| workbench.go_to_occurrence(direction, cx))
        });
    }
    // The Color Scheme switch Rainbow brackets, as a toggle action.
    registry.handle_toggle(
        "editor.toggleRainbowBrackets",
        |_, cx| settings::global(cx).editor.rainbow_brackets,
        |_, window, cx| {
            let next = !settings::global(cx).editor.rainbow_brackets;
            if let Err(error) = settings::set("", SettingsScope::Global, "editor.rainbowBrackets", &next.into(), cx) {
                on_workbench(window, cx, move |workbench, _, cx| {
                    let request = NotificationRequest {
                        body: Some(error.message),
                        ..NotificationRequest::new(NotificationLevel::Error, "Cannot change the setting")
                    };
                    workbench.notify(request, cx);
                })?;
            }
            Ok(())
        },
    );
    // File › Reload All from Disk runs it after reading the open documents again.
    registry.handle("project.refresh", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            if let Some(tree) = workbench.project_tree().cloned() {
                tree.update(cx, ProjectTree::reload_all);
            }
        })
    });
}

/// Shows `text` (English source text) as the hint of `editor`, at `pos` (the caret when `None`),
/// once the current effects are delivered.
fn hint(editor: &Entity<Editor>, text: impl Into<String>, pos: Option<u32>, cx: &mut App) {
    let (editor, text) = (editor.downgrade(), text.into());
    cx.defer(move |cx| {
        let Some(editor) = editor.upgrade() else { return };
        if let Some(intelligence) = EditorIntelligence::of(&editor, cx) {
            intelligence.update(cx, |this, cx| this.show_hint_at(&text, pos, Tone::Info, None, cx));
        }
    });
}

/// The chain of syntax nodes from the one at the caret outward, as offsets of the text of `rope`.
fn chain_of(at: &NodeAt, rope: &ropey::Rope) -> Vec<ChainNode> {
    std::iter::once(&at.node)
        .chain(&at.ancestors)
        .map(|node| ChainNode {
            kind: node.kind.clone(),
            from: pos_to_offset(rope, node.range.start),
            to: pos_to_offset(rope, node.range.end),
            field: node.field.clone(),
        })
        .collect()
}

/// The first line of the statement `option` takes away, as the chooser previews it.
fn unwrap_preview(state: &EditState, option: &UnwrapOption) -> String {
    let (from, to) = option.target;
    let line_end = state.line_at(from).to;
    state.slice(from, to.min(line_end)).trim().to_owned()
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // The status bar's message

    /// The status bar's transient message (Next Occurrence's "2 of 5"), while it shows.
    pub fn status_info(&self) -> Option<&str> {
        self.extras.status_info.as_ref().map(|(text, _)| text.as_str())
    }

    /// Shows `text` in the status bar until a newer message replaces it or it times out.
    pub(super) fn show_status_info(&mut self, text: String, cx: &mut Context<Self>) {
        self.extras.shown += 1;
        let shown = self.extras.shown;
        self.extras.status_info = Some((text, shown));
        cx.notify();
        cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(STATUS_INFO_TIMEOUT).await;
            let _ = workbench.update(cx, |workbench: &mut Workbench, cx| {
                if workbench.extras.status_info.as_ref().is_some_and(|(_, at)| *at == shown) {
                    workbench.extras.status_info = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Next / Previous Occurrence

    /// Steps the results of the first tool window that can, in the order of focused, last active,
    /// last used and visible ones; the editor takes the keyboard unless that window had it.
    fn go_to_occurrence(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let Some(layout) = self.tool_windows() else { return };
        let focused = layout.focused().map(str::to_owned);
        let mut candidates: Vec<String> = Vec::new();
        let ordered = [layout.focused(), layout.last_active(), layout.last_used()].into_iter().flatten();
        for id in ordered.chain(layout.visible().ids()) {
            if !candidates.iter().any(|known| known == id) {
                candidates.push(id.to_owned());
            }
        }
        for id in candidates {
            let focus_editor = focused.as_deref() != Some(id.as_str());
            let stepped: Option<Option<OccurrenceStep>> = match id.as_str() {
                USAGES_TOOL_WINDOW => self
                    .usages_view()
                    .map(|view| view.update(cx, |view, cx| view.step_occurrence(direction, focus_editor, cx))),
                PROBLEMS_TOOL_WINDOW => self
                    .problems_view()
                    .map(|view| view.update(cx, |view, cx| view.step_occurrence(direction, focus_editor, cx))),
                TODO_TOOL_WINDOW => self.todo_window_view().map(|view| {
                    let forward = direction == Direction::Next;
                    view.update(cx, |view, cx| view.select_occurrence(forward, focus_editor, cx));
                    None
                }),
                _ => None,
            };
            let Some(step) = stepped else { continue };
            if let Some(step) = step {
                let i18n = &cx.default_global::<Localization>().0;
                let params = [("index", Param::from(step.index)), ("total", Param::from(step.total))];
                let text = format_message(&i18n.tx("{index} of {total}"), &params, i18n.locale());
                self.show_status_info(text, cx);
            }
            return;
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Folding and regions

    /// View › Context Info (⌃⇧Q): the header lines of the code around the caret, as the hint.
    fn show_context_info(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let max = settings::effective(&self.label, cx).editor.code_editing.sticky_lines_max.clamp(1, 20);
        let text = {
            let editor = editor.read(cx);
            let lines: Vec<String> = editor
                .context_header_lines(max)
                .into_iter()
                .map(|line| editor.line_text(line).trim_end().to_owned())
                .collect();
            lines.join("\n")
        };
        let text = if text.is_empty() { cx.default_global::<Localization>().0.tx(NO_CONTEXT_INFO) } else { text };
        self.show_hint(text, cx);
    }

    /// Navigate › Custom Folding… (⌥⌘.): the file's custom regions; the one chosen gets the caret.
    pub fn show_custom_regions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let regions = editor.read(cx).custom_regions();
        let (none, unnamed, title) = {
            let i18n = &cx.default_global::<Localization>().0;
            (i18n.tx(NO_CUSTOM_REGIONS), i18n.tx(NO_REGION_NAME), i18n.tx(CUSTOM_REGIONS_TITLE))
        };
        if regions.is_empty() {
            self.show_hint(none, cx);
            return;
        }
        let rows: Vec<ListRow> = regions
            .iter()
            .map(|region| ListRow {
                id: region.start_line.to_string(),
                label: if region.name.is_empty() { unnamed.clone() } else { region.name.clone() },
                detail: Some(format!(":{}", region.start_line + 1)),
                icon: Some("folder".to_owned()),
                ..ListRow::default()
            })
            .collect();
        let list = cx.new(|cx| ListPopup::new(&title, rows, ListOptions::default(), cx));
        let target = editor.downgrade();
        let events =
            cx.subscribe_in(&list, window, move |workbench, _, event: &ListPopupEvent, window, cx| match event {
                ListPopupEvent::Activated(id) => {
                    workbench.close_popup(window, cx);
                    let (Ok(line), Some(editor)) = (id.parse::<u32>(), target.upgrade()) else { return };
                    editor.update(cx, |editor, cx| editor.go_to_line_text(line, cx));
                    window.focus(&editor.focus_handle(cx), cx);
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
                    workbench.close_popup(window, cx);
                }
                ListPopupEvent::Active(_) | ListPopupEvent::PreviewWanted(_) => {}
            });
        self.open_popup(&list, events, window, cx);
        list.update(cx, |list, cx| list.opened(cx));
    }

    // ---------------------------------------------------------------------------------------------
    // Unwrap/Remove

    /// Code › Unwrap/Remove… (⇧⌘⌦): the statements around the caret that can be unwrapped, from the
    /// file's syntax tree; one is unwrapped at once, several are offered in a chooser.
    pub fn unwrap_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (read_only, language, doc, version, pos) = {
            let e = editor.read(cx);
            (e.is_read_only(), e.language().clone(), e.doc(), e.version(), offset_to_pos(e.rope(), e.caret()))
        };
        if read_only || !cx.has_global::<Backend>() {
            return;
        }
        let label = self.label.clone();
        let work = cx
            .global::<Backend>()
            .with_host(move |host| async move { syntax_chain(&host, &label, language, doc, pos).await });
        cx.spawn_in(window, async move |this: This, cx| {
            let answer = work.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
            let shown = this
                .update_in(cx, |workbench, window, cx| workbench.unwrap_answer(&editor, version, answer, window, cx));
            if let Err(error) = shown {
                tracing::debug!("Unwrap/Remove ended with its window: {error:#}");
            }
        })
        .detach();
    }

    fn unwrap_answer(
        &mut self,
        editor: &Entity<Editor>,
        version: u32,
        answer: ChainAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = match answer {
            Ok(Some(Some(at))) => at,
            Ok(Some(None)) => return,
            Ok(None) => {
                let text = cx.default_global::<Localization>().0.tx(UNWRAP_UNAVAILABLE);
                self.show_hint(text, cx);
                return;
            }
            Err(error) => {
                tracing::debug!("no syntax tree at the caret: {}", error.message);
                return;
            }
        };
        // The text moved on while the tree was read: its offsets no longer apply.
        if editor.read(cx).version() != version {
            return;
        }
        let (state, options) = {
            let e = editor.read(cx);
            let state = e.edit_state();
            let options = unwrap_options(&state, &chain_of(&at, e.rope()));
            (state, options)
        };
        match options.as_slice() {
            [] => {
                let text = cx.default_global::<Localization>().0.tx(NOTHING_TO_UNWRAP);
                self.show_hint(text, cx);
            }
            [only] => apply_unwrap(editor, only, window, cx),
            _ => self.choose_unwrap(editor, version, &state, options, window, cx),
        }
    }

    fn choose_unwrap(
        &mut self,
        editor: &Entity<Editor>,
        version: u32,
        state: &EditState,
        options: Vec<UnwrapOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows: Vec<ListRow> = options
            .iter()
            .enumerate()
            .map(|(index, option)| ListRow {
                id: index.to_string(),
                label: option.label.clone(),
                preview: Some(unwrap_preview(state, option)),
                ..ListRow::default()
            })
            .collect();
        let title = cx.default_global::<Localization>().0.tx(UNWRAP_TITLE);
        let list = cx.new(|cx| ListPopup::new(&title, rows, ListOptions::default(), cx));
        let target = editor.downgrade();
        let events =
            cx.subscribe_in(&list, window, move |workbench, _, event: &ListPopupEvent, window, cx| match event {
                ListPopupEvent::Activated(id) => {
                    workbench.close_popup(window, cx);
                    let chosen = id.parse::<usize>().ok().and_then(|index| options.get(index));
                    if let (Some(option), Some(editor)) = (chosen, target.upgrade())
                        && editor.read(cx).version() == version
                    {
                        apply_unwrap(&editor, option, window, cx);
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

    // ---------------------------------------------------------------------------------------------
    // Expression Type Info

    /// View › Expression Type Info (⌃⇧P): the type of the expression at the caret, from the
    /// signature in the language server's hover, as the hint at the expression.
    pub fn expression_type_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        if !cx.has_global::<Backend>() {
            return;
        }
        let (doc, caret, pos) = {
            let e = editor.read(cx);
            (e.doc(), e.caret(), offset_to_pos(e.rope(), e.caret()))
        };
        let label = self.label.clone();
        let request = Backend::next_request_id();
        let work = cx.global::<Backend>().with_host(move |host| async move {
            ide_host::ops::lsp::lsp_hover(&host, &label, request, doc, pos).await
        });
        cx.spawn_in(window, async move |_, cx| {
            let answer = work.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
            let shown = cx.update(|_, cx| match answer {
                Ok(hover) => {
                    let info = hover.as_ref().and_then(|hover| extract_type_info(Some(&hover.contents)));
                    match (hover, info) {
                        (Some(hover), Some(info)) => {
                            let at = hover.range.map(|range| range_to_offsets(editor.read(cx).rope(), range).start);
                            hint(&editor, info.text, at.or(Some(caret)), cx);
                        }
                        _ => hint(&editor, NO_TYPE_INFO, None, cx),
                    }
                }
                Err(error) => {
                    if let Some(text) = error_hint(&error, true) {
                        hint(&editor, text, None, cx);
                    }
                }
            });
            if let Err(error) = shown {
                tracing::debug!("Expression Type Info ended with its window: {error:#}");
            }
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Reformat File…

    /// The Reformat File… dialog, while it is open.
    pub fn reformat_dialog(&self) -> Option<Entity<ReformatDialog>> {
        self.vcs_dialog_view::<ReformatDialog>()
    }

    /// Code › Reformat File… (⌥⇧⌘L): the dialog of the scope and options, then Reformat Code (and
    /// Optimize Imports first when asked) with them.
    pub fn show_reformat_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (read_only, has_selection, name) = {
            let e = editor.read(cx);
            let name = e.path().file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            (e.is_read_only(), !e.selection().is_empty(), name)
        };
        if read_only {
            hint(&editor, READ_ONLY, None, cx);
            return;
        }
        let initial = cx.default_global::<LastReformatOptions>().0;
        let dialog = cx.new(|cx| ReformatDialog::new(&name, has_selection, true, initial, cx));
        let target = editor.downgrade();
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &ReformatDialogEvent, window, cx| {
            workbench.close_vcs_dialog(window, cx);
            let ReformatDialogEvent::Run(options) = *event else { return };
            let remembered = cx.default_global::<LastReformatOptions>().after_run(options, has_selection);
            cx.set_global(remembered);
            let Some(editor) = target.upgrade() else { return };
            window.focus(&editor.focus_handle(cx), cx);
            cx.spawn_in(window, async move |this: This, cx| {
                if options.optimize_imports
                    && let Err(error) = apply_silently(&this, &editor, ORGANIZE_IMPORTS, cx).await
                {
                    tracing::debug!("Reformat File ended with its window: {error:#}");
                    return;
                }
                let run = FormatRun { selection_only: options.selection_only, ..REFORMAT };
                if let Err(error) = format_document(&this, &editor, run, cx).await {
                    tracing::debug!("Reformat File ended with its window: {error:#}");
                }
            })
            .detach();
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Find

    /// The Find tool window's panel, once it is shown.
    fn find_panel(&self) -> Option<Entity<FindPanel>> {
        self.tool_window_view(super::FIND_TOOL_WINDOW).and_then(|view| view.downcast::<FindPanel>().ok())
    }
}

/// Reads whether the language of `doc` has a grammar and, if so, the syntax node at `pos` with its
/// ancestors.
async fn syntax_chain(
    host: &ide_host::Host,
    label: &str,
    language: ide_core::LanguageId,
    doc: DocId,
    pos: Pos,
) -> ChainAnswer {
    if !ide_host::ops::syntax::syntax_support(host, label, language)?.tree {
        return Ok(None);
    }
    let range = ide_core::Range { start: pos, end: pos };
    ide_host::ops::syntax::syntax_node_at(host, label, doc, range, None).await.map(Some)
}

/// Replaces the statement `option` takes away by the code it keeps, the caret where that code starts.
fn apply_unwrap(editor: &Entity<Editor>, option: &UnwrapOption, window: &mut Window, cx: &mut App) {
    let edit = option.edit();
    let applied = editor.update(cx, |editor, cx| editor.apply_edit(&edit.changes, edit.selections, edit.main, cx));
    if !applied {
        tracing::warn!("Unwrap/Remove: the editor refused the edit");
    }
    window.focus(&editor.focus_handle(cx), cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::editor::extras::FoldMarker;
    use crate::editor::intelligence::fake_server::FakeServer;
    use crate::editor::marks::MarkLayer;
    use crate::editor::view::EditorEvent;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::context::window_context;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use crate::workbench;
    use gpui_kit::{Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px};
    use serde_json::{Value, json};
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// A trusted project of `files` in a window, `open` open in the editor with the keyboard. With
    /// `answers` the IDE is in Full mode, `*.mock` files have the fake language server answering
    /// them, and the server has the opened file before this returns; without, no server runs.
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
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            workbench::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(dirs, cx).expect("the backend starts");
            workbench::menu::init(cx);
            let host = cx.global::<Backend>().host.clone();
            host.global_settings.set(SettingsScope::Global, "ide.mode", json!(mode)).expect("the mode is set");
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

    #[track_caller]
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

    /// Lets the backend work for a while, for asserting that nothing happened.
    fn idle(cx: &mut VisualTestContext) {
        for _ in 0..20 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn range(line: u32, from: u32, to: u32) -> Value {
        json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
    }

    fn text_edit(range: Value, new_text: &str) -> Value {
        json!({ "range": range, "newText": new_text })
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

        fn shown_lines(&self, cx: &mut VisualTestContext) -> Vec<u32> {
            self.active(cx).read_with(cx, |editor, _| editor.shown_lines())
        }

        fn at(&self, cx: &mut VisualTestContext) -> Option<(PathBuf, (u32, u32))> {
            self.workbench.read_with(cx, |w, cx| {
                w.editor().map(|e| (e.read(cx).path().to_path_buf(), e.read(cx).caret_position()))
            })
        }

        fn hint(&self, cx: &mut VisualTestContext) -> Option<String> {
            let editor = self.active(cx);
            let intelligence =
                cx.update(|_, cx| EditorIntelligence::of(&editor, cx)).expect("the editor's intelligence");
            intelligence.read_with(cx, |this, _| this.hint().map(str::to_owned))
        }

        #[track_caller]
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

        #[track_caller]
        fn wait_opened(&self, cx: &mut VisualTestContext, name: &str) {
            let name = format!("/{name}");
            settle(cx, |_| {
                let opened = self.requests("textDocument/didOpen");
                let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
                opened.iter().filter_map(uri).any(|uri| uri.ends_with(&name))
            });
        }

        fn caret_to(&self, cx: &mut VisualTestContext, line: u32, col: u32) {
            let editor = self.active(cx);
            editor.update(cx, |editor, cx| {
                let offset = editor.offset_of(line, col);
                editor.select(offset, offset, cx);
            });
            cx.run_until_parked();
        }

        fn run(&self, cx: &mut VisualTestContext, id: &str) {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
                .unwrap_or_else(|error| panic!("{id}: {error}"));
            cx.run_until_parked();
        }

        fn unavailable(&self, cx: &mut VisualTestContext, id: &str) -> bool {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).is_err()
        }

        fn focus_editor(&self, cx: &mut VisualTestContext) {
            let editor = self.active(cx);
            cx.update(|window, cx| {
                let focus = editor.focus_handle(cx);
                window.focus(&focus, cx);
            });
            cx.run_until_parked();
        }

        fn focus_project_tree(&self, cx: &mut VisualTestContext) {
            let tree = self.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
            cx.update(|window, cx| {
                let focus = tree.focus_handle(cx);
                window.focus(&focus, cx);
            });
            cx.run_until_parked();
        }

        fn wait_folds(&self, cx: &mut VisualTestContext) {
            let editor = self.active(cx);
            settle(cx, |cx| editor.read_with(cx, |e, _| e.folds_are_current()));
        }

        fn popup_list(&self, cx: &mut VisualTestContext) -> Entity<ListPopup> {
            settle(cx, |cx| self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_some()));
            self.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>()).expect("the list is open")
        }

        fn close(&self, cx: &mut VisualTestContext) {
            cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
        }
    }

    const NESTED: &str = "fn a() {\n    if x {\n        if y {\n            z();\n        }\n    }\n}\n";

    /// Expand All to Level 1 leaves the outermost regions open and folds the ones inside them; Level
    /// 2 opens the next level; Expand to Level 1 works on the region around the caret. Their keys run
    /// them in the editor and do nothing while the Project view has the keyboard.
    #[gpui_kit::test]
    async fn expand_to_level_folds_the_regions_nested_deeper_than_the_level(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
        f.wait_folds(cx);
        f.caret_to(cx, 0, 0);
        f.run(cx, "code.expandAllToLevel1");
        assert_eq!(f.shown_lines(cx), [0, 1, 6, 7]);
        f.run(cx, "code.expandAllToLevel2");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 5, 6, 7]);
        f.run(cx, "code.unfoldAll");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);
        // Inside the body of `if x`: that region is level 1, `if y` folds.
        f.caret_to(cx, 2, 4);
        f.run(cx, "code.expandToLevel1");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 5, 6, 7]);
        f.run(cx, "code.unfoldAll");
        // ⌘* 1 in the editor: the same.
        f.caret_to(cx, 2, 4);
        cx.simulate_keystrokes("cmd-* 1");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 5, 6, 7]);
        f.run(cx, "code.unfoldAll");

        // ⌘⇧* 1 in the editor.
        f.caret_to(cx, 0, 0);
        cx.simulate_keystrokes("cmd-shift-* 1");
        assert_eq!(f.shown_lines(cx), [0, 1, 6, 7]);
        f.run(cx, "code.unfoldAll");
        f.focus_project_tree(cx);
        assert!(f.unavailable(cx, "code.expandAllToLevel1"), "the editor does not have the keyboard");
        cx.simulate_keystrokes("cmd-shift-* 1");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);
        assert!(f.unavailable(cx, "code.expandToLevel1"));
        cx.simulate_keystrokes("cmd-* 1");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);
        f.close(cx);
    }

    /// Where line `line` shows its fold chevron, and a point of its breakpoint area.
    fn gutter_points(editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) -> (Point<Pixels>, Point<Pixels>) {
        cx.run_until_parked();
        editor.read_with(cx, |e, cx| {
            let at = e.position_of(e.offset_of(line, 0)).expect("the line is painted");
            let (bounds, line_height) = (e.painted_bounds().expect("the editor is painted"), e.line_height(cx));
            let y = at.y + line_height / 2.0;
            (point(at.x - e.fold_column_width(cx) / 2.0, y), point(bounds.left() + px(8.0), y))
        })
    }

    /// The presses the editor reports from its gutter, which set breakpoints.
    fn count_gutter_presses(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Rc<Cell<usize>> {
        let presses = Rc::new(Cell::new(0));
        let counted = Rc::clone(&presses);
        cx.update(|_, cx| {
            cx.subscribe(editor, move |_, event: &EditorEvent, _| {
                if matches!(event, EditorEvent::GutterPressed { .. }) {
                    counted.set(counted.get() + 1);
                }
            })
            .detach();
        });
        presses
    }

    /// The gutter shows a chevron on every line that starts a fold; a click on it folds the region
    /// and leaves the caret, the selection and the breakpoints alone; a click on the closed chevron
    /// opens it again.
    #[gpui_kit::test]
    async fn a_click_on_the_fold_chevron_folds_and_unfolds_its_region(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
        f.wait_folds(cx);
        let editor = f.active(cx);
        let marker = |cx: &mut VisualTestContext, line| editor.read_with(cx, |e, _| e.fold_marker(line));
        assert_eq!(marker(cx, 1), Some(FoldMarker::Open));
        assert_eq!(marker(cx, 3), None);
        let presses = count_gutter_presses(&editor, cx);
        // A selection outside the region, on the first line.
        editor.update(cx, |e, cx| e.select(1, 3, cx));
        let selection = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.selections().to_vec());
        let before = selection(cx);
        let (chevron, _) = gutter_points(&editor, 1, cx);
        cx.simulate_click(chevron, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(f.shown_lines(cx), [0, 1, 6, 7]);
        assert_eq!(marker(cx, 1), Some(FoldMarker::Closed));
        assert_eq!(selection(cx), before, "the caret and the selection stay");
        assert_eq!(presses.get(), 0, "no breakpoint is toggled");
        assert!(editor.read_with(cx, |e, _| e.marks(MarkLayer::Breakpoints).is_empty()));
        let (chevron, _) = gutter_points(&editor, 1, cx);
        cx.simulate_click(chevron, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(selection(cx), before);
        assert_eq!(presses.get(), 0);
        f.close(cx);
    }

    /// A press in the breakpoint area of a line that starts a fold is the breakpoint's: nothing
    /// folds.
    #[gpui_kit::test]
    async fn a_click_in_the_breakpoint_area_folds_nothing(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
        f.wait_folds(cx);
        let editor = f.active(cx);
        let presses = count_gutter_presses(&editor, cx);
        let (_, breakpoints) = gutter_points(&editor, 1, cx);
        cx.simulate_click(breakpoints, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(presses.get(), 1, "the press is reported from the gutter");
        assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);
        f.close(cx);
    }

    /// Context Info shows the header lines of the blocks around the caret, outermost first; on a
    /// line inside none it says so. ⌃⇧Q runs it in the editor only.
    #[gpui_kit::test]
    async fn context_info_shows_the_header_lines_of_the_code_around_the_caret(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
        f.wait_folds(cx);
        f.caret_to(cx, 3, 12);
        f.run(cx, "editor.contextInfo");
        let headers = "fn a() {\n    if x {\n        if y {";
        f.wait_hint(cx, headers);
        f.caret_to(cx, 0, 0);
        cx.simulate_keystrokes("ctrl-shift-q");
        f.wait_hint(cx, NO_CONTEXT_INFO);
        // Inside the blocks again, with the Project view's keyboard: nothing is shown.
        f.caret_to(cx, 3, 12);
        f.focus_project_tree(cx);
        assert!(f.unavailable(cx, "editor.contextInfo"));
        cx.simulate_keystrokes("ctrl-shift-q");
        idle(cx);
        assert_ne!(f.hint(cx).as_deref(), Some(headers), "the Project view has the keyboard");
        f.close(cx);
    }

    /// Custom Folding lists the file's regions by name, an unnamed one as "(no description)"; the one
    /// chosen gets the caret on its marker line; a file without regions says so.
    #[gpui_kit::test]
    async fn custom_folding_lists_the_regions_and_moves_the_caret_to_the_chosen_one(cx: &mut TestAppContext) {
        let text = "fn a() {}\n// region Alpha\nfn b() {}\n// endregion\n    // region\nfn c() {}\n// endregion\n";
        let files = [("src/main.rs", text), ("src/plain.rs", "fn d() {}\n")];
        let (f, cx) = open(cx, &files, "src/main.rs", None);
        f.focus_project_tree(cx);
        cx.simulate_keystrokes("cmd-alt-.");
        idle(cx);
        assert!(
            f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()),
            "the tree has the keyboard"
        );
        f.focus_editor(cx);
        // ⌥⌘. in the editor.
        cx.simulate_keystrokes("cmd-alt-.");
        let list = f.popup_list(cx);
        let lines = list.read_with(cx, |list, _| list.lines());
        assert_eq!(lines, ["Alpha · :2", "(no description) · :5"]);
        cx.simulate_keystrokes("down enter");
        assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()));
        assert_eq!(f.at(cx).map(|(_, caret)| caret), Some((4, 4)));

        let plain = f.root.join("src/plain.rs");
        f.workbench.update_in(cx, |w, window, cx| w.open_project_file(plain.clone(), window, cx));
        settle(cx, |cx| f.at(cx).is_some_and(|(path, _)| path == plain));
        f.run(cx, "navigate.customRegion");
        f.wait_hint(cx, NO_CUSTOM_REGIONS);
        f.close(cx);
    }

    /// Unwrap/Remove on the body of an `if` replaces the `if` by its body; where nothing can be
    /// unwrapped it says so, and in a language without a grammar it says it is not available. ⇧⌘⌦
    /// runs it in the editor and not while the Find bar has the keyboard.
    #[gpui_kit::test]
    async fn unwrap_replaces_the_statement_around_the_caret_by_its_body(cx: &mut TestAppContext) {
        let text = "fn f() {\n    if a {\n        foo();\n    }\n}\n";
        let files = [("src/main.rs", text), ("notes.txt", "if a {\n    foo();\n}\n")];
        let (f, cx) = open(cx, &files, "src/main.rs", None);
        // The Find bar's field has the keyboard, not the editor's text. (The Project view is not
        // used here: ⇧⌘⌦ there would be its own Delete.)
        f.caret_to(cx, 2, 8);
        f.run(cx, "find.inFile");
        cx.simulate_keystrokes("cmd-shift-delete");
        idle(cx);
        assert_eq!(f.text(cx), text, "the Find bar has the keyboard");
        cx.simulate_keystrokes("escape");
        f.focus_editor(cx);
        f.caret_to(cx, 2, 8);
        cx.simulate_keystrokes("cmd-shift-delete");
        settle(cx, |cx| f.text(cx) == "fn f() {\n    foo();\n}\n");
        assert_eq!(f.at(cx).map(|(_, caret)| caret), Some((1, 4)));

        f.caret_to(cx, 0, 1);
        f.run(cx, "code.unwrap");
        f.wait_hint(cx, NOTHING_TO_UNWRAP);

        let notes = f.root.join("notes.txt");
        f.workbench.update_in(cx, |w, window, cx| w.open_project_file(notes.clone(), window, cx));
        settle(cx, |cx| f.at(cx).is_some_and(|(path, _)| path == notes));
        f.caret_to(cx, 1, 4);
        f.run(cx, "code.unwrap");
        f.wait_hint(cx, UNWRAP_UNAVAILABLE);
        assert_eq!(f.text(cx), "if a {\n    foo();\n}\n");
        f.close(cx);
    }

    /// Expression Type Info shows the type in the server's hover signature at the expression; with
    /// no hover it says no type information was found. ⌃⇧P asks nothing while the Project view has
    /// the keyboard.
    #[gpui_kit::test]
    async fn expression_type_info_shows_the_type_of_the_expression_at_the_caret(cx: &mut TestAppContext) {
        let capabilities = json!({ "hoverProvider": true });
        let answers = json!({ "capabilities": capabilities.clone() });
        let (f, cx) = open(cx, &[("a.mock", "let x = 1\nx + 1\n")], "a.mock", Some(answers));
        let hover =
            json!({ "contents": { "kind": "markdown", "value": "```mock\nlet x: i32\n```" }, "range": range(1, 0, 1) });
        f.answer(json!({ "capabilities": capabilities.clone(), "textDocument/hover": hover }));
        f.caret_to(cx, 1, 0);
        // ⌃⇧P in the editor.
        cx.simulate_keystrokes("ctrl-shift-p");
        f.wait_hint(cx, "i32");
        assert_eq!(f.requests("textDocument/hover").len(), 1);

        f.answer(json!({ "capabilities": capabilities, "textDocument/hover": null }));
        f.run(cx, "code.expressionTypeInfo");
        f.wait_hint(cx, NO_TYPE_INFO);
        assert_eq!(f.requests("textDocument/hover").len(), 2);

        f.focus_project_tree(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        idle(cx);
        assert_eq!(f.requests("textDocument/hover").len(), 2, "the Project view has the keyboard");
        f.close(cx);
    }

    /// Reformat File… with a selection offers Selected text and starts on it: Run reformats the
    /// selection only. Opened again without a selection it reformats the whole file. Escape closes
    /// it without reformatting.
    #[gpui_kit::test]
    async fn reformat_file_reformats_the_scope_chosen_in_the_dialog(cx: &mut TestAppContext) {
        let capabilities = json!({ "documentFormattingProvider": true, "documentRangeFormattingProvider": true });
        let answers = json!({ "capabilities": capabilities.clone() });
        let (f, cx) = open(cx, &[("a.mock", "let  x=1\nlet  y=2\n")], "a.mock", Some(answers));
        f.answer(json!({
            "capabilities": capabilities,
            "textDocument/rangeFormatting": [text_edit(range(0, 3, 5), " ")],
            "textDocument/formatting": [text_edit(range(1, 3, 5), " ")],
        }));
        let editor = f.active(cx);
        editor.update(cx, |e, cx| e.select(0, 8, cx));
        f.focus_project_tree(cx);
        cx.simulate_keystrokes("cmd-alt-shift-l");
        idle(cx);
        assert!(f.workbench.read_with(cx, |w, _| w.reformat_dialog().is_none()), "the tree has the keyboard");
        f.focus_editor(cx);
        // ⌥⇧⌘L in the editor.
        cx.simulate_keystrokes("cmd-alt-shift-l");
        let dialog = f.workbench.read_with(cx, |w, _| w.reformat_dialog()).expect("the dialog is open");
        assert!(dialog.read_with(cx, |d, _| d.offers_selection() && d.options().selection_only));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| f.text(cx) == "let x=1\nlet  y=2\n");
        assert!(f.workbench.read_with(cx, |w, _| w.reformat_dialog().is_none()));
        assert_eq!(f.requests("textDocument/rangeFormatting").len(), 1);
        assert!(f.requests("textDocument/formatting").is_empty());

        editor.update(cx, |e, cx| e.select(0, 0, cx));
        f.run(cx, "code.reformatDialog");
        let dialog = f.workbench.read_with(cx, |w, _| w.reformat_dialog()).expect("the dialog is open");
        assert!(!dialog.read_with(cx, |d, _| d.offers_selection() || d.options().selection_only));
        cx.simulate_keystrokes("escape");
        assert!(f.workbench.read_with(cx, |w, _| w.reformat_dialog().is_none()));
        f.run(cx, "code.reformatDialog");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| f.text(cx) == "let x=1\nlet y=2\n");
        assert_eq!(f.requests("textDocument/formatting").len(), 1);
        f.close(cx);
    }

    /// While usages are highlighted the status bar says how many; once they are removed it no
    /// longer does.
    #[gpui_kit::test]
    async fn the_status_bar_shows_the_highlighted_usages_count(cx: &mut TestAppContext) {
        let capabilities = json!({ "documentHighlightProvider": true });
        let answers = json!({ "capabilities": capabilities.clone() });
        let (f, cx) = open(cx, &[("a.mock", "let foo = 1\nfoo + foo\n")], "a.mock", Some(answers));
        let highlights = json!([
            { "range": range(0, 4, 7), "kind": 3 },
            { "range": range(1, 0, 3), "kind": 2 },
            { "range": range(1, 6, 9), "kind": 2 },
        ]);
        f.answer(json!({ "capabilities": capabilities, "textDocument/documentHighlight": highlights }));
        let editor = f.active(cx);
        editor.update(cx, |e, cx| e.select(5, 5, cx));
        f.run(cx, "find.highlightUsages");
        settle(cx, |cx| editor.read_with(cx, |e, _| !e.marks(MarkLayer::Usages).is_empty()));
        let bar = f.workbench.read_with(cx, |w, _| w.status_bar().clone());
        let texts = |cx: &mut VisualTestContext| cx.update(|window, cx| bar.read(cx).texts(window, cx));
        settle(cx, |cx| texts(cx).iter().any(|text| text.starts_with("3 usages of 'foo' found (")));
        cx.simulate_keystrokes("escape");
        settle(cx, |cx| !texts(cx).iter().any(|text| text.contains("usages of 'foo'")));
        f.close(cx);
    }

    /// Next Occurrence steps through the TODO items from the TODO tool window, opening each; with the
    /// editor focused and no highlighted usages, Next Highlighted Usage (⌃⌥↓) does the same. Without
    /// a tool window that steps, nothing moves.
    #[gpui_kit::test]
    async fn next_occurrence_steps_through_the_results_of_the_tool_window(cx: &mut TestAppContext) {
        let files =
            [("src/a.rs", "// TODO first\nfn a() {}\n// FIXME second\n"), ("src/b.rs", "fn b() {} // TODO third\n")];
        let (f, cx) = open(cx, &files, "src/a.rs", None);
        let (a, b) = (f.root.join("src/a.rs"), f.root.join("src/b.rs"));
        f.caret_to(cx, 1, 2);
        // No tool window steps yet: the caret stays.
        f.run(cx, "navigate.nextOccurrence");
        assert_eq!(f.at(cx), Some((a.clone(), (1, 2))));

        f.run(cx, "toolwindow.todo");
        let view = f.workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO tool window is shown");
        settle(cx, |cx| view.update(cx, |view, cx| !view.lines(cx).is_empty()));
        f.run(cx, "navigate.nextOccurrence");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (0, 3))));
        // ⌥⌘↓ from the tool window.
        cx.simulate_keystrokes("cmd-alt-down");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (2, 3))));
        f.focus_editor(cx);
        cx.simulate_keystrokes("ctrl-alt-down");
        settle(cx, |cx| f.at(cx) == Some((b.clone(), (0, 13))));
        f.run(cx, "navigate.prevOccurrence");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (2, 3))));
        // ⌃⌥↑ in the editor, with no highlighted usages: Previous Occurrence.
        f.focus_editor(cx);
        cx.simulate_keystrokes("ctrl-alt-up");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (0, 3))));
        f.close(cx);
    }

    /// Rerun Search runs the Find tool window's search again, so a file written since is found; it
    /// is not available while the Find tool window is not the active one.
    #[gpui_kit::test]
    async fn rerun_search_finds_what_was_written_since_the_search(cx: &mut TestAppContext) {
        let files = [("src/alpha.rs", "fn widget() {}\n"), ("src/beta.rs", "fn other() {}\n")];
        let (f, cx) = open(cx, &files, "src/alpha.rs", None);
        assert!(f.unavailable(cx, "find.rerun"), "the Find tool window is not shown");
        cx.simulate_keystrokes("cmd-r");
        idle(cx);
        assert!(f.workbench.read_with(cx, |w, _| w.find_panel().is_none()), "⌘R in the editor shows no Find window");
        f.run(cx, "toolwindow.find");
        let panel = f.workbench.read_with(cx, |w, _| w.find_panel()).expect("the Find window holds the panel");
        panel.update(cx, |panel, cx| {
            panel.field().update(cx, |field, cx| field.set_text("widget", cx));
            panel.search(cx);
        });
        let files = |cx: &mut VisualTestContext| {
            let mut files: Vec<String> =
                panel.read_with(cx, |panel, _| panel.results().into_iter().map(|(file, _)| file).collect());
            files.sort_unstable();
            files
        };
        settle(cx, |cx| files(cx) == ["src/alpha.rs"]);
        // A file the search went through, changed on disk since.
        std::fs::write(f.root.join("src/beta.rs"), "fn widget_two() {}\n").expect("the file is written");
        // ⌘R with the Find tool window active.
        cx.simulate_keystrokes("cmd-r");
        settle(cx, |cx| files(cx) == ["src/alpha.rs", "src/beta.rs"]);
        f.close(cx);
    }

    /// Rainbow Brackets switches its Color Scheme setting and the action shows its state: on, the
    /// brackets are drawn by depth in the scheme's three colours, an unmatched one in the error
    /// colour, and a bracket inside a string keeps the string's colour; off, they are drawn as before.
    #[gpui_kit::test]
    async fn rainbow_brackets_colour_the_brackets_by_their_depth(cx: &mut TestAppContext) {
        let text = "fn a() { b(\"(\", c[0]); }\n)\n";
        let (f, cx) = open(cx, &[("src/main.rs", text)], "src/main.rs", None);
        let editor = f.active(cx);
        settle(cx, |cx| editor.read_with(cx, |e, _| !e.highlights().is_empty()));
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get("editor.toggleRainbowBrackets").expect("registered");
                action.is_toggled(window, cx)
            })
        };
        let painted = |cx: &mut VisualTestContext, line: u32, col: u32| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            editor.read_with(cx, |e, _| e.painted_color_at(line, col))
        };
        let color = |cx: &mut VisualTestContext, token: &str| cx.update(|_, cx| theme::color(cx, token));
        let (first, second, third, invalid) = (
            color(cx, "syn-bracket-1"),
            color(cx, "syn-bracket-2"),
            color(cx, "syn-bracket-3"),
            color(cx, "syn-invalid"),
        );
        assert_eq!(toggled(cx), Some(false));
        let string = painted(cx, 0, 12);
        let before: Vec<_> = [(0, 4), (0, 10), (0, 17), (1, 0)].iter().map(|&(l, c)| painted(cx, l, c)).collect();
        assert!(!before.contains(&Some(second)) && !before.contains(&Some(third)), "{before:?}");

        f.run(cx, "editor.toggleRainbowBrackets");
        assert!(cx.update(|_, cx| settings::global(cx).editor.rainbow_brackets));
        assert_eq!(toggled(cx), Some(true));
        settle(cx, |cx| painted(cx, 0, 10) == Some(second));
        for (col, expected) in
            [(4, first), (5, first), (7, first), (10, second), (17, third), (19, third), (20, second)]
        {
            assert_eq!(painted(cx, 0, col), Some(expected), "column {col}");
        }
        assert_eq!(painted(cx, 0, 23), Some(first));
        assert_eq!(painted(cx, 1, 0), Some(invalid), "the unmatched bracket");
        assert_eq!(painted(cx, 0, 12), string, "the bracket in the string");

        f.run(cx, "editor.toggleRainbowBrackets");
        assert!(!cx.update(|_, cx| settings::global(cx).editor.rainbow_brackets));
        settle(cx, |cx| painted(cx, 0, 10) == before[1]);
        f.close(cx);
    }

    /// Distraction Free Mode hides the editor's tab strip and gutter and centres the text at the
    /// text width of Settings › Editor › General; leaving it brings them back.
    #[gpui_kit::test]
    async fn distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text(cx: &mut TestAppContext) {
        let line = "x".repeat(120);
        let (f, cx) = open(cx, &[("notes.txt", line.as_str())], "notes.txt", None);
        let editor = f.active(cx);
        let tab =
            f.workbench.read_with(cx, |w, _| w.tabs().and_then(|tabs| tabs.pane(&tabs.active_pane)?.active.clone()));
        let tab: &'static str = Box::leak(format!("tab:{}", tab.expect("a tab")).into_boxed_str());
        let geometry = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            editor.read_with(cx, |e, cx| {
                let bounds = e.painted_bounds().expect("painted");
                let start = e.position_of(e.offset_of(0, 0)).expect("painted").x;
                let eighty = e.position_of(e.offset_of(0, 80)).expect("painted").x;
                (start - bounds.left(), eighty - start, bounds.size.width, e.fold_column_width(cx))
            })
        };
        let (left, _, _, fold) = geometry(cx);
        assert!(cx.debug_bounds(tab).is_some(), "the tab strip is drawn");
        assert_eq!(fold, px(crate::editor::view::FOLD_GUTTER));
        assert_eq!(left, cx.update(|_, cx| theme::length(cx, "w-gutter")) + fold, "the text starts after the gutter");

        f.run(cx, "editor.toggleZen");
        let (left, eighty, width, fold) = geometry(cx);
        assert!(cx.debug_bounds(tab).is_none(), "the tab strip is hidden");
        assert_eq!(fold, px(0.0), "no fold chevrons");
        let column = eighty + px(22.0);
        assert!(((width - column) / 2.0 - left).abs() < px(1.0), "centred: {left:?} of {width:?} for {column:?}");
        // Left of the text is no gutter: a press there sets no breakpoint.
        let presses = count_gutter_presses(&editor, cx);
        let top = editor.read_with(cx, |e, _| e.painted_bounds().expect("painted").top());
        let x = editor.read_with(cx, |e, _| e.painted_bounds().expect("painted").left()) + px(8.0);
        cx.simulate_click(point(x, top + px(4.0)), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(presses.get(), 0);

        f.run(cx, "editor.toggleZen");
        let (left, _, _, _) = geometry(cx);
        assert!(cx.debug_bounds(tab).is_some(), "the tab strip is back");
        assert_eq!(left, cx.update(|_, cx| theme::length(cx, "w-gutter")) + px(crate::editor::view::FOLD_GUTTER));
        f.close(cx);
    }

    /// Paste from History's Enter and Delete keys answer while its dialog has the keyboard: the
    /// window says the list is focused, and no longer once it is closed.
    #[gpui_kit::test]
    async fn the_paste_history_keys_are_enabled_while_its_dialog_has_the_keyboard(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "alpha beta\n")], "notes.txt", None);
        let editor = f.active(cx);
        editor.update(cx, |e, cx| e.select(0, 5, cx));
        f.run(cx, "edit.copy");
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window_context(window, cx).is_truthy("pasteHistoryListFocus"))
        };
        assert!(!focused(cx));
        f.run(cx, "code.pasteFromHistory");
        assert!(f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_some()));
        assert!(focused(cx));
        cx.simulate_keystrokes("escape");
        assert!(!focused(cx));
        f.close(cx);
    }

    /// Reload All from Disk reads the project tree again: a file written outside the IDE shows. The
    /// file watcher shows it as well, so the test cannot tell the reload from the watcher.
    #[gpui_kit::test]
    async fn reload_all_from_disk_reads_the_project_tree_again(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("a.txt", "one\n")], "a.txt", None);
        let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        let labels = |cx: &mut VisualTestContext| {
            tree.read_with(cx, |tree, _| tree.rows().iter().map(|row| row.node.label.to_string()).collect::<Vec<_>>())
        };
        settle(cx, |cx| labels(cx).contains(&"a.txt".to_owned()));
        std::fs::write(f.root.join("b.txt"), "two\n").expect("the file is written");
        f.run(cx, "file.reloadAllFromDisk");
        settle(cx, |cx| labels(cx).contains(&"b.txt".to_owned()));
        f.close(cx);
    }

    /// Go to Test with two tests of the file offers both; Create New Test… needs the New File
    /// dialog of the file templates, so without it the row is not offered and a file without tests
    /// opens no chooser.
    #[gpui_kit::test]
    async fn go_to_test_offers_create_new_test_only_with_the_file_templates(cx: &mut TestAppContext) {
        let files = [
            ("src/calc.py", "def add(a, b):\n    return a + b\n"),
            ("src/alone.py", "def one():\n    return 1\n"),
            ("tests/test_calc.py", "def test_add():\n    pass\n"),
            ("tests/calc_test.py", "def test_sum():\n    pass\n"),
        ];
        let (f, cx) = open(cx, &files, "src/calc.py", None);
        let indexed = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let host = cx.global::<Backend>().host.clone();
                ["test_calc.py", "calc_test.py", "alone.py"].into_iter().all(|name| {
                    let found = ide_host::ops::nav::find_files(&host, "w1", name.to_owned(), 50);
                    found.is_ok_and(|found| !found.is_empty())
                })
            })
        };
        settle(cx, indexed);
        assert!(!cx.update(|_, cx| cx.global::<ActionRegistry>().contains("templates.newFromTemplate")));
        f.run(cx, "navigate.gotoTest");
        let list = f.popup_list(cx);
        let lines = list.read_with(cx, |list, _| list.lines());
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(!lines.iter().any(|line| line.starts_with("Create New Test…")), "{lines:?}");
        cx.simulate_keystrokes("escape");
        settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()));

        let alone = f.root.join("src/alone.py");
        f.workbench.update_in(cx, |w, window, cx| w.open_project_file(alone.clone(), window, cx));
        settle(cx, |cx| f.at(cx).is_some_and(|(path, _)| path == alone));
        f.run(cx, "navigate.gotoTest");
        idle(cx);
        assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()), "no chooser");
        assert_eq!(f.at(cx).map(|(path, _)| path), Some(alone));
        f.close(cx);
    }

    /// Next Occurrence steps through the problems of the Problems window and the status bar says
    /// which of how many for a while; ⌥⌘↑ in the editor steps back.
    #[gpui_kit::test]
    async fn next_occurrence_says_which_of_how_many_in_the_status_bar(cx: &mut TestAppContext) {
        use super::super::tests::{publish, published};
        use ide_core::Severity;
        let (f, cx) = open(cx, &[("a.txt", "one\ntwo\nthree\n")], "a.txt", None);
        let a = f.root.join("a.txt");
        f.run(cx, "toolwindow.problems");
        assert!(f.workbench.read_with(cx, |w, _| w.problems_view()).is_some(), "the Problems window is shown");
        let problems = vec![
            published(0, 0, Severity::Error, "first"),
            published(1, 1, Severity::Error, "second"),
            published(2, 2, Severity::Error, "third"),
        ];
        publish(&f.workbench, cx, &a, problems);
        let bar = f.workbench.read_with(cx, |w, _| w.status_bar().clone());
        let says = |cx: &mut VisualTestContext, text: &str| {
            cx.update(|window, cx| bar.read(cx).texts(window, cx)).iter().any(|shown| shown == text)
        };
        f.run(cx, "navigate.nextOccurrence");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (0, 0))));
        assert!(says(cx, "1 of 3"));
        f.run(cx, "navigate.nextOccurrence");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (1, 1))));
        assert!(says(cx, "2 of 3") && !says(cx, "1 of 3"));
        f.focus_editor(cx);
        cx.simulate_keystrokes("cmd-alt-up");
        settle(cx, |cx| f.at(cx) == Some((a.clone(), (0, 0))));
        assert!(says(cx, "1 of 3"));
        // The message goes after eight seconds.
        cx.executor().advance_clock(Duration::from_secs(9));
        cx.run_until_parked();
        assert!(!says(cx, "1 of 3"));
        f.close(cx);
    }

    /// Next and Previous Occurrence need a project: in a window editing a loose file their keys
    /// move nothing.
    #[gpui_kit::test]
    async fn the_occurrence_keys_do_nothing_without_a_project(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let file = folder.path().canonicalize().expect("the folder exists").join("notes.txt");
        std::fs::write(&file, "// TODO one\n// TODO two\n").expect("the file is written");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            workbench::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            workbench::menu::init(cx);
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |w, window, cx| w.open_file(file.clone(), window, cx));
        let editor = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.editor().cloned());
        settle(cx, |cx| {
            let editor = editor(cx);
            cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
        });
        let editor = editor(cx).expect("an editor");
        editor.update(cx, |e, cx| e.select(15, 15, cx));
        let caret = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.caret_position());
        for (id, key) in [("navigate.nextOccurrence", "cmd-alt-down"), ("navigate.prevOccurrence", "cmd-alt-up")] {
            assert!(cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).is_err(), "{id} runs");
            cx.simulate_keystrokes(key);
            idle(cx);
            assert_eq!(caret(cx), (1, 3), "{key} moved the caret");
        }
    }

    /// In Paste from History, Enter pastes the chosen entry; Backspace edits the filter while it
    /// has text and deletes the chosen entry once it is empty; Delete deletes it.
    #[gpui_kit::test]
    async fn the_paste_history_keys_edit_the_filter_or_act_on_the_chosen_entry(cx: &mut TestAppContext) {
        use crate::editor::view::EditorClipboard;
        let (f, cx) = open(cx, &[("notes.txt", "alpha beta gamma\n")], "notes.txt", None);
        let editor = f.active(cx);
        for (from, to) in [(0, 5), (6, 10), (11, 16)] {
            editor.update(cx, |e, cx| e.select(from, to, cx));
            f.run(cx, "edit.copy");
        }
        let history = |cx: &mut VisualTestContext| -> Vec<String> {
            cx.update(|_, cx| {
                cx.default_global::<EditorClipboard>().0.entries().iter().map(|entry| entry.text.clone()).collect()
            })
        };
        assert_eq!(history(cx), ["gamma", "beta", "alpha"]);
        editor.update(cx, |e, cx| e.select(17, 17, cx));
        f.run(cx, "code.pasteFromHistory");
        let dialog = f.workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open");
        let filter = |cx: &mut VisualTestContext| dialog.read_with(cx, |d, cx| d.field().read(cx).text().to_owned());
        cx.simulate_input("be");
        cx.simulate_keystrokes("backspace");
        assert_eq!(filter(cx), "b");
        assert_eq!(history(cx), ["gamma", "beta", "alpha"], "Backspace edited the filter");
        cx.simulate_keystrokes("backspace");
        assert_eq!(filter(cx), "");
        cx.simulate_keystrokes("backspace");
        assert_eq!(history(cx), ["beta", "alpha"], "Backspace in the empty filter deletes the chosen entry");
        cx.simulate_keystrokes("delete");
        assert_eq!(history(cx), ["alpha"]);
        cx.simulate_keystrokes("enter");
        assert!(f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
        assert_eq!(f.text(cx), "alpha beta gamma\nalpha");
        f.close(cx);
    }

    /// Without an editor the text actions of this module do not run.
    #[gpui_kit::test]
    async fn the_editor_actions_need_an_editor(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("src/main.rs", "fn a() {}\n")], "src/main.rs", None);
        f.run(cx, "editor.closeTab");
        settle(cx, |cx| f.editor(cx).is_none());
        let ids = [
            "code.expandToLevel1",
            "code.expandAllToLevel5",
            "editor.contextInfo",
            "navigate.customRegion",
            "code.unwrap",
            "code.expressionTypeInfo",
            "code.reformatDialog",
        ];
        for id in ids {
            assert!(f.unavailable(cx, id), "{id} runs without an editor");
        }
        f.close(cx);
    }
}
