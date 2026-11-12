//! The editor side of live templates and postfix completion: Tab expansion, the template lookup,
//! the Surround With chooser, the completion source and the keys that walk a template's fields.
//!
//! Nothing here touches an editor. A command reads a [`TemplateEditor`] snapshot and answers with
//! what to do: a [`TemplateExpansion`] to apply, or a chooser to show whose pick
//! ([`PendingChoice::pick`]) gives the next step.

use super::editor_context::{CommentTokens, TemplateEnvInput, TreeNode, in_comment_or_string, template_functions};
use super::live_model::{
    ExpansionEnv, LiveTemplate, TemplateFilter, TemplateVariable, applicable_templates, called_functions,
    expand_template, uses_selection,
};
use super::live_store::LiveTemplateOptions;
use super::live_surrounders::{SurroundEntryKind, SurroundSelection, Surrounder, surround_entries, surrounders_for};
use super::popup_store::{ChooserItem, ChooserRequest};
use super::postfix_model::{
    PostfixCandidate, PostfixTarget, PostfixTemplate, candidates_for, find_postfix_target, is_compound,
    postfix_templates_for,
};
use super::postfix_store::PostfixSettings;
use super::snippet::{ActiveSnippet, FieldMove, SnippetInsertion, apply_snippet};
use crate::editor::buffer;
use ide_syntax::{StructuralRanges, SyntaxNode};
use ropey::Rope;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// The Settings page "Configure Live Templates…" opens.
pub const LIVE_TEMPLATES_PAGE: &str = "editor.liveTemplates";
/// Key of the chooser row that opens the settings.
pub const CONFIGURE_TEMPLATES_KEY: &str = "\u{0}configure";
const TEMPLATES_SECTION_KEY: &str = "\u{0}templates";
const SEPARATOR_KEY: &str = "\u{0}separator";
/// How much text before a position the name scan looks at (UTF-16 units).
const SCAN_BACK: u32 = 20_000;

/// What the template commands read of an editor.
#[derive(Debug, Clone)]
pub struct TemplateEditor<'a> {
    pub doc: &'a Rope,
    /// The main selection, from its start to its end.
    pub selection: Range<u32>,
    /// The caret: the moving end of the main selection.
    pub head: u32,
    /// How many selection ranges there are.
    pub cursors: usize,
    pub path: &'a str,
    pub language: &'a str,
    /// The syntax nodes around the caret, innermost first.
    pub ancestors: &'a [TreeNode],
    /// The syntax nodes around the `.` of a postfix key before the caret
    /// ([`super::postfix_model::postfix_key_at`]), innermost first.
    pub dot_ancestors: &'a [TreeNode],
    pub comment_tokens: &'a CommentTokens,
    /// Finer template contexts features report at the caret (`sql-statement`…).
    pub contexts: &'a [String],
    /// The project root, which names the user.
    pub project_root: &'a str,
    /// What one indent level is in the document.
    pub indent_unit: &'a str,
}

impl TemplateEditor<'_> {
    fn selection_is_empty(&self) -> bool {
        self.selection.start == self.selection.end
    }

    fn line_start(&self, pos: u32) -> u32 {
        buffer::line_range(self.doc, buffer::line_col(self.doc, pos).0).start
    }

    fn line_end(&self, pos: u32) -> u32 {
        buffer::line_range(self.doc, buffer::line_col(self.doc, pos).0).end
    }

    fn char_before(&self, pos: u32) -> String {
        if pos == 0 { String::new() } else { buffer::slice(self.doc, pos - 1, pos) }
    }
}

/// The user's name, as the home folder in the project root spells it.
pub fn user_name(root: &str) -> &str {
    let unix = ["/Users/", "/home/"].iter().find_map(|home| root.strip_prefix(home)).map(|rest| (rest, '/'));
    let windows = root
        .get(1..)
        .filter(|_| root.starts_with(|c: char| c.is_ascii_alphabetic()))
        .and_then(|rest| rest.strip_prefix(":\\Users\\"))
        .map(|rest| (rest, '\\'));
    unix.or(windows).and_then(|(rest, separator)| rest.split(separator).next()).unwrap_or_default()
}

/// What an expansion takes besides the template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpandOptions {
    /// Text for `$SELECTION$` and the indentation of its first line.
    pub selection: Option<String>,
    pub selection_indent: Option<String>,
    pub predefined: BTreeMap<String, String>,
    /// The postfix expression, for `suggestVariableName()`.
    pub expression: Option<String>,
}

/// A template to put in place of a range of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateExpansion {
    pub body: String,
    pub variables: Vec<TemplateVariable>,
    pub from: u32,
    pub to: u32,
    pub options: ExpandOptions,
    /// Select `from..to` before applying (Surround With keeps the wrapped text selected).
    pub select_range: bool,
}

/// An expansion ready for the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedTemplate {
    pub insertion: SnippetInsertion,
    /// Per field, in tab order: whether moving to it opens completion.
    pub completing: Vec<bool>,
    /// Open completion right away: the first field asks for it.
    pub complete_first: bool,
}

impl TemplateExpansion {
    fn new(body: &str, variables: &[TemplateVariable], from: u32, to: u32) -> Self {
        Self {
            body: body.to_owned(),
            variables: variables.to_vec(),
            from,
            to,
            options: ExpandOptions::default(),
            select_range: false,
        }
    }

    /// Whether a variable reads the clipboard, which then has to be fetched before
    /// [`TemplateExpansion::apply`].
    pub fn needs_clipboard(&self) -> bool {
        self.variables.iter().any(|variable| {
            [&variable.expression, &variable.default_value]
                .into_iter()
                .flatten()
                .any(|source| called_functions(source).iter().any(|function| function == "clipboard"))
        })
    }

