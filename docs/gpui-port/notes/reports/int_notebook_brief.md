# Integrator brief: the notebook editor for `.ipynb` files (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dnbi/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## The slice

Notes (seven hunks in the order to apply them with quoted anchors, `mod` lines, the rows expected with every test's full name, each behaviour of the webview's notebook editor with `file:line` and built / left out, the hypotheses and how to settle each, the tests most likely to fail, what the real window must be checked for): `$S/drafts/notebook_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Writer's report: `docs/gpui-port/notes/reports/notebook_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/notebook/{mod,model,view,cells,output_items,ansi,markdown_cell}.rs`, `app-gpui/src/notebook/tests/proofs24_notebook_{units,view}.rs`, `app-gpui/src/workbench/notebook.rs`, `app-gpui/src/workbench/tests/proofs24_e2e_notebook.rs`, and `app-gpui/src/workbench/tests/proofs24_e2e_notebook_pending.rs` (stays undeclared unless what it waits for gets built).

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/notebook/*.ts(x)`, their four test files, `Notebook.module.css`; the backend `crates/ide-notebook/`, `crates/ide-host/src/ops/notebook.rs`; the Tauri adapters `src-tauri/src/commands/notebook.rs`). Nothing of this was compiled: expect signature errors. Apply in stages, compiling and running each stage's tests by filter before the next: (1) the module with its unit tests (`ansi`, `output_items`, `markdown_cell`, `proofs24_notebook_units`); (2) the view and its tests; (3) the workbench wiring, the tab kind and the `.ipynb` routing, with the end-to-end tests; (4) Save All and Edit › Undo / Redo. Settle each of the writer's hypotheses (section (d) of the notes) by test or by reading, and say which way each went.

Decisions already made, do not reopen them:

- Cells are edited in the app's multi-line text field, because the app's code editor needs a document the backend opened from a file. An editable in-memory document is a later feature of its own; do not build it and do not touch `crates/`.
- No seam is added to `crates/ide-notebook` in this slice: a successful run and a restart of a live kernel stay untested, and their rows stay partial.
- The kernel binding ports on 127.0.0.1 inside a test is allowed (loopback only). Anything that would leave the machine is not.

Small additions the drafts do not have, in this order of value; build one only if it is small and local, otherwise list it as missing:

- Syntax colours in a code cell: the highlights of `Host::syntax()` for the cell's language (the call the diff sides and the Paste from History preview use, off the window's thread, a stale answer dropped), drawn through the text field's coloured ranges if it has them (`app-gpui/src/ui/text_field.rs`: `TextTag`, `set_tags`). If the text field cannot colour ranges without a change to how it paints, leave it out and say so.
- The corrections to the existing models the notes list (`app-gpui/src/features_a/notebook/`): apply those that the tests need; say which you left.

## Safety (hard)

- A notebook file is data from anywhere. Nothing named in a notebook is fetched, run or opened by drawing it. Check by test, not by reading alone: an output or a Markdown cell with a remote image, a script, a form, an iframe, an `on…` attribute and a `javascript:` link draws text only and asks the window for nothing; SVG is never drawn. A corrupt or truncated image does not panic and is decoded off the window's thread; a very large image does not block the window (say what happens with a 50 MB payload: measured, with a generated file in the temp folder).
- A link in a Markdown cell or an output is opened only by a click of the user, and no test opens the system's browser or any program: read how other tests observe an opened address (grep `open_url` in `app-gpui/src/workbench` tests) and use the same seam.
- Tests never start the machine's Python, a Jupyter kernel or anything under the user's home. The drafts answer the interpreter lookup through the test build's `SdkDetection` global with a script inside the temp folder: check that no test can fall through to a search of the machine (a test without the global set must find no interpreter, and that is asserted once). Every stand-in process a test starts (`sleep 60`) is gone when the test ends, pass or fail: after your last whole-suite run, `pgrep -fl 'sleep 60'` prints nothing of yours (quote it).
- A notebook in a project that is not trusted starts no process: the test asserts the marker file was never written.
- The real binary: only through `$S/insp8/run.sh` (short `PATH`, temp `HOME`, `ZDOTDIR`, `OPENIDEA_GPUI_HOME`, temp project). Before you open a notebook in it, read what opening one starts: if showing a notebook looks up interpreters (runs any program of the machine, `/usr/bin/python3` included, which on macOS can raise a system dialog), make the inspection script's `no-toolchains` step cover that lookup too (debug builds only, `app-gpui/src/inspect.rs`), put that step first, and say how you know nothing was started (the app's log, the process list of your own PID's children). If you cannot show that, do not open a notebook in the real binary and say so. Never run a cell, restart a kernel or pick an interpreter in the real binary. Do not open the Agents tool window or the Services window in it.
- With that settled, look at your own binary's window: a notebook with a code cell, a Markdown cell (rendered and being edited), a stream output, an error output with colours and a small PNG output (write the `.ipynb` yourself in the temp project; no real output of the machine), in the light theme and the dark one. Look at each shot: spacing between cells, the toolbar, the selected cell's mark, nothing cut or overlapping, the image at its size.

## Changed since the notes were written

BEFORE LAUNCH (for the manager; delete this paragraph then): compute the existing files the hunks anchor on (`lib.rs`, `workbench/mod.rs`, `workbench/tab_views.rs`, `workbench/edit_actions.rs`) against the tree the notes were written on (commit 4a3fd6fe) and say here which changed.

Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name (`point.registerEditorKind:notebook`, the file editor provider `notebook`, `feature:notebook`, the eleven `ipc:notebook_*` keys, the `NotebookKernel` event, `unit:src/features/notebook/{OutputView.test.tsx,security.test.ts,outputs.test.ts,session.test.ts}`) with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, for a `unit:` row "N of M cases" with each case's native test and, where a native test checks a neighbour of the spec's case rather than the case itself, say so (such a row is partial); an `ipc:` row is done only when a test asserts state that can only come from that backend function called as the Tauri adapter calls it. Then: every behaviour of the webview's notebook editor you saw missing natively; which way each hypothesis went; the shots' absolute paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you (among them `platform/error_log_sink.rs` and whatever of `app-gpui/src/deployment/`, `app-gpui/src/http_client/` is still undeclared when you start). No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
