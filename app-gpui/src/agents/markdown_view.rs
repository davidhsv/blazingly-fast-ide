//! Agent replies drawn from their Markdown blocks: paragraphs of styled runs (bold, italic, strike, inline
//! code, links), headings, lists (nested, ordered, and task items written `[ ]` / `[x]`), block quotes, code
//! blocks with their language and a Copy control, tables and rules.
//!
//! Images and Mermaid diagrams are slots: an image is a box with its alternative text, a diagram is its
//! source in a code block, still copyable. A click on a link or a Copy control is reported to the owner
//! through a [`MarkdownHandler`]; [`MarkdownView`] is a view of one message that turns them into
//! [`MarkdownEvent`]s and writes copied text to the clipboard.

use crate::agents::markdown::{Block, Image, Inline, LinkAction, Run, link_action, parse};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Div, EventEmitter, Font, FontStyle, FontWeight, InteractiveText,
    IntoElement, Render, SharedString, StrikethroughStyle, StyledText, TextRun, UnderlineStyle, Window, div, font,
    prelude::*, px,
};
use std::borrow::Cow;
use std::rc::Rc;

/// What a click in a message asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownClick {
    /// A link: what opening it does (an external URL, a file at a line, or nothing).
    Link(LinkAction),
    /// A Copy control of a code block or a diagram, with the text it copies.
    Copy(String),
}

/// Who a drawn message reports its clicks to.
pub type MarkdownHandler = Rc<dyn Fn(&MarkdownClick, &mut Window, &mut App)>;

fn family(cx: &App, token: &str, fallback: &str) -> Font {
    let name = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families(token).into_iter().next())
        .unwrap_or_else(|| fallback.to_owned());
    font(SharedString::from(name))
}

/// Heading sizes by level, in pixels; deeper levels use the body size.
fn heading_size(level: u8, cx: &App) -> gpui_kit::Pixels {
    match level {
        1 => px(20.0),
        2 => px(17.0),
        3 => px(15.0),
        _ => theme::length(cx, "fs-ui"),
    }
}

/// A list item written as a task (`[ ] todo`, `[x] done`): whether it is done, and its blocks without the
/// marker.
fn task_item(item: &[Block]) -> Option<(bool, Vec<Block>)> {
    let Some(Block::Paragraph(content)) = item.first() else { return None };
    let Some(Inline::Text(run)) = content.first() else { return None };
    let (done, rest) = if let Some(rest) = run.text.strip_prefix("[ ] ") {
        (false, rest)
    } else if let Some(rest) = run.text.strip_prefix("[x] ").or_else(|| run.text.strip_prefix("[X] ")) {
        (true, rest)
    } else {
        return None;
    };
    let mut content = content.clone();
    content[0] = Inline::Text(Run { text: rest.to_owned(), style: run.style.clone() });
    let mut blocks = item.to_vec();
    blocks[0] = Block::Paragraph(content);
    Some((done, blocks))
}

/// The marker of list item `index` and its blocks (a task item's without its `[ ]`).
fn item_marker<'a>(ordered: Option<u64>, index: usize, item: &'a [Block]) -> (Marker, Cow<'a, [Block]>) {
    match (task_item(item), ordered) {
        (Some((done, blocks)), _) => (Marker::Task(done), Cow::Owned(blocks)),
        (None, Some(start)) => (Marker::Number(start.saturating_add(index as u64)), Cow::Borrowed(item)),
        (None, None) => (Marker::Bullet, Cow::Borrowed(item)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Bullet,
    Number(u64),
    Task(bool),
}

impl Marker {
    fn text(self) -> String {
        match self {
            Self::Bullet => "-".to_owned(),
            Self::Number(n) => format!("{n}."),
            Self::Task(true) => "[x]".to_owned(),
            Self::Task(false) => "[ ]".to_owned(),
        }
    }
}

fn inline_text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text(run) => run.text.clone(),
            Inline::Image(image) => format!("[image: {}]", image.alt),
        })
        .collect()
}

/// What `blocks` draw, a line per paragraph, heading, code block or table row: `## Title`, `- item` (nested
/// items indented by two spaces), `3. item`, `[x] done`, `> quoted`, `[code rust] fn main() {}`,
/// `[diagram] graph TD`, `| a | b |`, `---`, and `[image: alt]` where an image stands.
pub fn block_lines(blocks: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    for block in blocks {
        lines_of(block, "", &mut out);
    }
    out
}

