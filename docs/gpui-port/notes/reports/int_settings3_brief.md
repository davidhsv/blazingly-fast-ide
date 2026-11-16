# Integrator brief: the three templates pages of Settings: Postfix Completion, Live Templates, File and Code Templates (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dset5i/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

## The slice

Notes (hunks in four groups with quoted anchors, the rows with every test's full name, behaviours with `file:line`, hypotheses, the tests most likely to fail, what the real window must be checked for): `$S/drafts/settings_pages5_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Report: `docs/gpui-port/notes/reports/settings_pages5_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/settings_view/pages/editor_postfix_completion.rs`; `editor_live_templates.rs` with `editor_live_templates/{fields,tests}.rs`; `editor_file_templates.rs` with `editor_file_templates/{files_tab,fragments,tests}.rs` and `editor_file_templates/extension_guard_pending.rs`; `app-gpui/src/templates/files_store.rs` (a new store for file templates); `app-gpui/src/workbench/settings_template_pages.rs`; `app-gpui/src/workbench/tests/proofs32_settings_{templates_fixtures,postfix,live_templates,file_templates}.rs` and `proofs32_settings_templates_pending.rs`.

The webview's pages: `src/features/templates/settings/{PostfixPage,LiveTemplatesPage,FileTemplatesPage}.tsx`, their styles `Settings.module.css`, their spec `src/features/templates/__tests__/views.test.tsx`, the storage `src/features/templates/storage.ts`, `sync.ts`, `files/store.ts`. The notes are hypotheses; where a note and the webview source disagree, the webview source wins. Land one page at a time in the order Postfix Completion, Live Templates, File and Code Templates: its hunks, compile, its tests by filter, fix, next. If a page does not converge, take its hunks out by editing, leave its files undeclared, say exactly where it stands, and go on.

## Part 0: a file planned from a template can leave its folder (read by the writer; confirm by test first)

`app-gpui/src/templates/files_model.rs` `plan_file` checks the typed name but appends the template's extension unchecked, so an extension such as `/../../x` plans a file outside the folder; the webview's model has the same gap (`src/features/templates/files/model.ts:135-145`). Write the failing test first (the model's, and `editor_file_templates/extension_guard_pending.rs` once the page compiles), then make `plan_file` refuse an extension that is not a plain extension (no separator, no `..`, nothing the file-name check refuses), and make the page and the store refuse to store one (say what the user sees). A template's extension, file name and text come from the user and from plugins: after the fix, list every field of a file template that ends in a path and say for each where it is checked.

## Points to settle by measurement, not by the notes

- These pages save each change at once, by the writer's reading; check it in the webview for each page and what the dialog's Apply, Reset, Cancel and OK then do; the native dialog does the same (the tests assert that Apply stays off and Reset hidden, by the notes: confirm that is what the webview shows).
- Storage: the pages read and write the UI storage under `openidea.postfix.v1`, `openidea.liveTemplates.v1`, `openidea.liveTemplates.options.v1`, `openidea.fileTemplates.v1` and its `.scheme:<root>` and `.project:<root>` keys. For each key compare the stored JSON with what the webview stores for the same edits (read `storage.ts`, `sync.ts`, `files/store.ts` and the stores' tests; field names, key order where it matters, how the project root is written in the key): a setting written by one app must be read by the other. The existing native stores (`app-gpui/src/templates/live_store.rs`, `postfix_store.rs`) are the single owners of their keys; the new `files_store.rs` becomes the owner of the file templates' keys and nothing else writes them.
- Nothing in the native app reads these settings yet, by the writer's reading (no live template expansion from the stores, no postfix settings in completion, no New from Template): confirm by grep what the editor's completion already takes from `crate::templates` (`app-gpui/src/editor_lsp/snippet.rs`, `completion.rs`), and wire only what is a few lines (the completion reading the enabled state of postfix templates or of live templates, if it already offers them from the built-in lists); everything else stays in `proofs32_settings_templates_pending.rs`, undeclared, and the page rows stay partial with that as the reason. Do not build the templates feature here.
- `editor_live_templates.rs` is over the size the brief asked for: split its drawing into a child module.
- The Contexts field of Live Templates keeps what is typed where the webview's rewrites itself on every keystroke (and swallows a typed comma): keep the native behaviour only if the stored list is the same for the same final text; assert it.
- Cost: a page with hundreds of templates draws only its visible rows, filters without walking more than the list once per keystroke, and never serializes the whole store on the window's thread more than once per change; say what each keystroke costs for the built-in lists' size.

## Safety

- A template's text is data: nothing on these pages expands, runs or evaluates it; plugin templates arrive only through the setter the drafts have (no plugin host is started by a test).
- No test uses the network, a toolchain or anything under the user's home; the UI storage a test writes is the one under its temp home (assert the path once).
- The real binary: only through `$S/insp8/run.sh` (short `PATH`, temp `HOME`, `ZDOTDIR`, `OPENIDEA_GPUI_HOME`), with a temp project under a neutral path (`/private/tmp/openidea-inspect-<random>/`, removed afterwards). Never open the Agents tool window or the Services window. Shots, light and dark: each page; Live Templates with a template selected and its fields; File and Code Templates on each tab with a template selected; a long list filtered.

## Rows

`settings:editor.general.postfixCompletion`, `settings:editor.liveTemplates`, `settings:editor.fileTemplates` (check the key form against three existing keys of the Settings pages section in `docs/gpui-port/parity.json`), `unit:src/features/templates/__tests__/views.test.tsx` (12 cases: 7 are these pages', by the notes). A page row is done only when tests in a project window open Settings on the page and change each kind of control with the mouse or the keyboard and assert what was stored, and what the page's setting does in the app is either shown by a test or named as the missing part; a unit row only when every case of the spec file has a native test with the case's name in snake case checking the same thing.

## Changed since the notes were written

The notes were written on the tree at e1db281b. Since then the gutter content and seven Settings pages landed (SSH Configurations, Deployment servers, Run Targets; Project Trust, TODO, Log, GitHub). Measured at launch (`git diff --numstat e1db281b` over the existing files the hunks anchor on): changed are `workbench/mod.rs` (+21 −3), `workbench/settings_window.rs` (+1: the mount call of `workbench/settings_feature_pages.rs`, the module that puts the four pages that apply at once on their pages) and `settings_view/pages/mod.rs` (+8: `pub mod` lines); unchanged are `templates/mod.rs` and `templates/files_model.rs` (so `plan_file` is still unguarded: Part 0 is yours). Re-anchor every hunk from its quoted text and read the place before you edit it.

Build on what just landed instead of beside it: `workbench/settings_feature_pages.rs` already mounts pages that apply at once, gives the Settings search their labels (`set_page_labels`) and asserts in its tests that Apply stays off, Reset is not shown and Cancel keeps the change (the helper `assert_nothing_staged` in `workbench/tests/proofs31_settings_feature_pages.rs`); the TODO page follows the UI storage so that a change made in another window shows on an open page (read `settings_view/pages/editor_todo.rs` and `features_a/todo.rs`: the storage functions live with the feature, not with the page). Mount your three pages the same way (the draft's `workbench/settings_template_pages.rs` may fold into that module), keep the stores' functions in `app-gpui/src/templates/`, and make an open page follow the storage where the webview's does.

Other agents' undeclared drafts now in the tree, not yours: `app-gpui/src/templates/expand.rs`, `chooser_view.rs`, `new_file_dialog.rs`, `update_tag_dialog.rs`, every `proofs33_*` and `proofs34_*` file, `app-gpui/src/workbench/editor_templates.rs`, `template_files.rs`, `template_actions.rs`, `emmet.rs`, `workbench/tests/proofs26_code_preview_pending.rs`, `platform/error_log_sink.rs`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: Part 0 first (the failing test, the fix, every path-ending field and where it is checked); one section per page (what the webview does on a change and on Apply, Reset, Cancel, OK with `file:line`; the stored JSON compared with the webview's; the tests); the rows table with the proving tests by full name and for partial what is missing; which way each hypothesis went; the shots' absolute paths and what you saw in each.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