    /// The edit, the selection and the fields of the expansion in `editor`. `clipboard` is the
    /// clipboard text when [`TemplateExpansion::needs_clipboard`].
    pub fn apply(&self, editor: &TemplateEditor<'_>, clipboard: Option<&str>) -> AppliedTemplate {
        let before = buffer::slice(editor.doc, self.from.saturating_sub(SCAN_BACK), self.from);
        let user = user_name(editor.project_root);
        let fns = template_functions(&TemplateEnvInput {
            ancestors: editor.ancestors,
            before: &before,
            pos: self.from,
            path: editor.path,
            language: editor.language,
            comment_tokens: editor.comment_tokens,
            clipboard,
            user: Some(user),
            expression: self.options.expression.as_deref(),
        });
        let env = ExpansionEnv {
            fns,
            selection: self.options.selection.clone(),
            selection_indent: self.options.selection_indent.clone(),
            predefined: self.options.predefined.clone(),
            now: None,
        };
        let expansion = expand_template(&self.body, &self.variables, &env);
        let main = if self.select_range { self.from..self.to } else { editor.selection.clone() };
        let insertion = apply_snippet(&expansion.snippet, editor.doc, main, self.from, self.to, editor.indent_unit);
        AppliedTemplate { insertion, completing: expansion.completing, complete_first: expansion.complete_first }
    }
}

/// What a chooser's pick continues with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingChoice {
    /// Several templates share the abbreviation that starts at `from`.
    Template {
        from: u32,
        templates: Vec<LiveTemplate>,
    },
    Surround(SurroundChoice),
    /// Which expression a postfix template applies to.
    PostfixExpression {
        template: PostfixTemplate,
        target_end: u32,
        candidates: Vec<PostfixCandidate>,
    },
}

/// What Surround With wraps, and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurroundChoice {
    pub from: u32,
    pub to: u32,
    /// The text to wrap, as it was when the chooser opened.
    pub selection: String,
    /// Indentation of the first wrapped line.
    pub indent: String,
    surrounders: Vec<Surrounder>,
    templates: Vec<LiveTemplate>,
}

/// What picking a chooser item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    Expand(TemplateExpansion),
    /// Open the Settings on this page.
    OpenSettings(&'static str),
    /// The item no longer applies (the document changed under the chooser).
    Nothing,
}

fn postfix_expansion(template: &PostfixTemplate, candidate: &PostfixCandidate, end: u32) -> TemplateExpansion {
    let wrap = template.wrap_compound && is_compound(&candidate.text);
    let expr = if wrap { format!("({})", candidate.text) } else { candidate.text.clone() };
    let mut expansion = TemplateExpansion::new(&template.body, &template.variables, candidate.from, end);
    expansion.options.predefined.insert("EXPR".to_owned(), expr);
    expansion.options.expression = Some(candidate.text.clone());
    expansion
}

impl PendingChoice {
    /// The step after the item `key` was picked, in the document as it is now with its caret at
    /// `head`.
    pub fn pick(&self, key: &str, doc: &Rope, head: u32) -> Picked {
        match self {
            Self::Template { from, templates } => match templates.iter().find(|t| t.id == key) {
                Some(t) => Picked::Expand(TemplateExpansion::new(&t.body, &t.variables, *from, head)),
                None => Picked::Nothing,
            },
            Self::Surround(choice) => choice.pick(key, doc),
            Self::PostfixExpression { template, target_end, candidates } => {
                match key.parse::<usize>().ok().and_then(|index| candidates.get(index)) {
                    // The key may have grown since the target was computed.
                    Some(candidate) => Picked::Expand(postfix_expansion(template, candidate, (*target_end).max(head))),
                    None => Picked::Nothing,
                }
            }
        }
    }
}

impl SurroundChoice {
    fn pick(&self, key: &str, doc: &Rope) -> Picked {
        if key == CONFIGURE_TEMPLATES_KEY {
            return Picked::OpenSettings(LIVE_TEMPLATES_PAGE);
        }
        let surrounder = self.surrounders.iter().find(|s| s.id == key).map(|s| (&s.body, &s.variables));
        let template = self.templates.iter().find(|t| t.id == key).map(|t| (&t.body, &t.variables));
        let Some((body, variables)) = surrounder.or(template) else {
            return Picked::Nothing;
        };
        if buffer::slice(doc, self.from, self.to) != self.selection {
            return Picked::Nothing;
        }
        let mut expansion = TemplateExpansion::new(body, variables, self.from, self.to);
        expansion.options.selection = Some(self.selection.clone());
        expansion.options.selection_indent = Some(self.indent.clone());
        expansion.select_range = true;
        Picked::Expand(expansion)
    }
}

/// What a template command does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateAction {
    Expand(TemplateExpansion),
    /// Show `chooser`; the picked item's key goes to `pending`.
    Choose {
        chooser: ChooserRequest,
        pending: PendingChoice,
    },
}

// ---------------------------------------------------------------------------
// Live templates

/// A character an abbreviation may hold.
fn is_abbreviation_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '!' | '#' | '@' | '.' | '-')
}

/// The abbreviation candidate before `pos`: where it starts and its text. It stops at a `.`, so
/// that the key of `x.if` is not one.
pub fn word_before(doc: &Rope, pos: u32) -> Option<(u32, String)> {
    let (line, _) = buffer::line_col(doc, pos);
    let before = buffer::slice(doc, buffer::line_range(doc, line).start, pos);
    let run = &before[before.trim_end_matches(is_abbreviation_char).len()..];
    let text = run.rsplit('.').next().unwrap_or(run);
    (!text.is_empty()).then(|| (pos - text.len() as u32, text.to_owned()))
}

/// The templates applicable at `pos` of `editor`.
pub fn templates_at<'t>(editor: &TemplateEditor<'_>, pos: u32, templates: &'t [LiveTemplate]) -> Vec<&'t LiveTemplate> {
    let filter =
        TemplateFilter { in_comment_or_string: in_comment_or_string(editor.ancestors, pos), contexts: editor.contexts };
    applicable_templates(templates, editor.language, filter)
}

fn template_items(templates: &[&LiveTemplate]) -> Vec<ChooserItem> {
    templates
        .iter()
        .map(|t| ChooserItem {
            detail: Some(t.description.clone()),
            search: Some(t.description.clone()),
            ..ChooserItem::new(t.id.as_str(), t.abbrev.as_str())
        })
        .collect()
}