fn lines_of(block: &Block, indent: &str, out: &mut Vec<String>) {
    match block {
        Block::Paragraph(content) => out.push(format!("{indent}{}", inline_text(content))),
        Block::Heading { level, content } => {
            out.push(format!("{indent}{} {}", "#".repeat(usize::from(*level)), inline_text(content)));
        }
        Block::Code { language, text } => {
            let language = language.as_deref().map(|l| format!(" {l}")).unwrap_or_default();
            out.push(format!("{indent}[code{language}] {text}"));
        }
        Block::Mermaid { source } => out.push(format!("{indent}[diagram] {source}")),
        Block::Quote(inner) => {
            let indent = format!("{indent}> ");
            for block in inner {
                lines_of(block, &indent, out);
            }
        }
        Block::List { ordered, items } => {
            for (index, item) in items.iter().enumerate() {
                let (marker, blocks) = item_marker(*ordered, index, item);
                let mut inner = Vec::new();
                for block in blocks.iter() {
                    lines_of(block, "", &mut inner);
                }
                for (k, line) in inner.into_iter().enumerate() {
                    if k == 0 {
                        out.push(format!("{indent}{} {line}", marker.text()));
                    } else {
                        out.push(format!("{indent}  {line}"));
                    }
                }
            }
        }
        Block::Table { header, rows } => {
            let row = |cells: &[Vec<Inline>]| {
                let cells: Vec<String> = cells.iter().map(|cell| inline_text(cell)).collect();
                format!("{indent}| {} |", cells.join(" | "))
            };
            out.push(row(header));
            out.extend(rows.iter().map(|cells| row(cells)));
        }
        Block::Rule => out.push(format!("{indent}---")),
    }
}

/// Draws `blocks`. `prefix` tells this message's parts apart: the text of its paragraphs is selected as
/// `<prefix>-text-<n>`, its Copy controls as `<prefix>-copy-<n>` (in the order of
/// [`crate::agents::markdown::copy_texts`]) and its image slots as `<prefix>-image-<n>`. Relative file
/// links resolve against `cwd`.
pub fn markdown_element(blocks: &[Block], prefix: &str, cwd: &str, on_click: &MarkdownHandler, cx: &App) -> Div {
    let mut builder = Builder {
        prefix,
        cwd,
        on_click,
        ui: family(cx, "font-ui", ".SystemUIFont"),
        mono: family(cx, "font-mono", theme::fonts::BUNDLED_MONO_FAMILY),
        texts: 0,
        copies: 0,
        images: 0,
    };
    let mut children = Vec::with_capacity(blocks.len());
    for block in blocks {
        children.push(builder.block(block, cx));
    }
    div().flex().flex_col().gap(px(6.0)).min_w_0().children(children)
}

struct Builder<'a> {
    prefix: &'a str,
    cwd: &'a str,
    on_click: &'a MarkdownHandler,
    ui: Font,
    mono: Font,
    texts: usize,
    copies: usize,
    images: usize,
}

