# Integrator brief: the Built-in tab of the Agents page of Settings (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dset7i/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

## The slice

Notes (seven hunks with quoted anchors, the rows with every test's full name, the 26 spec cases one by one, behaviours with `file:line`, hypotheses, the tests most likely to fail): `$S/drafts/settings_pages7_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Report: `docs/gpui-port/notes/reports/settings_pages7_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/settings_view/pages/tools_agents.rs`, `tools_agents/proofs36_agents_view.rs`, `app-gpui/src/workbench/settings_agents.rs`, `app-gpui/src/workbench/tests/proofs36_settings_agents.rs`.

The webview: `src/features/agents/AgentRegistryPage.tsx`, `BuiltinAgents.tsx`, `registry.ts`, the spec `agentRegistry.test.tsx`, and the backend's `crates/` code for `agent_providers`, `agent_set_command`, `agent_set_env` (`src-tauri/src/commands/` shows the adapters). The notes are hypotheses; where a note and the webview source disagree, the webview source wins. Only the Built-in tab is drafted; the ACP Registry tab is not written and is not drawn: do not build it, and make sure no code path of this slice can fetch the registry, install or remove an agent (the writer found that `cfg!(test)` inside the backend's registry crate is false in the native app's tests, so a test that reached `registry_list` would go to the network: nothing here may call it).

## Part 0: an agent's environment may hold API keys (hard, first)

The Built-in tab edits each agent's environment variables. The decision, recorded for the owner: the field shows them as plain text while they are edited, exactly as the webview's does (`BuiltinAgents.tsx:186-193`); they are the user's own configuration, stored as plain text in `<config>/agents/<id>.toml` by the backend, and a masked field would make the pairs uneditable. Everything else about them is held to the rule for secrets:

- No `Debug` output contains an environment value. The writer lists existing compiled types whose derived `Debug` prints them: `ExternalWrite` and `Details` of the Built-in agents model, `ExternalChange` (`app-gpui/src/settings_model/staging.rs`), the dialog's own debug output and the Apply run's debug output in the same file. First a test per type that fails today (format the value with `{:?}` holding an environment with a marker value and assert the marker is absent), then hand-written `Debug` that prints the variables' names and `<redacted>` for values (read how `SecretText` in `app-gpui/src/database/store.rs` does it). Then grep every `{:?}` and `tracing::` in the files this slice touches and in `crates/` code on the path of `agent_set_env` and `agent_providers` (read only) for anything that prints an environment, and say what you found; a log line in `crates/` that prints values is reported, not edited, unless it is on this slice's path and a one-line change (then say exactly which line; the manager runs the Tauri suite).
- No log line, notice, status text, test failure message or panic message of the native app contains a value: assertions on environments compare lengths or names, or use a marker that is not a real key format; the end-to-end test that reads `<config>/agents/claude.toml` asserts the file's content but never prints it on failure (compare, then fail with a fixed message).
- The footer message for a refused pair ("Expected NAME=value: …") repeats the pair in the webview: natively show the variable's name or the line's number instead of the whole text, and record that as a deliberate difference.
- The values never reach the clipboard except by the user's own copy from the field, never the Settings search index (assert that searching the dialog for a marker value finds nothing), never a tooltip, never the window's title.
- The real binary: this page shows the real `[env]` of the user's agents if it runs over the user's home. It is opened only through `$S/insp8/run.sh` with a temp `HOME` and `OPENIDEA_GPUI_HOME` (assert in your notes, by reading `agent_providers`, which folder it lists under those, and that showing the page starts no program: the writer says it only checks whether each built-in command is an executable in a `PATH` directory; with the launcher's short `PATH` that finds nothing of the user's). Before any shot, write an agent file of your own under the temp config folder with a made-up variable (`DEMO_FLAG=1`), and look at the shot for anything that is not yours. If you cannot show that the page lists only the temp home's files, do not open it and say so.

## Points to settle by measurement, not by the notes

- Apply: the writer found that values kept outside the settings were refused by Apply with "This window cannot do this" (`workbench/settings_window.rs`) and that the dialog's modified-pages list ignored them, so Reset never appeared; hunks 2, 3 and 7 change shared dialog code. Confirm each by a test that fails first, and run every existing Settings test by filter before and after (the staged pages `proofs19_*`, the pages that apply at once `proofs31_*`, `proofs32_*`, `proofs28_*` to `proofs30_*`): Apply, Reset, Cancel and the modified marks of every other page are unchanged.
- The write runs on the backend's runtime and a failure (an unreadable or read-only agent file) is told in the dialog as the webview tells it, leaves the staged value in place, and writes no half file (assert the old file's bytes).
- A binary path typed or chosen with Browse… is stored as text and never run by this page; say what validates it in the webview (nothing? a check that it exists?) and do the same.
- The unit row: 25 of 26 cases are claimed, 8 of them proved on the model only because the registry view does not exist, and four existing test names write the spec's `%s` as a word: a case counts only when its native test checks the same thing through what exists; say for each of the 26 which test, and count honestly.

## Safety

- No test uses the network, starts an agent or any program of the machine, or reads anything under the user's real home; the agent files a test reads and writes are under its temp config folder (assert the path once).
- Never open the Agents tool window or the Services window in the real binary.

## Rows

`settings:tools.agents` (partial: the ACP Registry tab is not built), `unit:src/features/agents/agentRegistry.test.tsx`, `ipc:agent_set_command`, `ipc:agent_set_env`, `ipc:agent_providers` if a test of this page asserts state only it gives. Check which keys exist in `docs/gpui-port/parity.json`.

## Changed since the notes were written

BEFORE LAUNCH (for the manager; delete this paragraph then): the notes were written on the tree at 843ed10c while the cancelled-search integration was in flight; say here which of the existing files the seven hunks anchor on changed (`settings_view/pages/mod.rs`, `settings_model/staging.rs`, `settings_view/dialog.rs`, `settings_view/custom_controls.rs`, `workbench/mod.rs`, `workbench/settings_window.rs`).

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. If any test fails in a whole-suite run, keep that run's log and quote the failure whole (never a line that holds an environment value).

## Report

Besides the format of `$S/int_rules.md`: Part 0 first, one line per bullet with the test and whether it failed before the change, and what the grep of `crates/` found; the shared dialog changes with the tests that failed first; the 26 spec cases one by one; the rows table; which way each hypothesis went; the shots' absolute paths and what you saw in each (or why none was taken).

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. Under `crates/` nothing, except a one-line log change on this slice's path as Part 0 says. No edits under `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
