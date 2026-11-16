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
use ide_notebook::{NotebookCell, NotebookEdit};
use std::collections::HashMap;
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
    /// Decoded output and attachment images by their base64 data; `None` for data GPUI cannot draw.
    pub(super) images: HashMap<String, Option<Arc<GpuiImage>>>,
    pub(super) decodes: usize,
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
            images: HashMap::new(),
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

    /// The toolbar's state is the frozen or unloaded notebook's: nothing can be done.
    pub(super) fn disabled(&self, cx: &App) -> bool {
        let state = self.model.read(cx).state();
        state.frozen || state.snapshot.is_none()
    }

    /// The cell the toolbar and ⇧Enter act on: the selected one, else the first code cell.
    pub fn active_cell(&self, cx: &App) -> Option<NotebookCell> {
        let model = self.model.read(cx);
        let cells = model.cells();
        let selected = model.selected.as_ref().and_then(|id| cells.iter().find(|cell| cell.id == *id)).cloned();
        selected.or_else(|| cells.into_iter().find(|cell| cell.cell_type == "code"))
    }

    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarButton> {
        let model = self.model.read(cx);
        let state = model.state();
        let disabled = self.disabled(cx);
        let running = state.running;
        let history_disabled = disabled || running;
        let active_code = self.active_cell(cx).is_some_and(|cell| cell.cell_type == "code");
        let any_code = model.cells().iter().any(|cell| cell.cell_type == "code");
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
        if let Some(cell) = self.active_cell(cx).filter(|cell| cell.cell_type == "code") {
            self.invoke(NotebookOp::Execute(Some(vec![cell.id])), cx);
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
    pub(super) fn show_cell_type_menu(&self, cell: &NotebookCell, position: Point<Pixels>, cx: &mut Context<Self>) {
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
        let after = self.model.read(cx).cells().last().map(|cell| cell.id.clone());
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
                if !self.active_cell(cx).is_some_and(|cell| cell.cell_type == "code") {
                    return false;
                }
                self.run_active(cx);
            }
            None => return false,
        }
        true
    }

    /// Gives every cell that shows its source an editor holding it, read-only while the notebook
    /// is frozen; a source the backend changed replaces the editor's text.
    pub(super) fn sync_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (cells, frozen, editing) = {
            let model = self.model.read(cx);
            (model.cells(), self.disabled(cx), model.markdown_editing.clone())
        };
        let shown: Vec<&NotebookCell> = cells
            .iter()
            .filter(|cell| cell.cell_type == "code" || (cell.cell_type == "markdown" && editing.contains(&cell.id)))
            .collect();
        let ids: Vec<&str> = cells.iter().map(|cell| cell.id.as_str()).collect();
        self.markdown.retain(&ids);
        let keep: Vec<String> = shown.iter().map(|cell| cell.id.clone()).collect();
        let gone: Vec<String> = self.editors.keys().filter(|id| !keep.contains(id)).cloned().collect();
        for id in gone {
            if let Some(editor) = self.editors.remove(&id) {
                input::release_keys(editor.claim, cx);
            }
        }
        for cell in shown {
            if !self.editors.contains_key(&cell.id) {
                let editor = self.cell_editor_for(cell, window, cx);
                self.editors.insert(cell.id.clone(), editor);
            }
            let Some(editor) = self.editors.get(&cell.id) else { continue };
            editor.area.update(cx, |area, cx| {
                if area.text() != cell.source {
                    area.set_text(&cell.source, cx);
                }
                if area.is_read_only() != frozen {
                    area.set_read_only(frozen, cx);
                }
            });
        }
    }

    fn cell_editor_for(&mut self, cell: &NotebookCell, window: &mut Window, cx: &mut Context<Self>) -> CellEditor {
        let area = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_text(&cell.source, cx);
            area.set_rows(2, 32, cx);
            area
        });
        let id = cell.id.clone();
        let edited = cx.subscribe(&area, move |view, area, event: &TextAreaEvent, cx| {
            if *event == TextAreaEvent::Changed {
                let source = area.read(cx).text().to_owned();
                let edit = NotebookEdit::SetSource { cell: id.clone(), source };
                view.model.update(cx, |model, cx| model.edit(edit, cx));
            }
        });
        let focus = area.focus_handle(cx);
        let id = cell.id.clone();
        let focused = cx.on_focus_in(&focus, window, move |view, _, cx| view.select(&id, cx));
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            focus,
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        CellEditor { area, claim, _subscriptions: vec![edited, focused] }
    }

    /// The image of base64 `data` in `format`, decoded once for every output and preview showing it.
    pub(super) fn image(&mut self, data: &str, format: ImageFormat) -> Option<Arc<GpuiImage>> {
        if let Some(decoded) = self.images.get(data) {
            return decoded.clone();
        }
        self.decodes += 1;
        let decoded =
            super::output_items::image_bytes(data).map(|bytes| Arc::new(GpuiImage::from_bytes(format, bytes)));
        self.images.insert(data.to_owned(), decoded.clone());
        decoded
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
