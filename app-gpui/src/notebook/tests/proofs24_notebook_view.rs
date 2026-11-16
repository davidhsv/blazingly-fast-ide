//! The notebook editor drawn from a model filled by hand: its toolbar and cell toolbars follow the
//! notebook's state, keys and clicks start the notebook operations (the backend calls the model
//! sends are recorded, and answered where a test needs it), Markdown cells switch between preview
//! and source, raw cells stay read-only, and the error bar says what failed.

use crate::features_a::notebook::session::{Answer, Call, Request};
use crate::features_b::viewers::render::{LinkTarget, resolve_link};
use crate::notebook::model::{NotebookModel, NotebookModelEvent};
use crate::notebook::view::{NotebookView, NotebookViewEvent};
use crate::platform::input;
use crate::theme;
use gpui_kit::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use ide_core::IpcPath;
use ide_notebook::{NotebookCell, NotebookEdit, NotebookSnapshot};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

type Calls = Rc<RefCell<Vec<Request>>>;
type Events = Rc<RefCell<Vec<NotebookViewEvent>>>;

fn cell(id: &str, cell_type: &str, source: &str, outputs: Vec<Value>) -> NotebookCell {
    NotebookCell {
        id: id.to_owned(),
        cell_type: cell_type.to_owned(),
        source: source.to_owned(),
        metadata: json!({}),
        outputs,
        execution_count: None,
        attachments: json!({}),
    }
}

fn snapshot(cells: Vec<NotebookCell>, version: u32) -> NotebookSnapshot {
    NotebookSnapshot {
        id: "document-a".to_owned(),
        path: IpcPath(PathBuf::from("/p/a.ipynb")),
        version,
        dirty: false,
        can_undo: false,
        can_redo: false,
        nbformat: 4,
        nbformat_minor: 5,
        metadata: json!({}),
        cells,
    }
}

struct Open<'a> {
    view: Entity<NotebookView>,
    model: Entity<NotebookModel>,
    calls: Calls,
    events: Events,
    cx: &'a mut VisualTestContext,
}

fn open(cx: &mut TestAppContext, cells: Vec<NotebookCell>) -> Open<'_> {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        let model = cx.new(|cx| NotebookModel::new("/p/a.ipynb", cx));
        NotebookView::new(model, window, cx)
    });
    cx.update(|window, _| window.activate_window());
    let model = view.read_with(cx, |view, _| view.model().clone());
    let calls: Calls = Rc::default();
    let events: Events = Rc::default();
    let (seen_calls, seen_events) = (calls.clone(), events.clone());
    cx.update(|_, cx| {
        cx.subscribe(&model, move |_, event: &NotebookModelEvent, _| match event {
            NotebookModelEvent::Send(requests) => seen_calls.borrow_mut().extend(requests.iter().cloned()),
        })
        .detach();
        cx.subscribe(&view, move |_, event: &NotebookViewEvent, _| seen_events.borrow_mut().push(event.clone()))
            .detach();
    });
    model.update(cx, |model, cx| model.load_opened(snapshot(cells, 1), cx));
    cx.run_until_parked();
    Open { view, model, calls, events, cx }
}

fn names(calls: &Calls) -> Vec<&'static str> {
    calls
        .borrow()
        .iter()
        .map(|request| match &request.call {
            Call::Open { .. } => "open",
            Call::Get { .. } => "get",
            Call::Edit { .. } => "edit",
            Call::Undo { .. } => "undo",
            Call::Redo { .. } => "redo",
            Call::Save { .. } => "save",
            Call::Reload { .. } => "reload",
            Call::Close { .. } => "close",
            Call::Interrupt { .. } => "interrupt",
            Call::Cancel { .. } => "cancel",
            Call::Execute { .. } => "execute",
            Call::Restart { .. } => "restart",
            Call::Environments => "environments",
        })
        .collect()
}

fn last_call(calls: &Calls) -> Option<Call> {
    calls.borrow().last().map(|request| request.call.clone())
}

fn enabled(open: &mut Open<'_>, id: &str) -> bool {
    let view = open.view.clone();
    view.read_with(open.cx, |view, cx| view.toolbar(cx).iter().any(|button| button.id == id && button.enabled))
}

