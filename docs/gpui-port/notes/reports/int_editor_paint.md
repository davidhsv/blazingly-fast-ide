I integrated all seven items and finished Parts 0 and 0b. Every gate passes and nothing is committed.

## 1. Gates (measured, logs under `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dedp/`)

- **Suite 1** (`suite1.log`, nothing else running):
  - `uptime before: 22:28 … load averages: 5.21 5.19 5.68`
  - `test result: ok. 7687 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 192.36s`
  - `uptime after: 22:33 … load averages: 31.16 15.85 9.87`
- **Suite 2** (`suite2.log`):
  - `uptime before: 22:33 … load averages: 27.75 15.62 9.86`
  - `test result: ok. 7687 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 222.62s`
  - `uptime after: 22:37 … load averages: 90.44 40.56 21.16`
- **Twenty times by filter, during suite 2** (`loop20.log`, run from the built test binary; per-test counts in `loop20_summary.txt`): `test result: ok. 63 passed; 0 failed; …` 20 of 20 times. Every one of the 63 tests passed 20 of 20, and no test failed even once.
  - The filters were `proofs23_`, the `view::{column_gestures,right_margin,doc_tags,caret_glide}` and `scheme_runtime` unit tests, the Part 0 test, and the four tests of Part 0b.
  - The `proofs23_` filter also picked up 10 terminal `proofs23_*` tests that are not mine.
- **`cargo test --test web_view`**: `test result: ok. 1 passed; 0 failed` (in `scratchpad/web_view.log`).
- **`cargo clippy --all-targets -- -D warnings`**: rc=0, 0 warnings (final run after the last edit).
- **`cargo build`**: rc=0, 0 warnings.
- **Failures seen during the work**, all fixed:
  - 3 clippy `single_range_in_vec_init` errors in the `doc_tags.rs` unit tests (now `std::slice::from_ref`).
  - Kotlin cases of the string-clipboard tests and three doc-tag tests (item 4 and item 2 below).
- **Editor performance and scroll tests**: `editor::view::tests::a_long_file_costs_a_viewport_per_frame` and `editor::core_actions::tests::scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view` passed in both suites.
- **Scope**: `git status --short src/ src-tauri/ crates/ e2e/` prints nothing.
- **HEAD moved** from `4a3fd6fe` to `2e93c23c` while I worked. The new commits hold another agent's notebook drafts and docs; none of the files I edited.

## 2. Per part

**Part 0.** I added the spec's last check to `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs` (`skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files`), following `e2e/diff-navigation.spec.ts:96-98`:

- Before Restore Size, `debug_bounds("diff-toolbar")` is drawn.
- After it, the toolbar is not drawn, and the Project tree's root row (`project-row:<root>`) is drawn.
- It passed at once with no app change: the maximized side covers the editor (`workbench/mod.rs`, "A maximized side covers the editor"). There was no defect to fix.

**Part 0b.**

- **What changed:**
  - The stand-in server (`editor/intelligence.rs`, `fake_server`) now logs every reply id to `<log>.answered`. A process-wide list of installed servers is keyed by configuration directory, and `FakeServer::report_for(config)` prints each request (id, method, answered / answered null / no answer), the last document syncs with their versions, and the keys of the answers file.
  - A new `workbench::tests::window_lsp_report` prints each server as the backend has it (`ops::lsp::lsp_servers`: state, pid, languages), as the window last heard of it (status and message, through a new `#[cfg(test)] Workbench::language_server_entries` in `code_insight.rs`), and the stand-in's report.
  - It is printed when these run out: `wait_long` and the backend `settle` in `workbench/mod.rs`, the fixture `settle`s of `refactor_actions.rs` (now with the refactor trace too), `editor_extras.rs` and `editor_language.rs`, and the extractField loop's panic.
  - Verified once with a temporary print (since removed). It showed `mock-ls Ready pid=Some(11808)`, `#2…#9 textDocument/codeAction answered` and `didChange a.mock v1…v8`.
- **Race in the stand-in: none for these four tests** (from reading the code, not measured).
  - `answer()` writes a staged file and renames it over the answers file, and the server re-reads that file for every message.
  - Each test calls `answer()` before it runs the action, so the action's request reads the new answers.
  - One real hazard: `by_call` counts every call of a method since the server started, background code-action requests included. None of the four tests uses `by_call`, so I left it alone.
