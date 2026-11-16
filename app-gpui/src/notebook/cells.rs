//! The cells of the notebook editor as drawn: each cell's toolbar (its execution count or number,
//! its type, Run Cell or Edit / Preview Markdown, Move Up / Down, Insert Code Cell Below, Delete),
//! its source (an editor, a Markdown preview, or the read-only text of a raw or unknown cell) and a
//! code cell's outputs; below them the bar that adds a code or Markdown cell.

use super::ansi::AnsiRun;
use super::markdown_cell::markdown_blocks;
use super::model::NotebookOp;
use super::output_items::{OutputItem, RasterFormat, output_items};
use super::view::{Body, NotebookView, NotebookViewEvent, known};
use crate::editor_lsp::markdown::Run;
use crate::features_b::viewers::render::{Inline, Node, PreviewBlock};
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::tooltip::tooltip;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Font, FontStyle, FontWeight, Hsla, Image as GpuiImage, ImageFormat,
    InteractiveText, IntoElement, MouseButton, MouseDownEvent, Rgba, SharedString, Stateful, StyledText, TextRun,
    UnderlineStyle, WeakEntity, div, font, img, prelude::*, px,
};
use ide_notebook::{NotebookCell, NotebookEdit};
use std::collections::HashMap;
use std::sync::Arc;

/// The theme's code font.
pub(super) fn mono_font(cx: &App) -> Font {
    let family =
        crate::theme::installed_fonts::theme_family(cx, "font-mono", theme::fonts::BUNDLED_MONO_FAMILY).to_string();
    font(SharedString::from(family))
}

fn ui_font(cx: &App) -> Font {
    let family =
        crate::theme::installed_fonts::theme_family(cx, "font-ui", theme::fonts::BUNDLED_UI_FAMILY).to_string();
    font(SharedString::from(family))
}

fn rgb((r, g, b): (u8, u8, u8)) -> Hsla {
    Rgba { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: 1.0 }.into()
}

/// The selector of part `part` of the cell at `index`.
fn selector(index: usize, part: &str) -> String {
    format!("notebook-cell-{index}-{part}")
}

/// The image format and base64 data of a `data:` PNG or JPEG URL.
fn data_url_image(url: &str) -> Option<(ImageFormat, &str)> {
    let rest = url.strip_prefix("data:image/")?;
    let (mime, data) = rest.split_once(";base64,")?;
    let format = match mime.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpeg" => ImageFormat::Jpeg,
        _ => return None,
    };
    Some((format, data))
}

/// The `data:` URLs of the images `nodes` hold.
fn image_urls(nodes: &[Node], out: &mut Vec<String>) {
    let inline = |content: &[Inline], out: &mut Vec<String>| {
        out.extend(content.iter().filter_map(|item| match item {
            Inline::Image(image) => Some(image.source.clone()),
            Inline::Text(_) => None,
        }));
    };
    for node in nodes {
        match node {
            Node::Paragraph(content) | Node::Heading { content, .. } => inline(content, out),
            Node::Quote(inner) => image_urls(inner, out),
            Node::List { items, .. } => items.iter().for_each(|item| image_urls(&item.content, out)),
            Node::Table { header, rows } => header.iter().chain(rows.iter().flatten()).for_each(|c| inline(c, out)),
            Node::Image(image) => out.push(image.source.clone()),
            Node::Element(element) => image_urls(&element.children, out),
            _ => {}
        }
    }
}

/// Draws the blocks of a Markdown preview.
struct Draw<'a> {
    cx: &'a App,
    view: WeakEntity<NotebookView>,
    notebook: String,
    images: HashMap<String, Arc<GpuiImage>>,
    ui: Font,
    mono: Font,
    texts: usize,
    cell: usize,
}