impl Builder<'_> {
    fn block(&mut self, block: &Block, cx: &App) -> AnyElement {
        match block {
            Block::Paragraph(content) => self.inline(content, None, cx),
            Block::Heading { level, content } => div()
                .text_size(heading_size(*level, cx))
                .child(self.inline(content, Some(FontWeight::BOLD), cx))
                .into_any_element(),
            Block::Code { language, text } => self.code(language.as_deref(), text, cx),
            Block::Mermaid { source } => self.code(Some("mermaid"), source, cx),
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
                let mut rows = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let (marker, blocks) = item_marker(*ordered, index, item);
                    let mut children = Vec::with_capacity(blocks.len());
                    for block in blocks.iter() {
                        children.push(self.block(block, cx));
                    }
                    rows.push(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(div().flex_none().min_w(px(16.0)).child(self.marker(marker, cx)))
                            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.0)).children(children)),
                    );
                }
                div().flex().flex_col().gap(px(4.0)).children(rows).into_any_element()
            }
            Block::Table { header, rows } => {
                let border = theme::color(cx, "border");
                let mut lines = Vec::with_capacity(rows.len() + 1);
                lines.push(self.table_row(header, true, cx));
                for cells in rows {
                    lines.push(self.table_row(cells, false, cx));
                }
                div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(border)
                    .rounded(theme::length(cx, "radius-small"))
                    .children(lines)
                    .into_any_element()
            }
            Block::Rule => div().h(px(1.0)).my(px(4.0)).bg(theme::color(cx, "border")).into_any_element(),
        }
    }

    fn marker(&self, marker: Marker, cx: &App) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        match marker {
            Marker::Bullet => div().text_color(muted).child("•").into_any_element(),
            Marker::Number(n) => div().text_color(muted).child(SharedString::from(format!("{n}."))).into_any_element(),
            Marker::Task(done) => div()
                .mt(px(2.0))
                .size(px(13.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .when(done, |el| el.child(theme::icon(cx, "check", 11.0, theme::color(cx, "fg"))))
                .into_any_element(),
        }
    }

    fn table_row(&mut self, cells: &[Vec<Inline>], header: bool, cx: &App) -> Div {
        let border = theme::color(cx, "border");
        let weight = header.then_some(FontWeight::BOLD);
        let mut children = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter().enumerate() {
            children.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(6.0))
                    .py(px(3.0))
                    .when(index > 0, |el| el.border_l_1().border_color(border))
                    .child(self.inline(cell, weight, cx)),
            );
        }
        div()
            .flex()
            .when(header, |el| el.bg(theme::color(cx, "bg-elev")))
            .when(!header, |el| el.border_t_1().border_color(border))
            .children(children)
    }

    /// Inline content: runs of text in one element, broken where an image stands.
    fn inline(&mut self, content: &[Inline], weight: Option<FontWeight>, cx: &App) -> AnyElement {
        let mut parts = Vec::new();
        let mut runs: Vec<&Run> = Vec::new();
        for inline in content {
            match inline {
                Inline::Text(run) => runs.push(run),
                Inline::Image(image) => {
                    if !runs.is_empty() {
                        parts.push(self.text(&std::mem::take(&mut runs), weight, cx));
                    }
                    parts.push(self.image(image, cx));
                }
            }
        }
        if !runs.is_empty() || parts.is_empty() {
            parts.push(self.text(&runs, weight, cx));
        }
        if parts.len() == 1 {
            return parts.remove(0);
        }
        div().flex().flex_col().gap(px(4.0)).children(parts).into_any_element()
    }

    fn text(&mut self, runs: &[&Run], weight: Option<FontWeight>, cx: &App) -> AnyElement {
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
                weight: if style.bold { FontWeight::BOLD } else { weight.unwrap_or(base.weight) },
                style: if style.italic { FontStyle::Italic } else { base.style },
                ..base.clone()
            };
            let link = style.link.is_some();
            text_runs.push(TextRun {
                len: run.text.len(),
                font,
                color: theme::color(cx, if link { "link" } else { "fg" }),
                background_color: style.code.then(|| theme::color(cx, "bg-elev")),
                underline: link.then(|| UnderlineStyle { thickness: px(1.0), color: None, wavy: false }),
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
            let on_click = Rc::clone(self.on_click);
            let cwd = self.cwd.to_owned();
            InteractiveText::new(SharedString::from(selector.clone()), styled)
                .on_click(ranges, move |index, window, cx| {
                    if let Some(href) = hrefs.get(index) {
                        on_click(&MarkdownClick::Link(link_action(href, &cwd)), window, cx);
                    }
                })
                .into_any_element()
        };
        div().debug_selector(move || selector).min_w_0().child(element).into_any_element()
    }

    fn image(&mut self, image: &Image, cx: &App) -> AnyElement {
        let n = self.images;
        self.images += 1;
        let selector = format!("{}-image-{n}", self.prefix);
        let label = if image.alt.is_empty() { image.source.clone() } else { image.alt.clone() };
        let muted = theme::color(cx, "fg-muted");
        div()
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .py(px(6.0))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .text_color(muted)
            .child(theme::icon(cx, "file-image", 14.0, muted))
            .child(div().min_w_0().child(SharedString::from(label)))
            .into_any_element()
    }

    /// A code block (a diagram's source too): its language and Copy control above the text, which keeps
    /// its lines and scrolls sideways.
    fn code(&mut self, language: Option<&str>, text: &str, cx: &App) -> AnyElement {
        let n = self.copies;
        self.copies += 1;
        let (border, muted) = (theme::color(cx, "border"), theme::color(cx, "fg-muted"));
        let copy_selector = format!("{}-copy-{n}", self.prefix);
        let on_click = Rc::clone(self.on_click);
        let copied = text.to_owned();
        let copy = div()
            .id(SharedString::from(copy_selector.clone()))
            .debug_selector(move || copy_selector)
            .size(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(theme::icon(cx, "copy", 14.0, muted))
            .on_click(move |_, window, cx| on_click(&MarkdownClick::Copy(copied.clone()), window, cx));
        let header = div()
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_between()
            .pl(px(8.0))
            .pr(px(2.0))
            .border_b_1()
            .border_color(border)
            .text_color(muted)
            .text_size(theme::length(cx, "fs-small"))
            .child(SharedString::from(language.unwrap_or_default().to_owned()))
            .child(copy);
        let shown = if text.is_empty() { " ".to_owned() } else { text.to_owned() };
        let run = TextRun {
            len: shown.len(),
            font: self.mono.clone(),
            color: theme::color(cx, "fg"),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let body = div()
            .id(SharedString::from(format!("{}-code-{n}", self.prefix)))
            .overflow_x_scroll()
            .whitespace_nowrap()
            .px(px(8.0))
            .py(px(6.0))
            .text_size(theme::length(cx, "fs-code"))
            .child(StyledText::new(shown).with_runs(vec![run]));
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .border_1()
            .border_color(border)
            .rounded(theme::length(cx, "radius-small"))
            .bg(theme::color(cx, "bg-editor"))
            .child(header)
            .child(body)
            .into_any_element()
    }
}

/// What a message view tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownEvent {
    /// A link was clicked: open what it names.
    Link(LinkAction),
    /// A Copy control put this text on the clipboard.
    Copied(String),
}

