//! Live templates and postfix templates in the window's editors: Tab expands the abbreviation or
//! the `expr.key` before the caret, the template's fields are then filled in with Tab, Shift+Tab,
//! Enter and Escape, Insert Live Template lists the templates in the completion lookup, and Surround
//! With wraps the selection, the statement at the caret or its line.
//!
//! An editor's keys are claimed on its focus before the keymap and the editor see them; what a key
//! does is decided on the models ([`crate::templates::expand`]). The templates and the options in
//! effect are the ones the UI storage holds under the Settings pages' keys when the key is pressed.

use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::editor::buffer;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::marks::{Mark, MarkKind, MarkLayer};
use crate::editor::view::{EditOrigin, Editor, EditorEvent};
use crate::editor_lsp::completion::ContributedApply;
use crate::editor_lsp::completion_items::LookupEntry;
use crate::editor_lsp::completion_view::{ContributedItem, LookupItem};
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::input;
use crate::platform::storage::UiStorage;
use crate::templates::chooser_view::{ChooserEvent, TemplateChooser};
use crate::templates::editor::{
    FieldKey, PendingChoice, Picked, TemplateAction, TemplateExpansion, apply_live_completion, multi_line_statement,
    surround_with as surround_choice,
};
use crate::templates::editor_context::CommentTokens;
use crate::templates::expand::{
    EditorSnapshot, EscapePress, FieldStep, LookupRow, TabPress, TemplateEdit, TemplateSession, TemplateStores,
    escape_press, insert_live_template as lookup_rows, literal_ancestors, tab_press,
};
use crate::templates::popup_store::ChooserRequest;
use crate::ui::popup_anchor::{CHOOSER_GAP, below, caret_bottom_left};
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, EntityId, Focusable, Keystroke, Pixels, Point, Size, WeakEntity,
    Window, point, px,
};
use ide_core::{IdeResult, Pos};
use ide_syntax::StructuralRanges;
use std::collections::HashMap;
use std::ops::Range;

/// The templates of the window's editors: the stores as last read, the template each editor is
/// filling in, and the chooser on screen.
#[derive(Default)]
pub(super) struct EditorTemplates {
    stores: TemplateStores,
    sessions: HashMap<EntityId, TemplateSession>,
    chooser: Option<OpenChooser>,
}

/// The chooser on screen: which editor it acts on and what its pick continues with.
struct OpenChooser {
    view: EntityId,
    editor: WeakEntity<Editor>,
    pending: PendingChoice,
}

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry
        .handle("code.insertLiveTemplate", |_, window, cx| on_workbench(window, cx, Workbench::insert_live_template));
    registry.handle("code.surroundWith", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.surround_with(false, window, cx))
    });
    registry.handle("code.surroundWithLiveTemplate", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.surround_with(true, window, cx))
    });
}

/// Whether the completion lookup of `editor` shows rows: it takes Tab and Enter then.
fn lookup_open(editor: &Entity<Editor>, cx: &App) -> bool {
    EditorIntelligence::of(editor, cx).is_some_and(|intelligence| intelligence.read(cx).lookup().read(cx).is_open())
}

/// Opens completion in `editor` once the edit that moved the caret has been reported, so that the
/// report does not close it again.
fn complete_in(editor: &Entity<Editor>, cx: &mut App) {
    if let Some(intelligence) = EditorIntelligence::of(editor, cx) {
        cx.defer(move |cx| intelligence.update(cx, |intelligence, cx| intelligence.complete(true, cx)));
    }
}

/// Where a template chooser opens: just under the caret's line; while the caret is not painted, near
/// the editor's top left corner.
fn chooser_place(editor: Option<&Editor>, _: Size<Pixels>, cx: &App) -> Point<Pixels> {
    let painted = |editor: &Editor| {
        caret_bottom_left(editor, cx)
            .or_else(|| editor.painted_bounds().map(|bounds| point(bounds.left() + px(40.0), bounds.top() + px(40.0))))
    };
    below(editor.and_then(painted).unwrap_or_else(|| point(px(40.0), px(40.0))), CHOOSER_GAP)
}

/// A row of Insert Live Template's lookup, which the templates expand when it is accepted.
fn lookup_item(row: LookupRow) -> LookupItem {
    let entry = LookupEntry {
        label: row.abbrev,
        display_label: None,
        detail: None,
        kind: "snippet",
        sort_text: None,
        boost: Some(row.boost),
        commit_characters: Vec::new(),
    };
    LookupItem::Contributed(Box::new(ContributedItem {
        entry,
        icon: None,
        type_text: Some(row.description).filter(|description| !description.is_empty()),
        apply: ContributedApply::Deferred(row.id),
        commit_characters: Vec::new(),
    }))
}

