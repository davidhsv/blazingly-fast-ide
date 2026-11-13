//! The Markdown preview: the blocks of [`super::render`] drawn as a scrolling document on the
//! editor background, in the preview font size plus the editor's font zoom.
//!
//! It draws headings, paragraphs of styled runs (bold, italic, strike-through, code, links),
//! nested and task lists, quotes, code blocks with their language (coloured by an optional
//! highlighter), tables, rules, the front matter as a table, images (decoded from the `data:`
//! URLs of the shared [`PreviewImages`], a placeholder with the alternative text until then or
//! when blocked), Mermaid fences as their labelled source, and the HTML containers READMEs use:
//! `<details>` that open and close on their `<summary>` ([`DetailsToggles`]), tables, lists,
//! headings and the `align` attribute.
//!
//! Each top-level block is a direct child of the scrolling element, so the drawn layout gives the
//! blocks' offsets ([`BlockTop`]) that [`PreviewScroll`] maps source lines onto. The view applies
//! its scroll effects after the frame that laid the blocks out and reports what the owner must
//! do as [`MarkdownViewEvent`]s: links, image reads, the line on screen, its scroll offset.

use super::hex::decode_base64;
use super::preview::{
    DetailsToggles, ImageView, Layout, LinkClick, LiveMarkdown, PreviewEffect, PreviewImages, PreviewScroll, link_click,
};
use super::render::{
    BlockTop, Element, FieldValue, FrontMatterRow, Image, Inline, ListItem, Node, PreviewBlock, all_nodes, anchor_line,
    details_written_open, render_markdown,
};
use crate::editor::buffer::byte_of_col;
use crate::editor_lsp::markdown::{Run, percent_decode};
use crate::platform::i18n::I18n;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::tooltip::tooltip;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla,
    Image as GpuiImage, ImageFormat, InteractiveText, IntoElement, MouseButton, MouseDownEvent, Pixels, Render,
    ScrollHandle, SharedString, StrikethroughStyle, StyledText, Subscription, TextRun, WeakEntity, Window, canvas, div,
    font, img, point, prelude::*, px, relative,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// What the preview asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownViewEvent {
    /// A link to a web page or a mail address was clicked.
    OpenUrl(String),
    /// A link to another file was clicked: open it, with the caret on the 0-based `line` if any.
    OpenFile { path: String, line: Option<u32> },
    /// Read this local image as a `data:` URL (`ide_host::ops::bytes::fs_read_image`) and hand the
    /// answer to the shared [`PreviewImages::read`], then notify its entity.
    ReadImage(String),
    /// Preview only: the source line on screen (Structure's Autoscroll from Source); `None` once
    /// the editor is back or the preview is gone.
    ShownLine(Option<u32>),
    /// The preview scrolled to this offset ([`super::preview::LiveSources::preview_scrolled`]).
    Scrolled(f64),
}

/// A coloured span of a code block, in UTF-16 code units of its text.
#[derive(Clone, Debug, PartialEq)]
pub struct HighlightSpan {
    pub from: u32,
    pub to: u32,
    pub color: Hsla,
}

/// Colours the text of a code block written in a language (`None` when the fence names none).
pub type Highlighter = Rc<dyn Fn(&str, Option<&str>, &App) -> Vec<HighlightSpan>>;

/// A `data:` URL as an image GPUI draws: base64 or percent-encoded, of a format GPUI decodes.
fn decode_data_url(url: &str) -> Option<Arc<GpuiImage>> {
    let rest = url.get(..5).filter(|scheme| scheme.eq_ignore_ascii_case("data:")).map(|_| &url[5..])?;
    let (meta, payload) = rest.split_once(',')?;
    let mut parts = meta.split(';');
    let mime = parts.next()?.trim().to_ascii_lowercase();
    let base64 = parts.any(|part| part.trim().eq_ignore_ascii_case("base64"));
    let bytes = if base64 {
        decode_base64(&payload.chars().filter(|c| !c.is_whitespace()).collect::<String>())?
    } else {
        percent_decode(payload).into_bytes()
    };
    let format = ImageFormat::from_mime_type(&mime)?;
    Some(Arc::new(GpuiImage::from_bytes(format, bytes)))
}

