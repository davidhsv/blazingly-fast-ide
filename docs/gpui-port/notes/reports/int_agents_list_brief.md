# Integrator brief: the Coding Agents page of Settings (`agents.list`): drafts written without a compiler

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dagli/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## The slice

Notes (hunks by part A to D with quoted anchors, the rows with every test's full name, the 33 spec cases one by one, behaviours with `file:line`, eighteen hypotheses and how to settle each, the tests most likely to fail, which old files are deleted): `$S/drafts/agents_list_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Report of the writer: `docs/gpui-port/notes/reports/agents_list_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/agents/agent_list.rs`, `agent_list/values.rs`, `agent_list/proofs38_agent_list.rs`, `app-gpui/src/settings_view/pages/agents_list.rs`, `agents_list/master.rs`, `details.rs`, `custom_agent.rs`, `proofs38_agents_list_view.rs`, `app-gpui/src/workbench/agent_list_backend.rs`, `settings_agents_list.rs`, `app-gpui/src/workbench/tests/proofs38_settings_agents_list.rs`. They replace an older draft of the page as it was before upstream changed it (`settings_view/pages/tools_agents.rs`, `tools_agents/`, `workbench/settings_agents.rs`, `workbench/tests/proofs36_settings_agents.rs`) and the obsolete half of the compiled `agents/builtin_agents.rs` with its tests: the notes say what goes; list every file the manager must `git rm`.

The webview: `src/features/agents/AgentRegistryPage.tsx`, `AgentList.tsx`, `AgentList.module.css` (the writer did not read lines 81 to 306: you do, for the details pane's layout), `agentSettings.ts`, `registry.ts`, the spec `agentRegistry.test.tsx` (33 cases). The notes are hypotheses; where a note and the webview source disagree, the webview source wins. Land A with B (the list, details, Sign In, New Tasks, Launch), then C (custom agents), then D (the registry); each complete or absent: if a part does not converge, mount the page without it (`custom: None`, `registry: None`), leave its tests undeclared, and say exactly where it stands.

## Hard rules of this page (first)

1. The network, fail closed. Every native call of the backend's `registry_list`, `registry_install`, `registry_remove` goes through the one seam of `workbench/agent_list_backend.rs`. Prove by reading and by a test: in a test build no code path reaches those backend functions (the writer says none is compiled in test builds: confirm with `cfg` reading and by grepping the test binary's sources for callers); a page opened in a test without a stand-in refuses with a plain message; no test installs, updates or removes a real agent or starts a program. Say what Install would run in the real app (the writer's hypothesis: npm or uv for some agents): that is why it is never pressed in the real binary.
2. Secrets. Agents' environment values may be API keys; the agent file keeps names with empty values and the values go to the secret store (a file vault under the test home). Every end-to-end test that lists or writes agents asserts `ide_ssh::secrets::secrets_in_files()` first, asserts the agent file's content without printing it on failure, and uses markers that are not real key formats. No `Debug`, log line, notice, status text, tooltip, title, search index entry, assertion or panic message holds a value: first a test per type that can hold one (the notes list them; the old compiled `builtin_agents::parse_env` puts the whole refused pair in its error: its replacement names the variable or the entry's number only), formatted with `{:?}` and `{:#?}`, then the code. Nothing formats the backend's `CustomAgentInput` with `{:?}`. If the integration before this one already made `ExternalWrite`, `Details`, `ExternalChange`, `SettingsDialog`, `ApplyRun` and `ApplyStart` print names only, keep its tests and build on them (the manager says under "Changed since").
3. A path typed or chosen with Browse… is stored as text and never run by this page; a custom agent's command and arguments are data.
4. The real binary: this page is never opened in it (it downloads the registry on open and reads the credential store). No shot of it. The Add Custom Agent dialog and the two confirmation panels are proven by view tests only. Say in your report what a later look would have to check.

## Points to settle by measurement, not by the notes

- The shared dialog changes (`staging.rs`, `dialog.rs`, `settings_window.rs`): each confirmed by a test that fails first; every existing Settings test by filter before and after (`proofs19_*`, `proofs28_*` to `proofs32_*`, `smoke_settings_pages`): Apply, Reset, Cancel and the modified marks of every other page unchanged.
- Apply writes on the backend's runtime; a failure (a read-only agent file) is told in the dialog as the webview tells it, leaves the staged value, writes no half file (assert the old bytes).
- The 1.5 s poll: the timer stops when the page closes and when no install runs (assert no timer is left: the app must not tick when idle).
- The unit row: for each of the 33 cases, the native test that checks the same thing through the view or the app; the two the writer proved on the model only and the `it.each` rows that cover one variant each count as the writer says only if you agree after reading both sides. Count honestly.
- The writer broke a rule once (it grepped GPUI's source under the user's cargo folder for `PathPromptOptions`): you may read the dependency sources cargo already fetched, as integrators do; nothing else under the user's home.

## Rows

`settings:agents.list`, `unit:src/features/agents/agentRegistry.test.tsx`, `ipc:agent_set_command`, `ipc:agent_set_env`, `ipc:agent_custom_save`, `ipc:agent_custom_remove`, `ipc:agent_custom_import`, `ipc:agent_registry_list`, `ipc:agent_registry_install`, `ipc:agent_registry_remove`, `ipc:cancel_request` if a test of this page proves it, `action:agents.addAgent` (its proof names the old page id), `ipc:agent_providers` (lowered on 2026-11-17 for the model picker's cost fields: say only what this page adds). Check which keys exist in `docs/gpui-port/parity.json`. A registry row is partial while only a stand-in answers it in tests.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): the notes were written on the tree at e123f0da5; say which of the files the hunks anchor on changed since (`settings_view/pages/mod.rs`, `agents/mod.rs`, `settings_model/staging.rs`, `settings_view/dialog.rs`, `workbench/settings_window.rs`, `workbench/mod.rs`, `settings_view/custom_controls.rs`, `agents/builtin_agents.rs`), whether the `Debug` redaction landed, and the other agents' undeclared drafts then in the tree.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole (never a line that holds an environment value).

## Report

Besides the format of `$S/int_rules.md`: the hard rules first, one line per point with its test and whether it failed before the code; the shared dialog changes with the tests that failed first; which parts landed; the 33 spec cases one by one; the rows table; which way each hypothesis went; files to `git rm`.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
