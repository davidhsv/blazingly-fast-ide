//! The notebook editor: a toolbar (Run Cell, Run All, Interrupt and Restart Kernel, the Python
//! interpreter, the kernel's state, Undo, Redo, Save, Reload from Disk), the error bar, and the
//! cells in order, each with its own toolbar, its source and, for code, its outputs.
//!
//! The view draws a [`NotebookModel`] shared by every tab of the file and starts its operations;
//! it calls no backend. A cell's source is edited in a [`TextArea`] whose own undo history is
//! never used: Undo and Redo (the toolbar, ⌘Z, ⇧⌘Z, ⌘Y) go to the notebook's history in the
//! backend, which holds source and structure changes alike. ⌘S saves, ⇧Enter and Ctrl+Enter run
//! the selected code cell. A source the backend changes (undo, reload) replaces the area's text.
//!
//! What the view cannot do itself it asks of its window as [`NotebookViewEvent`]s: a menu (cell
//! type, interpreter) and a link of a Markdown cell to follow.

use super::markdown_cell::MarkdownCache;
use super::model::{NotebookModel, NotebookOp};
use super::output_items::{OutputItem, RasterFormat, image_bytes, output_items};
use crate::features_b::viewers::render::{Image, Inline, Node, PreviewBlock};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::tooltip::tooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Image as GpuiImage, ImageFormat,
    IntoElement, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, PromptLevel, Render, SharedString, Stateful,
    Subscription, Window, div, prelude::*, px,
};
use ide_notebook::NotebookEdit;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

pub(super) const RUN_CELL: &str = "notebook-run-cell";
pub(super) const RUN_ALL: &str = "notebook-run-all";
pub(super) const INTERRUPT: &str = "notebook-interrupt";
pub(super) const RESTART: &str = "notebook-restart";
pub(super) const PYTHON: &str = "notebook-python";
pub(super) const REFRESH_PYTHON: &str = "notebook-refresh-python";
pub(super) const UNDO: &str = "notebook-undo";
pub(super) const REDO: &str = "notebook-redo";
pub(super) const SAVE: &str = "notebook-save";
pub(super) const RELOAD: &str = "notebook-reload";

/// A choice of one of the view's menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotebookChoice {
    CellType { cell: String, cell_type: String },
    Python(String),
}

/// What the view asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum NotebookViewEvent {
    /// Show this menu at `position`; hand the chosen row to [`NotebookView::choose`].
    ShowMenu { items: Vec<MenuItem<NotebookChoice>>, position: Point<Pixels> },
    /// A link of a Markdown cell was followed: `href` as written, relative to the notebook.
    OpenLink { notebook: String, href: String },
}

/// A toolbar button as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub title: &'static str,
    pub enabled: bool,
}

/// What the body of the editor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Loading,
    Unavailable,
    NoCells,
    Cells,
}

/// One output of a code cell as drawn: an image is the bytes the notebook holds, decoded once, or
/// `None` when they are not an image (its alternative text shows).
#[derive(Clone)]
pub(super) enum ShownOutput {
    Item(OutputItem),
    Image { image: Option<Arc<GpuiImage>>, alt: String },
}

/// A cell as the editor draws it, taken from the model whenever it changes: nothing that grows with
/// the notebook's outputs or attachments is computed while drawing.
#[derive(Clone)]
pub(super) struct ShownCell {
    pub(super) id: String,
    pub(super) cell_type: String,
    pub(super) source: String,
    pub(super) execution_count: Option<u32>,
    pub(super) outputs: Rc<Vec<ShownOutput>>,
    /// A Markdown cell's preview, its embedded images named by their key in [`NotebookView::image`].
    pub(super) preview: Option<Rc<Vec<PreviewBlock>>>,
}

/// The outputs of one code cell: those they were made from, and what they show once prepared.
struct CellOutputs {
    outputs: Vec<Value>,
    /// Bumped by every change, so a preparation that finishes after a newer one started is dropped.
    generation: u64,
    shown: Rc<Vec<ShownOutput>>,
    /// Whether `shown` is made from `outputs` (else the outputs before them still show).
    ready: bool,
}

/// The preview of one Markdown cell: what it was made from and its images' keys.
struct CellPreview {
    source: String,
    attachments: Value,
    built: Rc<Vec<PreviewBlock>>,
    shown: Rc<Vec<PreviewBlock>>,
    images: Vec<u64>,
}

