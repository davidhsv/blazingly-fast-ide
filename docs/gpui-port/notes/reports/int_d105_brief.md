# Integrator brief: the `proofs13` test batches (77 tests written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md` (how these tests are packaged and what a test must prove), `$S/view_rules.md`. Your own scratch subfolder: `$S/d105/`.

Format only files you edited, taken from `git diff --name-only` plus the `proofs13_*.rs` files this brief hands you. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish both batches. Stop early only if a batch is blocked, and say what blocks it. Work one file at a time (declare, compile, run that module's tests by filter, fix, next).

## The two batches

1. App shell: notes `docs/gpui-port/notes/integration/proofs13_app.md`, writer's report `docs/gpui-port/notes/reports/draft_proofs13_app.md`. 43 tests in 9 files: `app-gpui/src/workbench/tests/proofs13_{close_guard,light_edit_close,ide_mode,shell,windows,tasks,focus,database}.rs`, `app-gpui/src/workbench/editor_extras/tests/proofs13_editor.rs`. Needs the `mod` lines and two test-only accessors: `PromptDialog::body()` (`vcs/prompt_dialogs.rs`) and `TableEditorView::filter_options(kind)` (`database/table_editor_view.rs`).
2. Agents: notes `docs/gpui-port/notes/integration/proofs13_agents.md`, report `docs/gpui-port/notes/reports/draft_proofs13_agents.md`. 34 tests in 9 files plus fixtures: `app-gpui/src/workbench/tests/proofs13_agents_{fixtures,images,failed_send,stale_editor,delete_failure,config,new_task,pending}.rs`, `app-gpui/src/agents/session_view/tests/proofs13_agents_{session,session_pending}.rs`. Needs the `mod` lines, hunk A (`workbench/agents.rs`: a test-only global that makes a message send fail before it reaches the backend) and hunk B (`PromptDialog::message_text()`). Hunk B and the first batch's `PromptDialog::body()` ask for the same thing: add ONE accessor and make both batches use it.

## How to integrate

- The notes are hypotheses. A test that fails because the TEST is wrong is fixed in the test, without weakening what it checks. A test that fails because the APP does not do what the webview does is a finding: read the webview source the test cites; if the test is right, fix the app when the fix is small and local, otherwise move the test, unchanged, into a `proofs13_<topic>_pending.rs` file beside it and leave that file undeclared (the two agents `_pending` files are already such files: do not declare them). Never add `#[ignore]`.
- Known suspicions to settle with the webview source: Quit with "Don't Save" may still autosave the file (the quit path never marks leaving as decided; if `discards_without_saving` fails, that is a defect of the app: fix it); after an agent edit's send fails, the inline editor stays open and later draws over the message sent again; a large database value may arrive cut short in an export.
- Behaviours the writers found missing and that are small enough to build here if the webview source confirms them: `help.keymapReference` (what does the webview open? if it is a browser URL or a bundled file, do the same), `lightEdit.openProject` on the status item ("Open Containing Folder as Project"), Close Tab in a single-file window (what does the webview do when the last tab of such a window closes? follow it if it does not need a tab model). A second loose file opening as a tab of a single-file window needs a tab model there: leave those tests pending and say what the tab model would need.
- No test may wait real seconds, quit or restart the test process, or reach the network, a real agent, the real `gh`, Docker or a JVM: confirm it by reading each fixture before running it.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every `proofs13` module twenty times by filter (`cargo test --lib -- proofs13 > file`, twenty times; report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name, the count "N of M cases" for spec rows and, for partial, what is missing. List every test left in a `_pending` file with the behaviour it waits for, and every app fix with the webview `file:line` it follows.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{annotate,project_view_options,zoom_actions,build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/editor/view/annotations.rs`, `app-gpui/src/project_view/{pane,tree_clicks}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, any `_pending` test file, and every undeclared `proofs*` test file that this brief does not hand you.
