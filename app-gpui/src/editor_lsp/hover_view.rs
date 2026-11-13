//! The documentation popups of the editor: the hover (the problems under the pointer, then the
//! quick documentation), the brief declaration of the ⌘-hover link, the Quick Documentation popup
//! with its history (F1), and the one-line hint at the caret.
//!
//! None of them knows the editor's geometry: the owner draws each at the point it computes
//! (above the hovered range, above the caret) and feeds it the models' answers. Only the Quick
//! Documentation popup can take the keyboard, and only when the owner focuses it (F1 pressed on
//! an open popup); the others leave the keyboard to the editor and keep it there when clicked.
//!
//! [`markup_element`] draws documentation blocks for every popup that shows documentation.

use super::diagnostics::LintDiagnostic;
use super::hint::{Hint, Tone};
use super::hover::HoverPopup;
use super::markdown::{Block, DeclarationSummary, DocLink, Run, blocks_text, doc_link, is_empty_markup, markup_blocks};
use crate::platform::i18n::Localization;
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, App, Context, Div, EventEmitter, FocusHandle, Focusable, Font, FontStyle, FontWeight, InteractiveText,
    IntoElement, Keystroke, MouseButton, Render, SharedString, StrikethroughStyle, StyledText, TextRun, Window, div,
    font, prelude::*, px,
};
use ide_core::Severity;
use ide_lsp::types::Markup;
use std::rc::Rc;

/// Who a drawn documentation link reports its clicks to, with the link's target.
pub(crate) type LinkHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The first family of the theme's font list `token`, else `fallback`.
pub(crate) fn font_family(cx: &App, token: &str, fallback: &str) -> Font {
    let name = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families(token).into_iter().next())
        .unwrap_or_else(|| fallback.to_owned());
    font(SharedString::from(name))
}

/// The editor's code font.
pub(crate) fn code_font(cx: &App) -> Font {
    font_family(cx, "font-mono", theme::fonts::BUNDLED_MONO_FAMILY)
}

/// The shortcut of action `id` as the keymap shows it (`⌥↵`).
pub(crate) fn shortcut_label(cx: &App, id: &str) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(id, is_mac_host()))
}

/// English source text in the active locale.
pub(crate) fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// The surface every editor popup shares: the popup background, its border, radius and shadow.
pub(crate) fn popup_surface(cx: &App) -> Div {
    div()
        .bg(theme::color(cx, "bg-popup"))
        .border_1()
        .border_color(theme::color(cx, "border-popup"))
        .rounded(theme::length(cx, "radius-popup"))
        .text_color(theme::color(cx, "fg"))
}

/// Keeps the keyboard where it is when the popup is pressed: a popup that does not take focus
/// must not hand it to whatever tracks focus around it.
pub(crate) fn keeps_focus(element: Div) -> Div {
    element.on_mouse_down(MouseButton::Left, |_, window, cx| {
        window.prevent_default();
        cx.stop_propagation();
    })
}