/// The selection ranges of `editor`, from their start to their end.
fn selection_ranges(editor: &Editor) -> Vec<Range<u32>> {
    editor.selections().iter().map(|&(anchor, head)| anchor.min(head)..anchor.max(head)).collect()
}

impl Workbench {
    /// Claims the template keys of `editor` and follows its edits for the template it fills in.
    pub(super) fn install_editor_templates(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (workbench, target) = (cx.weak_entity(), editor.downgrade());
        let claim = input::claim_keys(
            editor.focus_handle(cx),
            move |keystroke, window, cx| {
                let Some(editor) = target.upgrade() else { return false };
                workbench
                    .update(cx, |workbench, cx| workbench.template_key(&editor, keystroke, window, cx))
                    .unwrap_or(false)
            },
            cx,
        );
        let id = editor.entity_id();
        cx.observe_release(editor, move |workbench, _, cx| {
            workbench.editor_templates.sessions.remove(&id);
            input::release_keys(claim, cx);
        })
        .detach();
        cx.subscribe_in(editor, window, |workbench, editor, event: &EditorEvent, _, cx| {
            workbench.template_editor_event(editor, event, cx);
        })
        .detach();
    }

    /// The template `editor` is filling in, if any.
    pub fn template_session(&self, editor: &Entity<Editor>) -> Option<&TemplateSession> {
        self.editor_templates.sessions.get(&editor.entity_id())
    }

    /// The template chooser while it is open.
    pub fn template_chooser(&self) -> Option<Entity<TemplateChooser>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// The popup `view` closed: a chooser among them takes nothing more.
    pub(super) fn template_chooser_closed(&mut self, view: &AnyView) {
        if self.editor_templates.chooser.as_ref().is_some_and(|open| open.view == view.entity_id()) {
            self.editor_templates.chooser = None;
        }
    }

    fn sync_template_stores(&mut self, cx: &App) {
        let Some(storage) = cx.try_global::<UiStorage>() else { return };
        self.editor_templates.stores.sync(|key| storage.get(key));
    }

    /// `editor` as the template commands read it now.
    fn template_snapshot(&self, editor: &Entity<Editor>, cx: &App) -> EditorSnapshot {
        let read = editor.read(cx);
        let doc = read.rope().clone();
        let comments = &read.facts().language.comments;
        EditorSnapshot {
            ancestors: literal_ancestors(read.highlights(), read.caret(), buffer::len_utf16(&doc)),
            selections: read.selections().to_vec(),
            main: read.main_index(),
            path: read.path().to_string_lossy().into_owned(),
            language: read.language().as_str().to_owned(),
            comment_tokens: CommentTokens { line: comments.line.clone(), block: comments.block.clone() },
            project_root: self.project_root(cx).map(|root| root.to_string_lossy().into_owned()).unwrap_or_default(),
            indent_unit: read.facts().indent_unit.clone(),
            doc,
        }
    }

    /// A key pressed in `editor`: whether the templates took it.
    fn template_key(
        &mut self,
        editor: &Entity<Editor>,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        match keystroke.key.as_str() {
            "tab" => self.template_tab(editor, modifiers.shift, window, cx),
            "enter" if !modifiers.shift => self.template_enter(editor, cx),
            "escape" if !modifiers.shift => self.template_escape(editor, cx),
            _ => false,
        }
    }

    fn template_tab(
        &mut self,
        editor: &Entity<Editor>,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_template_stores(cx);
        let snapshot = self.template_snapshot(editor, cx);
        let (read_only, lookup_open) = (editor.read(cx).is_read_only(), lookup_open(editor, cx));
        let session = self.editor_templates.sessions.get(&editor.entity_id());
        match tab_press(&snapshot, session, shift, read_only, lookup_open, &self.editor_templates.stores) {
            TabPress::Field(step) => self.step_template_field(editor, step, cx),
            TabPress::Template(action) => self.template_action(editor, *action, window, cx),
            TabPress::Editor => return false,
        }
        true
    }

    fn template_enter(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) -> bool {
        let lookup_open = lookup_open(editor, cx);
        let step = self.template_session(editor).and_then(|session| session.step(FieldKey::Enter, lookup_open));
        match step {
            Some(step) => {
                self.step_template_field(editor, step, cx);
                true
            }
            None => false,
        }
    }