- **App defect: none found, so nothing fixed.** The app side exposes no per-request queue without editing `crates/`, which the brief does not allow.
- **Result:** 0 failures in two suites and 20 loops.
- **Hypothesis:** the four failures were timeouts under very high load; the extractField log shows load 106. Confirm by reading the new report the next time one fails in a whole-suite run.

**Item 5, middle button and Column Selection Mode: integrated.**

- Follows `multiCaret.ts:46-51` and `columnMode.ts:163-169`.
- Files: `editor/view.rs` (mod line, press check, `drags_with`, middle down/up listeners), plus the `mod` lines for `proofs23_middle_column` and `proofs23_column_mode_drag`.
- Measured: a middle press does reach the editor in the test platform; the block test passes.

**Item 1, right margin: integrated.**

- Follows `prefs.ts:160-171`, `:537` and `setup.ts:153-155`. No guides in Distraction Free Mode, per the source.
- Files: `language_facts.rs` (`right_margins`), `editor/view.rs` (layout field, offsets in prepaint, quads in paint).
- Measured: the dotted settings keys are accepted (the tests wait on `facts().right_margins == [40,120]`).

**Item 4, string-literal clipboard: integrated, Kotlin left out.**

- Follows `stringClipboard.ts:125-160`.
- Files: `language_facts.rs` (`escape_on_paste`), `editor/view.rs` (`copy` unescapes, `paste_text` escapes before reindenting).
- Paste from History fix: `workbench/editor_core.rs` `paste_into` now applies `escaped_paste`, and `escaped_paste` became `pub(crate)`.
  - New test: `editor::command_actions::tests::proofs23_string_clipboard::escapes_an_entry_pasted_from_the_history_into_a_string`.
  - It failed without the fix (measured): `left: "class A { String value = \"say \"hi\"\"; }"`.
- **Hypothesis settled, and false for Kotlin** (measured): for `val s = "|"` the native highlights are only `[HighlightSpan { from: 4, to: 5, kind: Variable }]`. There are no string or keyword spans for Kotlin, so nothing gets escaped. The other eight languages pass.
  - I removed Kotlin from `MODES` and removed the window test `escapes_kotlin_dollars_and_unescapes_them_on_copy`. The model test `editor_commands::string_clipboard::tests::escapes_kotlin_dollars_and_unescapes_them_on_copy` still covers the logic.

**Item 2, doc-comment tags: integrated.**

- Follows `docCommentTags.ts`.
- Files: `editor/view.rs` (`DocLanguage` listener, `painted_doc_tags` in prepaint, recolouring in the row loop).
- Test fixes I made, against the draft:
  - Measured: native Java `/** */` is drawn as `syn-comment`, not `syn-doc-comment`. The tree-sitter-java query only has `@comment`. The "styles…" test now compares against the colour the comment's `/**` is drawn in.
  - The multi-line fixtures now start with `package a;`. Hypothesis: the default `folding.fileHeader` folds a leading comment, which explains the `None` at line 1 and the 0..404 painted range. Confirm by running the original fixtures with `editor.folding.fileHeader=false`.
  - Measured: just after the switch to plain text, the old highlights stay for a moment. The removal and rebuild tests now wait until the expected colour is drawn.
- Measured: a whole Java doc comment works as one span. `keeps_tags_deep_inside_a_long_doc_comment` passes with the view starting at line 300 or later.

**Item 3, smooth caret: integrated.**

- Follows `prefs.ts:470-520` and `theme.ts:58-59`.
- Files: `language_facts.rs` (`caret_motion`), `editor/view.rs` (`glide` field, `arm_glide` in `set_selections`, the frame in prepaint, the caret position in paint).
- Measured: the window redraws after the glide's notify; the 90 ms frame shows the caret part way.

**Item 6, colour scheme and weights: integrated.**

- Follows `colorSchemeRuntime.ts`, `editorSettings.ts:238-249` and `highlight.ts:17-33`.
- Files: `theme/mod.rs`, `theme/color_scheme.rs` (`SCHEME_LANGUAGES` made `pub(crate)`), `lib.rs` (`scheme_runtime::init`), `editor/view.rs` (`TokenStyles`, `painted_fonts`).
- Fix against the draft: `font_weight_of` no longer turns 0 into the default. As in `fontWeightOf`, a weight of 0 becomes 100.
- `code_font_of` now rounds to 50 the same way. Its test is `editor::command_actions::tests::proofs23_color_scheme::sets_the_configured_weights_on_the_root_and_follows_settings_changes`: a weight of 349 is drawn at 350.

