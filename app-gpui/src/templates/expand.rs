//! What a key press of an editor does with live and postfix templates, on the models alone: the
//! stores read from the UI storage, the editor as the template commands see it, the template being
//! filled in and what Tab, Shift+Tab, Enter and Escape do to it.
//!
//! A key press costs a scan of the caret's line (the word before the caret, the postfix key), one
//! pass over the templates (about 120 built-in live templates, about 100 postfix templates) and a
//! binary search in the editor's highlights for the comment or string at the caret. Nothing walks
//! the document; a stored value is parsed again only when its text changed.

use super::editor::{
    AppliedTemplate, CompletionApply, EscapeAction, FieldKey, TabOutcome, TemplateAction, TemplateEditor,
    TemplateLookup, escape_template, field_key, tab, template_completions,
};
use super::editor_context::{CommentTokens, TreeNode};
use super::live_model::LiveTemplate;
use super::live_store::{LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY, LiveTemplateOptions, LiveTemplates};
use super::postfix_store::{POSTFIX_KEY, PostfixSettings};
use super::snippet::ActiveSnippet;
use crate::editor::view::HighlightSpan;
use crate::find::utf16;
use ide_core::TextChange;
use ide_language::SyntaxHighlightKind;
use ropey::Rope;
use std::ops::Range;

/// The live templates, their options and the postfix settings in effect, as the UI storage holds
/// them under their keys. What the Settings pages write there is what the next key press uses.
#[derive(Debug, Clone, Default)]
pub struct TemplateStores {
    live_raw: Option<String>,
    options_raw: Option<String>,
    postfix_raw: Option<String>,
    live: LiveTemplates,
    options: LiveTemplateOptions,
    postfix: PostfixSettings,
}

impl TemplateStores {
    /// Brings the stores up to what `stored` gives for their keys; a key whose text is unchanged
    /// is not parsed again.
    pub fn sync<'s>(&mut self, stored: impl Fn(&str) -> Option<&'s str>) {
        let live = stored(LIVE_TEMPLATES_KEY);
        if live != self.live_raw.as_deref() {
            self.live.reload(live);
            self.live_raw = live.map(str::to_owned);
        }
        let options = stored(LIVE_TEMPLATE_OPTIONS_KEY);
        if options != self.options_raw.as_deref() {
            self.options = LiveTemplateOptions::parse(options);
            self.options_raw = options.map(str::to_owned);
        }
        let postfix = stored(POSTFIX_KEY);
        if postfix != self.postfix_raw.as_deref() {
            self.postfix = PostfixSettings::parse(postfix);
            self.postfix_raw = postfix.map(str::to_owned);
        }
    }

    pub fn templates(&self) -> &[LiveTemplate] {
        self.live.templates()
    }

    pub fn template(&self, id: &str) -> Option<&LiveTemplate> {
        self.templates().iter().find(|template| template.id == id)
    }

    pub fn options(&self) -> LiveTemplateOptions {
        self.options
    }

    pub fn postfix(&self) -> &PostfixSettings {
        &self.postfix
    }
}

/// An editor as the template commands read it when a key is pressed. The text is a rope, so taking
/// it copies nothing.
#[derive(Debug, Clone)]
pub struct EditorSnapshot {
    pub doc: Rope,
    /// The selection ranges, `(anchor, head)` each.
    pub selections: Vec<(u32, u32)>,
    pub main: usize,
    pub path: String,
    pub language: String,
    /// The comment or string around the caret and the document, innermost first.
    pub ancestors: Vec<TreeNode>,
    pub comment_tokens: CommentTokens,
    pub project_root: String,
    pub indent_unit: String,
}

impl EditorSnapshot {
    /// The main selection as `(anchor, head)`.
    pub fn main_selection(&self) -> (u32, u32) {
        self.selections.get(self.main).copied().unwrap_or_default()
    }

    /// The editor as [`super::editor`]'s commands take it.
    pub fn template_editor(&self) -> TemplateEditor<'_> {
        let (anchor, head) = self.main_selection();
        TemplateEditor {
            doc: &self.doc,
            selection: anchor.min(head)..anchor.max(head),
            head,
            cursors: self.selections.len(),
            path: &self.path,
            language: &self.language,
            ancestors: &self.ancestors,
            // No syntax tree is at hand without the backend: the postfix target is the expression the
            // text scan finds, without the wider ones a tree would add.
            dot_ancestors: &[],
            comment_tokens: &self.comment_tokens,
            contexts: &[],
            project_root: &self.project_root,
            indent_unit: &self.indent_unit,
        }
    }
}

/// Which literal a highlight is part of, as the template contexts name them.
fn literal_name(kind: SyntaxHighlightKind) -> Option<&'static str> {
    use SyntaxHighlightKind as K;
    match kind {
        K::Comment | K::DocComment => Some("Comment"),
        K::String | K::StringSpecial | K::Escape => Some("String"),
        _ => None,
    }
}