impl Draw<'_> {
    fn color(&self, token: &str) -> Hsla {
        theme::color(self.cx, token)
    }

    fn text(&mut self, runs: &[&Run], bold: bool) -> AnyElement {
        let mut text = String::new();
        let mut text_runs = Vec::new();
        let mut ranges = Vec::new();
        let mut hrefs: Vec<String> = Vec::new();
        for run in runs.iter().filter(|run| !run.text.is_empty()) {
            let start = text.len();
            text.push_str(&run.text);
            let style = &run.style;
            let base = if style.code { &self.mono } else { &self.ui };
            text_runs.push(TextRun {
                len: run.text.len(),
                font: Font {
                    weight: if style.bold || bold { FontWeight::BOLD } else { base.weight },
                    style: if style.italic { FontStyle::Italic } else { base.style },
                    ..base.clone()
                },
                color: if style.link.is_some() { self.color("link") } else { self.color("fg") },
                background_color: style.code.then(|| self.color("bg-elev")),
                underline: None,
                strikethrough: None,
            });
            if let Some(href) = &style.link {
                ranges.push(start..text.len());
                hrefs.push(href.clone());
            }
        }
        if text.is_empty() {
            return div().into_any_element();
        }
        let n = self.texts;
        self.texts += 1;
        let id = format!("notebook-cell-{}-text-{n}", self.cell);
        let styled = StyledText::new(text).with_runs(text_runs);
        let element = if hrefs.is_empty() {
            styled.into_any_element()
        } else {
            let (view, notebook) = (self.view.clone(), self.notebook.clone());
            InteractiveText::new(SharedString::from(id.clone()), styled)
                .on_click(ranges, move |index, _, cx| {
                    if let Some(href) = hrefs.get(index) {
                        let event = NotebookViewEvent::OpenLink { notebook: notebook.clone(), href: href.clone() };
                        let _ = view.update(cx, |_, cx| cx.emit(event));
                    }
                })
                .into_any_element()
        };
        div().debug_selector(move || id).min_w_0().child(element).into_any_element()
    }

    fn inlines(&mut self, content: &[Inline], bold: bool) -> AnyElement {
        let mut children = Vec::new();
        let mut runs: Vec<&Run> = Vec::new();
        for item in content {
            match item {
                Inline::Text(run) => runs.push(run),
                Inline::Image(image) => {
                    if !runs.is_empty() {
                        children.push(self.text(&std::mem::take(&mut runs), bold));
                    }
                    children.push(self.image(&image.source, &image.alt));
                }
            }
        }
        if !runs.is_empty() {
            children.push(self.text(&runs, bold));
        }
        div().flex().flex_wrap().items_end().gap(px(4.0)).children(children).into_any_element()
    }

    fn image(&self, source: &str, alt: &str) -> AnyElement {
        match self.images.get(source) {
            Some(image) => img(image.clone()).max_w_full().into_any_element(),
            None => {
                div().text_color(self.color("fg-muted")).child(SharedString::from(alt.to_owned())).into_any_element()
            }
        }
    }

    fn nodes(&mut self, nodes: &[Node]) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node)).collect()
    }

    fn node(&mut self, node: &Node) -> AnyElement {
        match node {
            Node::Paragraph(content) => div().mb(px(6.0)).child(self.inlines(content, false)).into_any_element(),
            Node::Heading { level, content, .. } => {
                let size = match level {
                    1 => 1.6,
                    2 => 1.35,
                    3 => 1.15,
                    _ => 1.0,
                };
                div()
                    .mb(px(6.0))
                    .text_size(theme::length(self.cx, "fs-ui") * size)
                    .child(self.inlines(content, true))
                    .into_any_element()
            }
            Node::Code { text, .. } | Node::Mermaid { source: text } => div()
                .mb(px(6.0))
                .p(px(8.0))
                .bg(self.color("bg-elev"))
                .font(self.mono.clone())
                .child(SharedString::from(text.trim_end_matches('\n').to_owned()))
                .into_any_element(),
            Node::Quote(inner) => {
                let children = self.nodes(inner);
                div()
                    .mb(px(6.0))
                    .pl(px(10.0))
                    .border_l_2()
                    .border_color(self.color("border"))
                    .text_color(self.color("fg-muted"))
                    .children(children)
                    .into_any_element()
            }
            Node::List { ordered, items } => {
                let mut rows = Vec::new();
                for (at, item) in items.iter().enumerate() {
                    let marker = match (ordered, item.checked) {
                        (_, Some(true)) => "☑".to_owned(),
                        (_, Some(false)) => "☐".to_owned(),
                        (Some(start), None) => format!("{}.", start + at as u64),
                        (None, None) => "•".to_owned(),
                    };
                    let content = self.nodes(&item.content);
                    rows.push(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(div().flex_none().min_w(px(14.0)).child(SharedString::from(marker)))
                            .child(div().flex_1().min_w_0().children(content)),
                    );
                }
                div().mb(px(6.0)).children(rows).into_any_element()
            }
            Node::Table { header, rows } => {
                let border = self.color("border");
                let mut drawn = Vec::new();
                for (at, row) in std::iter::once(header).chain(rows.iter()).enumerate() {
                    let cells: Vec<AnyElement> = row
                        .iter()
                        .map(|cell| {
                            let content = self.inlines(cell, at == 0);
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(8.0))
                                .py(px(4.0))
                                .border_1()
                                .border_color(border)
                                .child(content)
                        })
                        .map(IntoElement::into_any_element)
                        .collect();
                    drawn.push(div().flex().children(cells));
                }
                div().mb(px(6.0)).children(drawn).into_any_element()
            }
            Node::Rule => div().my(px(6.0)).h(px(1.0)).bg(self.color("border")).into_any_element(),
            Node::Image(image) => self.image(&image.source, &image.alt),
            Node::Element(element) => {
                let children = self.nodes(&element.children);
                div().children(children).into_any_element()
            }
            Node::FrontMatter(_) => div()
                .mb(px(6.0))
                .font(self.mono.clone())
                .text_color(self.color("fg-muted"))
                .child(SharedString::from(node.text()))
                .into_any_element(),
        }
    }
}

