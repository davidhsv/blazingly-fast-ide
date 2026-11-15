# Integrator brief: the Refactor actions through the language server, then the Git actions on the editor's text

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/drv/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do Part A, then Part B. Work one file at a time. Stop early only if a part is blocked, and say what blocks it.

## Part A: Refactor actions

Notes: `docs/gpui-port/notes/integration/refactor_actions_integration.md`, report `docs/gpui-port/notes/reports/draft_refactor_actions.md`. Draft (committed, undeclared): `app-gpui/src/workbench/refactor_actions.rs` (11 actions: the eight menu refactorings, `refactor.thisMenu`, `refactor.safeDelete`, `navigate.superMethod`; 13 tests). Hunks: the `mod` line, a workbench field with its initialiser, the register call, and `pub(super) async fn apply_code_action` in `workbench/editor_language.rs`. The draft was written before an editor ran left-button mouse shortcuts on its own text and before Quick Evaluate's hint helper became `pub(super)` (`workbench/editor_language.rs` `hint`): re-read what it uses.

The notes are hypotheses; the webview source wins (`src/editor/lsp/actions.ts`, `refactorThis`, `src/editor/navigation/index.ts`, their `*.test.ts`). It wires models that exist and that nothing used (`app-gpui/src/editor_refactor/`): the inventory has rows marked model only for them. Report every `unit:` and `module:` row whose model this slice makes reachable, with the spec's case count and which cases a window test now proves.

Not in this slice: Pull Members Up and Push Members Down (no model, no dialog), the in-place naming after an extract, the syntax-only Introduce Variable.

## Part B: Git actions on the editor's text

Notes: `docs/gpui-port/notes/integration/vcs_text_actions_integration.md`, report `docs/gpui-port/notes/reports/draft_vcs_text_actions.md`. Drafts (committed, undeclared): `app-gpui/src/workbench/vcs_text_actions.rs` (9 actions, 11 tests), `app-gpui/src/vcs_more/caret_conflicts.rs`, `app-gpui/src/vcs/{selection_history_view,external_log_view}.rs`. Hunks: `pub mod` lines, the workbench `mod` line and register call, two tab kinds in `workbench/tab_views.rs`.

The webview source wins (`src/features/vcs/actions.tsx`, `historyActions.tsx`, `RevisionBrowser`, the specs beside them). Tests use temp Git repositories configured as the existing tests configure them (no global config, no hooks), a second local repository as a remote, the test build's path-prompt response for a picked folder; no network, no real `gh`.

After both parts pass, screenshots in your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, a temp Git project with one committed file changed in two places; launcher `$S/insp8/run.sh`, steps `$S/insp/USAGE.md`): Show History for Selection's tab; the Refactor This popup outside an editor ("Nothing here"). Look at each: nothing cut, popups inside the window.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the two notes name (the `action:` rows, and the `unit:`/`module:` rows of the refactor models and of `selectionHistory.test.tsx`) with the state it reaches, the proving tests by full name, for partial what is missing; for each action whether its `when` clause is read on and off by a test; the `ipc:` rows the actions stand for with both call sites, only where the state a test asserts can only come from that backend function; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (`workbench/diff_tab.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `platform/error_log_sink.rs`, any agents draft still undeclared) and every undeclared `proofs*` test file this brief does not hand you.