/// The syntax nodes around `pos` that tell a comment or a string from code, innermost first: the
/// comment or string literal touching `pos` when there is one, then the document of `len` units.
/// `spans` are the editor's highlights in document order; the literal is the run of touching spans
/// of its kind, found from `pos` outwards.
pub fn literal_ancestors(spans: &[HighlightSpan], pos: u32, len: u32) -> Vec<TreeNode> {
    let document = TreeNode::new("Document", 0, len);
    let first = spans.partition_point(|span| span.to < pos);
    let hit = spans[first..]
        .iter()
        .take_while(|span| span.from <= pos)
        .position(|span| literal_name(span.kind).is_some())
        .map(|at| first + at);
    let Some(hit) = hit else { return vec![document] };
    let Some(name) = literal_name(spans[hit].kind) else { return vec![document] };
    let (mut from, mut to) = (spans[hit].from, spans[hit].to);
    for span in spans[..hit].iter().rev() {
        if literal_name(span.kind) != Some(name) || span.to < from {
            break;
        }
        from = from.min(span.from);
    }
    for span in &spans[hit + 1..] {
        if literal_name(span.kind) != Some(name) || span.from > to {
            break;
        }
        to = to.max(span.to);
    }
    vec![TreeNode::new(name, from, to), document]
}

/// The template being filled in: its fields, which of them ask for completion, and the edits it
/// made itself whose report from the editor is still to come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSession {
    fields: ActiveSnippet,
    /// Per field, in tab order: moving to it opens completion.
    completing: Vec<bool>,
    /// The expansion's own edit is reported by the editor after the session started; the fields
    /// are already where that edit put them, so its report must not move them again.
    own_edits: usize,
}

impl TemplateSession {
    pub fn fields(&self) -> &ActiveSnippet {
        &self.fields
    }

    /// The field the caret is in, from 0 in tab order.
    pub fn active_field(&self) -> usize {
        self.fields.active
    }

    /// Every occurrence of every field, to draw them.
    pub fn field_ranges(&self) -> Vec<Range<u32>> {
        self.fields.ranges.iter().map(|range| range.from..range.to).collect()
    }

    /// The editor changed the text by `changes` and the selection is now `selection`: the fields
    /// follow, or the session ends (a deletion crossed a field's edge, the caret left the field).
    pub fn edited(&self, changes: &[TextChange], selection: &[Range<u32>]) -> Option<Self> {
        if self.own_edits > 0 {
            return Some(Self { own_edits: self.own_edits - 1, ..self.clone() });
        }
        let fields = self.fields.after_update(changes, Some(selection))?;
        Some(Self { fields, ..self.clone() })
    }

    /// The selection moved without an edit: the session goes on while it stays in the active field.
    pub fn selection_moved(&self, selection: &[Range<u32>]) -> Option<Self> {
        let fields = self.fields.after_update(&[], Some(selection))?;
        Some(Self { fields, ..self.clone() })
    }

    /// Where `key` takes the caret: the next field (Tab, Enter) or the previous one (Shift+Tab).
    /// `None` leaves the key to the editor: the lookup takes Tab and Enter while it is open, and
    /// Shift+Tab has nowhere to go from the first field.
    pub fn step(&self, key: FieldKey, lookup_open: bool) -> Option<FieldStep> {
        let moved = field_key(&self.fields, key, lookup_open)?;
        let target = match key {
            FieldKey::ShiftTab => self.fields.active.saturating_sub(1),
            FieldKey::Tab | FieldKey::Enter => self.fields.active + 1,
        };
        Some(FieldStep {
            selections: moved.selection.iter().map(|range| (range.start, range.end)).collect(),
            session: moved.active.map(|fields| Self { fields, completing: self.completing.clone(), own_edits: 0 }),
            complete: self.completing.get(target).copied().unwrap_or(false),
        })
    }
}

/// A move to another field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldStep {
    /// The new selection, `(anchor, head)` each: every occurrence of the field.
    pub selections: Vec<(u32, u32)>,
    /// The session after the move; `None` once the last field is reached.
    pub session: Option<TemplateSession>,
    /// The field asks for completion.
    pub complete: bool,
}

/// An expansion as one edit of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateEdit {
    pub changes: Vec<TextChange>,
    /// The selection afterwards, in the new text: every occurrence of the first field, or the caret
    /// after the inserted text when the template has no field.
    pub selections: Vec<(u32, u32)>,
    /// The session to keep once the edit is applied; `None` for a template with one field.
    pub session: Option<TemplateSession>,
    /// Completion opens at once: the first field asks for it.
    pub complete_first: bool,
}

