# Integrator brief: editor behaviours whose models the view never used (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dedp/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

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