    fn template_escape(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) -> bool {
        let (selections, main) = {
            let read = editor.read(cx);
            (read.selections().to_vec(), read.main_index())
        };
        match escape_press(&selections, self.template_session(editor), lookup_open(editor, cx)) {
            EscapePress::DropSelection(carets) => {
                editor.update(cx, |editor, cx| editor.set_selections(carets, main, cx))
            }
            EscapePress::EndSession => {
                self.editor_templates.sessions.remove(&editor.entity_id());
                self.show_template_fields(editor, cx);
            }
            EscapePress::Editor => return false,
        }
        true
    }

    /// Puts the caret in another field of the template `editor` fills in.
    fn step_template_field(&mut self, editor: &Entity<Editor>, step: FieldStep, cx: &mut Context<Self>) {
        let FieldStep { selections, session, complete } = step;
        self.keep_template_session(editor, session);
        editor.update(cx, |editor, cx| editor.set_selections(selections, 0, cx));
        self.show_template_fields(editor, cx);
        if complete {
            complete_in(editor, cx);
        }
    }

    fn keep_template_session(&mut self, editor: &Entity<Editor>, session: Option<TemplateSession>) {
        let id = editor.entity_id();
        match session {
            Some(session) => {
                self.editor_templates.sessions.insert(id, session);
            }
            None => {
                self.editor_templates.sessions.remove(&id);
            }
        }
    }

    /// Draws the fields of the template `editor` fills in; none once it is done.
    fn show_template_fields(&self, editor: &Entity<Editor>, cx: &mut App) {
        let marks = self
            .template_session(editor)
            .map(|session| session.field_ranges().into_iter().map(|range| Mark::new(range, MarkKind::SnippetField)))
            .into_iter()
            .flatten()
            .collect();
        editor.update(cx, |editor, cx| editor.set_marks(MarkLayer::Snippet, marks, cx));
    }

    /// The fields follow what the editor reports, and the template ends when the caret leaves its field.
    fn template_editor_event(&mut self, editor: &Entity<Editor>, event: &EditorEvent, cx: &mut Context<Self>) {
        let Some(session) = self.template_session(editor) else { return };
        let selection = selection_ranges(editor.read(cx));
        let next = match event {
            EditorEvent::Edited { changes, .. } => session.edited(changes, &selection),
            EditorEvent::SelectionMoved => session.selection_moved(&selection),
            EditorEvent::Hovered { .. } | EditorEvent::GutterPressed { .. } => return,
        };
        self.keep_template_session(editor, next);
        self.show_template_fields(editor, cx);
    }