fn click(open: &mut Open<'_>, selector: &'static str) {
    open.cx.run_until_parked();
    let bounds = open.cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    open.cx.simulate_click(bounds.center(), Modifiers::default());
    open.cx.run_until_parked();
}

fn focus_cell(open: &mut Open<'_>, id: &str) {
    let area = open.view.read_with(open.cx, |view, _| view.cell_editor(id)).expect("the cell shows an editor");
    open.cx.update(|window, cx| window.focus(&area.focus_handle(cx), cx));
    open.cx.run_until_parked();
}

/// The toolbar of a loaded notebook: Run Cell and Run All on (a code cell is selected by default),
/// Interrupt Kernel off; while a batch runs Run Cell, Run All, Undo and the interpreter are off,
/// Interrupt Kernel on, and the kernel shows busy.
#[gpui_kit::test]
async fn the_toolbar_follows_the_notebooks_state(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("c1", "code", "print(1)", Vec::new())]);
    assert!(enabled(&mut open, "notebook-run-cell") && enabled(&mut open, "notebook-run-all"));
    assert!(!enabled(&mut open, "notebook-interrupt") && enabled(&mut open, "notebook-restart"));
    assert!(!enabled(&mut open, "notebook-undo") && !enabled(&mut open, "notebook-redo"));
    click(&mut open, "notebook-run-cell");
    assert!(matches!(last_call(&open.calls), Some(Call::Execute { cells, .. }) if cells == ["c1"]));
    assert!(!enabled(&mut open, "notebook-run-cell") && !enabled(&mut open, "notebook-run-all"));
    assert!(enabled(&mut open, "notebook-interrupt") && !enabled(&mut open, "notebook-python"));
    let view = open.view.clone();
    assert_eq!(view.update(open.cx, |view, cx| view.kernel_text(cx)), "busy");
}

/// ⇧Enter in a code cell's editor runs that cell; in a Markdown cell's source it breaks the line.
#[gpui_kit::test]
async fn shift_enter_runs_the_selected_code_cell_and_breaks_the_line_elsewhere(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("c1", "code", "a = 1", Vec::new()), cell("c2", "code", "b = 2", Vec::new())]);
    focus_cell(&mut open, "c2");
    open.cx.simulate_keystrokes("shift-enter");
    assert!(matches!(last_call(&open.calls), Some(Call::Execute { cells, .. }) if cells == ["c2"]));
    let area = open.view.read_with(open.cx, |view, _| view.cell_editor("c2")).expect("an editor");
    assert_eq!(area.read_with(open.cx, |area, _| area.text().to_owned()), "b = 2", "no line break was typed");
}

/// Typing in a cell queues a source edit of that cell against the notebook's version; ⌘Z then
/// asks the notebook's history to undo instead of the editor's own.
#[gpui_kit::test]
async fn typing_queues_a_source_edit_and_cmd_z_undoes_in_the_notebook_history(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("c1", "code", "", Vec::new())]);
    focus_cell(&mut open, "c1");
    open.cx.simulate_input("x");
    let edit = last_call(&open.calls);
    assert!(
        matches!(&edit, Some(Call::Edit { version: 1, edit: NotebookEdit::SetSource { cell, source }, .. })
            if cell == "c1" && source == "x"),
        "{edit:?}"
    );
    let ticket = open.calls.borrow().last().map(|request| request.ticket).expect("a ticket");
    let mut acknowledged = snapshot(vec![cell("c1", "code", "x", Vec::new())], 2);
    acknowledged.can_undo = true;
    open.model.update(open.cx, |model, cx| model.answer(ticket, Ok(Answer::Snapshot(Box::new(acknowledged))), cx));
    open.cx.simulate_keystrokes("cmd-z");
    assert_eq!(names(&open.calls).last(), Some(&"undo"));
    let area = open.view.read_with(open.cx, |view, _| view.cell_editor("c1")).expect("an editor");
    assert_eq!(area.read_with(open.cx, |area, _| area.text().to_owned()), "x", "the editor's own undo did not run");
}