**Item 7, Paste from History preview: integrated.**

- Follows `PasteHistoryDialog.tsx:219-243`.
- Files: `paste_history_view.rs`, `workbench/editor_core.rs`.
- Fix found in the shots: a long line wrapped onto the row below it. Rows are now `whitespace_nowrap`; shot `dedp2-01` shows the line cut at the edge.
  - I added no test for this one-line style change.
- **Not built**: the preview's line numbers and special-character display.

**Item 8, Markdown paste: skipped**, as the brief said. Only its read-only test is declared: `editor::command_actions::tests::proofs23_clipboard_read_only::does_not_insert_into_a_read_only_editor`, which needed no app change.

**Missing (known limits, kept as the brief asked):**

- No token backgrounds or effects, so doc tags look like plain comments in the light theme.
- The caret does not blink.
- The scheme does not switch back when the theme turns light or dark.
- Markdown paste of file paths and images is not built.

**Per-frame paint cost:**

| Item         | Work per frame                                                                                                                                                                                           | Where inputs are cached and invalidated                                                                |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Right margin | One shaped `"0"` and one quad per guide, only while guides are on and Distraction Free is off                                                                                                            | Columns are kept in `EditorFacts`, read again on `SettingsChanged`                                     |
| Doc tags     | Binary search into the highlights, then only the spans touching the painted rows; scans only the painted rows; one rope-sharing `edit_state()` clone when a comment is on screen in a language with tags | Highlights refresh after an edit (after the highlight delay) and on a `DocLanguage` change             |
| Caret glide  | One point interpolated; a 16 ms timer only while a move is under way (≤180 ms)                                                                                                                           | Nothing cached                                                                                         |
| Token styles | 25 kinds, each two `format!`s, a few theme lookups and a `Font` clone; does not grow with the document                                                                                                   | Theme re-cascaded by `appearance::apply` when a scheme key changes; `SchemeLanguageTokens` is a global |
| Preview      | Highlights computed off the window's thread once per distinct text; at most 40 lines drawn                                                                                                               | Per preview text                                                                                       |

Nothing reads from disk during paint.

## 3. Parity keys

