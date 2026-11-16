# Integrator brief: the shared code preview of Find in Files, Bookmarks and Find Usages (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dprevi/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## The slice

Notes (78 hunks grouped Shared, Find in Files, Bookmarks, Usages, Shared (unsaved previews), Declarations, with quoted anchors; the rows expected; each behaviour of the webview's previews with `file:line`; the hypotheses; the tests most likely to fail; what the real window must be checked for): `$S/drafts/code_preview_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Its first section says what the webview really does: read it first. The writer's script `$S/dprev/hunks.py` checks every hunk's anchor against the files: run it before applying and re-anchor what moved. Writer's report: `docs/gpui-port/notes/reports/code_preview_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/ui/code_preview.rs`, `app-gpui/src/ui/code_preview/{line_tokens,proofs26_code_preview}.rs`, `app-gpui/src/workbench/tests/proofs26_code_preview.rs`, `app-gpui/src/code_insight/usages_view/tests/proofs26_usage_preview.rs`, and `app-gpui/src/workbench/tests/proofs26_code_preview_pending.rs` (stays undeclared unless what it waits for gets built).

What the webview does (the writer read it; check it yourself in `src/features/search/find/parts.tsx:105-242`, `src/ui/SourcePreview.tsx`, `src/features/search/__tests__/codePreview.test.tsx`, `src/features/code-insight/usagePreview.test.tsx`): the preview of Find in Files and of Bookmarks is an editable second editor on the document, sharing edits, undo and save with the tabs; the Find Usages pane and the Show Usages popup's preview are read-only. The drafts therefore hold a real `Editor` on the backend's document, editable or read-only.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins. Apply in stages, each compiled and its tests run by filter before the next: (1) Shared; (2) the unsaved-previews hunks, BEFORE any place gets an editable preview (see the next section); (3) Find in Files; (4) Bookmarks; (5) Usages; (6) Declarations of the remaining tests. If a stage does not converge, take its hunks out by editing, leave its files undeclared, say exactly where it stands, and go on.

## Data loss comes first (hard)

An editable preview makes documents dirty that no tab shows. The writer read that autosave, Save All and the unsaved count behind the close question see tab editors only, and that Replace leaves open documents unsaved. No stage may land an editable preview unless all of this is proven by tests that fail without the code:

- An edit typed in the preview of a file with no tab is saved by Save All and by autosave (as the settings say), is counted by the question asked when the window or the project closes, and is never dropped silently: closing the dialog, choosing another result, closing the project, quitting.
- Replace and Replace All on a file the preview holds open end with the disk and the document agreeing, or with the document dirty and counted as above: read what the webview does after a replace on an open document (`crates/ide-host/src/ops/search/commands.rs`, `src/features/search/`) and do the same. The existing Replace tests that read the disk must keep asserting the user-visible result; do not weaken them to make them pass: if the right behaviour is that the document stays unsaved, the test asserts the document's text AND that the file is counted as unsaved.
- A file that changed on disk while the preview holds it with unsaved edits is handled as an editor tab handles it (the same question or reload), not overwritten silently.
- A document a preview opened is closed when the preview lets go of it and nothing else holds it (no leak of backend documents: assert the backend's open-document count returns to what it was). The writer found `preview_todo` drops its task while the open keeps running, so that document is never closed: fix it the same way and test it.

## Part 0: Save All saves only the active editor (measured by reading, to fix first)

`file.save` ("Save All", `app-gpui/src/editor/actions.rs`) saves the active text editor only and reports a failed save to the log only. The webview's saves every open document and custom document and raises one combined error to the user (`src/editor/core/controller.ts` `saveAll`, `manager.saveAll("explicit")`; read how the error is shown). The notebook slice put a wrapper in front of it that also saves the window's notebooks (`app-gpui/src/workbench/notebook.rs` `register_actions`): keep that working. First a test that fails today: two unsaved text tabs, Save All, both files on disk hold the text and neither tab is unsaved; and a save that fails (a read-only file in the temp folder) tells the user as the webview does and leaves that tab unsaved. Then fix it where the other "save everything first" paths already do it (`Workbench::save_all` in `workbench/mod.rs`, `workbench/branches.rs`, `workbench/project_files.rs`: read them, use one path, do not add a fourth). The row `action:file.save` is done only with those tests.

## Differences the draft leaves against the webview: close them or list them

- All IDE shortcuts are blocked while the preview has the keyboard; the webview blocks only the text-editing ones. Follow the webview (read which keys it lets through: the spec's "editing keys stay out of the results, Escape bubbles" case and the component).
- The preview shows fold chevrons; the webview's shows none. Hide them (read which gutter parts the webview's preview has).
- An empty current line gets no tint; a row keeps old colours when only its context changed. Fix if small.
- Undo shared between a tab and the preview of the same file (spec cases 3, 5 and 6) needs one undo history per document, which the editor does not have: do not build it here; those cases stay pending and the unit row partial. Say what a user sees today when both hold the same file: the text must stay the same in both (assert it), only the undo stacks differ.
- The Show Usages popup's preview stays as it is.

## Cost

The preview reads files that can be large. Nothing reads a file, parses it or walks the whole text on the window's thread or in paint; a result passed over before its file opened is dropped and its document closed. Say what paint iterates per frame, what the 80 ms debounce covers, and measure once: Find in Files over a temp project with a 5 MB file and a result in it; the dialog stays responsive (typing in the query still lands while the file opens). `a_result_passed_over_before_its_file_opens_is_dropped` depends on timing by the writer's own word: make it wait on states, not on time.

## Changed since the notes were written

The notes were written on a tree between c70a011c and the HTTP Client integration. Since then the HTTP Client, the visual defects slice, the notebook and the syntax tree commands landed. Measured at launch (`git diff --numstat c70a011c` over the existing files the hunks name): changed are `editor/view.rs` (+213 −41: the gutter has ordered icon columns and a numbers column of its own width, the row tints are painted before the right margin guides; the checker `$S/dprev/hunks.py` stops at its first miss there, a line that moved from 2379 to 2481), `workbench/code_insight.rs` (+20 −2) and `workbench/mod.rs` (+125 −8); unchanged are `code_insight/usages_popup.rs`, `code_insight/usages_view.rs`, `editor/marks.rs`, `everywhere/bookmarks_popup.rs`, `find/popup_view.rs`, `ui/mod.rs`, `workbench/tests/proofs12_find_popup.rs`, `workbench/tests/proofs_editor_search_find.rs`. The checker works by line numbers, so use it only as a list of the hunks: apply each hunk from its quoted anchor in the notes and read the place first. Also new since the notes: `file.save` has a wrapper in front of it that saves the window's notebooks (`workbench/notebook.rs` `register_actions`), `ui/text_area.rs` has `set_code_font`, the editor has a structure service (`editor/structure.rs`) that commands ask, and Kotlin has native colours. A preview editor shows no icon columns and no change lane, as the webview's shows none (read which gutter parts the webview's preview has). Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

Other agents' undeclared drafts now in the tree, not yours: `app-gpui/src/editor/view/gutter_marks.rs`, `vcs/hunk_popup.rs`, `workbench/editor_gutter.rs`, `workbench/editor_gutter/`, `settings_view/pages/build_deployment.rs`, `settings_view/pages/build_deployment/`, every `proofs27_*` and `proofs28_*` test file, `platform/error_log_sink.rs`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## The real window

Only through `$S/insp8/run.sh` (short `PATH`, temp `HOME`, `ZDOTDIR`, `OPENIDEA_GPUI_HOME`), with a temp project under a neutral path that does not hold the account's name (for example a folder you create under `/private/tmp/openidea-inspect-<random>/`, removed afterwards), because the previews draw paths. Look at: Find in Files with a result selected (the preview coloured, in the editor's font, the match marked, its line in view, line numbers, no fold chevrons), the Bookmarks dialog with a bookmark selected, the Find Usages tool window with a usage selected (read-only, every usage of the file marked), in the light theme and the dark one. Type one character in the Find in Files preview and look at how the unsaved state shows. Do not open the Agents tool window or the Services window.

## Report

Besides the format of `$S/int_rules.md`: the data-loss section first, one line per bullet above with the test that proves it and whether it failed without the code; one table of every `parity.json` key the notes name (`unit:src/features/search/__tests__/codePreview.test.tsx`, `unit:src/features/code-insight/usagePreview.test.tsx`, `point.registerOverlay:search.findInFiles`, `point.registerOverlay:search.bookmarks`, `point.registerToolWindow:usages`, `action:find.inPath`, `action:find.replaceInPath`, `action:navigate.showBookmarks`, `action:find.usages`, `feature:search`) with the state it reaches, the proving tests by full name, "N of M cases" for the unit rows (a native test that fills the colours directly instead of going through the backend's read covers the view, not the case: say which are which), and for partial what is missing; which way each hypothesis went; the cost measurements; the shots' absolute paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