/// The source of an embedded image whose decoding started under `key`.
fn image_source(key: u64) -> String {
    format!("notebook-image:{key}")
}

/// The key of an embedded image's source, as [`image_source`] wrote it.
pub(super) fn image_key(source: &str) -> Option<u64> {
    source.strip_prefix("notebook-image:")?.parse().ok()
}

/// The image format and base64 data of a `data:` PNG or JPEG URL.
pub(super) fn data_url_image(url: &str) -> Option<(ImageFormat, &str)> {
    let rest = url.strip_prefix("data:image/")?;
    let (mime, data) = rest.split_once(";base64,")?;
    let format = match mime.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpeg" => ImageFormat::Jpeg,
        _ => return None,
    };
    Some((format, data))
}

/// Every image of `nodes`, nested ones included, in document order.
fn images_mut(nodes: &mut [Node], visit: &mut impl FnMut(&mut Image)) {
    let inline = |content: &mut [Inline], visit: &mut dyn FnMut(&mut Image)| {
        for item in content {
            if let Inline::Image(image) = item {
                visit(image);
            }
        }
    };
    for node in nodes {
        match node {
            Node::Paragraph(content) | Node::Heading { content, .. } => inline(content, visit),
            Node::Quote(inner) => images_mut(inner, visit),
            Node::List { items, .. } => items.iter_mut().for_each(|item| images_mut(&mut item.content, visit)),
            Node::Table { header, rows } => {
                header.iter_mut().chain(rows.iter_mut().flatten()).for_each(|cell| inline(cell, visit));
            }
            Node::Image(image) => visit(image),
            Node::Element(element) => images_mut(&mut element.children, visit),
            _ => {}
        }
    }
}

/// The bytes of an image as GPUI draws them; `None` when the data is not base64.
fn decode(data: &str, format: ImageFormat) -> Option<Arc<GpuiImage>> {
    let data: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    image_bytes(&data).map(|bytes| Arc::new(GpuiImage::from_bytes(format, bytes)))
}

/// What the outputs of a code cell show, with their images decoded, and how many were.
fn prepare_outputs(outputs: &[Value]) -> (Vec<ShownOutput>, usize) {
    let mut decoded = 0;
    let shown = output_items(outputs)
        .into_iter()
        .map(|item| match item {
            OutputItem::Image { format, base64, alt } => {
                decoded += 1;
                let format = match format {
                    RasterFormat::Png => ImageFormat::Png,
                    RasterFormat::Jpeg => ImageFormat::Jpeg,
                };
                ShownOutput::Image { image: decode(&base64, format), alt }
            }
            item => ShownOutput::Item(item),
        })
        .collect();
    (shown, decoded)
}

/// The editor of one cell's source.
pub(super) struct CellEditor {
    pub(super) area: Entity<TextArea>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

pub struct NotebookView {
    pub(super) model: Entity<NotebookModel>,
    pub(super) focus: FocusHandle,
    pub(super) editors: HashMap<String, CellEditor>,
    pub(super) markdown: MarkdownCache,
    /// The cells as drawn, in order.
    pub(super) shown: Vec<ShownCell>,
    outputs: HashMap<String, CellOutputs>,
    previews: HashMap<String, CellPreview>,
    /// The decoded images of Markdown previews by key; a key being decoded is in `decoding`.
    pub(super) images: HashMap<u64, Arc<GpuiImage>>,
    decoding: HashSet<u64>,
    next_image: u64,
    next_generation: u64,
    decodes: usize,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NotebookViewEvent> for NotebookView {}

/// Whether `cell_type` has an editor: code and Markdown; raw and unknown cells are read-only.
pub(super) fn known(cell_type: &str) -> bool {
    matches!(cell_type, "code" | "markdown")
}

/// A key the notebook takes for itself before the keymap and the text area see it.
enum KeyCommand {
    Save,
    Undo,
    Redo,
    RunActive,
}

fn key_command(keystroke: &Keystroke) -> Option<KeyCommand> {
    let modifiers = &keystroke.modifiers;
    let key = keystroke.key.to_ascii_lowercase();
    if (modifiers.platform || modifiers.control) && !modifiers.alt {
        match key.as_str() {
            "s" if !modifiers.shift => return Some(KeyCommand::Save),
            "z" if modifiers.shift => return Some(KeyCommand::Redo),
            "z" => return Some(KeyCommand::Undo),
            "y" => return Some(KeyCommand::Redo),
            _ => {}
        }
    }
    (key == "enter" && (modifiers.shift || modifiers.control)).then_some(KeyCommand::RunActive)
}

impl NotebookView {
    pub fn new(model: Entity<NotebookModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| {
            input::release_keys(view.claim, cx);
            for editor in view.editors.values() {
                input::release_keys(editor.claim, cx);
            }
        })
        .detach();
        let subscriptions = vec![cx.observe_in(&model, window, |view, _, window, cx| {
            view.sync_editors(window, cx);
            cx.notify();
        })];
        let mut view = Self {
            model,
            focus,
            editors: HashMap::new(),
            markdown: MarkdownCache::default(),
            shown: Vec::new(),
            outputs: HashMap::new(),
            previews: HashMap::new(),
            images: HashMap::new(),
            decoding: HashSet::new(),
            next_image: 0,
            next_generation: 0,
            decodes: 0,
            claim,
            _subscriptions: subscriptions,
        };
        view.sync_editors(window, cx);
        view
    }