    fn template_action(
        &mut self,
        editor: &Entity<Editor>,
        action: TemplateAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            TemplateAction::Expand(expansion) => self.expand_template(editor, &expansion, cx),
            TemplateAction::Choose { chooser, pending } => {
                self.show_template_chooser(editor, chooser, pending, window, cx)
            }
        }
    }

    /// Puts `expansion` into `editor` as one edit and one undo step, with every occurrence of its
    /// first field selected; a template with several fields is then filled in field by field.
    fn expand_template(&mut self, editor: &Entity<Editor>, expansion: &TemplateExpansion, cx: &mut Context<Self>) {
        let snapshot = self.template_snapshot(editor, cx);
        // `clipboard()` is the one template function that reads anything outside the editor.
        let clipboard =
            if expansion.needs_clipboard() { cx.read_from_clipboard().and_then(|item| item.text()) } else { None };
        let edit = TemplateEdit::new(expansion.apply(&snapshot.template_editor(), clipboard.as_deref()));
        let TemplateEdit { changes, selections, session, complete_first } = edit;
        let applied =
            editor.update(cx, |editor, cx| editor.apply_edit_from(EditOrigin::Other, &changes, selections, 0, cx));
        if !applied {
            return;
        }
        self.keep_template_session(editor, session);
        self.show_template_fields(editor, cx);
        if complete_first {
            complete_in(editor, cx);
        }
    }

    fn show_template_chooser(
        &mut self,
        editor: &Entity<Editor>,
        request: ChooserRequest,
        pending: PendingChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| TemplateChooser::new(request, cx));
        let events = cx.subscribe_in(&view, window, |workbench, view, event: &ChooserEvent, window, cx| {
            workbench.template_chooser_event(view, event, window, cx);
        });
        let at = self.editor_popup_at(chooser_place, window, cx);
        self.open_popup_at(&view, events, at, window, cx);
        self.editor_templates.chooser =
            Some(OpenChooser { view: view.entity_id(), editor: editor.downgrade(), pending });
    }

    /// A pick closes the chooser, the keyboard goes back to the editor, and the pick is carried out
    /// on the document as it is now.
    fn template_chooser_event(
        &mut self,
        view: &Entity<TemplateChooser>,
        event: &ChooserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.editor_templates.chooser.take().filter(|open| open.view == view.entity_id());
        self.close_popup(window, cx);
        let (Some(open), ChooserEvent::Picked { key, .. }) = (open, event) else { return };
        let Some(editor) = open.editor.upgrade() else { return };
        let (doc, head) = {
            let read = editor.read(cx);
            (read.rope().clone(), read.caret())
        };
        match open.pending.pick(key, &doc, head) {
            Picked::Expand(expansion) => self.expand_template(&editor, &expansion, cx),
            Picked::OpenSettings(page) => window.defer(cx, move |window, cx| {
                execute_action("file.settings", Some(&serde_json::Value::from(page)), window, cx);
            }),
            Picked::Nothing => {}
        }
    }

    /// Insert Live Template: the completion lookup lists the live templates that apply at the caret,
    /// filtered by the word before it; accepting one expands it in place of that word.
    fn insert_live_template(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let Some(intelligence) = EditorIntelligence::of(&editor, cx) else { return };
        self.sync_template_stores(cx);
        let snapshot = self.template_snapshot(&editor, cx);
        let rows = lookup_rows(&snapshot, &self.editor_templates.stores);
        let items: Vec<LookupItem> = rows.rows.into_iter().map(lookup_item).collect();
        intelligence.update(cx, |intelligence, cx| intelligence.show_exclusive(rows.from, items, &rows.typed, cx));
    }

    /// A row of Insert Live Template's lookup was accepted in the active editor: the template `id`
    /// goes in place of the text from `from` to the caret.
    pub(super) fn accept_lookup_template(&mut self, from: u32, id: &str, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        self.sync_template_stores(cx);
        let Some(template) = self.editor_templates.stores.template(id).cloned() else { return };
        let caret = editor.read(cx).caret();
        self.expand_template(&editor, &apply_live_completion(&template, from.min(caret), caret), cx);
    }

    /// Surround With (`templates_only`: Surround with Live Template) in the active editor. A single
    /// caret first asks the backend for the statement around it, which is wrapped when it spans
    /// several whole lines; a selection or several carets open the chooser at once.
    fn surround_with(&mut self, templates_only: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let read = editor.read(cx);
        let caret_only = read.selections().len() == 1 && read.selection().is_empty();
        if !caret_only {
            self.open_surround(&editor, None, templates_only, window, cx);
            return;
        }
        let (label, doc, language, version) =
            (read.window_label().to_owned(), read.doc(), read.language().clone(), read.version());
        let line = buffer::line_col(read.rope(), read.caret()).0;
        let work = cx.global::<Backend>().with_host(move |host| async move {
            let support = ide_host::ops::syntax::syntax_support(&host, &label, language)?;
            if !support.tree {
                return Ok(None);
            }
            let pos = Pos::new(line, 0);
            ide_host::ops::syntax::syntax_structural_ranges(&host, &label, doc, pos, None).await.map(Some)
        });
        let target = editor.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let answer = work.await;
            let surrounded = this.update_in(cx, |workbench, window, cx| {
                workbench.surround_answered(&target, version, answer, templates_only, window, cx);
            });
            if let Err(error) = surrounded {
                tracing::debug!("Surround With ended with its window: {error:#}");
            }
        })
        .detach();
    }

    /// The statement at the caret came back: it is the target while the document is unchanged and its
    /// editor still the active one; on an error the caret's line is.
    fn surround_answered(
        &mut self,
        editor: &WeakEntity<Editor>,
        version: u32,
        answer: anyhow::Result<IdeResult<Option<StructuralRanges>>>,
        templates_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = editor.upgrade().filter(|editor| self.editor() == Some(editor)) else { return };
        let ranges = match answer {
            Ok(Ok(ranges)) => ranges,
            Ok(Err(error)) => {
                tracing::debug!("no statement to surround: {}", error.message);
                None
            }
            Err(error) => {
                tracing::warn!("asking for the statement to surround did not run: {error:#}");
                None
            }
        };
        let read = editor.read(cx);
        let target = if read.version() == version { multi_line_statement(read.rope(), ranges.as_ref()) } else { None };
        self.open_surround(&editor, target, templates_only, window, cx);
    }

    fn open_surround(
        &mut self,
        editor: &Entity<Editor>,
        target: Option<Range<u32>>,
        templates_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_template_stores(cx);
        let snapshot = self.template_snapshot(editor, cx);
        let templates = self.editor_templates.stores.templates();
        let action = surround_choice(&snapshot.template_editor(), templates, target, templates_only);
        self.template_action(editor, action, window, cx);
    }
}