/// Draws documentation `blocks`. The text elements are selected as `<prefix>-text-<n>`, top to
/// bottom; a click on a link reports its target to `on_link`.
pub(crate) fn markup_element(blocks: &[Block], prefix: &str, on_link: &LinkHandler, cx: &App) -> Div {
    let ui = font_family(cx, "font-ui", ".SystemUIFont");
    let mut builder = Builder { prefix, on_link, ui, mono: code_font(cx), texts: 0 };
    let mut children = Vec::with_capacity(blocks.len());
    for block in blocks {
        children.push(builder.block(block, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .min_w_0()
        .text_size(theme::length(cx, "fs-ui"))
        .text_color(theme::color(cx, "fg-strong"))
        .children(children)
}

struct Builder<'a> {
    prefix: &'a str,
    on_link: &'a LinkHandler,
    ui: Font,
    mono: Font,
    texts: usize,
}

impl Builder<'_> {
    fn block(&mut self, block: &Block, cx: &App) -> AnyElement {
        match block {
            Block::Paragraph(runs) => self.text(runs, false, cx),
            Block::Heading { content, .. } => self.text(content, true, cx),
            Block::Code { text, .. } => self.code(text, cx),
            Block::Plain(text) => {
                let run = Run { text: text.clone(), style: Default::default() };
                self.text(std::slice::from_ref(&run), false, cx)
            }
            Block::Quote(inner) => {
                let mut children = Vec::with_capacity(inner.len());
                for block in inner {
                    children.push(self.block(block, cx));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .pl(px(10.0))
                    .border_l_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .text_color(theme::color(cx, "fg-muted"))
                    .children(children)
                    .into_any_element()
            }
            Block::List { ordered, items } => {
                let muted = theme::color(cx, "fg-muted");
                let mut rows = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let marker = match ordered {
                        Some(start) => format!("{}.", start.saturating_add(index as u64)),
                        None => "•".to_owned(),
                    };
                    let mut children = Vec::with_capacity(item.len());
                    for block in item {
                        children.push(self.block(block, cx));
                    }
                    rows.push(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                div().flex_none().min_w(px(12.0)).text_color(muted).child(SharedString::from(marker)),
                            )
                            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.0)).children(children)),
                    );
                }
                div().flex().flex_col().gap(px(4.0)).children(rows).into_any_element()
            }
            Block::Table { header, rows } => {
                let border = theme::color(cx, "border-strong");
                let mut lines = Vec::with_capacity(rows.len() + 1);
                lines.push(self.table_row(header, true, cx));
                for cells in rows {
                    lines.push(self.table_row(cells, false, cx));
                }
                div().flex().flex_col().border_1().border_color(border).children(lines).into_any_element()
            }
            Block::Rule => div().h(px(1.0)).my(px(4.0)).bg(theme::color(cx, "border-popup")).into_any_element(),
        }
    }

    fn table_row(&mut self, cells: &[Vec<Run>], header: bool, cx: &App) -> Div {
        let border = theme::color(cx, "border-strong");
        let mut children = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter().enumerate() {
            children.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(6.0))
                    .py(px(2.0))
                    .when(index > 0, |el| el.border_l_1().border_color(border))
                    .child(self.text(cell, header, cx)),
            );
        }
        div().flex().when(!header, |el| el.border_t_1().border_color(border)).children(children)
    }

    /// Runs of text in one element: bold, italic, code, strike-through, and links that report
    /// their target when clicked.
    fn text(&mut self, runs: &[Run], bold: bool, cx: &App) -> AnyElement {
        let n = self.texts;
        self.texts += 1;
        let mut text = String::new();
        let mut text_runs = Vec::with_capacity(runs.len());
        let mut ranges = Vec::new();
        let mut hrefs: Vec<String> = Vec::new();
        for run in runs.iter().filter(|run| !run.text.is_empty()) {
            let start = text.len();
            text.push_str(&run.text);
            let style = &run.style;
            let base = if style.code { &self.mono } else { &self.ui };
            let font = Font {
                weight: if style.bold || bold { FontWeight::BOLD } else { base.weight },
                style: if style.italic { FontStyle::Italic } else { base.style },
                ..base.clone()
            };
            let token = match (&style.link, style.code) {
                (Some(_), _) => "link",
                (None, true) => "fg-emphasis",
                (None, false) => "fg-strong",
            };
            text_runs.push(TextRun {
                len: run.text.len(),
                font,
                color: theme::color(cx, token),
                background_color: None,
                underline: None,
                strikethrough: style.strike.then(|| StrikethroughStyle { thickness: px(1.0), color: None }),
            });
            if let Some(href) = &style.link {
                ranges.push(start..text.len());
                hrefs.push(href.clone());
            }
        }
        if text.is_empty() {
            return div().into_any_element();
        }
        let styled = StyledText::new(text).with_runs(text_runs);
        let selector = format!("{}-text-{n}", self.prefix);
        let element = if hrefs.is_empty() {
            styled.into_any_element()
        } else {
            let on_link = Rc::clone(self.on_link);
            InteractiveText::new(SharedString::from(selector.clone()), styled)
                .on_click(ranges, move |index, window, cx| {
                    if let Some(href) = hrefs.get(index) {
                        on_link(href, window, cx);
                    }
                })
                .into_any_element()
        };
        div().debug_selector(move || selector).min_w_0().child(element).into_any_element()
    }

    /// A code block in the code font, its lines kept and wrapped.
    fn code(&mut self, text: &str, cx: &App) -> AnyElement {
        let n = self.texts;
        self.texts += 1;
        let shown = if text.is_empty() { " ".to_owned() } else { text.to_owned() };
        let run = TextRun {
            len: shown.len(),
            font: self.mono.clone(),
            color: theme::color(cx, "fg-emphasis"),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let selector = format!("{}-text-{n}", self.prefix);
        div()
            .debug_selector(move || selector)
            .min_w_0()
            .px(px(8.0))
            .py(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .bg(theme::color(cx, "bg-popup-inline"))
            .child(StyledText::new(shown).with_runs(vec![run]))
            .into_any_element()
    }
}

/// A handler that hands each clicked link to `view` through `clicked`.
fn link_handler<V: 'static>(
    cx: &mut Context<V>,
    clicked: fn(&mut V, &str, &mut Window, &mut Context<V>),
) -> LinkHandler {
    let weak = cx.weak_entity();
    Rc::new(move |href: &str, window: &mut Window, cx: &mut App| {
        let _ = weak.update(cx, |view, cx| clicked(view, href, window, cx));
    })
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

fn severity_icon(severity: Severity) -> (&'static str, &'static str) {
    match severity {
        Severity::Error => ("error", "danger"),
        Severity::Warning => ("warning", "warning"),
        Severity::Info | Severity::Hint => ("info", "info"),
    }
}

// ---------------------------------------------------------------------------
// The hover

/// What the hover asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverEvent {
    /// A documentation link was clicked.
    Link(DocLink),
    /// The quick fix link of problem `diagnostic` (an index into the shown problems): close the
    /// hover, focus the editor and apply the fix the owner offered with [`HoverView::set_quick_fix`].
    QuickFix { diagnostic: usize },
    /// "More actions…" of a problem: put the caret at `at` (the problem's start), focus the editor
    /// and run `code.showIntentions`.
    MoreActions { at: u32 },
}