/// One message drawn from its Markdown text.
pub struct MarkdownView {
    blocks: Vec<Block>,
    prefix: String,
    cwd: String,
    handler: MarkdownHandler,
}

impl EventEmitter<MarkdownEvent> for MarkdownView {}

impl MarkdownView {
    /// A message whose parts are selected under `prefix` and whose relative links resolve against `cwd`.
    pub fn new(prefix: &str, cwd: &str, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        let handler: MarkdownHandler = Rc::new(move |click: &MarkdownClick, _: &mut Window, cx: &mut App| {
            let click = click.clone();
            let _ = weak.update(cx, |view: &mut Self, cx| view.clicked(click, cx));
        });
        Self { blocks: Vec::new(), prefix: prefix.to_owned(), cwd: cwd.to_owned(), handler }
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let blocks = parse(text);
        if blocks != self.blocks {
            self.blocks = blocks;
            cx.notify();
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// What the message draws, as [`block_lines`].
    pub fn lines(&self) -> Vec<String> {
        block_lines(&self.blocks)
    }

    fn clicked(&mut self, click: MarkdownClick, cx: &mut Context<Self>) {
        match click {
            MarkdownClick::Link(action) => cx.emit(MarkdownEvent::Link(action)),
            MarkdownClick::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                cx.emit(MarkdownEvent::Copied(text));
            }
        }
    }
}

impl Render for MarkdownView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        markdown_element(&self.blocks, &self.prefix, &self.cwd, &self.handler, cx)
            .size_full()
            .p(px(8.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::markdown::copy_texts;
    use gpui_kit::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, point};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<MarkdownEvent>>>;

    fn open<'a>(cx: &'a mut TestAppContext, text: &str) -> (Entity<MarkdownView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let text = text.to_owned();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = MarkdownView::new("md", "/repo", cx);
            view.set_text(&text, cx);
            view
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &MarkdownEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    /// Clicks the first characters of a text element, where a link that starts it is.
    fn click_start(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = bounds_of(cx, selector);
        cx.simulate_click(bounds.origin + point(px(4.0), bounds.size.height / 2.0), Modifiers::default());
        cx.run_until_parked();
    }

    fn lines(view: &Entity<MarkdownView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    #[gpui_kit::test]
    async fn keeps_one_copy_control_outside_each_code_block(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "```rust\nfn one() {}\n```\n\nText\n\n```\ntwo\n```");
        assert_eq!(lines(&view, cx), ["[code rust] fn one() {}", "Text", "[code] two"]);
        assert!(cx.debug_bounds("md-copy-0").is_some() && cx.debug_bounds("md-copy-1").is_some());
        assert!(cx.debug_bounds("md-copy-2").is_none());
        // Each control belongs to its own block: the first one ends above the paragraph between the blocks.
        assert!(bounds_of(cx, "md-copy-0").bottom() <= bounds_of(cx, "md-text-0").top());
        assert!(bounds_of(cx, "md-text-0").bottom() <= bounds_of(cx, "md-copy-1").top());
        click(cx, "md-copy-1");
        assert_eq!(*events.borrow(), [MarkdownEvent::Copied("two".to_owned())]);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("two"));
        let copies: Vec<String> =
            view.read_with(cx, |view, _| copy_texts(view.blocks()).into_iter().map(str::to_owned).collect());
        assert_eq!(copies, ["fn one() {}", "two"]);
    }