/// Every image source of the preview, in document order.
fn image_sources(blocks: &[PreviewBlock]) -> Vec<String> {
    fn inline(content: &[Inline], out: &mut Vec<String>) {
        for item in content {
            if let Inline::Image(image) = item {
                out.push(image.source.clone());
            }
        }
    }
    let mut out = Vec::new();
    for node in all_nodes(blocks) {
        match node {
            Node::Image(image) => out.push(image.source.clone()),
            Node::Paragraph(content) | Node::Heading { content, .. } => inline(content, &mut out),
            Node::Table { header, rows } => {
                for cell in header.iter().chain(rows.iter().flatten()) {
                    inline(cell, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// How many `<details>` `nodes` hold, nested ones included.
fn count_details(nodes: &[Node]) -> usize {
    let block = PreviewBlock { line: 0, nodes: nodes.to_vec() };
    details_written_open(std::slice::from_ref(&block)).len()
}

/// A length in px from an HTML size attribute (`120`, `120px`); `None` for percentages and junk.
fn attribute_px(value: Option<&str>) -> Option<f32> {
    let value = value?.trim();
    let number = value.strip_suffix("px").unwrap_or(value).trim();
    number.parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0)
}

/// The text alignment an `align` attribute (or `<center>`) asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Start,
    Center,
    End,
}

impl Align {
    fn of(element: &Element) -> Option<Align> {
        if element.tag == "center" {
            return Some(Align::Center);
        }
        match element.attribute("align")?.trim().to_ascii_lowercase().as_str() {
            "center" | "middle" => Some(Align::Center),
            "right" => Some(Align::End),
            "left" | "justify" => Some(Align::Start),
            _ => None,
        }
    }

    fn apply(self, el: Div) -> Div {
        match self {
            Align::Start => el,
            Align::Center => el.text_center().items_center(),
            Align::End => el.text_right().items_end(),
        }
    }

    fn justify(self, el: Div) -> Div {
        match self {
            Align::Start => el,
            Align::Center => el.justify_center(),
            Align::End => el.justify_end(),
        }
    }
}

pub struct MarkdownView {
    path: String,
    text: String,
    blocks: Rc<Vec<PreviewBlock>>,
    written_open: Vec<bool>,
    details: DetailsToggles,
    images: Entity<PreviewImages>,
    /// The decoded images, by `data:` URL; `None` for one GPUI cannot draw.
    decoded: HashMap<String, Option<Arc<GpuiImage>>>,
    /// The preview font size in px, before the editor's font zoom.
    font_size: f64,
    font_zoom: f32,
    highlighter: Option<Highlighter>,
    scroll: ScrollHandle,
    /// Bumped when the blocks change; the frame that lays them out records it in `laid`.
    epoch: u64,
    laid: Rc<Cell<u64>>,
    /// The scrolling of the preview while shown, and a showing waiting for its first layout
    /// (`Some(sync)`).
    preview_scroll: Option<PreviewScroll>,
    pending_mount: Option<bool>,
    /// The editor's latest published state, and whether it waits to be applied after layout.
    live: Option<LiveMarkdown>,
    live_pending: bool,
    reported_top: f64,
    shown_line: Option<u32>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MarkdownViewEvent> for MarkdownView {}

impl MarkdownView {
    /// The preview of the Markdown file `path`, its local images read through the shared `images`.
    pub fn new(path: String, images: Entity<PreviewImages>, font_size: f64, cx: &mut Context<Self>) -> Self {
        let mut details = DetailsToggles::default();
        details.show(&path);
        let subscriptions = vec![cx.observe(&images, |view, _, cx| {
            view.refresh_images(cx);
            cx.notify();
        })];
        Self {
            path,
            text: String::new(),
            blocks: Rc::default(),
            written_open: Vec::new(),
            details,
            images,
            decoded: HashMap::new(),
            font_size,
            font_zoom: 0.0,
            highlighter: None,
            scroll: ScrollHandle::new(),
            epoch: 0,
            laid: Rc::default(),
            preview_scroll: None,
            pending_mount: None,
            live: None,
            live_pending: false,
            reported_top: 0.0,
            shown_line: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Colours code blocks from now on.
    pub fn set_highlighter(&mut self, highlighter: Option<Highlighter>, cx: &mut Context<Self>) {
        self.highlighter = highlighter;
        cx.notify();
    }

    /// Languages & Frameworks › Markdown's preview font size ([`super::preview::markdown_preview_font_size`]).
    pub fn set_font_size(&mut self, font_size: f64, cx: &mut Context<Self>) {
        self.font_size = font_size;
        cx.notify();
    }

    /// Renders `text` again; the `<details>` the reader toggled keep their state.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text == self.text && !self.blocks.is_empty() {
            return;
        }
        self.text = text.to_owned();
        self.blocks = Rc::new(render_markdown(text));
        self.written_open = details_written_open(&self.blocks);
        self.epoch += 1;
        self.refresh_images(cx);
        cx.notify();
    }

    /// The editor published a new state: text, scroll position, caret move or font zoom. What
    /// depends on the layout is applied once the frame has laid the new text out.
    pub fn update_live(&mut self, live: &LiveMarkdown, cx: &mut Context<Self>) {
        // Only a new text, an editor scroll or a caret move moves the preview: its own scrolling,
        // published back as `preview_top`, must not snap it back to the editor.
        let moved = live.text != self.text
            || self
                .live
                .as_ref()
                .is_none_or(|old| old.scroll_seq != live.scroll_seq || old.caret_seq != live.caret_seq);
        self.set_text(&live.text, cx);
        self.font_zoom = live.font_zoom;
        self.live = Some(live.clone());
        self.live_pending |= moved;
        cx.notify();
    }

    /// The preview is shown: side by side with the editor (`sync`, following its scrolling) or on
    /// its own (restoring its own offset and reporting the line on screen).
    pub fn show(&mut self, sync: bool, cx: &mut Context<Self>) {
        if self.preview_scroll.is_some() {
            self.set_sync(sync, cx);
        } else {
            self.pending_mount = Some(sync);
            cx.notify();
        }
    }

    /// Switched between Editor and Preview and Preview only while shown.
    pub fn set_sync(&mut self, sync: bool, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_mount.as_mut() {
            *pending = sync;
            return;
        }
        let top = self.scroll_top();
        let (blocks, height) = self.layout();
        let layout = Layout { blocks: &blocks, height };
        let Some(scroll) = self.preview_scroll.as_mut() else { return };
        let mut effects = scroll.set_sync(sync, layout, top);
        if sync && let Some(live) = self.live.as_ref() {
            effects.extend(scroll.editor_scrolled(live, layout));
        }
        self.apply(effects, cx);
    }

    /// The preview goes away: Preview only stops reporting the line on screen.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.pending_mount = None;
        if let Some(effect) = self.preview_scroll.take().and_then(PreviewScroll::unmount) {
            self.apply(vec![effect], cx);
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn blocks(&self) -> &[PreviewBlock] {
        &self.blocks
    }

    /// Whether each `<details>` shows open, in document order.
    pub fn details_open(&self) -> Vec<bool> {
        self.details.apply(&self.written_open)
    }

    /// The reader clicked the summary of the `index`-th `<details>`.
    pub fn toggle_details(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(open) = self.details_open().get(index).copied() {
            self.details.toggled(index, open);
            cx.notify();
        }
    }

    /// The text size in px: the preview font size plus the editor's font zoom.
    pub fn font_size(&self) -> f32 {
        self.font_size as f32 + self.font_zoom
    }

    /// The scroll offset from the top of the document.
    pub fn scroll_top(&self) -> f64 {
        f64::from(f32::from(-self.scroll.offset().y))
    }

    /// The line Preview only reports as on screen.
    pub fn shown_line(&self) -> Option<u32> {
        self.shown_line
    }

    /// How the image of `source` shows.
    pub fn image_view(&self, source: &str, cx: &App) -> ImageView {
        let fallback = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&fallback, |localization| &localization.0);
        self.images.read(cx).view(&self.path, source, i18n)
    }

    /// The offsets of the drawn blocks and the document's height, from the last frame.
    pub fn layout(&self) -> (Vec<BlockTop>, f64) {
        let bounds = self.scroll.bounds();
        let tops = self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                let item = self.scroll.bounds_for_item(index)?;
                Some(BlockTop { line: f64::from(block.line), top: f64::from(f32::from(item.top() - bounds.top())) })
            })
            .collect();
        let height = f64::from(f32::from(self.scroll.max_offset().y + bounds.size.height));
        (tops, height)
    }

    // ---------------------------------------------------------------------------------------------
    // Scrolling

    fn set_scroll_top(&mut self, top: f64) {
        let max = f64::from(f32::from(self.scroll.max_offset().y));
        let top = top.clamp(0.0, max.max(0.0));
        let offset = self.scroll.offset();
        self.scroll.set_offset(point(offset.x, px(-(top as f32))));
    }

    fn apply(&mut self, effects: Vec<PreviewEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                PreviewEffect::ScrollTo(top) => {
                    self.set_scroll_top(top);
                    cx.notify();
                }
                PreviewEffect::ShownLine(line) => {
                    self.shown_line = line;
                    cx.emit(MarkdownViewEvent::ShownLine(line));
                }
            }
        }
    }

    /// Runs after each frame: reports a scroll the frame shows, then applies what waited for the
    /// blocks to be laid out (showing the preview, following the editor).
    fn after_layout(&mut self, cx: &mut Context<Self>) {
        if self.laid.get() != self.epoch {
            return;
        }
        let (blocks, height) = self.layout();
        let layout = Layout { blocks: &blocks, height };
        let mut effects = Vec::new();
        let top = self.scroll_top();
        if (top - self.reported_top).abs() >= 0.5 {
            self.reported_top = top;
            cx.emit(MarkdownViewEvent::Scrolled(top));
            if let Some(scroll) = self.preview_scroll.as_mut() {
                effects.extend(scroll.scrolled(layout, top));
            }
        }
        if let Some(sync) = self.pending_mount.take() {
            let (scroll, mounted) = PreviewScroll::mount(sync, self.live.as_ref(), layout);
            self.preview_scroll = Some(scroll);
            effects.extend(mounted);
            self.live_pending = true;
        }
        if self.live_pending
            && let (Some(scroll), Some(live)) = (self.preview_scroll.as_mut(), self.live.as_ref())
        {
            self.live_pending = false;
            effects.extend(scroll.editor_scrolled(live, layout));
            effects.extend(scroll.caret_moved(live, layout));
        }
        self.apply(effects, cx);
    }

    fn link_clicked(&mut self, href: &str, cx: &mut Context<Self>) {
        match link_click(&self.path, href) {
            Some(LinkClick::OpenExternal(url)) => cx.emit(MarkdownViewEvent::OpenUrl(url)),
            Some(LinkClick::OpenFile { path, line }) => cx.emit(MarkdownViewEvent::OpenFile { path, line }),
            Some(LinkClick::ScrollToAnchor(id)) => self.scroll_to_anchor(&id, cx),
            None => {}
        }
    }

    /// Scrolls the block holding the heading `#id` to the top.
    pub fn scroll_to_anchor(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(line) = anchor_line(&self.blocks, id) else { return };
        if let Some(index) = self.blocks.iter().position(|block| block.line == line) {
            self.scroll.scroll_to_top_of_item(index);
            cx.notify();
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Images

    /// Asks for the local images the preview shows and decodes the ones read.
    fn refresh_images(&mut self, cx: &mut Context<Self>) {
        let sources = image_sources(&self.blocks);
        let path = self.path.clone();
        let requests: Vec<String> = self
            .images
            .update(cx, |images, _| sources.iter().filter_map(|source| images.request(&path, source)).collect());
        for request in requests {
            cx.emit(MarkdownViewEvent::ReadImage(request));
        }
        let views: Vec<ImageView> = {
            let fallback = I18n::default();
            let i18n = cx.try_global::<Localization>().map_or(&fallback, |localization| &localization.0);
            let images = self.images.read(cx);
            sources.iter().map(|source| images.view(&path, source, i18n)).collect()
        };
        for view in views {
            if let ImageView::Shown(url) = view
                && !self.decoded.contains_key(&url)
            {
                let image = decode_data_url(&url);
                self.decoded.insert(url, image);
            }
        }
    }
}

impl Focusable for MarkdownView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Builds the elements of the preview, top to bottom.
struct Builder<'a> {
    view: &'a MarkdownView,
    weak: WeakEntity<MarkdownView>,
    cx: &'a App,
    ui: Font,
    mono: Font,
    /// The text size in px.
    base: f32,
    /// Whether each `<details>` shows open, and how many were met so far.
    open: Vec<bool>,
    details: usize,
    texts: usize,
    align: Align,
    first: bool,
}

/// How a run of inline content looks beyond its own styles.
#[derive(Clone, Copy)]
struct TextLook {
    bold: bool,
    color: &'static str,
}

const BODY: TextLook = TextLook { bold: false, color: "fg-editor" };

impl Builder<'_> {
    fn em(&self, em: f32) -> Pixels {
        px(self.base * em)
    }

    fn color(&self, token: &str) -> Hsla {
        theme::color(self.cx, token)
    }

    fn tx(&self, text: &str) -> String {
        self.cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |l| l.0.tx(text))
    }

    fn nodes(&mut self, nodes: &[Node]) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node)).collect()
    }

    fn node(&mut self, node: &Node) -> AnyElement {
        let first = std::mem::replace(&mut self.first, false);
        match node {
            Node::FrontMatter(rows) => self.front_matter(rows),
            Node::Paragraph(content) => div().mb(self.em(1.0)).child(self.inline(content, BODY)).into_any_element(),
            Node::Heading { level, content, .. } => self.heading(*level, first, |builder| {
                vec![builder.inline(content, TextLook { bold: true, color: "fg-editor" })]
            }),
            Node::Code { language, text } => self.code(language.as_deref(), text, false),
            Node::Mermaid { source } => self.code(Some("mermaid"), source, true),
            Node::Quote(inner) => {
                let children = self.nodes(inner);
                div()
                    .mb(self.em(1.0))
                    .px(self.em(1.0))
                    .border_l_3()
                    .border_color(self.color("border-strong"))
                    .opacity(0.8)
                    .flex()
                    .flex_col()
                    .children(children)
                    .into_any_element()
            }
            Node::List { ordered, items } => self.list(*ordered, items),
            Node::Table { header, rows } => self.table(header, rows),
            Node::Rule => self.rule(),
            Node::Image(image) => {
                let shown = self.image(image);
                self.align.justify(div().mb(self.em(1.0)).flex()).child(shown).into_any_element()
            }
            Node::Element(element) => self.element(element, first),
        }
    }

    fn heading(&mut self, level: u8, first: bool, content: impl FnOnce(&mut Self) -> Vec<AnyElement>) -> AnyElement {
        let size = match level {
            1 => 1.9,
            2 => 1.5,
            3 => 1.25,
            _ => 1.0,
        };
        let top = if first { px(0.0) } else { self.em(1.2 * size) };
        let base = self.base;
        self.base = base * size;
        let children = content(self);
        let el = div()
            .debug_selector(move || format!("markdown-h{level}"))
            .mt(top)
            .mb(self.em(0.6))
            .text_size(px(self.base))
            .line_height(relative(1.25))
            .font_weight(FontWeight::BOLD)
            .when(level <= 2, |el| el.pb(self.em(0.3)).border_b_1().border_color(self.color("border-strong")))
            .children(children);
        self.base = base;
        self.align.apply(el.flex().flex_col()).into_any_element()
    }

    /// Inline content: one text element, or text and images wrapped in a row.
    fn inline(&mut self, content: &[Inline], look: TextLook) -> AnyElement {
        if !content.iter().any(|inline| matches!(inline, Inline::Image(_))) {
            let runs: Vec<&Run> = content
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text(run) => Some(run),
                    Inline::Image(_) => None,
                })
                .collect();
            return self.text(&runs, look);
        }
        let mut children = Vec::new();
        let mut runs: Vec<&Run> = Vec::new();
        for inline in content {
            match inline {
                Inline::Text(run) => runs.push(run),
                Inline::Image(image) => {
                    if runs.iter().any(|run| !run.text.trim().is_empty()) {
                        children.push(self.text(&runs, look));
                    }
                    runs.clear();
                    children.push(self.image(image));
                }
            }
        }
        if runs.iter().any(|run| !run.text.trim().is_empty()) {
            children.push(self.text(&runs, look));
        }
        self.align.justify(div().flex().flex_wrap().items_end().gap(px(4.0))).children(children).into_any_element()
    }

    /// Runs of text in one element; links report their target when clicked.
    fn text(&mut self, runs: &[&Run], look: TextLook) -> AnyElement {
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
                weight: if style.bold || look.bold { FontWeight::BOLD } else { base.weight },
                style: if style.italic { FontStyle::Italic } else { base.style },
                ..base.clone()
            };
            let color = if style.link.is_some() { self.color("link") } else { self.color(look.color) };
            text_runs.push(TextRun {
                len: run.text.len(),
                font,
                color,
                background_color: style.code.then(|| self.color("bg-editor-inline")),
                underline: None,
                strikethrough: style.strike.then_some(StrikethroughStyle { thickness: px(1.0), color: None }),
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
        let selector = format!("markdown-text-{n}");
        let element = if hrefs.is_empty() {
            styled.into_any_element()
        } else {
            let weak = self.weak.clone();
            InteractiveText::new(SharedString::from(selector.clone()), styled)
                .on_click(ranges, move |index, _, cx| {
                    if let Some(href) = hrefs.get(index) {
                        let _ = weak.update(cx, |view, cx| view.link_clicked(href, cx));
                    }
                })
                .into_any_element()
        };
        div().debug_selector(move || selector).min_w_0().child(element).into_any_element()
    }

    /// A code block: its text in the code font (coloured by the highlighter when there is one),
    /// its language in the corner; a Mermaid diagram shows its source under its name.
    fn code(&mut self, language: Option<&str>, source: &str, mermaid: bool) -> AnyElement {
        let n = self.texts;
        self.texts += 1;
        let code = source.trim_end_matches('\n');
        let shown = if code.is_empty() { " ".to_owned() } else { code.to_owned() };
        let fg = self.color("fg-editor");
        let mut runs = Vec::new();
        let mut spans = match (&self.view.highlighter, mermaid) {
            (Some(highlighter), false) => highlighter(code, language, self.cx),
            _ => Vec::new(),
        };
        let mut at = 0;
        spans.sort_by_key(|span| span.from);
        for span in spans {
            let (from, to) = (byte_of_col(&shown, span.from).max(at), byte_of_col(&shown, span.to));
            if to <= from {
                continue;
            }
            if from > at {
                runs.push(self.code_run(from - at, fg));
            }
            runs.push(self.code_run(to - from, span.color));
            at = to;
        }
        if at < shown.len() {
            runs.push(self.code_run(shown.len() - at, fg));
        }
        let label = if mermaid { Some(self.tx("Mermaid")) } else { language.map(str::to_owned) };
        let selector = format!("markdown-code-{n}");
        div()
            .debug_selector(move || selector)
            .relative()
            .mb(self.em(1.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(theme::length(self.cx, "radius-control"))
            .bg(self.color("bg-editor-inline"))
            .text_size(self.em(0.93))
            .line_height(relative(1.45))
            .text_left()
            .overflow_hidden()
            .children(label.map(|label| {
                div()
                    .absolute()
                    .top(px(4.0))
                    .right(px(8.0))
                    .text_size(theme::length(self.cx, "fs-small"))
                    .text_color(self.color("fg-faint"))
                    .child(SharedString::from(label))
            }))
            .child(StyledText::new(shown).with_runs(runs))
            .into_any_element()
    }

    fn code_run(&self, len: usize, color: Hsla) -> TextRun {
        TextRun { len, font: self.mono.clone(), color, background_color: None, underline: None, strikethrough: None }
    }

    fn list(&mut self, ordered: Option<u64>, items: &[ListItem]) -> AnyElement {
        let mut rows = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let marker = match (item.checked, ordered) {
                (Some(checked), _) => self.checkbox(checked),
                (None, Some(start)) => self.marker(format!("{}.", start.saturating_add(index as u64))),
                (None, None) => self.marker("•".to_owned()),
            };
            let children = self.list_item(&item.content);
            rows.push(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(marker)
                    .child(div().flex_1().min_w_0().flex().flex_col().children(children)),
            );
        }
        div().mb(self.em(1.0)).pl(self.em(1.2)).flex().flex_col().children(rows).into_any_element()
    }

    /// An item's content: its paragraphs and nested lists without their own bottom margin.
    fn list_item(&mut self, content: &[Node]) -> Vec<AnyElement> {
        content
            .iter()
            .map(|node| match node {
                Node::Paragraph(inline) => self.inline(inline, BODY),
                other => self.node(other),
            })
            .collect()
    }

    fn marker(&self, text: String) -> AnyElement {
        div()
            .flex_none()
            .min_w(self.em(0.8))
            .text_color(self.color("fg-secondary"))
            .child(SharedString::from(text))
            .into_any_element()
    }

    fn checkbox(&self, checked: bool) -> AnyElement {
        let size = self.em(0.9);
        div()
            .flex_none()
            .mt(self.em(0.3))
            .size(size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(3.0))
            .border_1()
            .border_color(self.color("border-strong"))
            .when(checked, |el| {
                let tick = theme::icon(self.cx, "check", self.base * 0.7, self.color("fg-strong"));
                el.bg(self.color("accent")).child(tick)
            })
            .into_any_element()
    }

    fn table(&mut self, header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> AnyElement {
        let mut lines = Vec::with_capacity(rows.len() + 1);
        lines.push(self.table_row(header, true));
        for cells in rows {
            lines.push(self.table_row(cells, false));
        }
        div().mb(self.em(1.0)).flex().flex_col().children(lines).into_any_element()
    }

    fn table_row(&mut self, cells: &[Vec<Inline>], header: bool) -> AnyElement {
        let border = self.color("border-strong");
        let mut children = Vec::with_capacity(cells.len());
        for cell in cells {
            let content = self.inline(cell, TextLook { bold: header, color: "fg-editor" });
            children.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(10.0))
                    .py(px(4.0))
                    .border_1()
                    .border_color(border)
                    .when(header, |el| el.bg(self.color("bg-editor-inline")))
                    .child(content),
            );
        }
        div().flex().children(children).into_any_element()
    }

    fn front_matter(&mut self, rows: &[FrontMatterRow]) -> AnyElement {
        let border = self.color("border-strong");
        let cell = || div().px(px(10.0)).py(px(4.0)).border_1().border_color(border);
        let lines: Vec<Div> = rows
            .iter()
            .map(|row| {
                let value = match &row.value {
                    FieldValue::Text(text) => div().child(SharedString::from(text.clone())),
                    FieldValue::Nested(text) => div()
                        .font_family(self.mono.family.clone())
                        .children(text.split('\n').map(|line| div().child(SharedString::from(line.to_owned())))),
                };
                div()
                    .flex()
                    .child(
                        cell()
                            .w(self.em(10.0))
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .bg(self.color("bg-editor-inline"))
                            .child(SharedString::from(row.key.clone())),
                    )
                    .child(cell().flex_1().min_w_0().child(value))
            })
            .collect();
        div()
            .debug_selector(|| "markdown-front-matter".to_owned())
            .mb(self.em(1.0))
            .flex()
            .flex_col()
            .children(lines)
            .into_any_element()
    }

    fn rule(&self) -> AnyElement {
        div().my(self.em(1.5)).h(px(1.0)).bg(self.color("border-strong")).into_any_element()
    }

    /// An image: drawn once read and decoded, else a box with its alternative text (dashed and
    /// explained when it is not loaded at all).
    fn image(&mut self, image: &Image) -> AnyElement {
        let (width, height) = (attribute_px(image.width.as_deref()), attribute_px(image.height.as_deref()));
        let shown = self.view.image_view(&image.source, self.cx);
        let drawn = match &shown {
            ImageView::Shown(url) => self.view.decoded.get(url).cloned().flatten(),
            _ => None,
        };
        if let Some(decoded) = drawn {
            return img(decoded)
                .max_w_full()
                .when_some(width, |el, width| el.w(px(width)))
                .when_some(height, |el, height| el.h(px(height)))
                .into_any_element();
        }
        let n = self.texts;
        self.texts += 1;
        let blocked = matches!(shown, ImageView::Blocked(_));
        let tip = match shown {
            ImageView::Blocked(Some(tip)) => Some(tip),
            _ => None,
        };
        let id = format!("markdown-image-{n}");
        div()
            .id(SharedString::from(id.clone()))
            .debug_selector(move || id.clone())
            .min_w(px(16.0))
            .min_h(px(16.0))
            .when_some(width, |el, width| el.w(px(width)))
            .when_some(height, |el, height| el.h(px(height)))
            .px(px(4.0))
            .border_1()
            .border_color(self.color("border-strong"))
            .when(blocked, |el| el.border_dashed())
            .text_color(self.color("fg-muted"))
            .text_size(theme::length(self.cx, "fs-small"))
            .child(SharedString::from(image.alt.clone()))
            .when_some(tip, |el, tip| el.tooltip(tooltip(tip)))
            .into_any_element()
    }

    fn element(&mut self, element: &Element, first: bool) -> AnyElement {
        let saved = self.align;
        if let Some(align) = Align::of(element) {
            self.align = align;
        }
        let built = match element.tag.as_str() {
            "details" => self.details(element),
            "summary" => self.container(&element.children).into_any_element(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = element.tag[1..].parse().unwrap_or(1);
                self.heading(level, first, |builder| builder.nodes(&element.children))
            }
            "p" => self.container(&element.children).mb(self.em(1.0)).into_any_element(),
            "hr" => self.rule(),
            "pre" => {
                let text = element.children.iter().map(Node::text).collect::<String>();
                self.code(None, &text, false)
            }
            "table" => {
                let children = self.nodes(&element.children);
                div().mb(self.em(1.0)).flex().flex_col().children(children).into_any_element()
            }
            "thead" | "tbody" | "tfoot" => self.container(&element.children).into_any_element(),
            "tr" => {
                let children = self.nodes(&element.children);
                div().flex().children(children).into_any_element()
            }
            "th" | "td" => {
                let header = element.tag == "th";
                let border = self.color("border-strong");
                self.container(&element.children)
                    .flex_1()
                    .min_w_0()
                    .px(px(10.0))
                    .py(px(4.0))
                    .border_1()
                    .border_color(border)
                    .when(header, |el| el.font_weight(FontWeight::BOLD).bg(self.color("bg-editor-inline")))
                    .into_any_element()
            }
            "ul" | "ol" => self.html_list(element),
            "li" => {
                let marker = self.marker("•".to_owned());
                let content = self.container(&element.children);
                div().flex().gap(px(6.0)).child(marker).child(content.flex_1().min_w_0()).into_any_element()
            }
            "dt" => self.container(&element.children).font_weight(FontWeight::BOLD).into_any_element(),
            "dd" => self.container(&element.children).pl(self.em(2.0)).into_any_element(),
            "caption" | "figcaption" => {
                self.container(&element.children).text_color(self.color("fg-secondary")).into_any_element()
            }
            _ => self.container(&element.children).into_any_element(),
        };
        self.align = saved;
        built
    }

    /// A container of nodes in a column, aligned as asked.
    fn container(&mut self, nodes: &[Node]) -> Div {
        let children = self.nodes(nodes);
        self.align.apply(div().flex().flex_col()).children(children)
    }

    fn html_list(&mut self, element: &Element) -> AnyElement {
        let ordered = element.tag == "ol";
        let mut number = 0u64;
        let mut rows = Vec::new();
        for child in &element.children {
            match child {
                Node::Element(item) if item.tag == "li" => {
                    number += 1;
                    let marker = self.marker(if ordered { format!("{number}.") } else { "•".to_owned() });
                    let content = self.container(&item.children);
                    rows.push(div().flex().gap(px(6.0)).child(marker).child(content.flex_1().min_w_0()));
                }
                other => {
                    let built = self.node(other);
                    rows.push(div().child(built));
                }
            }
        }
        div().mb(self.em(1.0)).pl(self.em(1.2)).flex().flex_col().children(rows).into_any_element()
    }

    /// A `<details>`: its summary, a disclosure triangle that opens and closes it, and its content
    /// while open. Nested `<details>` of hidden content still count, so the indices stay in step
    /// with [`details_written_open`].
    fn details(&mut self, element: &Element) -> AnyElement {
        let index = self.details;
        self.details += 1;
        let open = self.open.get(index).copied().unwrap_or(false);
        let mut summary = None;
        let mut content = Vec::new();
        for child in &element.children {
            match child {
                Node::Element(inner) if inner.tag == "summary" && summary.is_none() => {
                    summary = Some(self.container(&inner.children));
                }
                other if open => content.push(self.node(other)),
                other => self.details += count_details(std::slice::from_ref(other)),
            }
        }
        let label = summary.unwrap_or_else(|| div().child(SharedString::from(self.tx("Details")))).flex_1().min_w_0();
        let chevron = if open { "chevron-down" } else { "chevron-right" };
        let weak = self.weak.clone();
        let selector = format!("markdown-summary-{index}");
        let row = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex()
            .items_start()
            .gap(px(4.0))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |view, cx| view.toggle_details(index, cx));
            })
            .child(div().flex_none().mt(self.em(0.3)).child(theme::icon(
                self.cx,
                chevron,
                self.base * 0.8,
                self.color("fg-secondary"),
            )))
            .child(label);
        div()
            .mb(self.em(1.0))
            .flex()
            .flex_col()
            .child(row)
            .when(open, |el| el.child(div().pl(self.em(1.0)).flex().flex_col().children(content)))
            .into_any_element()
    }
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let blocks = Rc::clone(&self.blocks);
        let weak = cx.weak_entity();
        let children: Vec<AnyElement> = {
            let app: &App = cx;
            let family = |token: &str, fallback: &str| {
                let name = app
                    .try_global::<theme::ActiveTheme>()
                    .and_then(|active| active.theme.font_families(token).into_iter().next())
                    .unwrap_or_else(|| fallback.to_owned());
                font(SharedString::from(name))
            };
            let mut builder = Builder {
                view: self,
                weak: weak.clone(),
                cx: app,
                ui: family("font-ui", theme::fonts::BUNDLED_UI_FAMILY),
                mono: family("font-mono", theme::fonts::BUNDLED_MONO_FAMILY),
                base: self.font_size(),
                open: self.details_open(),
                details: 0,
                texts: 0,
                align: Align::Start,
                first: true,
            };
            blocks
                .iter()
                .enumerate()
                .map(|(index, block)| {
                    let nodes = builder.nodes(&block.nodes);
                    div()
                        .debug_selector(move || format!("markdown-block-{index}"))
                        .flex()
                        .flex_col()
                        .children(nodes)
                        .into_any_element()
                })
                .collect()
        };
        let laid = Rc::clone(&self.laid);
        let epoch = self.epoch;
        let after = canvas(
            move |_, _, cx| {
                laid.set(epoch);
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |view, cx| view.after_layout(cx));
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0();
        let ui_family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-ui").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_UI_FAMILY.to_owned());
        let bottom = window.viewport_size().height * 0.4;
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("markdown-preview")
                    .debug_selector(|| "markdown-preview".to_owned())
                    .track_focus(&self.focus)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .pt(px(16.0))
                    .px(px(28.0))
                    .pb(bottom)
                    .bg(theme::color(cx, "bg-editor"))
                    .text_color(theme::color(cx, "fg-editor"))
                    .font_family(ui_family)
                    .text_size(px(self.font_size()))
                    .line_height(relative(1.6))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, window, cx| window.focus(&view.focus, cx)),
                    )
                    .children(children),
            )
            .child(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::viewers::render::nodes_text;
    use crate::platform::input;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<MarkdownViewEvent>>>;

    const PATH: &str = "/p/README.md";

    fn open<'a>(cx: &'a mut TestAppContext, text: &str) -> (Entity<MarkdownView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let images = cx.update(|cx| cx.new(|_| PreviewImages::default()));
        let (view, cx) = cx.add_window_view(|_, cx| MarkdownView::new(PATH.to_owned(), images, 14.0, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &MarkdownViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.set_text(text, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click_start(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.origin + point(px(4.0), bounds.size.height / 2.0), Modifiers::default());
        cx.run_until_parked();
    }

    const DETAILS: &str = "<details>\n<summary>One</summary>\n\nFirst\n\n</details>\n\n\
                           <details open>\n<summary>Two</summary>\n\nSecond\n\n</details>\n";

    // src/features/viewers/markdown/previewDetails.test.tsx › preview <details> › keeps what the reader opened and
    // closed across edits
    #[gpui_kit::test]
    async fn keeps_what_the_reader_opened_and_closed_across_edits(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, DETAILS);
        assert_eq!(view.read_with(cx, |view, _| view.details_open()), [false, true]);
        assert!(cx.debug_bounds("markdown-summary-0").is_some());
        click_start(cx, "markdown-summary-0");
        click_start(cx, "markdown-summary-1");
        assert_eq!(view.read_with(cx, |view, _| view.details_open()), [true, false]);

        view.update(cx, |view, cx| view.set_text(&format!("{DETAILS}\nMore\n"), cx));
        cx.run_until_parked();
        let shown: String =
            view.read_with(cx, |view, _| view.blocks().iter().map(|block| nodes_text(&block.nodes)).collect());
        assert!(shown.contains("More"));
        assert_eq!(view.read_with(cx, |view, _| view.details_open()), [true, false]);
    }

    #[gpui_kit::test]
    async fn reports_link_clicks_as_what_they_open(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, "[Docs](https://example.com/docs)\n\n[Guide](docs/guide.md#L3)\n");
        click_start(cx, "markdown-text-0");
        click_start(cx, "markdown-text-1");
        let opened: Vec<MarkdownViewEvent> =
            events.borrow().iter().filter(|event| !matches!(event, MarkdownViewEvent::Scrolled(_))).cloned().collect();
        assert_eq!(
            opened,
            [
                MarkdownViewEvent::OpenUrl("https://example.com/docs".to_owned()),
                MarkdownViewEvent::OpenFile { path: "/p/docs/guide.md".to_owned(), line: Some(2) },
            ]
        );
    }

    #[gpui_kit::test]
    async fn asks_for_local_images_and_blocks_remote_ones(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "![logo](img/logo.png) ![badge](https://example.com/b.svg)\n");
        assert_eq!(*events.borrow(), [MarkdownViewEvent::ReadImage("/p/img/logo.png".to_owned())]);
        let local = view.read_with(cx, |view, cx| view.image_view("img/logo.png", cx));
        let remote = view.read_with(cx, |view, cx| view.image_view("https://example.com/b.svg", cx));
        assert_eq!(local, ImageView::Loading);
        assert_eq!(remote, ImageView::Blocked(Some("Remote images are not loaded".to_owned())));
        assert!(cx.debug_bounds("markdown-image-0").is_some());
    }

    #[test]
    fn decodes_data_urls_of_formats_gpui_draws() {
        // A 1×1 transparent GIF.
        let gif = "data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==";
        let image = decode_data_url(gif).expect("a GIF decodes");
        assert_eq!(image.format, ImageFormat::Gif);
        assert!(decode_data_url("data:image/avif;base64,AAAA").is_none());
        assert!(decode_data_url("data:image/png;base64,***").is_none());
        let svg = decode_data_url("data:image/svg+xml,%3Csvg%2F%3E").expect("a percent-encoded SVG decodes");
        assert_eq!(svg.bytes, b"<svg/>");
    }

    #[test]
    fn reads_sizes_from_html_attributes() {
        assert_eq!(attribute_px(Some("120")), Some(120.0));
        assert_eq!(attribute_px(Some(" 64px ")), Some(64.0));
        assert_eq!(attribute_px(Some("50%")), None);
        assert_eq!(attribute_px(None), None);
    }
}