enum HoverContent {
    Empty,
    Hover {
        diagnostics: Vec<LintDiagnostic>,
        /// The title of each problem's preferred quick fix, once the server named one.
        fixes: Vec<Option<String>>,
        blocks: Vec<Block>,
    },
    Declaration(DeclarationSummary),
}

/// The hover popup and the ⌘-hover declaration. The owner draws it above the hovered range: at
/// [`crate::editor::view::Editor::position_of`] of the popup's `from`, bottom edge on the top of
/// that line, flipping below the line when there is no room above.
pub struct HoverView {
    content: HoverContent,
    on_link: LinkHandler,
}

impl EventEmitter<HoverEvent> for HoverView {}

impl HoverView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let on_link = link_handler(cx, |view: &mut Self, href, _, cx| view.link(href, cx));
        Self { content: HoverContent::Empty, on_link }
    }

    /// Shows `popup` (from [`super::hover::hover_popup`]): its problems, then its documentation.
    pub fn show(&mut self, popup: &HoverPopup, cx: &mut Context<Self>) {
        let blocks = popup
            .hover
            .as_ref()
            .filter(|hover| !is_empty_markup(Some(&hover.contents)))
            .map(|hover| markup_blocks(&hover.contents))
            .unwrap_or_default();
        self.content = HoverContent::Hover {
            fixes: vec![None; popup.diagnostics.len()],
            diagnostics: popup.diagnostics.clone(),
            blocks,
        };
        cx.notify();
    }

    /// The server named the preferred quick fix of problem `diagnostic`: its link shows before
    /// "More actions…".
    pub fn set_quick_fix(&mut self, diagnostic: usize, title: &str, cx: &mut Context<Self>) {
        if let HoverContent::Hover { fixes, .. } = &mut self.content
            && let Some(slot) = fixes.get_mut(diagnostic)
        {
            *slot = Some(title.to_owned());
            cx.notify();
        }
    }

    /// Shows the brief declaration of the ⌘-hover link ([`super::markdown::declaration_summary`]).
    pub fn show_declaration(&mut self, summary: DeclarationSummary, cx: &mut Context<Self>) {
        self.content = HoverContent::Declaration(summary);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = HoverContent::Empty;
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        !matches!(self.content, HoverContent::Empty)
    }

    /// What the popup shows, a line per part: `error: message`, `  source`, `  fix: title`,
    /// `  More actions…` for each problem, then the documentation's text; a declaration's code or
    /// text.
    pub fn lines(&self) -> Vec<String> {
        match &self.content {
            HoverContent::Empty => Vec::new(),
            HoverContent::Declaration(DeclarationSummary::Code { code, .. }) => vec![code.clone()],
            HoverContent::Declaration(DeclarationSummary::Text(text)) => vec![text.clone()],
            HoverContent::Hover { diagnostics, fixes, blocks } => {
                let mut out = Vec::new();
                for (diagnostic, fix) in diagnostics.iter().zip(fixes) {
                    out.push(format!("{}: {}", severity_name(diagnostic.severity), diagnostic.message));
                    out.extend(diagnostic.source.iter().map(|source| format!("  {source}")));
                    out.extend(fix.iter().map(|fix| format!("  fix: {fix}")));
                    if diagnostic.severity != Severity::Hint {
                        out.push("  More actions…".to_owned());
                    }
                }
                if !blocks.is_empty() {
                    out.push(blocks_text(blocks));
                }
                out
            }
        }
    }

    fn link(&mut self, href: &str, cx: &mut Context<Self>) {
        let link = doc_link(href);
        if link != DocLink::Ignored {
            cx.emit(HoverEvent::Link(link));
        }
    }

    fn diagnostics(&self, diagnostics: &[LintDiagnostic], fixes: &[Option<String>], cx: &mut Context<Self>) -> Div {
        let more = tx(cx, "More actions…");
        let more = match shortcut_label(cx, "code.showIntentions") {
            Some(keys) => format!("{more} {keys}"),
            None => more,
        };
        let fix_keys = shortcut_label(cx, "code.applyQuickFix");
        let link = theme::color(cx, "link");
        let rows = diagnostics.iter().zip(fixes).enumerate().map(|(index, (diagnostic, fix))| {
            let (icon, color) = severity_icon(diagnostic.severity);
            let at = diagnostic.from;
            let fix = fix.as_ref().map(|title| {
                let label = match &fix_keys {
                    Some(keys) => format!("{title} {keys}"),
                    None => title.clone(),
                };
                div()
                    .id(SharedString::from(format!("hover-fix-{index}")))
                    .debug_selector(move || format!("hover-fix-{index}"))
                    .text_color(link)
                    .hover(|el| el.underline())
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(HoverEvent::QuickFix { diagnostic: index })))
            });
            let more = (diagnostic.severity != Severity::Hint).then(|| {
                div()
                    .id(SharedString::from(format!("hover-more-{index}")))
                    .debug_selector(move || format!("hover-more-{index}"))
                    .text_color(link)
                    .hover(|el| el.underline())
                    .child(SharedString::from(more.clone()))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(HoverEvent::MoreActions { at })))
            });
            div()
                .flex()
                .gap(px(6.0))
                .child(div().flex_none().pt(px(1.0)).child(theme::icon(cx, icon, 16.0, theme::color(cx, color))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_color(theme::color(cx, "fg-strong"))
                                .child(SharedString::from(diagnostic.message.clone())),
                        )
                        .child(
                            div()
                                .mt(px(6.0))
                                .flex()
                                .flex_wrap()
                                .gap(px(12.0))
                                .items_baseline()
                                .children(diagnostic.source.clone().map(|source| {
                                    div()
                                        .text_color(theme::color(cx, "fg-subtle"))
                                        .text_size(theme::length(cx, "fs-small"))
                                        .child(SharedString::from(source))
                                }))
                                .children(fix)
                                .children(more),
                        ),
                )
        });
        div().flex().flex_col().gap(px(8.0)).children(rows.collect::<Vec<_>>())
    }
}