/// A cell's toolbar moves it up and down (not past either end), inserts a code cell below it,
/// deletes it and changes its type; while a batch runs none of them can be used.
#[gpui_kit::test]
async fn the_cell_toolbar_moves_inserts_deletes_and_retypes_cells(cx: &mut TestAppContext) {
    let mut open = open(
        cx,
        vec![
            cell("c1", "code", "a", Vec::new()),
            cell("c2", "code", "b", Vec::new()),
            cell("m", "markdown", "", Vec::new()),
        ],
    );
    let edit_of = |calls: &Calls| match last_call(calls) {
        Some(Call::Edit { edit, .. }) => Some(edit),
        _ => None,
    };
    click(&mut open, "notebook-cell-0-up");
    assert!(edit_of(&open.calls).is_none(), "the first cell does not move up");
    click(&mut open, "notebook-cell-0-down");
    assert!(
        matches!(edit_of(&open.calls), Some(NotebookEdit::MoveCell { cell, before: Some(before) }) if cell == "c1" && before == "m")
    );
    let ticket = open.calls.borrow().last().map(|request| request.ticket).expect("a ticket");
    let moved = snapshot(
        vec![
            cell("c2", "code", "b", Vec::new()),
            cell("c1", "code", "a", Vec::new()),
            cell("m", "markdown", "", Vec::new()),
        ],
        2,
    );
    open.model.update(open.cx, |model, cx| model.answer(ticket, Ok(Answer::Snapshot(Box::new(moved))), cx));
    click(&mut open, "notebook-cell-1-insert");
    assert!(
        matches!(edit_of(&open.calls), Some(NotebookEdit::InsertCell { after: Some(after), cell_type }) if after == "c1" && cell_type == "code")
    );
    open.calls.borrow_mut().clear();
    click(&mut open, "notebook-cell-2-delete");
    // The insert is still being sent: the delete waits behind it in the queue.
    assert!(open.calls.borrow().is_empty());
    let pending = open.model.read_with(open.cx, |model, _| model.state().pending);
    assert_eq!(pending, 2);
    let bounds = open.cx.debug_bounds("notebook-cell-0-type").expect("the type is drawn");
    open.cx.simulate_click(bounds.center(), Modifiers::default());
    let shown = open
        .events
        .borrow()
        .iter()
        .any(|event| matches!(event, NotebookViewEvent::ShowMenu { items, .. } if items.len() == 3));
    assert!(shown, "the cell type menu lists Code, Markdown and Raw");
}

/// A Markdown cell shows its preview; Edit Markdown shows its source in an editor and Preview
/// Markdown the preview again. An empty Markdown cell offers Edit Markdown in its place.
#[gpui_kit::test]
async fn markdown_cells_switch_between_preview_and_source(cx: &mut TestAppContext) {
    let mut open = open(
        cx,
        vec![cell("m1", "markdown", "# Title\n\nSome *text*", Vec::new()), cell("m2", "markdown", "", Vec::new())],
    );
    let view = open.view.clone();
    let preview = view.read_with(open.cx, |view, cx| view.preview_text("m1", cx)).expect("a preview");
    assert_eq!(preview, ["Title", "Some text"]);
    assert!(view.read_with(open.cx, |view, _| view.cell_editor("m1")).is_none());
    assert!(open.cx.debug_bounds("notebook-cell-1-edit-markdown").is_some());
    click(&mut open, "notebook-cell-0-markdown");
    let area = view.read_with(open.cx, |view, _| view.cell_editor("m1")).expect("the source is edited");
    assert_eq!(area.read_with(open.cx, |area, _| area.text().to_owned()), "# Title\n\nSome *text*");
    click(&mut open, "notebook-cell-0-markdown");
    assert!(view.read_with(open.cx, |view, _| view.cell_editor("m1")).is_none());
}

/// A raw cell and a cell of an unknown type show their source read-only under a notice; only
/// the raw cell's type can be changed.
#[gpui_kit::test]
async fn raw_and_unknown_cells_are_shown_read_only(cx: &mut TestAppContext) {
    let mut open =
        open(cx, vec![cell("r", "raw", "raw text", Vec::new()), cell("v", "vendor.future", "opaque", Vec::new())]);
    assert!(
        open.cx.debug_bounds("notebook-cell-0-raw").is_some() && open.cx.debug_bounds("notebook-cell-1-raw").is_some()
    );
    let view = open.view.clone();
    assert!(view.read_with(open.cx, |view, _| view.cell_editor("r").is_none() && view.cell_editor("v").is_none()));
    for selector in ["notebook-cell-0-type", "notebook-cell-1-type"] {
        let bounds = open.cx.debug_bounds(selector).expect("the type is drawn");
        open.cx.simulate_click(bounds.center(), Modifiers::default());
    }
    let menus = open.events.borrow().iter().filter(|event| matches!(event, NotebookViewEvent::ShowMenu { .. })).count();
    assert_eq!(menus, 1, "an unknown cell's type cannot be changed");
}

