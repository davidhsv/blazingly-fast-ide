# Integrator brief: four Editor settings pages and three sets of effect tests (a draft to declare, wire and prove; five rows to parity, two narrowed)

Read first: `$S/int_rules.md` (hard rules and report format) and `$S/view_rules.md`. Your own scratch subfolder: `$S/dsep/`.

Format only files you edited, taken from `git diff --name-only` plus new files you created. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## What exists

A writer without a compiler left a draft, committed and not declared (nothing references it, it has never been compiled): `app-gpui/src/settings_view/pages/editor_{natural_languages,spelling,font_preview,code_style_tabs}_pending.rs`, `app-gpui/src/workbench/tests/proofs46_settings_editor_pages_pending.rs` and `app-gpui/src/workbench/tests/proofs46_spelling_typo_effect_pending.rs`. Its notes are `docs/gpui-port/notes/designs/settings_editor_pages_integration.md`: read them whole before anything else (the hunks for existing files, the hypotheses with what settles each, the section "Measured by text: the Typo inspection is not reached", the tests most likely to fail first, and the table "Rows if everything lands"). The notes' anchors were taken at an older HEAD: find each anchor again by its text, not by its line number.

The draft is a proposal, not a specification: the webview at HEAD is (`src/features/settings/pages/editor/`). Where they differ, the webview wins and you say so.

## Rows this slice is to raise

To parity (each on the tests the notes' table names, checked against the webview page, not against the draft):

- `settings:editor.naturalLanguages`: the page, with the bundled dictionaries and nothing stored.
- `settings:editor.font`: the Preview under the controls following the edits before Apply, its text not stored (needs the notes' text area hunk).
- `settings:editor.codeStyle.other`: the Scheme row, and Tab size and Use tab character applied on the page being what Tab inserts in a plain-text file.
- `settings:editor.fileEncodings`: the note, a project encoding chosen on the page decoding a project file, With BOM writing the BOM to a new file.
- `settings:editor.general.codeFolding`: the four options changed through the Settings window deciding the fold column and the folds a file opens with.

Narrowed (quote in the report the clause closed and the clause left):

- `settings:editor.naturalLanguages.spelling`: the dictionaries edited and stored. Its remaining clause is that no editor runs the Typo inspection, so a word accepted on the page is not seen to stop being a typo. Read the notes' section on it and `proofs46_spelling_typo_effect_pending.rs`: if wiring the inspection into an open editor is small (about forty lines, no new view, no new backend call), do it and prove the effect; otherwise leave the test file undeclared, and say exactly what is missing.
- `settings:editor.codeStyle`: the tabs and the right margins. The clauses about the line separator, indent detection, EditorConfig support and the rest stay unless a test of yours closes one.

Read each row's whole current text in `docs/gpui-port/parity.json` first: if it has a clause the notes' table does not mention, name it in the report.

## What to do

1. Settle the notes' hypotheses first, each by the cheap check the notes name. A hypothesis that turns out false changes the draft before it is compiled.
2. Declare the draft under its final names (no `_pending` in a declared module or file name; rename with `git mv`), apply the hunks, and make it compile. Fix what the compiler and clippy find in the draft's own files; a hunk that no longer fits the code at HEAD is rewritten against the code as it is.
3. Run the draft's tests. Each one that fails: decide by the webview whether the test or the code is wrong, and say which in the report.
4. Each new window test is seen failing with its guard switched off: one mutation per script run that restores the file in a trap and compares hashes (`docs/gpui-port/notes/tools/mutation_check_example.sh`). Rebuild any test a mutation does not fail. Tests of a page's model alone need no mutation and raise no row.
5. No assertion of an existing test is weakened; an existing test whose expectation changes is named with the webview's `file:line`. `workbench::tests::smoke_settings_pages::every_settings_page_opens_and_applies_a_change_made_with_the_mouse` walks every page: it must still pass with the new pages.

If the whole draft cannot land within your budget, land page by page in the order of the rows above and stop cleanly at the end of a page. Never leave a declared module half wired: a page that is not finished is taken out again by editing.

## Care

- Tests use a temp home and project; no network; nothing under the user's home; no program of the machine is run. Fonts: a test may name a family and must not depend on which fonts the machine has beyond what the area's tests already assume. Every wait covers exactly what the next assertion needs: the suite runs under a load above 100.
- The real binary is not used in this slice.
- The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.
- No new dependencies, no `unsafe`, no stubs, no `#[ignore]`. Comments say why the code is as it is, never where it came from or which task added it.

## Changed since

HEAD is the commit `git log -1` shows (the worktree is clean except docs the manager is editing: do not touch `docs/`). Since the draft (`e3af47146`) landed, among others: the Editor Tabs settings, the remote folder chooser, wiring batch 1 (the TODO preview is an editable editor now; breakpoints move with edits), three tests-only harvests, the Services tree's folder nodes (`app-gpui/src/run_testing/services.rs`), and a fix of the tool window stripe buttons (a click's action is deferred out of the listener: a listener of a view must never run an action whose handler updates that same view; use `window.defer` as `app-gpui/src/workbench/mod.rs` does for the stripe button). The undeclared files to leave alone are listed in `$S/undeclared_now.txt` (refreshed at this HEAD; your own draft's files are in it: those you declare). The suite at this HEAD: 8,436 tests.

The owner uses release builds of this branch daily now. In your report, add a section "Mouse paths not tested": every clickable thing your slice adds or wires (buttons, rows, menu rows, links) for which no test clicks it.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings; `node docs/gpui-port/gen-data.mjs --check` if you touched anything it reads. Never pipe a test run into `head`. Start no test run with `&`. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: a table with one line per row above (key; raised, narrowed or not; the tests, by full name; the mutation; the clause that remains, quoted, and why); the hypotheses and how each was settled; every difference from the webview's page you found, fixed or not, with `file:line`; the draft's tests that failed first and which side was wrong; what in the notes or in the rows' texts turned out false.

## Do not touch

Every other undeclared draft under `app-gpui/src` and every other undeclared `proofs*` or `*_pending.rs` test file (`$S/undeclared_now.txt`). No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
