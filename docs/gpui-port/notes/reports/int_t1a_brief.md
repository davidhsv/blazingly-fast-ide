# Integrator brief: an agent's workspace edit can be undone; templates follow the merged webview (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dt1ai/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## The slices

Notes (every hunk with a quoted anchor, grouped so each item lands alone with its own `mod` lines; the rows; every spec case with its native test; hypotheses; the existing tests whose expectations change; the tests most likely to fail): `$S/drafts/t1a_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). The writer's report: `docs/gpui-port/notes/reports/t1a_writer.md`. Draft files (committed, undeclared, never compiled): slice A `app-gpui/src/workbench/tests/proofs39_agent_undo.rs`, `proofs39_agent_undo_pending.rs` (stays undeclared unless what it waits for exists); M1 `app-gpui/src/templates/proofs39_completion_enter.rs`, `app-gpui/src/editor/intelligence/tests/proofs39_completion_snippet.rs`; M2 `app-gpui/src/templates/postfix_action.rs`, `app-gpui/src/workbench/editor_templates/postfix_action.rs`, `app-gpui/src/templates/proofs39_java_var.rs`, `app-gpui/src/workbench/tests/proofs39_java_var.rs`, `app-gpui/src/settings_view/pages/editor_postfix_completion/tests/proofs39_java_var_row.rs`; M3 `app-gpui/src/templates/postfix_model/zig.rs`, `proofs39_zig_postfix.rs`, `settings_view/pages/editor_postfix_completion/tests/proofs39_zig_tab.rs`; M4 `templates/live_builtins/zig.rs`, `proofs39_zig_live.rs`, `settings_view/pages/editor_live_templates/tests/proofs39_zig_group.rs`; M5 `templates/live_surrounders/zig.rs`, `proofs39_zig_surrounders.rs`; M8 `templates/proofs39_django_emmet.rs`.

The webview decides where a note and the source disagree: slice A `src/features/code-insight/workspaceUndo.ts`, `src/editor/core/model/workspaceHistory.ts`, the handler of `WorkspaceEditApplied` and their specs; slice B `src/features/templates/editor.ts`, `postfix/model.ts`, `live/builtins.ts`, `live/surrounders.ts`, `emmet/model.ts` and their specs. Order: slice A first (data is at stake), then M3, M4, M5 (lists), M1, M2, M8. If an item does not converge, take its hunks out by editing, leave its files undeclared, say where it stands, and go on.

## Slice A: points to settle by measurement (data first)

- A failing test first: an agent-marked workspace edit over two files in a project window on the real backend, then Edit › Undo: both files' bytes on disk and in open editors are back; Redo applies it again. No agent is started: the event reaches the handler as the existing tests of `app-gpui/src/code_insight/workspace_undo.rs` do, and `spawn_guard` stays in force.
- The writer's hunk A5 starts the listener when the project opens, because today it starts with the first editor and an edit made with no editor open is dropped: confirm by a test that fails first (an edit applied before any editor opens can be undone), and check nothing is listened to twice once an editor opens (one record per edit: assert the history's length).
- Undo is refused when a file changed since, and then every file keeps its bytes (the writer's test): read what the webview tells the user and what the native app tells; where the texts differ (the writer says the backend's own message shows natively in one case) list it, do not invent a message.
- The writer found that a workspace edit clears an open editor's own undo history, so typing made before the edit can no longer be undone, where the webview keeps it; and that `app-gpui/src/editor_model/workspace_history.rs` is compiled and used by nothing. Wiring that model into the editors is the fix for both remaining differences: do it only if it is contained (say its size after reading); otherwise list it as the next slice with what you learnt. Either way add a test that states today's behaviour of earlier typing after a workspace edit, so a later change is seen.
- Undo of an agent's edit never touches a file outside the project and never recreates or deletes a file the edit did not create or delete (assert the tree).

## Slice B: points to settle by measurement

- The lists are copied from the webview: for each Zig list compare count and order with the webview's by a script over both files (the writer counts 12 postfix templates, 19 live templates, 10 surrounders; a reader had said 20 live templates) and say the numbers.
- M1: Enter in a snippet that came from a picked completion is the editor's (a new line where the caret is, the session's fields left as the webview leaves them: read `editor.ts:199-211`); Tab still moves between fields in both kinds. Every existing template-session test passes unchanged.
- M2: Java `.var` removes the key, ends the statement, selects the expression and runs Introduce Variable; when the action is not available it falls back to the template. No language server of the machine is started (the stand-in only); the fallback is tested without any server.
- The Settings pages' tests whose expectations change (new Zig tab and group, the `.var` row's description) are named in the notes with the webview's `file:line`; no other assertion changes.

## Rows

`event:WorkspaceEditApplied`, `unit:src/features/code-insight/workspaceUndo.test.ts`, `unit:src/features/templates/__tests__/editor.test.ts`, `postfix.test.ts`, `live.test.ts`, `surrounders.test.ts`, `emmet.test.ts`, `settings:editor.general.postfixCompletion`, `settings:editor.liveTemplates`: for each spec file every case at HEAD with its native test by full name; a row is done only if every case has one that asserts the same thing.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): the notes were written on the tree at 76bde66d6 while the third follow-upstream integration was in flight; say which files the hunks anchor on changed since and the other agents' undeclared drafts then in the tree.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: slice A first (each point with its test and whether it failed before the code; what remains different from the webview); slice B per item; the lists' counts; the rows with every spec case; which way each hypothesis went.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`. The real binary is not needed; if you run it, only through `$S/insp8/run.sh` with a temp home and project under a neutral path `/private/tmp/openidea-inspect-<random>/`; never the Agents tool window, the Agents pages of Settings or a debug session.
