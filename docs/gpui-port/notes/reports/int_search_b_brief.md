# Integrator brief: Search Everywhere's scope, history and layout; edited files in Recent Files (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dsbi/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## The slice

Notes (the list of every missing clause of the search rows with its verdict; 26 hunks with quoted anchors grouped by piece; per row and per spec case what is reached; hypotheses with the command that settles each; the existing tests whose expectations change; the tests most likely to fail; what a look at the real window must check): `$S/drafts/search_b_integration.md` (read this scratch copy). The writer's report: `docs/gpui-port/notes/reports/search_b_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/everywhere/view/memory.rs`, `everywhere/view/parts.rs`, `everywhere/view/proofs41_stale.rs`, `app-gpui/src/workbench/tests/proofs41_e2e_search_everywhere.rs`, `proofs41_e2e_search_layout.rs`, `proofs41_recent_edited.rs`.

The webview decides where a note and the source disagree: `src/features/search/` (`everywhere/SearchEverywhere.tsx` and its CSS module, the stores, `index.ts`), the specs `src/features/search/__tests__/dialogs.test.tsx`, `fileStructure.test.tsx`, `inherited.test.ts`, `searchKeys.test.ts`, and `e2e/searchEverywhereLayout.spec.ts`. Order: piece 1 (Search Everywhere: the scope toggle and the automatic widening, query history, recent actions, the two modifier-Enter keys, the row and frame details), piece 2 (edited files in Recent Files), piece 3 (the tests). If a piece does not converge, take its hunks out by editing, leave its files undeclared, say where it stands, and go on.

## Points to settle by measurement, not by the notes

- For each behaviour, see its test fail before the code (against the tree without the hunk, or with the behaviour switched off) and keep the log; a test never seen failing is reported as such.
- A late answer of an old query is never shown, also across the automatic widening (the query that found nothing in the project and is searched everywhere, then a new query typed before the wide answer arrives: the new query's project results are shown, not the old wide ones). Nothing lists or reads the project's files on the window's thread.
- The writer found a complete and tested store (`app-gpui/src/everywhere/store.rs`, `SearchEverywhereState`) that no native code used, and built on it: confirm it is the one place that holds the scope, the history and the recent actions, and that nothing of the old view's own state duplicates it afterwards.
- Query history and recent actions: where are they kept (per window, per project, stored?) in the webview, and the same natively; a test of two project windows shows neither sees the other's history if the webview's do not. Nothing of them is read from a stored list of the user's in a test.
- The layout tests assert the webview's numbers (760 px wide, less in a narrow window, the header wrapping at 680 px or below, the 16 px icon slot): read each number in the CSS module and the e2e spec at HEAD and say where each comes from; the two the writer calls estimates (tab widths and wrapping) are measured in the real window.
- Existing tests whose expectations change are named in the notes with the webview's `file:line` (Find Action's empty text after widening; reopening a tab starts with the last query selected); run every Search Everywhere, Find Action, Go to File, Symbol and Recent Files test by filter before and after. No other assertion changes.
- ⌥↵ on an action row runs `settings.assignShortcut`: read what the webview's does on rows that are not actions and do not invent it; ⌘↵ on the Text tab opens Find in Files with the query.

## The real window

A paint slice: look at it. Only through `$S/insp8/run.sh` with a temp `HOME`, `OPENIDEA_GPUI_HOME` and a project under a neutral path `/private/tmp/openidea-inspect-<random>/` (never under the scratch folder; removed afterwards). Shots, light and dark: Search Everywhere on All with mixed file and action rows; the Actions tab with the check box; a query with nothing in the project and the widened result; the popup in a narrow window (the header wrapped); Recent Files with Show edited only. Never the Agents tool window, the Agents pages of Settings, the Services window's Services tab or a debug session.

## Rows

The actions `navigate.searchEverywhere`, `navigate.gotoFile`, `navigate.gotoSymbol`, `navigate.findAction`, `navigate.searchText`, `navigate.recentFiles`, `navigate.switcher`; the overlays `search.everywhere` and `search.recent`; the key claim registered at `src/features/search/everywhere/SearchEverywhere.tsx:86`; the specs `dialogs.test.tsx` (23 cases at HEAD; the record says 22), `fileStructure.test.tsx`, `inherited.test.ts`, `searchKeys.test.ts`, `e2e/searchEverywhereLayout.spec.ts`: for each spec file every case at HEAD with its native test by full name or MISSING; for each row what is proven now and what stays missing. Check which keys exist in `docs/gpui-port/parity.json`. The writer says the record's clause "the empty-result text is missing" is stale: say whether it is.

## Changed since

HEAD is `0df1bf911`. The notes were written on the tree at `5663403bf`; since then three integrations landed (the second half of the pre-merge behaviour fixes, the tests-only slice, the UI language following its setting). The files the hunks anchor on (`git diff --numstat 5663403bf HEAD`): `app-gpui/src/everywhere/view.rs` (unchanged), `app-gpui/src/everywhere/recent_view.rs` (unchanged), `app-gpui/src/workbench/mod.rs` (+13 −2), `app-gpui/src/workbench/editor_core.rs` (unchanged). In `workbench/mod.rs` the tool window header now translates its title and has a test selector `tool-window-title:<id>:<title>`, and the Select In chooser passes the contributed menu groups: re-anchor the hunks there from their quoted text and read the place before you edit it. The UI now follows the Language setting (`app-gpui/src/platform/ui_language.rs`): the writer's test that switches to Chinese does so with `set_locale`; that still works. The real binary's launcher needs a neutral base now: `INSPECT_BASE=/private/tmp/openidea-inspect-<random> $S/insp8/run.sh …` (see the end of `$S/int_rules.md`). Other agents' undeclared drafts in the tree, not yours: `app-gpui/src/settings_view/pages/tools_agents.rs`, `tools_agents/`, `workbench/settings_agents.rs`, `settings_view/pages/agents_list.rs`, `agents_list/`, `agents/agent_list.rs`, `agents/agent_list/`, `workbench/agent_list_backend.rs`, `workbench/settings_agents_list.rs`, `settings_view/pages/remote_folder_browser.rs`, `remote_folder_browser/`, `build_deployment/remote_folder.rs`, every `proofs36_*`, `proofs37_*` and `proofs38_*` file, every `*_pending.rs`, `platform/error_log_sink.rs`, `workbench/app_actions/tests/proofs6_app_actions.rs`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: one section per piece (each behaviour with its test and whether it failed before the code, with the log's path; what is left); the rows and every spec case; which way each hypothesis went; the shots' absolute paths and what you saw in each.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