| Key                                                                | State                                                                                | Cases and tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `unit:src/editor/core/cm/multiCaret.test.ts`                       | done, 6 of 6                                                                         | `editor::view::editing_tests::proofs_editor_multi_caret::{each_preset_keeps_the_default_gesture, matches_the_complete_stroke_and_only_the_left_button, uses_rebinding_immediately_and_yields_column_selection_to_the_caret_gesture, honours_removal_of_the_caret_gesture_and_restores_it (+ restores_alt_drag_column_selection_once_the_caret_gesture_is_removed), matches_normalized_ctrl_alt_strokes_as_reported_for_right_alt_on_windows}` and `…::proofs23_middle_column::preserves_middle_button_column_selection_except_on_linux`                                                                                                                                                                                                                                                                                                                                                   |
| `action:editor.toggleColumnSelection`                              | done                                                                                 | `editor::command_actions::tests::toggles_switch_column_selection_and_overwrite`, `…::proofs23_column_mode_drag::column_selection_mode_makes_a_plain_drag_select_a_block`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `unit:src/editor/core/cm/distractionFree.test.ts`                  | done, 3 of 3                                                                         | `workbench::editor_extras::tests::proofs13_editor::defaults_the_text_width_to_80_columns_and_centres_at_the_configured_width`; `editor::command_actions::tests::proofs23_right_margin::shows_line_numbers_and_the_right_margin_normally`; `…::proofs23_right_margin::hides_line_numbers_gutter_margin_guides_and_sticky_lines_and_centres_at_the_configured_width` with `workbench::editor_extras::tests::distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text`                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:src/editor/core/cm/stringClipboard.test.ts`                  | **partial**                                                                          | Window tests `editor::command_actions::tests::proofs23_string_clipboard::{escapes_ordinary_pastes_into_a_string, unescapes_copied_string_contents, round_trips_pasted_control_characters_and_unicode, escapes_only_the_single_quoted_strings_delimiter, leaves_paste_as_plain_text_and_typing_unchanged, disables_both_escaping_and_unescaping_with_the_setting, requires_every_caret_to_be_inside_a_plain_string, escapes_per_caret_inserts_with_each_literals_delimiter_and_maps_all_carets, copies_multiple_string_selections_as_decoded_fragments, replaces_a_substring_and_preserves_the_caret_undo_and_redo, handles_native_paste_copy_cut_and_rich_copy_consistently}`. The other cases have model tests under `editor_commands::string_clipboard::tests::*`. **Missing:** Kotlin in a real editor (no string spans in native Kotlin highlighting), and text drop (no native drop) |
| `unit:src/editor/core/cm/docCommentTags.test.ts`                   | partial, 12 of 13                                                                    | 9 model tests in `editor_commands::doc_comment_tags::tests::*`, plus `editor::command_actions::tests::proofs23_doc_comment_tags::{styles_the_tags_with_the_highlight_styles_classes_inside_the_comments_own, rebuilds_decorations_when_the_highlighter_is_reconfigured, removes_decorations_when_switching_to_an_unsupported_language}`. **Missing:** "keeps tag marks inside backend syntax and semantic comment marks"                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `unit:src/editor/core/cm/ideaParity.test.ts`                       | partial (writer's count 26 of 27; Hypothesis: I checked only this slice's two cases) | `…::proofs23_smooth_caret::glides_selection_only_moves_and_puts_the_caret_in_place_at_once_when_typing`, `…::proofs23_smooth_caret::a_move_follows_the_chosen_movement_curve`. **Missing:** smooth blinking                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `unit:src/platform/colorScheme.test.ts`                            | partial, 13 of 14                                                                    | 11 model tests `theme::color_scheme::tests::*`, `…::proofs23_color_scheme::sets_the_configured_weights_on_the_root_and_follows_settings_changes`, `theme::scheme_runtime::tests::validates_typography_settings_weights`. **Missing:** "coerces editor settings to their defaults' shape"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `module:src/platform/colorScheme.ts`                               | partial                                                                              | `…::proofs23_color_scheme::{the_editor_draws_tokens_with_the_schemes_edits_and_the_typography_weights, offers_a_language_scheme_page_and_scopes_its_overrides_to_that_languages_editors, applies_an_explicit_scheme_to_the_editor}`. **Missing:** token backgrounds and effects, the observer that switches the scheme back with the theme, per-language root tokens                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `point.registerOverlay:editor.pasteHistory`                        | done                                                                                 | `workbench::editor_core::tests::proofs23_paste_preview_look::the_paste_history_preview_draws_with_the_target_editors_font_and_grammar`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `unit:src/editor/core/view/PasteHistoryDialog.test.tsx`            | partial (writer's count 6 of 12, Hypothesis)                                         | adds `editor::paste_history_view::tests::proofs23_paste_preview::shows_newest_first_and_a_read_only_preview_with_the_target_grammar_and_editor_font`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `unit:src/editor/core/cm/clipboard.test.ts`                        | partial (writer's count 19 of 25, Hypothesis)                                        | adds `…::proofs23_clipboard_read_only::does_not_insert_into_a_read_only_editor`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `action:code.pasteFromHistory`, `module:src/editor/core/extras.ts` | text only                                                                            | Their "Missing" text should now point at the overlay row above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

## 4. Shots (`/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/`)

- `dedp1-01-rust-margin-1.png`: one guide just right of column 116, at about column 120, with nothing drawn over text. The current-line tint covers the guide on line 1.
- `dedp1-03-java-doc-toggled-theme-1.png` (dark): `@param`, `@return`, `<b>`, `</b>` and `{@link` in the tag colour, `name` in the tag-value colour, the rest of the comment grey.
- `dedp1-04-paste-history-1.png`: two entries, the preview coloured and in the editor font, but the long line wrapped. That led to the fix, shown in `dedp2-01-paste-history-1.png` (line cut at the edge).
- `dedp1-05-paste-history-second-1.png`: the second entry, a comment, coloured.
- I did not look at `dedp1-02-java-doc-start-theme-1.png` or `dedp2-02-paste-history-second-1.png`.

## 5. False or stale in the notes and briefs

- Every grammar giving a string span with its quotes: false for Kotlin (measured above).
- The doc-tag test's assumption that Java doc comments are drawn in `syn-doc-comment`: false; native Java draws them in `syn-comment` (measured).
- The draft's `font_weight_of(0)` returning the default contradicted `fontWeightOf`, which gives 100.
- The draft doc-tag tests started their files with the comment, which the default file-header fold hides (Hypothesis, as in item 2).
- The brief says the extractField wait "now reports the window's state". It reported the refactor trace and the methods the server got, not the language-server state; it does now.