impl Render for HoverView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.content {
            HoverContent::Empty => return div(),
            HoverContent::Declaration(DeclarationSummary::Code { code, .. }) => {
                let block = Block::Code { language: None, text: code.clone() };
                markup_element(std::slice::from_ref(&block), "hover", &self.on_link, cx).into_any_element()
            }
            HoverContent::Declaration(DeclarationSummary::Text(text)) => {
                div().child(SharedString::from(text.clone())).into_any_element()
            }
            HoverContent::Hover { diagnostics, fixes, blocks } => {
                let problems = (!diagnostics.is_empty()).then(|| self.diagnostics(diagnostics, fixes, cx));
                let docs = (!blocks.is_empty()).then(|| {
                    markup_element(blocks, "hover", &self.on_link, cx).when(problems.is_some(), |el| {
                        el.mt(px(10.0)).pt(px(10.0)).border_t_1().border_color(theme::color(cx, "border-popup"))
                    })
                });
                div()
                    .id("hover-scroll")
                    .max_h(px(380.0))
                    .overflow_y_scroll()
                    .px(px(14.0))
                    .py(px(10.0))
                    .children(problems)
                    .children(docs)
                    .into_any_element()
            }
        };
        let declaration = matches!(self.content, HoverContent::Declaration(_));
        keeps_focus(
            popup_surface(cx)
                .debug_selector(|| "hover-popup".to_owned())
                .max_w(px(580.0))
                .text_size(theme::length(cx, "fs-ui"))
                .when(declaration, |el| el.px(px(8.0)).py(px(4.0)))
                .child(body),
        )
    }
}