    #[gpui_kit::test]
    async fn keeps_web_links_external(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "[docs](https://example.com/docs) for more");
        click_start(cx, "md-text-0");
        let external = LinkAction::External("https://example.com/docs".to_owned());
        assert_eq!(*events.borrow(), [MarkdownEvent::Link(external)]);
    }

    #[gpui_kit::test]
    async fn opens_file_links_in_the_editor_at_their_line(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "[main](/repo/main.rs#L7) changed");
        click_start(cx, "md-text-0");
        let expected = link_action("/repo/main.rs#L7", "/repo");
        assert!(matches!(expected, LinkAction::File(_)), "{expected:?}");
        assert_eq!(*events.borrow(), [MarkdownEvent::Link(expected)]);
    }

    #[gpui_kit::test]
    async fn text_outside_a_link_opens_nothing(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "plain words before [docs](https://example.com)");
        click_start(cx, "md-text-0");
        assert!(events.borrow().is_empty());
    }

    #[gpui_kit::test]
    async fn renders_the_fence_as_a_placeholder_holding_the_source(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "```mermaid\ngraph TD\n  A --> B\n```");
        assert_eq!(lines(&view, cx), ["[diagram] graph TD\n  A --> B"]);
        // The source stays copyable on the block.
        click(cx, "md-copy-0");
        assert_eq!(*events.borrow(), [MarkdownEvent::Copied("graph TD\n  A --> B".to_owned())]);
    }

    #[gpui_kit::test]
    async fn draws_headings_lists_quotes_tables_and_rules(cx: &mut TestAppContext) {
        let text = "## Plan\n\n1. first\n2. second\n   - nested\n\n> quoted\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---";
        let (view, _, cx) = open(cx, text);
        assert_eq!(
            lines(&view, cx),
            ["## Plan", "1. first", "2. second", "  - nested", "> quoted", "| a | b |", "| 1 | 2 |", "---"]
        );
        // One text element per paragraph, heading and table cell, top to bottom.
        let (heading, first) = (bounds_of(cx, "md-text-0"), bounds_of(cx, "md-text-1"));
        assert!(heading.bottom() <= first.top());
    }

    #[gpui_kit::test]
    async fn draws_task_items_with_a_checkbox_instead_of_their_marker(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "- [x] done\n- [ ] todo");
        assert_eq!(lines(&view, cx), ["[x] done", "[ ] todo"]);
    }

    #[gpui_kit::test]
    async fn shows_an_image_as_a_slot_with_its_alternative_text(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "Before ![a chart](chart.png) after");
        assert_eq!(lines(&view, cx), ["Before [image: a chart] after"]);
        let (before, image) = (bounds_of(cx, "md-text-0"), bounds_of(cx, "md-image-0"));
        let after = bounds_of(cx, "md-text-1");
        assert!(before.bottom() <= image.top() && image.bottom() <= after.top());
    }

    #[test]
    fn numbers_ordered_lists_from_their_start() {
        assert_eq!(block_lines(&parse("3. three\n4. four")), ["3. three", "4. four"]);
    }
}