impl NotebookView {
    pub(super) fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let message = |title: String, detail: Option<String>, cx: &App| {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(title))
                .children(detail.map(SharedString::from))
        };
        let tx = |text: &str, cx: &mut Context<Self>| cx.default_global::<Localization>().0.tx(text);
        match self.body(cx) {
            Body::Loading => {
                let title = tx("Loading notebook…", cx);
                return message(title, None, cx).into_any_element();
            }
            Body::Unavailable => {
                let (title, detail) =
                    (tx("Notebook unavailable", cx), tx("Notebook editing and execution require the desktop app.", cx));
                return message(title, Some(detail), cx).into_any_element();
            }
            Body::NoCells | Body::Cells => {}
        }
        let cells = self.model.read(cx).cells();
        let disabled = self.disabled(cx);
        let running = self.model.read(cx).state().running;
        let mut list = div()
            .id("notebook-scroll")
            .debug_selector(|| "notebook-scroll".to_owned())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll();
        if cells.is_empty() {
            let title = tx("This notebook has no cells", cx);
            let add = self.add_button("notebook-empty-add-code", "Add Code Cell", "code", !disabled, cx);
            list = list.child(message(title, None, cx).py(px(32.0)).child(add));
        } else {
            let active = self.active_cell(cx).map(|cell| cell.id);
            for (index, cell) in cells.iter().enumerate() {
                let selected = active.as_deref() == Some(cell.id.as_str());
                let drawn = self.render_cell(index, cell, &cells, selected, disabled, running, cx);
                list = list.child(div().px(px(12.0)).py(px(6.0)).child(drawn));
            }
        }
        let enabled = !(disabled || running);
        let add_code = self.add_button("notebook-add-code", "Add Code Cell", "code", enabled, cx);
        let add_markdown = self.add_button("notebook-add-markdown", "Add Markdown Cell", "markdown", enabled, cx);
        list.child(
            div()
                .flex()
                .justify_center()
                .flex_wrap()
                .gap(px(8.0))
                .pt(px(12.0))
                .px(px(16.0))
                .pb(px(32.0))
                .child(add_code)
                .child(add_markdown),
        )
        .into_any_element()
    }

    fn add_button(
        &self,
        id: &'static str,
        title: &'static str,
        cell_type: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label = cx.default_global::<Localization>().0.tx(title);
        let fg = theme::color(cx, "fg-strong");
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .bg(theme::color(cx, "bg-button"))
            .text_color(fg)
            .when(!enabled, |el| el.opacity(0.5))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _, _, cx| view.add_cell(cell_type, cx))))
            .child(theme::icon(cx, "plus", 12.0, fg))
            .child(SharedString::from(label))
    }

    /// A small icon button of a cell's toolbar.
    fn cell_button(
        &self,
        id: String,
        icon: &'static str,
        title: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let color = theme::color(cx, "fg-muted");
        let selector = id.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .size(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-toolbar-button"))
            .when(!enabled, |el| el.opacity(0.4))
            .tooltip(tooltip(title))
            .child(theme::icon(cx, icon, 12.0, color))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cell(
        &mut self,
        index: usize,
        cell: &NotebookCell,
        cells: &[NotebookCell],
        selected: bool,
        frozen: bool,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let structural = frozen || running;
        let is_known = known(&cell.cell_type);
        let editing = self.model.read(cx).markdown_editing.contains(&cell.id);
        let count = if cell.cell_type == "code" {
            format!("[{}]", cell.execution_count.map_or_else(|| " ".to_owned(), |count| count.to_string()))
        } else {
            (index + 1).to_string()
        };
        let type_label = {
            let i18n = &cx.default_global::<Localization>().0;
            match cell.cell_type.as_str() {
                "code" => i18n.tx("Code"),
                "markdown" => i18n.tx("Markdown"),
                "raw" => i18n.tx("Raw"),
                other => other.to_owned(),
            }
        };
        let type_enabled = !(structural || (!is_known && cell.cell_type != "raw"));
        let id = cell.id.clone();
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .min_h(px(28.0))
            .px(px(6.0))
            .py(px(2.0))
            .bg(theme::color(cx, "bg-elev"))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(
                div()
                    .debug_selector(move || selector(index, "count"))
                    .min_w(px(32.0))
                    .mr(px(6.0))
                    .text_right()
                    .font(mono_font(cx))
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(count)),
            );
        let typed = cell.clone();
        toolbar = toolbar.child(
            div()
                .id(SharedString::from(selector(index, "type")))
                .debug_selector(move || selector(index, "type"))
                .h(px(20.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(!type_enabled, |el| el.opacity(0.5))
                .when(type_enabled, |el| {
                    el.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            view.show_cell_type_menu(&typed, event.position, cx);
                        }),
                    )
                })
                .child(SharedString::from(type_label)),
        );
        if cell.cell_type == "code" {
            let enabled = !(frozen || running);
            let run = id.clone();
            toolbar = toolbar.child(self.cell_button(selector(index, "run"), "run", "Run Cell", enabled, cx).when(
                enabled,
                |el| {
                    el.on_click(cx.listener(move |view, _, _, cx| {
                        let op = NotebookOp::Execute(Some(vec![run.clone()]));
                        view.model.update(cx, |model, cx| model.invoke(op, cx));
                    }))
                },
            ));
        }
        if cell.cell_type == "markdown" {
            let (icon, title) = if editing { ("eye", "Preview Markdown") } else { ("edit", "Edit Markdown") };
            let toggled = id.clone();
            toolbar =
                toolbar.child(self.cell_button(selector(index, "markdown"), icon, title, !frozen, cx).when(
                    !frozen,
                    |el| {
                        el.on_click(cx.listener(move |view, _, window, cx| {
                            view.set_markdown_editing(&toggled, !editing, window, cx)
                        }))
                    },
                ));
        }
        toolbar = toolbar.child(div().flex_1());
        let edit_button = |part: &str,
                           icon: &'static str,
                           title: &'static str,
                           enabled: bool,
                           edit: NotebookEdit,
                           view: &Self,
                           cx: &mut Context<Self>| {
            view.cell_button(selector(index, part), icon, title, enabled, cx).when(enabled, |el| {
                el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    let edit = edit.clone();
                    view.model.update(cx, |model, cx| model.edit(edit, cx));
                }))
            })
        };
        let up = (index > 0).then(|| cells[index - 1].id.clone());
        let down_before = cells.get(index + 2).map(|cell| cell.id.clone());
        let last = index + 1 == cells.len();
        toolbar = toolbar
            .child(edit_button(
                "up",
                "arrow-up",
                "Move Cell Up",
                !structural && index > 0,
                NotebookEdit::MoveCell { cell: id.clone(), before: up },
                self,
                cx,
            ))
            .child(edit_button(
                "down",
                "arrow-down",
                "Move Cell Down",
                !structural && !last,
                NotebookEdit::MoveCell { cell: id.clone(), before: down_before },
                self,
                cx,
            ))
            .child(edit_button(
                "insert",
                "plus",
                "Insert Code Cell Below",
                !structural,
                NotebookEdit::InsertCell { after: Some(id.clone()), cell_type: "code".to_owned() },
                self,
                cx,
            ))
            .child(edit_button(
                "delete",
                "trash",
                "Delete Cell",
                !structural,
                NotebookEdit::DeleteCell { cell: id.clone() },
                self,
                cx,
            ));
        let body = self.render_source(index, cell, is_known, editing, frozen, cx);
        let outputs =
            (cell.cell_type == "code" && !cell.outputs.is_empty()).then(|| self.render_outputs(index, cell, cx));
        let chosen = id.clone();
        div()
            .id(SharedString::from(format!("notebook-cell-{index}")))
            .debug_selector(move || format!("notebook-cell-{index}"))
            .min_w_0()
            .overflow_hidden()
            .border_1()
            .border_color(theme::color(cx, if selected { "accent" } else { "border" }))
            .rounded(theme::length(cx, "radius-small"))
            .capture_any_mouse_down(cx.listener(move |view, _: &MouseDownEvent, _, cx| view.select(&chosen, cx)))
            .child(toolbar)
            .child(body)
            .children(outputs)
    }

    fn render_source(
        &mut self,
        index: usize,
        cell: &NotebookCell,
        is_known: bool,
        editing: bool,
        frozen: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mono = mono_font(cx);
        if !is_known {
            let notice = cx
                .default_global::<Localization>()
                .0
                .tx("Raw or unknown cell; original content is preserved read-only.");
            return div()
                .debug_selector(move || selector(index, "raw"))
                .px(px(12.0))
                .py(px(8.0))
                .max_h(px(480.0))
                .overflow_hidden()
                .child(
                    div()
                        .text_color(theme::color(cx, "fg-muted"))
                        .text_size(theme::length(cx, "fs-small"))
                        .py(px(4.0))
                        .child(SharedString::from(notice)),
                )
                .child(div().font(mono).child(SharedString::from(cell.source.clone())))
                .into_any_element();
        }
        if cell.cell_type == "markdown" && !editing {
            let id = cell.id.clone();
            if cell.source.is_empty() {
                let label = cx.default_global::<Localization>().0.tx("Edit Markdown");
                return div()
                    .id(SharedString::from(selector(index, "edit-markdown")))
                    .debug_selector(move || selector(index, "edit-markdown"))
                    .m(px(8.0))
                    .px(px(8.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .text_color(theme::color(cx, "fg-muted"))
                    .when(frozen, |el| el.opacity(0.5))
                    .when(!frozen, |el| {
                        el.on_click(
                            cx.listener(move |view, _, window, cx| view.set_markdown_editing(&id, true, window, cx)),
                        )
                    })
                    .child(SharedString::from(label))
                    .into_any_element();
            }
            let blocks = self.markdown.blocks(&cell.id, &cell.source, &cell.attachments);
            let images = self.preview_images(&blocks);
            let notebook = self.model.read(cx).path().to_owned();
            let view = cx.weak_entity();
            let children = {
                let app: &App = cx;
                let mut draw = Draw { cx: app, view, notebook, images, ui: ui_font(app), mono, texts: 0, cell: index };
                blocks.iter().flat_map(|block| draw.nodes(&block.nodes)).collect::<Vec<_>>()
            };
            return div()
                .id(SharedString::from(selector(index, "preview")))
                .debug_selector(move || selector(index, "preview"))
                .px(px(14.0))
                .py(px(8.0))
                .min_h(px(32.0))
                .line_height(gpui_kit::relative(1.6))
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    if event.click_count() == 2 && !view.disabled(cx) {
                        view.set_markdown_editing(&id, true, window, cx);
                    }
                }))
                .children(children)
                .into_any_element();
        }
        match self.editors.get(&cell.id) {
            Some(editor) => div()
                .debug_selector(move || selector(index, "editor"))
                .min_h(px(48.0))
                .px(px(8.0))
                .py(px(6.0))
                .font(mono)
                .child(editor.area.clone())
                .into_any_element(),
            None => div().min_h(px(48.0)).into_any_element(),
        }
    }

    /// The images a preview shows, decoded.
    fn preview_images(&mut self, blocks: &[PreviewBlock]) -> HashMap<String, Arc<GpuiImage>> {
        let mut urls = Vec::new();
        for block in blocks {
            image_urls(&block.nodes, &mut urls);
        }
        let mut images = HashMap::new();
        for url in urls {
            if let Some((format, data)) = data_url_image(&url)
                && let Some(image) =
                    self.image(&data.chars().filter(|c| !c.is_whitespace()).collect::<String>(), format)
            {
                images.insert(url, image);
            }
        }
        images
    }

    fn ansi_text(&self, runs: &[AnsiRun], error: bool, cx: &App) -> AnyElement {
        let mono = mono_font(cx);
        let base = theme::color(cx, if error { "danger" } else { "fg" });
        let mut text = String::new();
        let mut text_runs = Vec::new();
        for run in runs.iter().filter(|run| !run.text.is_empty()) {
            text.push_str(&run.text);
            text_runs.push(TextRun {
                len: run.text.len(),
                font: Font {
                    weight: if run.bold { FontWeight::BOLD } else { mono.weight },
                    style: if run.italic { FontStyle::Italic } else { mono.style },
                    ..mono.clone()
                },
                color: run.fg.map_or(base, rgb),
                background_color: run.bg.map(rgb),
                underline: run.underline.then_some(UnderlineStyle { thickness: px(1.0), color: None, wavy: false }),
                strikethrough: None,
            });
        }
        if text.is_empty() {
            return div().into_any_element();
        }
        div().min_w_0().child(StyledText::new(text).with_runs(text_runs)).into_any_element()
    }

    fn render_outputs(&mut self, index: usize, cell: &NotebookCell, cx: &mut Context<Self>) -> Div {
        let items = output_items(&cell.outputs);
        let notice = |text: String, cx: &App| {
            div()
                .py(px(4.0))
                .text_color(theme::color(cx, "fg-muted"))
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(text))
        };
        let mut drawn: Vec<AnyElement> = Vec::new();
        for item in items {
            let tx = |text: &str, cx: &mut Context<Self>| cx.default_global::<Localization>().0.tx(text);
            match item {
                OutputItem::Text { runs, error, truncated } => {
                    drawn.push(self.ansi_text(&runs, error, cx));
                    if truncated {
                        let text = tx("Output preview truncated; full output is retained in the notebook.", cx);
                        drawn.push(notice(text, cx).into_any_element());
                    }
                }
                OutputItem::Image { format, base64, alt } => {
                    let format = match format {
                        RasterFormat::Png => ImageFormat::Png,
                        RasterFormat::Jpeg => ImageFormat::Jpeg,
                    };
                    match self.image(&base64, format) {
                        Some(image) => drawn.push(img(image).max_w_full().my(px(4.0)).into_any_element()),
                        None => drawn.push(notice(tx(&alt, cx), cx).into_any_element()),
                    }
                }
                OutputItem::Svg { alt } => drawn.push(notice(tx(&alt, cx), cx).into_any_element()),
                OutputItem::Html { text } => {
                    let label = tx("Static notebook HTML output", cx);
                    drawn.push(notice(label, cx).into_any_element());
                    drawn.push(div().whitespace_normal().child(SharedString::from(text)).into_any_element());
                }
                OutputItem::InertJavaScript { source } => {
                    let text = tx("JavaScript output is inert. Interactive widgets are not supported.", cx);
                    drawn.push(notice(text, cx).into_any_element());
                    drawn.push(div().font(mono_font(cx)).child(SharedString::from(source)).into_any_element());
                }
                OutputItem::Notice { text, detail } => {
                    let text = format!("{} {detail}", tx(text, cx));
                    drawn.push(notice(text, cx).into_any_element());
                }
            }
        }
        div()
            .debug_selector(move || selector(index, "outputs"))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .px(px(12.0))
            .py(px(8.0))
            .max_h(px(640.0))
            .overflow_hidden()
            .children(drawn)
    }

    /// The text the outputs of cell `id` show, one entry per output item, for tests and the
    /// accessibility tree.
    pub fn output_texts(&self, id: &str, cx: &App) -> Vec<String> {
        let model = self.model.read(cx);
        let Some(cell) = model.cells().into_iter().find(|cell| cell.id == id) else { return Vec::new() };
        output_items(&cell.outputs)
            .into_iter()
            .map(|item| match item {
                OutputItem::Text { runs, .. } => super::ansi::plain_text(&runs),
                OutputItem::Image { alt, .. } | OutputItem::Svg { alt } => alt,
                OutputItem::Html { text } => text,
                OutputItem::InertJavaScript { source } => source,
                OutputItem::Notice { text, detail } => format!("{text} {detail}"),
            })
            .collect()
    }

    /// The text the Markdown preview of cell `id` shows, block by block.
    pub fn preview_text(&self, id: &str, cx: &App) -> Option<Vec<String>> {
        let cell = self.model.read(cx).cells().into_iter().find(|cell| cell.id == id)?;
        let blocks = markdown_blocks(&cell.source, &cell.attachments);
        Some(blocks.iter().flat_map(|block| block.nodes.iter().map(Node::text)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_name_their_raster_format() {
        assert!(matches!(data_url_image("data:image/png;base64,AAAA"), Some((ImageFormat::Png, "AAAA"))));
        assert!(matches!(data_url_image("data:image/jpeg;base64,BB"), Some((ImageFormat::Jpeg, "BB"))));
        assert!(data_url_image("data:image/svg+xml;base64,AA").is_none());
        assert!(data_url_image("https://x.invalid/a.png").is_none());
    }
}