// ---------------------------------------------------------------------------
// Quick Documentation (F1)

/// An entry of the Quick Documentation history.
#[derive(Debug, Clone)]
pub struct DocEntry {
    pub markup: Markup,
    /// The language code blocks without an info string are in.
    pub language: Option<String>,
    /// Whether Edit Source can find the documented element's declaration (it is resolved on
    /// demand, through [`QuickDocEvent::EditSource`]).
    pub has_source: bool,
}

/// What the Quick Documentation popup asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickDocEvent {
    /// Edit Source (F4) of history entry `entry`: resolve the declaration (`lsp_goto` definition),
    /// close the popup and open it focused.
    EditSource { entry: usize },
    /// Show in Documentation Tool Window: the history moves to the tool window, the popup closes.
    MoveToWindow,
    /// A link was clicked. A source link without the primary modifier shows that symbol's
    /// documentation here ([`QuickDocView::push`]), else the owner closes the popup and opens it.
    Link { link: DocLink, primary: bool },
    /// Escape with the keyboard in the popup.
    Dismissed,
}

/// The Quick Documentation popup: the shown entry of its history, and a toolbar with Back,
/// Forward, Edit Source and Show in Documentation Tool Window. It leaves the keyboard to the
/// editor until the owner focuses it (F1 on the open popup); then ← / → walk the history, F4
/// edits the source and Escape closes. The owner draws it below the caret line at
/// [`crate::editor::view::Editor::position_of`] of the documented range's start, flipping above
/// when there is no room below.
pub struct QuickDocView {
    entries: Vec<DocEntry>,
    index: usize,
    blocks: Vec<Block>,
    focus: FocusHandle,
    on_link: LinkHandler,
    key_claim: u64,
}

impl EventEmitter<QuickDocEvent> for QuickDocView {}

impl QuickDocView {
    /// The popup showing entry `index` of `entries` (clamped).
    pub fn new(entries: Vec<DocEntry>, index: usize, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let on_link = link_handler(cx, |view: &mut Self, href, window, cx| view.link(href, window, cx));
        let mut view = Self { entries, index: 0, blocks: Vec::new(), focus, on_link, key_claim };
        view.show(index);
        view
    }

    pub fn entries(&self) -> &[DocEntry] {
        &self.entries
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// The text of the shown entry.
    pub fn text(&self) -> String {
        blocks_text(&self.blocks)
    }

    /// Whether Back, Forward and Edit Source can be used now.
    pub fn toolbar(&self) -> (bool, bool, bool) {
        let source = self.entries.get(self.index).is_some_and(|entry| entry.has_source);
        (self.index > 0, self.index + 1 < self.entries.len(), source)
    }

    fn show(&mut self, index: usize) {
        self.index = index.min(self.entries.len().saturating_sub(1));
        self.blocks = self.entries.get(self.index).map(|entry| markup_blocks(&entry.markup)).unwrap_or_default();
    }

    /// A link followed from the shown entry: `entry` shows next, and the entries after the shown
    /// one are dropped.
    pub fn push(&mut self, entry: DocEntry, cx: &mut Context<Self>) {
        self.entries.truncate(self.index + 1);
        self.entries.push(entry);
        self.show(self.entries.len() - 1);
        cx.notify();
    }

    /// ← (`-1`) / → (`1`) through the history; nothing past its ends.
    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let last = self.entries.len().saturating_sub(1);
        let index = self.index.saturating_add_signed(delta).min(last);
        if index != self.index {
            self.show(index);
            cx.notify();
        }
    }

    fn edit_source(&mut self, cx: &mut Context<Self>) {
        if self.toolbar().2 {
            cx.emit(QuickDocEvent::EditSource { entry: self.index });
        }
    }

