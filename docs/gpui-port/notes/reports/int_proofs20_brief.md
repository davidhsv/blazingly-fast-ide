# Integrator brief: the `proofs20` tests (27 tests written without a compiler for rows that only lacked a test)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/dp20/`.

Format only files you edited, taken from `git diff --name-only` plus the test files this brief hands you by name. Never build a file list from `git status`.

Work one file at a time (declare, compile, run that module's tests by filter, fix, next). Do not stop on your own estimate of elapsed time.

## The two batches

A. Notes `docs/gpui-port/notes/integration/proofs20_a.md`, report `docs/gpui-port/notes/reports/draft_proofs20_a.md`. Files (8 tests): `app-gpui/src/workbench/debug/tests/proofs20_debug.rs`, `app-gpui/src/workbench/tests/{proofs20_database,proofs20_run_widget}.rs`, `app-gpui/src/workbench/run_context/tests/proofs20_run.rs`, `app-gpui/src/workbench/build_tools/tests/proofs20_build_title.rs`.
B. Notes `docs/gpui-port/notes/integration/proofs20_b.md`, report `docs/gpui-port/notes/reports/draft_proofs20_b.md`. Files (19 tests): `app-gpui/src/workbench/tests/proofs20_shell.rs`, `app-gpui/src/workbench/app_actions/tests/proofs20_layouts.rs`, `app-gpui/src/workbench/project_view_actions/tests/proofs20_tree.rs`, `app-gpui/src/workbench/editor_language/tests/{proofs20_generate,proofs20_mcp_problems}.rs`, `app-gpui/src/platform/keymap/tests/proofs20_new_element.rs`, `app-gpui/src/workbench/zoom_actions/tests/proofs20_quick_switch.rs`, `app-gpui/src/workbench/github/tests/proofs20_remote_branch.rs`.

The notes give the `mod` lines and two hunks (an accessor `TitleBar::project_avatar_fill`; a dev-dependency on the repository's own `ide-mcp` crate).

## Rules for this batch

- The notes are hypotheses. A test that fails because the TEST is wrong is fixed in the test, never by weakening an assertion: each test exists to prove one inventory row, and the row's standard is in the notes' table (an action through the registry or its key with its condition read on and off; a context key read on and off; an IPC row only when the asserted state can only come from that backend function and the native code calls the adapter's function).
- A test that fails because the APP does not do what the webview does is a finding: read the webview source. If the fix is small and local, make it, with the test as its proof. Known one: the Remote Branch field of the pull-request flow must clean the name as it is typed, as the webview does (a space becomes `-`); the model function exists (`clean_up_branch_name_on_typing`). If it is not small, move the test, unchanged, into a `proofs20_<topic>_pending.rs` file beside it, undeclared. No `#[ignore]`.
- The database test starts a stand-in ClickHouse server on a loopback port (the debugger tests already listen on loopback). Before running it, read how the backend's HTTP client is built: if a proxy from the environment or the IDE's proxy settings could be used for `127.0.0.1`, the request (with the test's made-up password) could leave the machine: in that case do not run the test; move it to a pending file and say so. "Save password" must stay off in the test: saving goes to the OS keychain. Confirm the test build's keychain guard covers this path.
- The dev-dependency on `ide-mcp` is allowed only if `app-gpui/Cargo.lock` gains no package (the crate is already in the lock file); otherwise leave that test undeclared.
- No real JVM, debugger, Python, Maven, Gradle, language server, agent binary, `gh`, Docker, network beyond the loopback stand-ins, login shell, system Trash, keychain, or anything under the user's home. Read each fixture before running it. The Python tests use a stand-in interpreter in the temp project: check that nothing resolves a `python` from the PATH (the test build's guards for run launchers cover `java`, `mvn`, `gradle`; if a stand-in could fall through to a real interpreter, make the test name the stand-in absolutely and assert on it before the run).

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script.

## Report

Besides the format of `$S/int_rules.md`: one table: `parity.json` key → state it reaches (done or partial) → proving tests by full name → for partial what is missing → for `ipc:` rows both call sites. Include `module:src/workbench/tasks.ts` (the notes say an existing test already proves it: read that test and say whether the row is done). Every app fix with the webview source it follows. Every test left pending with what it waits for.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (Settings pages `settings_view/pages/{languages_,general_,build_,tools_,vcs_,editor_files}*.rs`, `workbench/{refactor_actions,vcs_text_actions,agents_views,diff_tab}.rs`, `agents/{config_pickers_view,new_task_view}.rs`, `agents/composer_view/pickers.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `vcs_more/caret_conflicts.rs`, `vcs/{selection_history_view,external_log_view}.rs`, `platform/error_log_sink.rs`) and every undeclared `proofs*` test file this brief does not hand you.
