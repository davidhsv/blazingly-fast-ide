# Integrator brief: editor behaviours whose models the view never used (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dedp/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## Part 0: one missing check of a diff flow (do this first, small)

`workbench::diff_tab::tests::proofs18_e2e_diff_navigation::skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files` (`app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs`) drives the third flow of `e2e/diff-navigation.spec.ts` to its end but leaves out the spec's last check (line 96: after Restore Size with the Project view maximized, the diff's toolbar is hidden, then the Project tree is visible). Add that assertion as the spec states it: the diff tab's viewer (its toolbar) is not drawn while the Project tool window is maximized. Assert it on what the window draws (the element's bounds or its absence from the rendered tree, as other tests of a maximized tool window do: grep `maximized_side` and `is_tool_window_visible` for the pattern), not on a flag. If the native app does draw the editor area under a maximized tool window, that is a defect against the webview: fix it and say so. Report the assertion and whether it failed before any fix.

## Part 0b: four waits on the stand-in language server that failed once each (bounded: half a day at most)

Four different tests failed once each in whole-suite runs under load (about 30 to 100), each waiting thirty seconds for something a language server's answer brings, and each passes alone:

- `workbench::refactor_actions::tests::apply_the_single_matching_action_or_offer_a_choice` ("refactor.extractField did not happen", `$S/verify_ddfg_suite2.log`); 0 failures since in 1,288 runs alone and 18 suites. Its wait now reports the window's state.
- `workbench::refactor_actions::tests::confirms_first_delete_function_and_searches_nothing_when_cancelled` ("the dialog did not happen", `$S/dterm/sl_anyfail3.log`).
- `workbench::popup_placement::tests::show_usages_opens_under_the_caret` ("the usages popup did not happen", `$S/dterm/sl_anyfail7.log`).
- `workbench::project_view_options::tests::reformat_and_optimize_imports_in_the_tree_change_the_selected_file` ("c.mock's imports sorted did not happen ... reached later: None", `$S/verify_dterm_suite2.log` line 7657: the whole report is there; the Git and tree traces show nothing pending).

What they share is a hypothesis, not a finding: each runs an action that asks the stand-in language server (code actions, references) right after the test changed the server's answers or the document. Do not loop single tests (1,288 runs alone found nothing). Instead make the failure explain itself once, in the shared place: when a `wait_long` / `settle` of a workbench test runs out, its report also prints the language-server side of the window (for each server of the project: started or not, its state, the requests sent with their ids and methods and whether each got its answer, the last error; and what the stand-in server received and answered, if the test has one). Read how the stand-in server takes a changed set of answers (`server.answer(...)`) and say whether a request that arrives before the new answers are in place can be answered with the old ones or not at all: if that race exists in the stand-in, fix the stand-in (the tests then wait on the server having taken the answers, a state, not a sleep). If the app can lose a request or an answer (a server still starting, a document version mismatch dropped silently where the webview retries or shows something), that is an app defect: fix it with a unit test that fails without the fix. If any test fails in one of your whole-suite runs, keep that run's log and quote the failure whole in the report. Say plainly what was measured and what stays a hypothesis.

## Changed since the notes were written

The notes were anchored on the tree at commit c0e0e25a. None of the existing files their hunks anchor on changed since (measured: the files named in the notes against `git diff --name-only c0e0e25a`; only the draft files themselves are in both lists). `app-gpui/src/workbench/mod.rs` did change (the Agents New Task page, the refactorings, the Git text actions, the diff tab, the Terminal's groups); the notes put no hunk there. Re-anchor every hunk from its quoted text anyway and read the place before you edit it.

## The slice

Notes (per item: hunks in the order to apply them with quoted anchors, `mod` lines, parity keys with case counts, the tests most likely to fail): `docs/gpui-port/notes/integration/editor_paint_integration.md`. Writer's report: `docs/gpui-port/notes/reports/editor_paint_writer.md`. Draft files (committed, undeclared), all under `app-gpui/src/`: modules `editor/view/{column_gestures,right_margin,string_literals,doc_tags,caret_glide,token_style}.rs`, `editor/paste_history_view/preview.rs`, `theme/scheme_runtime.rs`; tests `editor/view/editing_tests/proofs23_middle_column.rs`, `editor/paste_history_view/tests/proofs23_paste_preview.rs`, `workbench/editor_core/tests/proofs23_paste_preview_look.rs`, `editor/command_actions/tests/proofs23_{column_mode_drag,right_margin,string_clipboard,doc_comment_tags,smooth_caret,color_scheme,clipboard_read_only}.rs`.

Seven items, each independent: (1) right margin guides, (2) doc-comment tags, (3) smooth caret movement, (4) string-literal clipboard, (5) column selection with the middle button and in Column Selection Mode, (6) colour-scheme overrides and font weights, (7) the Paste from History preview with the file's grammar and the editor font. Apply ONE item at a time: its hunks, its `mod` lines, compile, its tests by filter, fix, then the next. If an item does not converge, take its hunks out by editing, leave its files undeclared, say exactly where it stands, and go on to the next.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/editor/core/cm/*.ts` and their `*.test.ts`, `src/platform/colorScheme.ts`, `src/editor/core/view/PasteHistoryDialog.tsx`). The hunks were anchored on the tree at commit c0e0e25a. Settle the writer's hypotheses by test before building on them: each grammar gives a string literal as one span that includes its quotes (otherwise escaping silently does nothing: the test must fail then, not pass); a whole doc comment is one span; a middle-button press reaches the editor in the test platform; the window redraws after the glide's notify; the dotted settings keys are accepted.

- Paint cost: `editor/view.rs` is the hot path. No hunk may do work in paint that is not bounded by the visible lines, and nothing may read files, fonts or settings from disk during paint. For each painted item say in the report what it costs per frame (what it iterates) and where its inputs are cached and invalidated. Run the existing editor performance or scroll tests if the crate has any, and say which.
- Paste from History is not escaped inside a string literal by the draft (the webview escapes it): apply the writer's one-line fix with a test.
- Known limits the writer recorded (keep them as "Missing" in the report, do not build more than a small local fix): no token backgrounds or effects (so doc tags look like plain comments in the light theme), the caret does not blink natively, the scheme does not switch back when the theme turns light or dark, Markdown paste of file paths and images is not built (item 8 of the notes is skipped).
- After it passes, look at your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, temp project; launcher `$S/insp8/run.sh`, steps `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md`): a Rust file with the right margin on; a Java file with a doc comment in the dark theme; Paste from History open over a Rust file with two entries. Look at each shot: guides at the right column, nothing drawn over text, the preview coloured and in the editor font.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script.

## Report

Besides the format of `$S/int_rules.md`: per item, integrated or not; one table of every `parity.json` key the notes name with the state it reaches (done or partial), "N of M cases" for a `unit:` row with each case's native test by full name, and for partial what is missing; the per-frame cost of each painted item; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` test file this brief does not hand you. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.

Among those drafts: `app-gpui/src/deployment/`, `workbench/deployment.rs`, `app-gpui/src/http_client/`, `workbench/http_client.rs`, `workbench/http_client/`, `platform/error_log_sink.rs`, every `*_pending.rs` this brief does not hand you.

Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Read the scratch copy of the notes under `$S/drafts/` where one exists: the copies under `docs/` were reformatted by the commit hook. The real binary is launched only through `$S/insp8/run.sh`, which gives it a short `PATH` and a temp `HOME`. Do not open the Agents tool window or the Services window in it.