    fn link(&mut self, href: &str, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = window.modifiers();
        let primary = if is_mac_host() { modifiers.platform } else { modifiers.control };
        let link = doc_link(href);
        if link != DocLink::Ignored {
            cx.emit(QuickDocEvent::Link { link, primary });
        }
    }

    /// The keys the popup handles while it has the keyboard; with a modifier they are not its.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = keystroke.modifiers;
        if m.platform || m.control || m.alt || m.shift {
            return false;
        }
        match keystroke.key.as_str() {
            "left" => self.step(-1, cx),
            "right" => self.step(1, cx),
            "f4" if self.toolbar().2 => self.edit_source(cx),
            "escape" => cx.emit(QuickDocEvent::Dismissed),
            _ => return false,
        }
        true
    }

    fn button(
        id: &'static str,
        icon: &'static str,
        title: &str,
        shortcut: Option<String>,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: fn(&mut Self, &mut Context<Self>),
    ) -> AnyElement {
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        let hover_bg = theme::color(cx, "bg-toolbar-hover");
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| el.hover(|el| el.bg(hover_bg)))
            .tooltip({
                let (title, keys) = (SharedString::from(title.to_owned()), shortcut.map(SharedString::from));
                move |window, cx| {
                    let tooltip = Tooltip::new(title.clone());
                    let tooltip = match &keys {
                        Some(keys) => tooltip.with_shortcut(keys.clone()),
                        None => tooltip,
                    };
                    tooltip.shown(window, cx)
                }
            })
            .child(theme::icon(cx, icon, 16.0, color))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _, _, cx| on_click(view, cx))))
            .into_any_element()
    }
}

impl Focusable for QuickDocView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for QuickDocView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (back, forward, source) = self.toolbar();
        let (back_title, forward_title) = (tx(cx, "Back"), tx(cx, "Forward"));
        let (source_title, window_title) = (tx(cx, "Edit Source"), tx(cx, "Show in Documentation Tool Window"));
        let window_keys = shortcut_label(cx, "code.quickDocumentation");
        let border = theme::color(cx, "border-popup");
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .py(px(2.0))
            .border_t_1()
            .border_color(border)
            .child(Self::button("doc-back", "arrow-left", &back_title, Some("←".into()), back, cx, |view, cx| {
                view.step(-1, cx)
            }))
            .child(Self::button(
                "doc-forward",
                "arrow-right",
                &forward_title,
                Some("→".into()),
                forward,
                cx,
                |view, cx| view.step(1, cx),
            ))
            .child(div().flex_1());
        if source {
            toolbar = toolbar.child(Self::button(
                "doc-edit-source",
                "edit",
                &source_title,
                Some("F4".into()),
                true,
                cx,
                Self::edit_source,
            ));
        }
        let toolbar = toolbar.child(Self::button(
            "doc-to-window",
            "open-in-tool-window",
            &window_title,
            window_keys,
            true,
            cx,
            |_, cx| cx.emit(QuickDocEvent::MoveToWindow),
        ));
        popup_surface(cx)
            .track_focus(&self.focus)
            .debug_selector(|| "quick-doc".to_owned())
            .w(px(560.0))
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                // A press inside keeps the keyboard where it is: the popup only takes it from F1.
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                div()
                    .id("quick-doc-body")
                    .max_h(px(380.0))
                    .overflow_y_scroll()
                    .px(px(14.0))
                    .py(px(10.0))
                    .child(markup_element(&self.blocks, "doc", &self.on_link, cx)),
            )
            .child(toolbar)
    }
}

// ---------------------------------------------------------------------------
// The hint

/// The one-line hint at the caret ([`super::hint::HintState`]): an error, "Waiting…", "No
/// suggestions". The owner draws it above the caret, at
/// [`crate::editor::view::Editor::position_of`] of the hint's `pos`, flipping below when there is
/// no room above.
#[derive(Default)]
pub struct HintView {
    hint: Option<(String, Tone)>,
}