impl TemplateEdit {
    pub fn new(applied: AppliedTemplate) -> Self {
        let AppliedTemplate { insertion, completing, complete_first } = applied;
        let caret = insertion.from + utf16::len(&insertion.insert);
        let selections = if insertion.selection.is_empty() {
            vec![(caret, caret)]
        } else {
            insertion.selection.iter().map(|range| (range.start, range.end)).collect()
        };
        let changes = vec![insertion.change()];
        let session = insertion.active.map(|fields| TemplateSession { fields, completing, own_edits: 1 });
        Self { changes, selections, session, complete_first }
    }
}

/// What Tab or Shift+Tab does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabPress {
    /// The caret goes to another field of the template being filled in.
    Field(FieldStep),
    /// A postfix or live template expands, or a chooser asks which.
    Template(Box<TemplateAction>),
    /// The key is the editor's.
    Editor,
}

/// Tab (`shift` false) or Shift+Tab in an editor: the fields of the template being filled in come
/// first, then postfix templates, then live templates. Emmet's turn and an editor that is
/// read-only or whose lookup is open leave the key to the editor.
pub fn tab_press(
    snapshot: &EditorSnapshot,
    session: Option<&TemplateSession>,
    shift: bool,
    read_only: bool,
    lookup_open: bool,
    stores: &TemplateStores,
) -> TabPress {
    let key = if shift { FieldKey::ShiftTab } else { FieldKey::Tab };
    if let Some(step) = session.and_then(|session| session.step(key, lookup_open)) {
        return TabPress::Field(step);
    }
    if shift {
        return TabPress::Editor;
    }
    let editor = snapshot.template_editor();
    match tab(&editor, read_only, lookup_open, stores.templates(), stores.options(), stores.postfix()) {
        TabOutcome::Template(action) => TabPress::Template(Box::new(action)),
        TabOutcome::TryEmmet | TabOutcome::Ignore => TabPress::Editor,
    }
}

/// What Escape does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapePress {
    /// Put a caret at the head of every selection range; the template goes on.
    DropSelection(Vec<(u32, u32)>),
    /// Leave the template.
    EndSession,
    /// The key is the editor's (or the open lookup's).
    Editor,
}

/// Escape while a template is filled in: first the field's selection goes, then the template.
/// Without a template the key is the editor's.
pub fn escape_press(selections: &[(u32, u32)], session: Option<&TemplateSession>, lookup_open: bool) -> EscapePress {
    if session.is_none() {
        return EscapePress::Editor;
    }
    let ranges: Vec<(Range<u32>, u32)> =
        selections.iter().map(|&(anchor, head)| (anchor.min(head)..anchor.max(head), head)).collect();
    match escape_template(&ranges, lookup_open, true) {
        EscapeAction::DropSelection(heads) => {
            EscapePress::DropSelection(heads.into_iter().map(|head| (head, head)).collect())
        }
        EscapeAction::ClearSnippet => EscapePress::EndSession,
        EscapeAction::CloseLookup | EscapeAction::PassThrough => EscapePress::Editor,
    }
}

/// One row of Insert Live Template's lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupRow {
    pub id: String,
    pub abbrev: String,
    pub description: String,
    /// An exact abbreviation ranks first.
    pub boost: i32,
}

/// Insert Live Template's lookup: the live templates that apply at the caret, for the word before
/// it (none after a dot), which their abbreviations replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateLookupRows {
    /// Where the replaced word starts: the caret when there is none.
    pub from: u32,
    /// The word typed so far, which the lookup filters on.
    pub typed: String,
    pub rows: Vec<LookupRow>,
}

/// The rows Insert Live Template lists in `snapshot`.
pub fn insert_live_template(snapshot: &EditorSnapshot, stores: &TemplateStores) -> TemplateLookupRows {
    let editor = snapshot.template_editor();
    let mut lookup = TemplateLookup::default();
    lookup.insert_live_template(true);
    let result = template_completions(
        &editor,
        editor.head,
        stores.templates(),
        lookup,
        LiveTemplateOptions::default(),
        &PostfixSettings::default(),
    );
    let Some(result) = result else {
        return TemplateLookupRows { from: editor.head, typed: String::new(), rows: Vec::new() };
    };
    let typed = crate::editor::buffer::slice(&snapshot.doc, result.from, editor.head);
    let rows = result
        .options
        .into_iter()
        .filter_map(|option| match option.apply {
            CompletionApply::Live(id) => Some(LookupRow {
                id,
                abbrev: option.label,
                description: option.type_text.unwrap_or_default(),
                boost: option.boost,
            }),
            CompletionApply::Postfix(_) => None,
        })
        .collect();
    TemplateLookupRows { from: result.from, typed, rows }
}

/// Whether text typed into Insert Live Template's lookup keeps its rows: what an abbreviation may
/// hold, but for a dot.
pub fn lookup_keeps_rows(typed: &str) -> bool {
    super::editor::ValidFor::Abbreviation.matches(typed)
}