/// Tab on an abbreviation: expands the template it names, or asks which when several do. `None`
/// when the word before the caret names no template applicable there.
pub fn expand_live_template_at_caret(
    editor: &TemplateEditor<'_>,
    templates: &[LiveTemplate],
) -> Option<TemplateAction> {
    if !editor.selection_is_empty() {
        return None;
    }
    let (from, word) = word_before(editor.doc, editor.head)?;
    // `x.if` is a postfix key, not an abbreviation.
    if editor.char_before(from) == "." {
        return None;
    }
    let matching: Vec<&LiveTemplate> = templates_at(editor, editor.head, templates)
        .into_iter()
        .filter(|t| t.abbrev == word && !uses_selection(&t.body))
        .collect();
    match matching.as_slice() {
        [] => None,
        [only] => Some(TemplateAction::Expand(TemplateExpansion::new(&only.body, &only.variables, from, editor.head))),
        several => Some(TemplateAction::Choose {
            chooser: ChooserRequest {
                title: "Choose Template".to_owned(),
                items: template_items(several),
                filterable: true,
                empty_text: Some("No matching templates".to_owned()),
                ..ChooserRequest::default()
            },
            pending: PendingChoice::Template { from, templates: several.iter().map(|&t| t.clone()).collect() },
        }),
    }
}

/// Whether the open (or opening) lookup is Insert Live Template's, which lists templates only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateLookup {
    active: bool,
}

impl TemplateLookup {
    pub fn is_active(self) -> bool {
        self.active
    }

    /// Insert Live Template was invoked: the completion that opens now lists templates only.
    /// `completion_started` is whether a completion could be started at all.
    pub fn insert_live_template(&mut self, completion_started: bool) {
        self.active = completion_started;
    }

    /// The completion lookup went away.
    pub fn completion_closed(&mut self) {
        self.active = false;
    }
}

// ---------------------------------------------------------------------------
// Surround With

fn offset_of(doc: &Rope, pos: ide_core::Pos) -> u32 {
    buffer::offset_at(doc, pos.line, pos.col)
}

/// The whole lines of the statement (or member) the structure service found at a caret, when it
/// spans several lines; `None` when the statement is a single line, broken, or shares its lines
/// with other code.
pub fn multi_line_statement(doc: &Rope, ranges: Option<&StructuralRanges>) -> Option<Range<u32>> {
    let ranges = ranges?;
    let span = |node: &SyntaxNode| offset_of(doc, node.range.start)..offset_of(doc, node.range.end);
    let statement = ranges.statement.as_ref().map(|node| (node, span(node)));
    let declaration = ranges.declaration.as_ref().map(|node| (node, span(node)));
    // A statement outside the declaration at the caret belongs to something else.
    let inside = |outer: &Range<u32>, inner: &Range<u32>| outer.start <= inner.start && inner.end <= outer.end;
    let (node, span) = match (statement, declaration) {
        (Some(statement), Some(declaration)) if !inside(&declaration.1, &statement.1) => declaration,
        (Some(statement), _) => statement,
        (None, declaration) => declaration?,
    };
    if node.has_error {
        return None;
    }
    let (first, _) = buffer::line_col(doc, span.start);
    let (last, _) = buffer::line_col(doc, span.end);
    if first == last {
        return None;
    }
    let (first, last) = (buffer::line_range(doc, first), buffer::line_range(doc, last));
    let lead = buffer::slice(doc, first.start, span.start);
    let trail = buffer::slice(doc, span.end, last.end);
    (lead.trim().is_empty() && matches!(trail.trim(), "" | ";")).then_some(first.start..last.end)
}