    pub fn model(&self) -> &Entity<NotebookModel> {
        &self.model
    }

    /// The editor of cell `id`'s source, while it shows one.
    pub fn cell_editor(&self, id: &str) -> Option<Entity<TextArea>> {
        self.editors.get(id).map(|editor| editor.area.clone())
    }

    /// How many images were decoded so far.
    pub fn image_decodes(&self) -> usize {
        self.decodes
    }

    /// Whether an image is still being decoded off the window's thread.
    pub fn decoding_images(&self) -> bool {
        !self.decoding.is_empty() || self.outputs.values().any(|outputs| !outputs.ready)
    }

    /// How many output images of cell `id` are drawn as images (decoded and valid).
    pub fn drawn_output_images(&self, id: &str) -> usize {
        let Some(cell) = self.shown.iter().find(|cell| cell.id == id) else { return 0 };
        cell.outputs.iter().filter(|output| matches!(output, ShownOutput::Image { image: Some(_), .. })).count()
    }

    /// The toolbar's state is the frozen or unloaded notebook's: nothing can be done.
    pub(super) fn disabled(&self, cx: &App) -> bool {
        let state = self.model.read(cx).state();
        state.frozen || state.snapshot.is_none()
    }

    /// The cell the toolbar and ⇧Enter act on, as its id and type: the selected one, else the first
    /// code cell.
    pub fn active_cell<'a>(&self, cx: &'a App) -> Option<(&'a str, &'a str)> {
        let model = self.model.read(cx);
        let cells = model.acknowledged_cells();
        let selected = model.selected.as_deref().and_then(|id| cells.iter().find(|cell| cell.id == id));
        selected
            .or_else(|| cells.iter().find(|cell| cell.cell_type == "code"))
            .map(|cell| (cell.id.as_str(), cell.cell_type.as_str()))
    }

    fn active_code_cell(&self, cx: &App) -> Option<String> {
        self.active_cell(cx).filter(|(_, cell_type)| *cell_type == "code").map(|(id, _)| id.to_owned())
    }

    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarButton> {
        let model = self.model.read(cx);
        let state = model.state();
        let disabled = self.disabled(cx);
        let running = state.running;
        let history_disabled = disabled || running;
        let active_code = self.active_code_cell(cx).is_some();
        let any_code = model.acknowledged_cells().iter().any(|cell| cell.cell_type == "code");
        let (can_undo, can_redo) = state.snapshot.as_ref().map_or((false, false), |s| (s.can_undo, s.can_redo));
        let button = |id, title, enabled| ToolbarButton { id, title, enabled };
        vec![
            button(RUN_CELL, "Run Cell", !(disabled || running || !active_code)),
            button(RUN_ALL, "Run All", !(disabled || running || !any_code)),
            button(INTERRUPT, "Interrupt Kernel", !(disabled || !running)),
            button(RESTART, "Restart Kernel", !disabled),
            button(PYTHON, "Select Python interpreter", !(disabled || running)),
            button(REFRESH_PYTHON, "Refresh Python interpreters", !(disabled || running)),
            button(UNDO, "Undo", !(history_disabled || (!can_undo && state.pending == 0))),
            button(REDO, "Redo", !(history_disabled || !can_redo)),
            button(SAVE, "Save", !disabled),
            button(RELOAD, "Reload Notebook from Disk", !disabled),
        ]
    }