impl HintView {
    /// Shows `hint` in the active locale, or nothing.
    pub fn set_hint(&mut self, hint: Option<&Hint>, cx: &mut Context<Self>) {
        let shown = hint.map(|hint| {
            let text = cx.try_global::<Localization>().map_or_else(|| hint.text.clone(), |l| hint.display(&l.0));
            (text, hint.tone)
        });
        if shown != self.hint {
            self.hint = shown;
            cx.notify();
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.hint.as_ref().map(|(text, _)| text.as_str())
    }

    pub fn tone(&self) -> Option<Tone> {
        self.hint.as_ref().map(|(_, tone)| *tone)
    }
}

impl Render for HintView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((text, tone)) = &self.hint else { return div() };
        let background = match tone {
            Tone::Info => "bg-popup",
            Tone::Error => "bg-feedback-error",
            Tone::Warning => "bg-feedback-warning",
        };
        keeps_focus(
            popup_surface(cx)
                .debug_selector(|| "editor-hint".to_owned())
                .bg(theme::color(cx, background))
                .max_w(px(480.0))
                .px(px(10.0))
                .py(px(6.0))
                .text_size(theme::length(cx, "fs-ui"))
                .text_color(theme::color(cx, "fg-strong"))
                .child(SharedString::from(text.clone())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::hover::{NavigationModifier, hover_popup};
    use super::super::markdown::SourceLink;
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext, point};
    use ide_lsp::types::Hover;
    use ropey::Rope;
    use std::cell::RefCell;

    type Log<E> = Rc<RefCell<Vec<E>>>;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn markdown(value: &str) -> Markup {
        Markup { kind: "markdown".into(), value: value.into() }
    }

    fn subscribe<V: EventEmitter<E> + 'static, E: Clone + 'static>(
        view: &Entity<V>,
        cx: &mut VisualTestContext,
    ) -> Log<E> {
        let log: Log<E> = Rc::default();
        let seen = Rc::clone(&log);
        cx.update(|_, cx| cx.subscribe(view, move |_, event: &E, _| seen.borrow_mut().push(event.clone())).detach());
        log
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn click_start(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.origin + point(px(4.0), bounds.size.height / 2.0), Modifiers::default());
        cx.run_until_parked();
    }

    fn problem(message: &str, severity: Severity) -> LintDiagnostic {
        LintDiagnostic {
            from: 6,
            to: 10,
            severity,
            message: message.into(),
            source: Some("ts(2322)".into()),
            mark_class: None,
        }
    }

    fn hovered(diagnostics: Vec<LintDiagnostic>, docs: Option<&str>) -> HoverPopup {
        let hover = docs.map(|value| Hover { contents: markdown(value), range: None });
        hover_popup(&NavigationModifier::new(true), &Rope::from_str("const value = 1"), 7, diagnostics, hover)
            .expect("there is something to show")
    }

    #[gpui_kit::test]
    async fn shows_the_problems_then_the_documentation_and_reports_their_links(cx: &mut TestAppContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(|_, cx| HoverView::new(cx));
        let events = subscribe::<_, HoverEvent>(&view, cx);
        let popup = hovered(vec![problem("Type 'string' is not assignable", Severity::Error)], Some("`value`: number"));
        view.update(cx, |view, cx| view.show(&popup, cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| view.lines()),
            ["error: Type 'string' is not assignable", "  ts(2322)", "  More actions…", "value: number"]
        );
        click(cx, "hover-more-0");
        assert_eq!(*events.borrow(), [HoverEvent::MoreActions { at: 6 }]);

        // The server named the preferred fix: its link shows and applies it.
        view.update(cx, |view, cx| view.set_quick_fix(0, "Convert to number", cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.lines()).contains(&"  fix: Convert to number".to_owned()));
        click(cx, "hover-fix-0");
        assert_eq!(events.borrow().last(), Some(&HoverEvent::QuickFix { diagnostic: 0 }));
    }

    #[gpui_kit::test]
    async fn a_hint_offers_no_more_actions(cx: &mut TestAppContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(|_, cx| HoverView::new(cx));
        let popup = hovered(vec![problem("Unused variable", Severity::Hint)], None);
        view.update(cx, |view, cx| view.show(&popup, cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["hint: Unused variable", "  ts(2322)"]);
        assert!(cx.debug_bounds("hover-more-0").is_none());
    }

    #[gpui_kit::test]
    async fn opens_documentation_links_as_the_model_reads_them(cx: &mut TestAppContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(|_, cx| HoverView::new(cx));
        let events = subscribe::<_, HoverEvent>(&view, cx);
        let popup = hovered(Vec::new(), Some("[Order](file:///p/Order.java#L12) is the type"));
        view.update(cx, |view, cx| view.show(&popup, cx));
        cx.run_until_parked();
        click_start(cx, "hover-text-0");
        let target = SourceLink { path: "/p/Order.java".into(), line: 11, col: 0 };
        assert_eq!(*events.borrow(), [HoverEvent::Link(DocLink::Source(target))]);
    }

    #[gpui_kit::test]
    async fn shows_the_declaration_of_the_navigation_link(cx: &mut TestAppContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(|_, cx| HoverView::new(cx));
        let summary = DeclarationSummary::Code { code: "class Order".into(), language: Some("java".into()) };
        view.update(cx, |view, cx| view.show_declaration(summary, cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["class Order"]);
        assert!(cx.debug_bounds("hover-popup").is_some());
        view.update(cx, |view, cx| view.clear(cx));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.is_visible()));
        assert!(cx.debug_bounds("hover-popup").is_none());
    }

    fn entry(text: &str, has_source: bool) -> DocEntry {
        DocEntry { markup: Markup { kind: "plaintext".into(), value: text.into() }, language: None, has_source }
    }

    fn open_doc(
        cx: &mut TestAppContext,
        entries: Vec<DocEntry>,
        index: usize,
    ) -> (Entity<QuickDocView>, Log<QuickDocEvent>, &mut VisualTestContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(move |_, cx| QuickDocView::new(entries, index, cx));
        let events = subscribe::<_, QuickDocEvent>(&view, cx);
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    #[gpui_kit::test]
    async fn history_helpers_drop_forward_entries_and_clamp_steps(cx: &mut TestAppContext) {
        let entries = vec![entry("a", false), entry("b", false), entry("c", false)];
        let (view, _, cx) = open_doc(cx, entries, 0);
        view.update(cx, |view, cx| view.push(entry("d", false), cx));
        let texts = |view: &QuickDocView| -> Vec<String> {
            view.entries().iter().map(|entry| entry.markup.value.clone()).collect()
        };
        assert_eq!(view.read_with(cx, |view, _| texts(view)), ["a", "d"]);
        assert_eq!(view.read_with(cx, |view, _| view.index()), 1);
        view.update(cx, |view, cx| view.step(1, cx));
        assert_eq!(view.read_with(cx, |view, _| view.index()), 1);
        view.update(cx, |view, cx| view.step(-1, cx));
        assert_eq!(view.read_with(cx, |view, _| view.index()), 0);
    }

    #[gpui_kit::test]
    async fn walks_the_popup_history_with_left_right_and_opens_the_source_with_f4(cx: &mut TestAppContext) {
        let (view, events, cx) = open_doc(cx, vec![entry("first", false), entry("second", true)], 1);
        assert!(view.read_with(cx, |view, _| view.text()).contains("second"));
        cx.simulate_keystrokes("left");
        assert!(view.read_with(cx, |view, _| view.text()).contains("first"));
        assert!(cx.debug_bounds("doc-edit-source").is_none());
        // F4 does nothing for an entry without a source.
        cx.simulate_keystrokes("f4");
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("right");
        assert!(view.read_with(cx, |view, _| view.text()).contains("second"));
        cx.simulate_keystrokes("f4");
        assert_eq!(*events.borrow(), [QuickDocEvent::EditSource { entry: 1 }]);
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&QuickDocEvent::Dismissed));
    }

    #[gpui_kit::test]
    async fn moves_the_popup_to_the_documentation_tool_window(cx: &mut TestAppContext) {
        let (_, events, cx) = open_doc(cx, vec![entry("docs", false)], 0);
        click(cx, "doc-to-window");
        assert_eq!(*events.borrow(), [QuickDocEvent::MoveToWindow]);
    }

    #[gpui_kit::test]
    async fn shows_the_hint_in_its_tone_and_hides_it(cx: &mut TestAppContext) {
        init(cx);
        let (view, cx) = cx.add_window_view(|_, _| HintView::default());
        let hint = Hint { pos: 0, text: "No suggestions".into(), tone: Tone::Error };
        view.update(cx, |view, cx| view.set_hint(Some(&hint), cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| (view.text().map(str::to_owned), view.tone())),
            (Some("No suggestions".to_owned()), Some(Tone::Error))
        );
        assert!(cx.debug_bounds("editor-hint").is_some());
        view.update(cx, |view, cx| view.set_hint(None, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("editor-hint").is_none());
    }
}