/// A kernel failure naming ipykernel shows in the error bar with the install hint; the kernel
/// state follows the backend's event.
#[gpui_kit::test]
async fn a_kernel_failure_naming_ipykernel_shows_the_install_hint(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("c1", "code", "", Vec::new())]);
    let message = "No module named ipykernel_launcher";
    open.model.update(open.cx, |model, cx| model.kernel_event("document-a", "/p/a.ipynb", "dead", Some(message), cx));
    let view = open.view.clone();
    assert_eq!(view.read_with(open.cx, |view, cx| view.error_text(cx)).as_deref(), Some(message));
    assert!(view.read_with(open.cx, |view, cx| view.shows_kernel_help(cx)));
    assert!(open.cx.debug_bounds("notebook-kernel-help").is_some());
    assert_eq!(view.update(open.cx, |view, cx| view.kernel_text(cx)), "dead");
}

/// Reload Notebook from Disk asks before it discards: Cancel sends nothing, Discard Changes and
/// Reload reloads the document.
#[gpui_kit::test]
async fn reload_from_disk_asks_before_it_discards(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("c1", "code", "", Vec::new())]);
    click(&mut open, "notebook-reload");
    open.cx.simulate_prompt_answer("Cancel");
    open.cx.run_until_parked();
    assert!(!names(&open.calls).contains(&"reload"));
    click(&mut open, "notebook-reload");
    open.cx.simulate_prompt_answer("Discard Changes and Reload");
    open.cx.run_until_parked();
    assert_eq!(names(&open.calls).last(), Some(&"reload"));
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › opens web links with the system handler
//
// The view hands the link to its window, which opens a web page with the system's handler.
#[gpui_kit::test]
async fn opens_web_links_with_the_system_handler(cx: &mut TestAppContext) {
    let mut open = open(cx, vec![cell("m1", "markdown", "[docs](https://example.com/docs)", Vec::new())]);
    click(&mut open, "notebook-cell-0-text-0");
    let events = open.events.borrow().clone();
    let link = events.iter().find_map(|event| match event {
        NotebookViewEvent::OpenLink { notebook, href } => Some((notebook.clone(), href.clone())),
        NotebookViewEvent::ShowMenu { .. } => None,
    });
    let (notebook, href) = link.expect("the link is followed");
    assert_eq!(href, "https://example.com/docs");
    assert!(matches!(resolve_link(&notebook, &href), Some(LinkTarget::External { url }) if url == href));
}

// src/features/notebook/OutputView.test.tsx › notebook image outputs › does not re-sanitize an unchanged SVG when
// a fresh snapshot arrives
//
// SVG is not drawn; the image work that must not repeat is the decoding of a PNG output.
#[gpui_kit::test]
async fn does_not_re_sanitize_an_unchanged_svg_when_a_fresh_snapshot_arrives(cx: &mut TestAppContext) {
    let png = |data: &str| json!({ "output_type": "display_data", "data": { "image/png": data }, "metadata": {} });
    let open = open(cx, vec![cell("c1", "code", "plot()", vec![png("iVBORw0KGgo=")])]);
    let view = open.view.clone();
    open.cx.run_until_parked();
    assert_eq!(view.read_with(open.cx, |view, _| view.image_decodes()), 1);
    // A fresh notebook with the same output.
    let fresh = snapshot(vec![cell("c1", "code", "plot()", vec![png("iVBORw0KGgo=")])], 2);
    open.model.update(open.cx, |model, cx| model.load_opened(fresh, cx));
    open.cx.run_until_parked();
    assert_eq!(view.read_with(open.cx, |view, _| view.image_decodes()), 1);
    let changed = snapshot(vec![cell("c1", "code", "plot()", vec![png("AAAA")])], 3);
    open.model.update(open.cx, |model, cx| model.load_opened(changed, cx));
    open.cx.run_until_parked();
    assert_eq!(view.read_with(open.cx, |view, _| view.image_decodes()), 2);
}