    /// The kernel's state as the toolbar shows it: busy while a batch runs.
    pub fn kernel_text(&self, cx: &mut App) -> String {
        let state = self.model.read(cx).state();
        let shown = if state.running { "busy".to_owned() } else { state.kernel.clone() };
        cx.default_global::<Localization>().0.tx(&shown)
    }

    /// The error bar's message, when it shows.
    pub fn error_text(&self, cx: &App) -> Option<String> {
        self.model.read(cx).state().error.clone()
    }

    /// Whether the error bar says how to install ipykernel: the failure names it.
    pub fn shows_kernel_help(&self, cx: &App) -> bool {
        self.error_text(cx).is_some_and(|error| error.to_ascii_lowercase().contains("ipykernel"))
    }

    pub fn body(&self, cx: &App) -> Body {
        let state = self.model.read(cx).state();
        match &state.snapshot {
            None if state.loading => Body::Loading,
            None => Body::Unavailable,
            Some(snapshot) if snapshot.cells.is_empty() => Body::NoCells,
            Some(_) => Body::Cells,
        }
    }

    /// Selects cell `id` for the toolbar and ⇧Enter.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.model.update(cx, |model, cx| {
            if model.selected.as_deref() != Some(id.as_str()) {
                model.selected = Some(id);
                cx.notify();
            }
        });
    }

    fn invoke(&self, op: NotebookOp, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.invoke(op, cx));
    }

    /// Carries out a toolbar button, as a click on it does.
    pub fn press(&mut self, id: &str, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.toolbar(cx).iter().any(|button| button.id == id && button.enabled) {
            return;
        }
        match id {
            RUN_CELL => self.run_active(cx),
            RUN_ALL => self.invoke(NotebookOp::Execute(None), cx),
            INTERRUPT => self.invoke(NotebookOp::Interrupt, cx),
            RESTART => self.invoke(NotebookOp::Restart, cx),
            PYTHON => self.show_python_menu(position, cx),
            REFRESH_PYTHON => self.model.update(cx, |model, cx| model.detect_environments(cx)),
            UNDO => self.invoke(NotebookOp::Undo, cx),
            REDO => self.invoke(NotebookOp::Redo, cx),
            SAVE => self.invoke(NotebookOp::Save, cx),
            RELOAD => self.ask_reload(window, cx),
            _ => {}
        }
    }

    fn run_active(&self, cx: &mut Context<Self>) {
        if let Some(cell) = self.active_code_cell(cx) {
            self.invoke(NotebookOp::Execute(Some(vec![cell])), cx);
        }
    }

    /// Reload Notebook from Disk asks first: it drops every unsaved change and stops execution.
    fn ask_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (title, body, discard, cancel) = {
            let i18n = &cx.default_global::<Localization>().0;
            (
                i18n.tx("Reload Notebook from Disk"),
                i18n.tx("Discard all unsaved notebook changes and stop execution? This cannot be undone."),
                i18n.tx("Discard Changes and Reload"),
                i18n.tx("Cancel"),
            )
        };
        let answer = window.prompt(PromptLevel::Warning, &title, Some(&body), &[discard.as_str(), cancel.as_str()], cx);
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |model, cx| model.discard_and_resume(cx));
            }
        })
        .detach();
    }

    fn show_python_menu(&self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let mut items: Vec<MenuItem<NotebookChoice>> = model
            .environments()
            .iter()
            .map(|sdk| MenuItem::entry(sdk.name.clone(), Some(NotebookChoice::Python(sdk.path.clone()))))
            .collect();
        if let Some(python) = model.state().python.clone()
            && !model.environments().iter().any(|sdk| sdk.path == python)
        {
            items.insert(0, MenuItem::entry(python.clone(), Some(NotebookChoice::Python(python))));
        }
        if !items.is_empty() {
            cx.emit(NotebookViewEvent::ShowMenu { items, position });
        }
    }

    /// The cell type menu of cell `id`: Code, Markdown, Raw, and the cell's own unknown type.
    pub(super) fn show_cell_type_menu(&self, cell: &ShownCell, position: Point<Pixels>, cx: &mut Context<Self>) {
        let mut types: Vec<(String, String)> = [("code", "Code"), ("markdown", "Markdown"), ("raw", "Raw")]
            .map(|(v, l)| (v.to_owned(), l.to_owned()))
            .into();
        if !["code", "markdown", "raw"].contains(&cell.cell_type.as_str()) {
            types.push((cell.cell_type.clone(), cell.cell_type.clone()));
        }
        let items = types
            .into_iter()
            .map(|(value, label)| {
                let choice = NotebookChoice::CellType { cell: cell.id.clone(), cell_type: value };
                MenuItem::entry(label, Some(choice))
            })
            .collect();
        cx.emit(NotebookViewEvent::ShowMenu { items, position });
    }

    /// The row chosen in one of the view's menus.
    pub fn choose(&mut self, choice: NotebookChoice, cx: &mut Context<Self>) {
        match choice {
            NotebookChoice::CellType { cell, cell_type } => {
                self.model.update(cx, |model, cx| model.edit(NotebookEdit::SetCellType { cell, cell_type }, cx));
            }
            NotebookChoice::Python(python) => self.model.update(cx, |model, cx| model.select_python(&python, cx)),
        }
    }

    /// Adds a cell of `cell_type` after the last one.
    pub fn add_cell(&mut self, cell_type: &str, cx: &mut Context<Self>) {
        let after = self.model.read(cx).acknowledged_cells().last().map(|cell| cell.id.clone());
        let edit = NotebookEdit::InsertCell { after, cell_type: cell_type.to_owned() };
        self.model.update(cx, |model, cx| model.edit(edit, cx));
    }

    /// Shows the source of Markdown cell `id` (or its preview again).
    pub fn set_markdown_editing(&mut self, id: &str, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            if editing {
                model.markdown_editing.insert(id.to_owned());
            } else {
                model.markdown_editing.remove(id);
            }
            cx.notify();
        });
        self.sync_editors(window, cx);
        if editing && let Some(editor) = self.editors.get(id) {
            window.focus(&editor.area.focus_handle(cx), cx);
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        match key_command(keystroke) {
            Some(KeyCommand::Save) => self.invoke(NotebookOp::Save, cx),
            Some(KeyCommand::Undo) => self.invoke(NotebookOp::Undo, cx),
            Some(KeyCommand::Redo) => self.invoke(NotebookOp::Redo, cx),
            Some(KeyCommand::RunActive) => {
                if self.active_code_cell(cx).is_none() {
                    return false;
                }
                self.run_active(cx);
            }
            None => return false,
        }
        true
    }

    /// Gives every cell that shows its source an editor holding it, read-only while the notebook
    /// is frozen; a source the backend changed replaces the editor's text. Then takes the cells as
    /// drawn from the model.
    pub(super) fn sync_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let (cells, frozen, editing) = {
            let model = model.read(cx);
            let pending = model.pending_sources();
            let cells: Vec<(String, String, String)> = model
                .acknowledged_cells()
                .iter()
                .map(|cell| {
                    let source = pending.iter().rev().find(|(id, _)| *id == cell.id).map_or(&cell.source, |(_, s)| s);
                    (cell.id.clone(), cell.cell_type.clone(), source.clone())
                })
                .collect();
            (cells, self.disabled(cx), model.markdown_editing.clone())
        };
        let shown: Vec<&(String, String, String)> = cells
            .iter()
            .filter(|(id, cell_type, _)| cell_type == "code" || (cell_type == "markdown" && editing.contains(id)))
            .collect();
        let ids: Vec<&str> = cells.iter().map(|(id, _, _)| id.as_str()).collect();
        self.markdown.retain(&ids);
        let keep: Vec<String> = shown.iter().map(|(id, _, _)| id.clone()).collect();
        let gone: Vec<String> = self.editors.keys().filter(|id| !keep.contains(id)).cloned().collect();
        for id in gone {
            if let Some(editor) = self.editors.remove(&id) {
                input::release_keys(editor.claim, cx);
            }
        }
        for (id, _, source) in shown {
            if !self.editors.contains_key(id) {
                let editor = self.cell_editor_for(id, source, window, cx);
                self.editors.insert(id.clone(), editor);
            }
            let Some(editor) = self.editors.get(id) else { continue };
            editor.area.update(cx, |area, cx| {
                if area.text() != source {
                    area.set_text(source, cx);
                }
                if area.is_read_only() != frozen {
                    area.set_read_only(frozen, cx);
                }
            });
        }
        self.sync_shown(&cells, cx);
    }

    /// The cells as drawn: outputs whose data changed are prepared again off the window's thread,
    /// previews whose source or attachments changed are rebuilt; the rest is kept as it is.
    fn sync_shown(&mut self, cells: &[(String, String, String)], cx: &mut Context<Self>) {
        enum Preview {
            None,
            Kept(Rc<Vec<PreviewBlock>>),
            Build(Value),
        }
        let live: HashSet<&str> = cells.iter().map(|(id, _, _)| id.as_str()).collect();
        self.outputs.retain(|id, _| live.contains(id.as_str()));
        let mut shown = Vec::with_capacity(cells.len());
        let mut previews = Vec::with_capacity(cells.len());
        let mut prepare = Vec::new();
        {
            let model = self.model.read(cx);
            for ((id, cell_type, source), cell) in cells.iter().zip(model.acknowledged_cells()) {
                let outputs = if cell_type == "code" && !cell.outputs.is_empty() {
                    match self.outputs.get(id).filter(|current| current.outputs == cell.outputs) {
                        Some(current) => current.shown.clone(),
                        None => {
                            self.next_generation += 1;
                            let generation = self.next_generation;
                            // What showed before stays until the new outputs are ready.
                            let before = self.outputs.get(id).map(|current| current.shown.clone()).unwrap_or_default();
                            let outputs = cell.outputs.clone();
                            prepare.push((id.clone(), generation, outputs.clone()));
                            self.outputs.insert(
                                id.clone(),
                                CellOutputs { outputs, generation, shown: before.clone(), ready: false },
                            );
                            before
                        }
                    }
                } else {
                    self.outputs.remove(id);
                    Rc::default()
                };
                let preview = if cell_type == "markdown" && !source.is_empty() {
                    match self.previews.get(id) {
                        Some(preview) if preview.source == *source && preview.attachments == cell.attachments => {
                            Preview::Kept(preview.shown.clone())
                        }
                        _ => Preview::Build(cell.attachments.clone()),
                    }
                } else {
                    Preview::None
                };
                previews.push(preview);
                shown.push(ShownCell {
                    id: id.clone(),
                    cell_type: cell_type.clone(),
                    source: source.clone(),
                    execution_count: cell.execution_count,
                    outputs,
                    preview: None,
                });
            }
        }
        let gone: Vec<String> = self.previews.keys().filter(|id| !live.contains(id.as_str())).cloned().collect();
        for id in gone {
            if let Some(preview) = self.previews.remove(&id) {
                self.forget_images(&preview.images);
            }
        }
        for (cell, preview) in shown.iter_mut().zip(previews) {
            cell.preview = match preview {
                Preview::Kept(blocks) => Some(blocks),
                Preview::Build(attachments) => Some(self.preview(&cell.id, &cell.source, attachments, cx)),
                Preview::None => {
                    if let Some(preview) = self.previews.remove(&cell.id) {
                        self.forget_images(&preview.images);
                    }
                    None
                }
            };
        }
        self.shown = shown;
        for (id, generation, outputs) in prepare {
            let prepared = cx.background_executor().spawn(async move { prepare_outputs(&outputs) });
            cx.spawn(async move |view, cx| {
                let (shown, decoded) = prepared.await;
                let _ = view.update(cx, |view, cx| {
                    let Some(current) = view.outputs.get_mut(&id).filter(|current| current.generation == generation)
                    else {
                        return;
                    };
                    let shown = Rc::new(shown);
                    current.shown = shown.clone();
                    current.ready = true;
                    view.decodes += decoded;
                    if let Some(cell) = view.shown.iter_mut().find(|cell| cell.id == id) {
                        cell.outputs = shown;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The preview of Markdown cell `id`, rebuilt only when its source or attachments changed; its
    /// embedded images are decoded off the window's thread.
    fn preview(&mut self, id: &str, source: &str, attachments: Value, cx: &mut Context<Self>) -> Rc<Vec<PreviewBlock>> {
        let built = self.markdown.blocks(id, source, &attachments);
        if let Some(preview) = self.previews.get_mut(id)
            && Rc::ptr_eq(&preview.built, &built)
        {
            source.clone_into(&mut preview.source);
            preview.attachments = attachments;
            return preview.shown.clone();
        }
        if let Some(preview) = self.previews.remove(id) {
            self.forget_images(&preview.images);
        }
        let mut blocks = (*built).clone();
        let mut started = Vec::new();
        for block in &mut blocks {
            images_mut(&mut block.nodes, &mut |image| {
                if let Some((format, data)) = data_url_image(&image.source) {
                    let key = self.next_image;
                    self.next_image += 1;
                    started.push((key, format, data.to_owned()));
                    image.source = image_source(key);
                }
            });
        }
        let keys: Vec<u64> = started.iter().map(|(key, _, _)| *key).collect();
        for (key, format, data) in started {
            self.decoding.insert(key);
            let decoded = cx.background_executor().spawn(async move { decode(&data, format) });
            cx.spawn(async move |view, cx| {
                let image = decoded.await;
                let _ = view.update(cx, |view, cx| {
                    // A preview rebuilt meanwhile no longer shows this image.
                    if view.decoding.remove(&key) {
                        view.decodes += 1;
                        if let Some(image) = image {
                            view.images.insert(key, image);
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let shown = Rc::new(blocks);
        let preview = CellPreview { source: source.to_owned(), attachments, built, shown: shown.clone(), images: keys };
        self.previews.insert(id.to_owned(), preview);
        shown
    }

    fn forget_images(&mut self, keys: &[u64]) {
        for key in keys {
            self.images.remove(key);
            self.decoding.remove(key);
        }
    }

    fn cell_editor_for(&mut self, id: &str, source: &str, window: &mut Window, cx: &mut Context<Self>) -> CellEditor {
        let area = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_text(source, cx);
            area.set_rows(2, 32, cx);
            area.set_code_font(true, cx);
            area
        });
        let cell = id.to_owned();
        let edited = cx.subscribe(&area, move |view, area, event: &TextAreaEvent, cx| {
            if *event == TextAreaEvent::Changed {
                let source = area.read(cx).text().to_owned();
                let edit = NotebookEdit::SetSource { cell: cell.clone(), source };
                view.model.update(cx, |model, cx| model.edit(edit, cx));
            }
        });
        let focus = area.focus_handle(cx);
        let id = id.to_owned();
        let focused = cx.on_focus_in(&focus, window, move |view, _, cx| view.select(&id, cx));
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            focus,
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        CellEditor { area, claim, _subscriptions: vec![edited, focused] }
    }

    fn tool_button(&self, button: &ToolbarButton, icon: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = button.id;
        let enabled = button.enabled;
        let color = theme::color(cx, "fg-muted");
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-toolbar-button"))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .when(!enabled, |el| el.opacity(0.4))
            .tooltip(tooltip(button.title))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    view.press(id, event.position, window, cx);
                }),
            )
            .child(theme::icon(cx, icon, 14.0, color))
    }

    fn text_button(
        &self,
        button: &ToolbarButton,
        icon: &'static str,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = button.id;
        let label = cx.default_global::<Localization>().0.tx(button.title);
        let (fg, bg) = if primary {
            (theme::color(cx, "fg-button-primary"), theme::color(cx, "bg-button-primary"))
        } else {
            (theme::color(cx, "fg-strong"), theme::color(cx, "bg-button"))
        };
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .h(px(24.0))
            .px(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .bg(bg)
            .text_color(fg)
            .when(!button.enabled, |el| el.opacity(0.5))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    view.press(id, event.position, window, cx);
                }),
            )
            .child(theme::icon(cx, icon, 12.0, fg))
            .child(SharedString::from(label))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let buttons = self.toolbar(cx);
        let find = |id: &str| buttons.iter().find(|button| button.id == id).cloned();
        let border = theme::color(cx, "border");
        let separator = move || div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(border);
        let python = self.model.read(cx).state().python.clone();
        let running = self.model.read(cx).state().running;
        let kernel = self.kernel_text(cx);
        let mut row = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(3.0))
            .min_h(px(36.0))
            .px(px(8.0))
            .py(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-editor-tabs"))
            .text_size(theme::length(cx, "fs-small"));
        if let Some(button) = find(RUN_CELL) {
            row = row.child(self.text_button(&button, "run", true, cx));
        }
        if let Some(button) = find(RUN_ALL) {
            row = row.child(self.text_button(&button, "run", false, cx));
        }
        for (id, icon) in [(INTERRUPT, "stop"), (RESTART, "refresh")] {
            if let Some(button) = find(id) {
                row = row.child(self.tool_button(&button, icon, cx));
            }
        }
        row = row.child(separator());
        if let Some(button) = find(PYTHON) {
            let label = python.unwrap_or_else(|| cx.default_global::<Localization>().0.tx("Select Python interpreter"));
            let enabled = button.enabled;
            row = row.child(
                div()
                    .id(PYTHON)
                    .debug_selector(|| PYTHON.to_owned())
                    .min_w(px(120.0))
                    .max_w(px(260.0))
                    .h(px(24.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .rounded(theme::length(cx, "radius-small"))
                    .when(!enabled, |el| el.opacity(0.5))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            view.press(PYTHON, event.position, window, cx);
                        }),
                    )
                    .child(SharedString::from(label)),
            );
        }
        if let Some(button) = find(REFRESH_PYTHON) {
            row = row.child(self.tool_button(&button, "refresh", cx));
        }
        let error = self.error_text(cx);
        let muted = theme::color(cx, "fg-muted");
        let busy = running.then(|| theme::icon(cx, "refresh", 12.0, muted));
        row = row.child(
            div()
                .id("notebook-kernel")
                .debug_selector(|| "notebook-kernel".to_owned())
                .flex()
                .items_center()
                .gap(px(5.0))
                .px(px(6.0))
                .text_color(muted)
                .when_some(error, |el, error| el.tooltip(tooltip(error)))
                .children(busy)
                .child(SharedString::from(kernel)),
        );
        row = row.child(div().flex_1());
        for (id, icon) in [(UNDO, "undo"), (REDO, "redo"), (SAVE, "save"), (RELOAD, "refresh")] {
            if let Some(button) = find(id) {
                row = row.child(self.tool_button(&button, icon, cx));
            }
        }
        row
    }

    fn render_error(&self, cx: &mut Context<Self>) -> Option<Div> {
        let error = self.error_text(cx)?;
        let model = self.model.read(cx);
        let pending = model.state().pending;
        let loaded = model.state().snapshot.is_some();
        let sources = model.pending_sources();
        let help = self.shows_kernel_help(cx);
        let (help_text, details) = {
            let i18n = &cx.default_global::<Localization>().0;
            (
                i18n.tx(
                    "Install ipykernel in the selected Python environment, then restart the kernel. No packages are installed automatically.",
                ),
                i18n.tx("Uncommitted source edits"),
            )
        };
        let mono = super::cells::mono_font(cx);
        let mut message = div()
            .debug_selector(|| "notebook-error".to_owned())
            .flex_1()
            .min_w_0()
            .whitespace_normal()
            .child(SharedString::from(error));
        if help {
            message =
                message.child(div().debug_selector(|| "notebook-kernel-help".to_owned()).mt(px(6.0)).child(help_text));
        }
        if pending > 0 {
            message =
                message.child(div().mt(px(6.0)).child(details)).children(sources.into_iter().map(|(_, source)| {
                    div().max_h(px(200.0)).overflow_hidden().font(mono.clone()).child(SharedString::from(source))
                }));
        }
        let retry = |id: &'static str, title: &'static str, op: NotebookOp, cx: &mut Context<Self>| {
            let label = cx.default_global::<Localization>().0.tx(title);
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .flex_none()
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "bg-button"))
                .text_color(theme::color(cx, "fg-strong"))
                .on_click(cx.listener(move |view, _, _, cx| view.invoke(op.clone(), cx)))
                .child(SharedString::from(label))
        };
        let mut bar = div()
            .flex_none()
            .flex()
            .items_start()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(8.0))
            .text_color(theme::color(cx, "danger"))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(message);
        if pending > 0 {
            bar = bar.child(retry("notebook-retry-edits", "Retry Pending Edits", NotebookOp::Flush, cx));
        }
        if !loaded {
            bar = bar.child(retry("notebook-retry", "Retry", NotebookOp::Load, cx));
        }
        Some(bar)
    }
}

impl Focusable for NotebookView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotebookView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let error = self.render_error(cx);
        let body: AnyElement = self.render_body(cx);
        div()
            .debug_selector(|| "notebook".to_owned())
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(toolbar)
            .children(error)
            .child(body)
    }
}
