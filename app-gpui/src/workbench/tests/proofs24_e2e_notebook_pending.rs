//! Not declared: these flows need a kernel the test owns, and the backend can only start a real
//! Jupyter kernel (`<python> -m ipykernel_launcher -f <connection file>`, ZeroMQ with signed
//! messages on five loopback ports), which no test may start.
//!
//! They wait for a seam in `crates/ide-notebook`: `KernelManager::with_launcher(runtime_dir,
//! on_status, launcher)` taking an `Arc<dyn KernelLauncher>` whose default starts the process as
//! today, and a `NotebookService::with_kernels(…)` constructor the project would use in tests. A
//! launcher the test owns would answer `execute` with the outputs below without any process.
//! Once it exists the test's project installs it before opening the notebook.

use super::*;
use crate::notebook::view::NotebookView;

const NOTEBOOK: &str = r##"{
 "cells": [
  {"cell_type": "code", "execution_count": null, "id": "c1", "metadata": {}, "outputs": [], "source": ["print('hi')"]},
  {"cell_type": "code", "execution_count": null, "id": "c2", "metadata": {}, "outputs": [], "source": ["1/0"]}
 ],
 "metadata": {}, "nbformat": 4, "nbformat_minor": 5
}
"##;

/// Run All with a kernel that prints `hi` for the first cell and raises ZeroDivisionError for the
/// second: the first cell shows `[1]` and its output, the second `[2]` and the coloured traceback,
/// and the notebook is unsaved with the outputs in it.
#[gpui_kit::test]
async fn run_all_shows_each_cells_execution_count_and_outputs(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.ipynb", NOTEBOOK)]);
    // Needs: the project's notebook service built with a launcher answering
    //   c1 → execute_input {execution_count: 1}, stream stdout "hi\n", status ok
    //   c2 → execute_input {execution_count: 2}, error {ename: ZeroDivisionError, traceback: ["\u{1b}[31mZeroDivisionError\u{1b}[0m"]}
    crate::notebook::test_support::install_kernel_launcher(&root, cx);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.ipynb"), window, cx));
    wait_long(cx, "the notebook", |cx| workbench.read_with(cx, |w, _| w.notebook_view()).is_some());
    let view: Entity<NotebookView> = workbench.read_with(cx, |w, _| w.notebook_view()).expect("a notebook");
    click_on(cx, "notebook-run-all");
    wait_long(cx, "the outputs", |cx| {
        view.read_with(cx, |view, cx| {
            view.output_texts("c1", cx) == ["hi\n"] && !view.output_texts("c2", cx).is_empty()
        })
    });
    let counts = view.read_with(cx, |view, cx| {
        view.model().read(cx).cells().iter().map(|cell| cell.execution_count).collect::<Vec<_>>()
    });
    assert_eq!(counts, [Some(1), Some(2)]);
    assert_eq!(view.read_with(cx, |view, cx| view.output_texts("c2", cx)), ["ZeroDivisionError"]);
    assert!(view.read_with(cx, |view, cx| view.model().read(cx).is_dirty()));
}