/// Surround With: the chooser over what wraps the selection, or without one `target` (the
/// statement at the caret) or the current line. As in IDEA it lists the language's surrounders
/// (mnemonics 1–9, 0, A–Z), then the `$SELECTION$` live templates and "Configure Live
/// Templates…". `templates_only` is Surround with Live Template.
pub fn surround_with(
    editor: &TemplateEditor<'_>,
    live_templates: &[LiveTemplate],
    target: Option<Range<u32>>,
    templates_only: bool,
) -> TemplateAction {
    let doc = editor.doc;
    let templates: Vec<&LiveTemplate> =
        applicable_templates(live_templates, editor.language, TemplateFilter::default())
            .into_iter()
            .filter(|t| uses_selection(&t.body))
            .collect();
    let Range { start: mut from, end: mut to } = editor.selection.clone();
    let selected = from != to;
    if !selected {
        match target.filter(|target| target.start <= from && from <= target.end) {
            Some(target) => (from, to) = (target.start, target.end),
            None => (from, to) = (editor.line_start(from), editor.line_end(from)),
        }
    }
    // Start at the first line's indentation, end before trailing blank lines.
    let first_start = editor.line_start(from);
    let first_line = buffer::slice(doc, first_start, editor.line_end(from));
    let indent = &first_line[..first_line.len() - first_line.trim_start_matches([' ', '\t']).len()];
    let code_start = first_start + indent.len() as u32;
    from = from.max(code_start);
    let wrapped = buffer::slice(doc, from, to.max(from));
    let selection = wrapped.trim_end();
    to = from + selection.encode_utf16().count() as u32;
    let after = buffer::slice(doc, to, editor.line_end(to));
    let lines = !selected || (from == code_start && after.trim().is_empty());
    let one_line = buffer::line_col(doc, from).0 == buffer::line_col(doc, to).0;
    let inline = selected && one_line && !selection.trim().is_empty();
    let surrounders =
        if templates_only { Vec::new() } else { surrounders_for(editor.language, SurroundSelection { lines, inline }) };

    let mut items: Vec<ChooserItem> = Vec::new();
    for entry in surround_entries(&surrounders, &templates) {
        let label = match entry.kind {
            SurroundEntryKind::Surrounder => {
                surrounders.iter().find(|s| s.id == entry.id).map(|s| (s.description.clone(), None))
            }
            SurroundEntryKind::Template => {
                if !templates_only && !items.iter().any(|item| item.section) {
                    items.push(ChooserItem::section(TEMPLATES_SECTION_KEY, "Live templates"));
                }
                templates.iter().find(|t| t.id == entry.id).map(|t| (t.abbrev.clone(), Some(t.description.clone())))
            }
        };
        if let Some((label, detail)) = label {
            items.push(ChooserItem {
                detail,
                mnemonic: entry.mnemonic,
                mnemonic_index: entry.mnemonic_index,
                ..ChooserItem::new(entry.id, label)
            });
        }
    }
    if !templates.is_empty() {
        items.push(ChooserItem::section(SEPARATOR_KEY, ""));
        items.push(ChooserItem::new(CONFIGURE_TEMPLATES_KEY, "Configure Live Templates…"));
    }
    let (title, empty_text) = if templates_only {
        ("Surround with Live Template", "No live templates to surround with")
    } else {
        ("Surround With", "Nothing to surround with here")
    };
    TemplateAction::Choose {
        chooser: ChooserRequest {
            title: title.to_owned(),
            items,
            mnemonics: true,
            empty_text: Some(empty_text.to_owned()),
            ..ChooserRequest::default()
        },
        pending: PendingChoice::Surround(SurroundChoice {
            from,
            to,
            selection: selection.to_owned(),
            indent: indent.to_owned(),
            surrounders: surrounders.into_iter().cloned().collect(),
            templates: templates.into_iter().cloned().collect(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Postfix

/// Applies `template` to the expression before its key: at once when only one expression fits,
/// through the "Expressions" chooser when several do. `None` when none fits.
pub fn apply_postfix(template: &PostfixTemplate, target: &PostfixTarget, head: u32) -> Option<TemplateAction> {
    let candidates = candidates_for(template, target);
    match candidates.as_slice() {
        [] => None,
        [only] => Some(TemplateAction::Expand(postfix_expansion(template, only, target.end.max(head)))),
        several => {
            let items = several.iter().enumerate().map(|(index, candidate)| {
                let label = candidate.text.split_whitespace().collect::<Vec<_>>().join(" ");
                ChooserItem::new(index.to_string(), label)
            });
            Some(TemplateAction::Choose {
                chooser: ChooserRequest {
                    title: "Expressions".to_owned(),
                    items: items.collect(),
                    ..ChooserRequest::default()
                },
                pending: PendingChoice::PostfixExpression {
                    template: template.clone(),
                    target_end: target.end,
                    candidates: several.iter().map(|&candidate| candidate.clone()).collect(),
                },
            })
        }
    }
}

/// The postfix target before `pos` and the templates that have an expression to work on there.
fn postfix_at(
    editor: &TemplateEditor<'_>,
    pos: u32,
    settings: &PostfixSettings,
) -> Option<(PostfixTarget, Vec<&'static PostfixTemplate>)> {
    if !settings.enabled {
        return None;
    }
    let disabled: BTreeSet<String> = settings.disabled.iter().cloned().collect();
    let templates = postfix_templates_for(editor.language, &disabled);
    if templates.is_empty() || in_comment_or_string(editor.ancestors, pos) {
        return None;
    }
    let target = find_postfix_target(editor.doc, pos, editor.language, editor.dot_ancestors)?;
    let usable = templates.into_iter().filter(|t| !candidates_for(t, &target).is_empty()).collect();
    Some((target, usable))
}

/// Tab on `expr.key`: applies the postfix template `key` names. `None` when there is none.
pub fn expand_postfix_at_caret(editor: &TemplateEditor<'_>, settings: &PostfixSettings) -> Option<TemplateAction> {
    if !editor.selection_is_empty() || !settings.expand_with_tab {
        return None;
    }
    let (target, templates) = postfix_at(editor, editor.head, settings)?;
    if target.key.is_empty() {
        return None;
    }
    let template = templates.into_iter().find(|t| t.key == target.key)?;
    apply_postfix(template, &target, editor.head)
}

// ---------------------------------------------------------------------------
// Completion

/// What typing after the lookup opened may hold for its items to stay valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidFor {
    /// Letters, digits and `_`.
    Word,
    /// What an abbreviation may hold, but for the `.`.
    Abbreviation,
}

impl ValidFor {
    pub fn matches(self, typed: &str) -> bool {
        typed.chars().all(|c| match self {
            Self::Word => c.is_ascii_alphanumeric() || c == '_',
            Self::Abbreviation => c != '.' && is_abbreviation_char(c),
        })
    }
}

/// What accepting a lookup item expands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionApply {
    /// The live template with this id, in place of the completed range.
    Live(String),
    /// The postfix template with this id ([`apply_postfix_completion`]).
    Postfix(String),
}

/// One lookup item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateCompletion {
    pub label: String,
    /// Shown after the label.
    pub detail: Option<String>,
    /// Shown right-aligned, as IDEA shows a template's description.
    pub type_text: Option<String>,
    /// Ranks the item among the others.
    pub boost: i32,
    /// The section the item is listed in and the section's rank.
    pub section: Option<(&'static str, i32)>,
    pub apply: CompletionApply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    /// Where the completed text starts.
    pub from: u32,
    pub options: Vec<TemplateCompletion>,
    pub valid_for: ValidFor,
    /// For postfix items: the target they were offered for.
    pub postfix_target: Option<PostfixTarget>,
}

/// The live templates applicable at `pos` as lookup items replacing the word before it.
fn live_template_options(
    editor: &TemplateEditor<'_>,
    pos: u32,
    word: Option<&(u32, String)>,
    templates: &[LiveTemplate],
) -> Option<CompletionResult> {
    let list: Vec<&LiveTemplate> =
        templates_at(editor, pos, templates).into_iter().filter(|t| !uses_selection(&t.body)).collect();
    if list.is_empty() {
        return None;
    }
    let options = list.iter().map(|t| TemplateCompletion {
        label: t.abbrev.clone(),
        detail: None,
        type_text: Some(t.description.clone()),
        // An exact abbreviation comes first.
        boost: if word.is_some_and(|(_, text)| *text == t.abbrev) { 5 } else { -5 },
        section: None,
        apply: CompletionApply::Live(t.id.clone()),
    });
    Some(CompletionResult {
        from: word.map_or(pos, |(from, _)| *from),
        options: options.collect(),
        valid_for: ValidFor::Abbreviation,
        postfix_target: None,
    })
}

/// What the completion lookup lists of templates at `pos`: postfix templates after a dot, live
/// templates for a word, and every applicable live template while `lookup` (Insert Live Template)
/// is active.
pub fn template_completions(
    editor: &TemplateEditor<'_>,
    pos: u32,
    templates: &[LiveTemplate],
    lookup: TemplateLookup,
    live_options: LiveTemplateOptions,
    postfix: &PostfixSettings,
) -> Option<CompletionResult> {
    let word = word_before(editor.doc, pos);
    let word_after_dot = word.as_ref().is_some_and(|(from, _)| editor.char_before(*from) == ".");
    if lookup.is_active() {
        return live_template_options(editor, pos, word.as_ref().filter(|_| !word_after_dot), templates);
    }
    let after_dot = if word.is_some() { word_after_dot } else { editor.char_before(pos) == "." };
    if after_dot {
        if !postfix.show_in_completion {
            return None;
        }
        let (target, usable) = postfix_at(editor, pos, postfix).filter(|(_, usable)| !usable.is_empty())?;
        let options = usable.iter().map(|t| TemplateCompletion {
            label: t.key.clone(),
            detail: Some(t.description.clone()),
            type_text: None,
            boost: -10,
            section: Some(("Postfix", 99)),
            apply: CompletionApply::Postfix(t.id.clone()),
        });
        return Some(CompletionResult {
            from: target.dot + 1,
            options: options.collect(),
            valid_for: ValidFor::Word,
            postfix_target: Some(target),
        });
    }
    if !live_options.show_in_completion {
        return None;
    }
    live_template_options(editor, pos, Some(word.as_ref()?), templates)
}

/// Accepting a live template item: the template in place of `from..to`.
pub fn apply_live_completion(template: &LiveTemplate, from: u32, to: u32) -> TemplateExpansion {
    TemplateExpansion::new(&template.body, &template.variables, from, to)
}

/// Accepting a postfix item in `editor` as it is now: on the target found there (the key may
/// have been typed further), else on `offered`, the target the item was listed for.
pub fn apply_postfix_completion(
    editor: &TemplateEditor<'_>,
    template: &PostfixTemplate,
    offered: &PostfixTarget,
) -> Option<TemplateAction> {
    let now = find_postfix_target(editor.doc, editor.head, editor.language, editor.dot_ancestors);
    match now {
        Some(target) => apply_postfix(template, &PostfixTarget { end: editor.head, ..target }, editor.head),
        None => apply_postfix(template, offered, editor.head),
    }
}

// ---------------------------------------------------------------------------
// Keys

/// What Tab does where templates may expand.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::large_enum_variant, reason = "made once per Tab press and consumed at once")]
pub enum TabOutcome {
    /// A postfix or live template expands (or asks which).
    Template(TemplateAction),
    /// No template: an Emmet abbreviation may expand.
    TryEmmet,
    /// Tab is not the templates': the editor is read-only or the lookup takes it.
    Ignore,
}

/// Tab in the editor: postfix templates first, then live templates, then Emmet.
pub fn tab(
    editor: &TemplateEditor<'_>,
    read_only: bool,
    lookup_open: bool,
    templates: &[LiveTemplate],
    live_options: LiveTemplateOptions,
    postfix: &PostfixSettings,
) -> TabOutcome {
    if read_only || lookup_open {
        return TabOutcome::Ignore;
    }
    let live = || expand_live_template_at_caret(editor, templates).filter(|_| live_options.expand_with_tab);
    match expand_postfix_at_caret(editor, postfix).or_else(live) {
        Some(action) => TabOutcome::Template(action),
        None => TabOutcome::TryEmmet,
    }
}

/// A key that walks the fields of the template being filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    Tab,
    ShiftTab,
    Enter,
}

/// The move a field key makes in `snippet`: Tab and Enter go to the next field, Shift-Tab to the
/// previous one. `None` leaves the key to the editor: while the lookup is open Tab and Enter are
/// the lookup's, and there may be no field to go to.
pub fn field_key(snippet: &ActiveSnippet, key: FieldKey, lookup_open: bool) -> Option<FieldMove> {
    match key {
        FieldKey::ShiftTab => snippet.prev_field(),
        FieldKey::Tab | FieldKey::Enter if !lookup_open && snippet.has_next_field() => Some(snippet.next_field()),
        FieldKey::Tab | FieldKey::Enter => None,
    }
}

/// What Esc does in a template, as in IDEA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeAction {
    /// Close the lookup and put a caret at the head of every selection range.
    DropSelection(Vec<u32>),
    CloseLookup,
    /// Leave the template: its fields are forgotten.
    ClearSnippet,
    /// Esc is the editor's.
    PassThrough,
}

/// Esc: first drops the selection (closing the lookup) or closes the lookup; only with neither
/// does it leave the template. `selection` holds each range with its head.
pub fn escape_template(selection: &[(Range<u32>, u32)], lookup_open: bool, in_snippet: bool) -> EscapeAction {
    if selection.iter().any(|(range, _)| range.start != range.end) {
        EscapeAction::DropSelection(selection.iter().map(|(_, head)| *head).collect())
    } else if lookup_open {
        EscapeAction::CloseLookup
    } else if in_snippet {
        EscapeAction::ClearSnippet
    } else {
        EscapeAction::PassThrough
    }
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a single selection is the case under test")]
mod tests {
    use super::*;
    use crate::templates::live_builtins::builtin_templates;
    use crate::templates::snippet::testing::{INDENT, insert};
    use ide_core::{Pos, Range as PosRange};

    static NO_TOKENS: CommentTokens = CommentTokens { line: None, block: None };

    /// An editor on `doc` with the selection `from..to` and its caret at `to`.
    fn editor<'a>(
        doc: &'a Rope,
        language: &'a str,
        from: u32,
        to: u32,
        tokens: &'a CommentTokens,
    ) -> TemplateEditor<'a> {
        TemplateEditor {
            doc,
            selection: from..to,
            head: to,
            cursors: 1,
            path: "/p/src/Main",
            language,
            ancestors: &[],
            dot_ancestors: &[],
            comment_tokens: tokens,
            contexts: &[],
            project_root: "/p",
            indent_unit: INDENT,
        }
    }

    fn len(doc: &Rope) -> u32 {
        buffer::len_utf16(doc)
    }

    fn at(doc: &str, needle: &str) -> u32 {
        doc.find(needle).map_or(0, |byte| buffer::col_of_byte(doc, byte))
    }

    /// The ranges an expansion selects.
    type Selection = Vec<Range<u32>>;

    /// Applies to `doc` the expansion `applied` computes on it; returns the fields left active
    /// and the selection.
    fn expand(doc: &mut Rope, applied: impl Fn(&Rope) -> AppliedTemplate) -> (Option<ActiveSnippet>, Selection) {
        let applied = applied(doc);
        insert(doc, &applied.insertion);
        (applied.insertion.active, applied.insertion.selection)
    }

    fn expansion(action: Option<TemplateAction>) -> TemplateExpansion {
        match action {
            Some(TemplateAction::Expand(expansion)) => expansion,
            other => panic!("expected an expansion, got {other:?}"),
        }
    }

    fn choice(action: Option<TemplateAction>) -> (ChooserRequest, PendingChoice) {
        match action {
            Some(TemplateAction::Choose { chooser, pending }) => (chooser, pending),
            other => panic!("expected a chooser, got {other:?}"),
        }
    }

    fn picked(pending: &PendingChoice, key: &str, doc: &Rope, head: u32) -> TemplateExpansion {
        match pending.pick(key, doc, head) {
            Picked::Expand(expansion) => expansion,
            other => panic!("expected an expansion, got {other:?}"),
        }
    }

    /// Tab on the abbreviation `text` ends with, in a `language` file: the document afterwards,
    /// the fields left active and the selection.
    fn tab_expand(language: &str, text: &str, tokens: &CommentTokens) -> (Rope, Option<ActiveSnippet>, Selection) {
        let mut doc = Rope::from_str(text);
        let end = len(&doc);
        let action = expand_live_template_at_caret(&editor(&doc, language, end, end, tokens), builtin_templates());
        let expansion = expansion(action);
        let (active, selection) =
            expand(&mut doc, |doc| expansion.apply(&editor(doc, language, end, end, tokens), None));
        (doc, active, selection)
    }

    // Tab expansion › expands a live template abbreviation
    #[test]
    fn expands_a_live_template_abbreviation() {
        let (doc, _, selection) = tab_expand("typescript", "function f() {\n  log", &NO_TOKENS);
        assert_eq!(doc.to_string(), "function f() {\n  console.log();");
        let caret = "function f() {\n  console.log(".len() as u32;
        assert_eq!(selection, [caret..caret]);
    }

    // Tab expansion › falls through when nothing matches
    #[test]
    fn falls_through_when_nothing_matches() {
        let doc = Rope::from_str("zzz");
        let at_end = editor(&doc, "typescript", 3, 3, &NO_TOKENS);
        assert_eq!(expand_live_template_at_caret(&at_end, builtin_templates()), None);
    }

    // Tab expansion › does not expand code templates inside comments
    #[test]
    fn does_not_expand_code_templates_inside_comments() {
        let doc = Rope::from_str("// log");
        let comment = [TreeNode::new("comment", 0, 6), TreeNode::new("program", 0, 6)];
        let in_comment = TemplateEditor { ancestors: &comment, ..editor(&doc, "typescript", 6, 6, &NO_TOKENS) };
        assert_eq!(expand_live_template_at_caret(&in_comment, builtin_templates()), None);
    }

    // Tab expansion › expands todo with the language's comment start
    #[test]
    fn expands_todo_with_the_languages_comment_start() {
        let tokens = CommentTokens { line: Some("#".to_owned()), block: None };
        let (doc, _, _) = tab_expand("python", "x = 1\ntodo", &tokens);
        assert_eq!(doc.to_string(), "x = 1\n# TODO: ");
    }

    // Tab expansion › expands postfix templates replacing the expression
    #[test]
    fn expands_postfix_templates_replacing_the_expression() {
        let mut doc = Rope::from_str("fn main() {\n    items.iter().for");
        let end = len(&doc);
        let action = expand_postfix_at_caret(&editor(&doc, "rust", end, end, &NO_TOKENS), &PostfixSettings::default());
        let expansion = expansion(action);
        expand(&mut doc, |doc| expansion.apply(&editor(doc, "rust", end, end, &NO_TOKENS), None));
        assert_eq!(doc.to_string(), "fn main() {\n    for item in items.iter() {\n      \n    }");
    }

    // Tab expansion › wraps compound expressions for .not and offers a chooser
    //
    // The syntax chain around the dot is written by hand, as a tree-sitter grammar shapes the line.
    #[test]
    fn wraps_compound_expressions_for_not_and_offers_a_chooser() {
        let text = "class A { void f() { boolean b = x > y.not";
        let mut doc = Rope::from_str(text);
        let end = len(&doc);
        let (x, y) = (at(text, "x >"), at(text, "y.not"));
        let chain = [
            TreeNode::new("identifier", y, y + 1),
            TreeNode::new("field_access", y, end),
            TreeNode::new("binary_expression", x, end),
            TreeNode::new("variable_declarator", at(text, "b ="), end),
        ];
        fn at_end<'a>(doc: &'a Rope, chain: &'a [TreeNode], end: u32) -> TemplateEditor<'a> {
            TemplateEditor { dot_ancestors: chain, ..editor(doc, "java", end, end, &NO_TOKENS) }
        }
        let action = expand_postfix_at_caret(&at_end(&doc, &chain, end), &PostfixSettings::default());
        let (chooser, pending) = choice(action);
        assert_eq!(chooser.items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["y", "x > y"]);
        let expansion = picked(&pending, &chooser.items[1].key, &doc, end);
        expand(&mut doc, |doc| expansion.apply(&at_end(doc, &chain, end), None));
        assert_eq!(doc.to_string(), "class A { void f() { boolean b = !(x > y)");
    }

    // surround with › wraps the selected lines
    #[test]
    fn wraps_the_selected_lines() {
        let text = "def f():\n    a()\n    b()\n";
        let mut doc = Rope::from_str(text);
        let (from, to) = (at(text, "a()"), at(text, "b()") + 3);
        let selected = editor(&doc, "python", from, to, &NO_TOKENS);
        let (chooser, pending) = choice(Some(surround_with(&selected, builtin_templates(), None, false)));
        let item = chooser.items.iter().find(|item| item.label == "if").expect("the if surrounder");
        let expansion = picked(&pending, &item.key, &doc, to);
        assert!(expansion.select_range);
        expand(&mut doc, |doc| expansion.apply(&editor(doc, "python", from, to, &NO_TOKENS), None));
        assert_eq!(doc.to_string(), "def f():\n    if :\n      a()\n      b()\n    \n");
    }

    // surround with › lists the language's surrounders with numeric mnemonics; a mnemonic key picks
    #[test]
    fn lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks() {
        let text = "class A {\n  void f() {\n    a();\n  }\n}\n";
        let mut doc = Rope::from_str(text);
        let caret = at(text, "a();");
        let at_caret = editor(&doc, "java", caret, caret, &NO_TOKENS);
        let (chooser, pending) = choice(Some(surround_with(&at_caret, builtin_templates(), None, false)));
        assert!(chooser.mnemonics);
        let first: Vec<(Option<&str>, &str)> =
            chooser.items.iter().take(3).map(|item| (item.mnemonic.as_deref(), item.label.as_str())).collect();
        assert_eq!(first, [(Some("1"), "if"), (Some("2"), "if / else"), (Some("3"), "while")]);
        // Expression surrounders need a selection within the line.
        assert!(!chooser.items.iter().any(|item| item.label == "(expr)"));
        let nine = chooser.item_for_mnemonic("9").expect("the ninth surrounder");
        let expansion = picked(&pending, &nine.key, &doc, caret);
        expand(&mut doc, |doc| expansion.apply(&editor(doc, "java", caret, caret, &NO_TOKENS), None));
        assert!(doc.to_string().contains("synchronized (this) {"), "{doc}");
    }

    // surround with › offers expression surrounders for a selection inside a line
    #[test]
    fn offers_expression_surrounders_for_a_selection_inside_a_line() {
        let text = "fn main() {\n    let x = a + b;\n}\n";
        let doc = Rope::from_str(text);
        let from = at(text, "a + b");
        let selected = editor(&doc, "rust", from, from + 5, &NO_TOKENS);
        let (chooser, _) = choice(Some(surround_with(&selected, builtin_templates(), None, false)));
        assert_eq!(chooser.items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["(expr)", "!(expr)"]);
    }

    // template fields › Enter moves to the next field like Tab
    #[test]
    fn enter_moves_to_the_next_field_like_tab() {
        let (doc, active, selection) = tab_expand("typescript", "fori", &NO_TOKENS);
        let active = active.expect("the template has several fields");
        assert_eq!(buffer::slice(&doc, selection[0].start, selection[0].end), "i");
        let moved = field_key(&active, FieldKey::Enter, false).expect("Enter moves to the next field");
        assert_eq!(doc.to_string(), "for (let i = 0; i < .length; i++) {\n  \n}");
        let caret = "for (let i = 0; i < ".len() as u32;
        assert_eq!(moved.selection, [caret..caret]);
        // While the lookup is open Enter is the lookup's.
        assert_eq!(field_key(&active, FieldKey::Enter, true), None);
    }

    // template fields › Esc first drops the field's selection, then leaves the template
    #[test]
    fn esc_first_drops_the_fields_selection_then_leaves_the_template() {
        let (_, active, selection) = tab_expand("typescript", "fori", &NO_TOKENS);
        let active = active.expect("the template has several fields");
        let ranges: Vec<(Range<u32>, u32)> = selection.iter().map(|range| (range.clone(), range.end)).collect();
        let heads: Vec<u32> = selection.iter().map(|range| range.end).collect();
        assert_eq!(escape_template(&ranges, false, true), EscapeAction::DropSelection(heads.clone()));
        // The carets are still in the field: the template goes on.
        let carets: Vec<Range<u32>> = heads.iter().map(|&head| head..head).collect();
        let active = active.after_update(&[], Some(carets.as_slice())).expect("the template is still active");
        assert!(active.has_next_field());
        let carets: Vec<(Range<u32>, u32)> = carets.into_iter().map(|range| (range.clone(), range.end)).collect();
        assert_eq!(escape_template(&carets, false, true), EscapeAction::ClearSnippet);
        // Outside a template Enter and Esc are left to the editor.
        assert_eq!(escape_template(&carets, false, false), EscapeAction::PassThrough);
    }

    // ⌘J Insert Live Template › opens a lookup restricted to live templates and ends with it
    #[test]
    fn opens_a_lookup_restricted_to_live_templates_and_ends_with_it() {
        let doc = Rope::from_str("function f() {\n  ");
        let end = len(&doc);
        let editor = editor(&doc, "typescript", end, end, &NO_TOKENS);
        let mut lookup = TemplateLookup::default();
        assert!(!lookup.is_active());
        let complete = |lookup: TemplateLookup| {
            let live = LiveTemplateOptions::default();
            template_completions(&editor, end, builtin_templates(), lookup, live, &PostfixSettings::default())
        };
        // Without the lookup nothing is offered where there is no word.
        assert_eq!(complete(lookup), None);
        lookup.insert_live_template(true);
        assert!(lookup.is_active());
        // No word before the caret: every applicable template, from the caret.
        let result = complete(lookup).expect("the templates are listed");
        assert_eq!(result.from, end);
        let labels: Vec<&str> = result.options.iter().map(|option| option.label.as_str()).collect();
        assert!(labels.contains(&"log") && labels.contains(&"fori"));
        lookup.completion_closed();
        assert!(!lookup.is_active());
        lookup.insert_live_template(false);
        assert!(!lookup.is_active());
    }

    // completion source › offers postfix templates after a dot
    #[test]
    fn offers_postfix_templates_after_a_dot() {
        let doc = Rope::from_str("items.");
        let editor = editor(&doc, "typescript", 6, 6, &NO_TOKENS);
        let live = LiveTemplateOptions::default();
        let result = template_completions(
            &editor,
            6,
            builtin_templates(),
            TemplateLookup::default(),
            live,
            &PostfixSettings::default(),
        )
        .expect("postfix templates are offered");
        assert_eq!(result.from, 6);
        assert!(result.options.iter().any(|option| option.label == "for"));
        assert_eq!(result.valid_for, ValidFor::Word);
    }

    // completion source › offers live templates for a word
    #[test]
    fn offers_live_templates_for_a_word() {
        let doc = Rope::from_str("lo");
        let editor = editor(&doc, "typescript", 2, 2, &NO_TOKENS);
        let live = LiveTemplateOptions::default();
        let result = template_completions(
            &editor,
            2,
            builtin_templates(),
            TemplateLookup::default(),
            live,
            &PostfixSettings::default(),
        )
        .expect("live templates are offered");
        assert!(result.options.iter().any(|option| option.label == "log"));
        assert_eq!(result.from, 0);
    }

    const DOC: &str = "fun f() {\n    if (a) {\n        b()\n    }\n    c(); d()\n}\n";

    fn node(doc: &Rope, snippet: &str, has_error: bool) -> SyntaxNode {
        let from = at(DOC, snippet);
        let pos = |offset: u32| {
            let (line, col) = buffer::line_col(doc, offset);
            Pos::new(line, col)
        };
        SyntaxNode {
            kind: "statement".to_owned(),
            range: PosRange::new(pos(from), pos(from + snippet.len() as u32)),
            field: None,
            named: true,
            error: false,
            missing: false,
            has_error,
        }
    }

    // multiLineStatement (Surround With target) › takes the whole lines of a multi-line statement
    #[test]
    fn takes_the_whole_lines_of_a_multi_line_statement() {
        let doc = Rope::from_str(DOC);
        let ranges = StructuralRanges {
            statement: Some(node(&doc, "if (a) {\n        b()\n    }", false)),
            declaration: Some(node(&doc, DOC.trim_end(), false)),
            ..StructuralRanges::default()
        };
        let lines = buffer::line_range(&doc, 1).start..buffer::line_range(&doc, 3).end;
        assert_eq!(multi_line_statement(&doc, Some(&ranges)), Some(lines));
    }

    // multiLineStatement (Surround With target) › leaves single-line, broken and line-sharing statements to the
    // line logic
    #[test]
    fn leaves_single_line_broken_and_line_sharing_statements_to_the_line_logic() {
        let doc = Rope::from_str(DOC);
        let of = |statement: SyntaxNode| StructuralRanges { statement: Some(statement), ..StructuralRanges::default() };
        assert_eq!(multi_line_statement(&doc, Some(&of(node(&doc, "b()", false)))), None);
        let broken = of(node(&doc, "if (a) {\n        b()\n    }", true));
        assert_eq!(multi_line_statement(&doc, Some(&broken)), None);
        assert_eq!(multi_line_statement(&doc, Some(&of(node(&doc, "c(); d()", false)))), None);
        assert_eq!(multi_line_statement(&doc, None), None);
    }

    // Not from a spec: Tab tries postfix templates, then live templates, then leaves Emmet its
    // turn; a read-only editor and an open lookup keep Tab; Shift-Tab walks back.
    #[test]
    fn tab_tries_postfix_then_live_templates_then_emmet() {
        let live = LiveTemplateOptions::default();
        let postfix = PostfixSettings::default();
        let outcome = |text: &str, read_only: bool, lookup_open: bool, live: LiveTemplateOptions| {
            let doc = Rope::from_str(text);
            let end = len(&doc);
            let editor = editor(&doc, "typescript", end, end, &NO_TOKENS);
            tab(&editor, read_only, lookup_open, builtin_templates(), live, &postfix)
        };
        assert!(matches!(outcome("x.not", false, false, live), TabOutcome::Template(TemplateAction::Expand(_))));
        assert!(matches!(outcome("log", false, false, live), TabOutcome::Template(TemplateAction::Expand(_))));
        assert_eq!(outcome("zzz", false, false, live), TabOutcome::TryEmmet);
        assert_eq!(outcome("log", true, false, live), TabOutcome::Ignore);
        assert_eq!(outcome("log", false, true, live), TabOutcome::Ignore);
        let no_tab = LiveTemplateOptions { expand_with_tab: false, ..live };
        assert_eq!(outcome("log", false, false, no_tab), TabOutcome::TryEmmet);
        let (_, active, _) = tab_expand("typescript", "fori", &NO_TOKENS);
        let active = active.expect("the template has several fields");
        assert_eq!(field_key(&active, FieldKey::ShiftTab, false), None);
        let second = field_key(&active, FieldKey::Tab, false).and_then(|moved| moved.active).expect("a second field");
        assert_eq!(field_key(&second, FieldKey::ShiftTab, true).map(|back| back.selection.len()), Some(3));
    }

    // Not from a spec: the word before the caret stops at a dot; Surround With on a caret takes
    // the statement it is given, lists the `$SELECTION$` templates after the surrounders and
    // opens the settings from its last row; a pick is dropped once the wrapped text changed.
    #[test]
    fn surround_with_lists_templates_and_checks_the_document() {
        let doc = Rope::from_str("x.if");
        assert_eq!(word_before(&doc, 4), Some((2, "if".to_owned())));
        assert_eq!(word_before(&doc, 2), None);
        assert_eq!(user_name("/home/ann/p"), "ann");
        assert_eq!(user_name("C:\\Users\\bob\\p"), "bob");
        assert_eq!(user_name("/srv/p"), "");

        let text = "class A {\n  void f() {\n    a();\n    b();\n  }\n}\n";
        let doc = Rope::from_str(text);
        let caret = at(text, "a();");
        let target = at(text, "void")..at(text, "  }\n}") + 3;
        let at_caret = editor(&doc, "java", caret, caret, &NO_TOKENS);
        let action = surround_with(&at_caret, builtin_templates(), Some(target.clone()), false);
        let (chooser, pending) = choice(Some(action));
        let PendingChoice::Surround(wrapped) = &pending else { panic!("a surround choice") };
        assert_eq!((wrapped.from, wrapped.to, wrapped.indent.as_str()), (target.start, target.end, "  "));
        let keys: Vec<&str> = chooser.items.iter().map(|item| item.key.as_str()).collect();
        let section = keys.iter().position(|key| *key == TEMPLATES_SECTION_KEY).expect("the templates section");
        assert_eq!(keys[section + 1], "surround.java.callable");
        assert_eq!(chooser.items[section + 1].mnemonic.as_deref(), Some("C"));
        assert_eq!(keys.last(), Some(&CONFIGURE_TEMPLATES_KEY));
        assert_eq!(pending.pick(CONFIGURE_TEMPLATES_KEY, &doc, caret), Picked::OpenSettings(LIVE_TEMPLATES_PAGE));
        assert_eq!(pending.pick("java.0", &Rope::from_str("changed"), 0), Picked::Nothing);
        assert_eq!(pending.pick("no.such", &doc, caret), Picked::Nothing);
        let only = choice_of_templates(&at_caret);
        assert!(only.items.iter().all(|item| item.key.starts_with("surround.java.") || item.key.starts_with('\u{0}')));
    }

    fn choice_of_templates(editor: &TemplateEditor<'_>) -> ChooserRequest {
        let (chooser, _) = choice(Some(surround_with(editor, builtin_templates(), None, true)));
        assert_eq!(chooser.title, "Surround with Live Template");
        chooser
    }
}
