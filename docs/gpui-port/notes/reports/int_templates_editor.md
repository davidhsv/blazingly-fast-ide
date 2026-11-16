All four parts are in and compiled: Tab expansion with a template session, Insert Live Template, Surround With, and postfix on Tab. Outside a template session, Tab, Shift+Tab, Enter and Escape do exactly what they did before, which I measured before and after. Every gate is green. I committed nothing, ran no git command that changes anything, left no process running, and touched nothing outside `app-gpui/`.

Unless marked "Hypothesis:", every statement below is measured. Logs are in `$S/dtpli/`, written below as `dtpli/`.

## Keys

**How the native editor orders a key** (read from `platform/input.rs:139-165`): first the claims on the editor's focus, in the order they were made, then the keymap, then the editor's own `key_down`. The claims run in this order:

1. Parameter info: Escape, and Tab/Shift+Tab only when argument keys are set. Only a unit test of `signature_view.rs` sets them (line 313), so in the app its Tab does nothing.
2. The completion lookup, while open: Enter inserts the item, Tab replaces with it, Escape closes it.
3. The intelligence's Escape for the hover or hint.
4. The new templates claim. It is installed after the intelligence, in `workbench/editor_intelligence.rs` after `install_editor_core`.
5. The usages Escape, created only when usages are highlighted.

The keymap's Tab and Escape go to inline completion, which is not registered natively, so they do nothing. Then `key_down` handles Enter, Tab (step out of a pair, else indent) and Escape (Find bar, then several carets, then the selection). Shift+Tab is not handled by `key_down`: today it reaches the text input as a literal tab character. That is unchanged by this slice; the webview unindents (`indentWithTab`, `src/editor/core/cm/setup.ts:393`).

**The webview's order** (from reading the source). At the highest precedence: the slot's Escape, which closes the Find bar (`src/editor/core/model/slot.ts:160` → `controller.ts:707`), and the lookup's Tab (`src/editor/lsp/completion.ts:819`) and Enter/Escape. Then the snippet keymap, Tab/Enter as `unlessLookup` and Escape as `escapeTemplate` (`editor.ts:194-198`). Then the templates' Tab at high precedence (`editor.ts:573`), then `insertTab` / `indentWithTab`.

- Hypothesis: the lookup's keymap runs before the snippet keymap because CodeMirror appends the snippet keymap with `appendConfig` (`node_modules/@codemirror/autocomplete/dist/index.js:1622`), after the completion keymap that was in the initial config. Confirm by running `editor.test.ts` with the lookup open in a field and pressing Escape.

**Outside a template:**

- Before the slice I wrote one test per key and ran them on the untouched tree (`dtpli/keys_before3.log`: 5 passed). They pass unchanged after it.
  - `workbench::tests::proofs33_editor_keys::tab_outside_a_template_indents_the_caret_the_lines_and_every_caret`
  - `…::shift_tab_outside_a_template_types_a_tab_character`
  - `…::enter_outside_a_template_breaks_the_line_with_its_indentation`
  - `…::escape_outside_a_template_closes_the_find_bar_then_the_carets_then_the_selection`
  - `editor::intelligence::tests::proofs33_template_keys::tab_and_enter_go_to_the_open_lookup_before_a_templates_abbreviation`
- Typing `g(` in a TypeScript file inserted no closing parenthesis in the test, so I took the step-out-of-a-pair case out of the Tab test.
- Existing suites by filter (`editor`, `completion`, `templates`, `platform::input`, `find`, `snippet`): all 1895 tests that passed before still pass (`dtpli/base_*.log` vs `dtpli/after_*.log`, compared with `comm`: 0 missing).

**Inside a template:**

- The lookup takes Tab and Enter first; once it is closed, Enter and Tab walk the fields. Escape closes the lookup first: `editor::intelligence::tests::proofs33_template_keys::in_a_template_the_lookup_takes_tab_and_enter_before_the_fields`.
- The Find bar takes Escape before the template, as in the webview (`slot.ts:160`). The draft did the opposite; I changed `template_escape` so it gives the key back while the tab's Find bar is open: `workbench::tests::proofs33_template_session::escape_in_a_template_closes_the_find_bar_first`.
- Read-only editors never expand: `workbench::tests::proofs33_surround::the_template_actions_are_unavailable_in_a_read_only_editor`.
- Preview editors never expand. Code previews never go through `install_intelligence` (grep: only `workbench/mod.rs:4021` and `tab_views.rs:938` call it): `workbench::tests::proofs33_template_session::a_find_in_files_preview_never_expands_a_template`.

## Undo

- One undo step per expansion: one ⌘Z gives the abbreviation back (`proofs33_editor_templates::expands_a_live_template_abbreviation`). I kept the draft's behaviour; no spec case says otherwise.
- After filling fields, ⌘Z takes back one field's typing at a time, then the expansion (`…::enter_moves_to_the_next_field_like_tab`).
- Redo puts the text back with no template to fill in (`proofs33_template_session::undo_gives_the_abbreviation_back_and_ends_the_template_and_redo_puts_the_expansion_back`).
- With several carets, only the main caret expands, as CodeMirror's `snippet()` does (it uses `selection.main`, `index.js:1607-1609`): `…::several_carets_expand_the_template_at_the_main_caret_only`.

## How a template session ends

Each ending has a test, and in each the fields stop being drawn:

- The caret leaves the field: `esc_first_drops_the_fields_selection_then_leaves_the_template`.
- Escape: same test.
- The last stop: `enter_moves_to_the_next_field_like_tab`.
- An edit from another editor of the document: an edit before the fields moves them, an edit across a field ends the template, no panic (`an_edit_from_another_editor_moves_the_fields_or_ends_the_template`).
- Reload from disk: `reloading_the_file_from_disk_ends_the_template`.
- Tab closed: `closing_the_tab_forgets_its_template`.
- Undo: the redo test above.
- Hypothesis: a formatter, a rename or Replace in Files arrive through the same `EditOrigin::External` path as the reload and the other editor's edit, so they behave the same. Not run.

A session is plain data in a map, with no timer.

## Cost (debug build, measured with a probe test I then deleted)

| Case                                           | Per Tab         |
| ---------------------------------------------- | --------------- |
| Built-in templates (136)                       | about 19–23 µs  |
| 2,000 stored templates (2,136)                 | about 98–103 µs |
| Parsing the stored text once, after it changes | 17 ms           |

That is under the 1 ms threshold, so I made no change. The stores are compared with the stored text on each Tab and parsed again only when that text changes. Nothing in paint looks templates up.

In the window, a full Tab press averaged 1.62 ms with built-ins against 1.94 ms for Enter, and 2.66 ms against 2.37 ms with 2,000 stored. Hypothesis: the 2.66 ms average includes the one-off 17 ms parse on the first press.

## Template functions and what each reads

I listed them from `templates/live_model.rs:400-425` and `templates/editor_context.rs:273-285`. A grep of `templates/{live_model,editor_context,editor,expand,snippet}.rs` for `std::env`, `std::fs`, `Command::new`, `home_dir` and `USER` finds nothing; the only outside read besides the clipboard is `Local::now()`. Nothing is evaluated as code and no process is started.

| Function                                                     | Reads                                                                                                                                   |
| ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `clipboard()`                                                | The app clipboard, only when a variable calls it. Tested with the test clipboard only: `a_clipboard_variable_takes_the_clipboard_text`. |
| `date()`, `time()`                                           | The system clock and local time zone, as the webview's `new Date()` does.                                                               |
| `user()`                                                     | The project root string (`/Users/<name>`), as the webview's `userName()` (`editor.ts:77`); not the environment.                         |
| `className`, `methodName`, `functionName`                    | The document text before the caret (up to 20,000 units) and the comment/string nodes taken from the highlights.                         |
| `fileName`, `fileNameWithoutExtension`, `filePath`           | The document's path.                                                                                                                    |
| `suggestVariableName`, `expressionText`                      | The postfix expression.                                                                                                                 |
| `commentStart`, `commentEnd`, `lineCommentStart`             | The language's comment tokens.                                                                                                          |
| Case and text functions, `concat`, `substringBefore`, `enum` | Their arguments only.                                                                                                                   |
| `complete` and the other interactive ones                    | Return empty text; `complete()` opens the lookup.                                                                                       |

## Gates

- **Final suite, run 1** (alone), `dtpli/final_suite1.log`: uptime load before 4.32 / 4.99 / 5.43, after 7.60 / 7.30 / 6.43. `test result: ok. 8104 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 207.28s`
- **Final suite, run 2**, `dtpli/final_suite2.log`: load before 6.97 / 7.17 / 6.40, after 7.85 / 10.44 / 8.36. `test result: ok. 8104 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 247.12s`
- **Twenty runs during run 2** with the built test binary, filters `proofs33_` and `proofs32_settings_live_template_expands` (`dtpli/final_loop_1.log` … `final_loop_20.log`): 20 of 20 passed, `test result: ok. 49 passed; 0 failed` each time. No test failed in any run.
- **web_view** (`dtpli/final_web_view.log`): `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.03s`
- **Clippy** `--all-targets -- -D warnings` (`dtpli/final_clippy.log`): exit 0.
- **`cargo build`** (`dtpli/final_build.log`): exit 0, 0 warnings.
- Earlier passing runs on trees that were later changed: `dtpli/suite1.log` and `dtpli/suite2.log` (8103 passed), `dtpli/suite3.log` (8104 passed).

**Failures seen along the way, all fixed:**

- `dtpli/keys_before1.log` and `keys_before2.log`: my first guesses at today's behaviour were wrong (Shift+Tab types `\t`, the multi-caret spacing, the Enter-over-selection result, a function item inserting `()`, no auto-pair from typing `(`). I adjusted the tests to the measured behaviour before landing anything.
- `dtpli/t1.log`, three failures:
  - `an_edited_built_in_expands_as_edited_and_a_disabled_template_does_not` and `tab_expands_the_live_templates_the_settings_stored`: "attempt to add with overflow" at `editor/buffer.rs:50`. The draft helper called `offset_of(line, u32::MAX)`; it now uses `line_range().end`.
  - `does_not_expand_code_templates_inside_comments`: the backend did not deliver within thirty seconds. Highlights wait on the executor's clock, which `settle` does not move; it now waits with `wait_long`.
- `dtpli/snip_before.log` and `dtpli/dot_before.log`: intended failures that confirmed two bugs before fixing them (see Hypotheses).
- `dtpli/sur.log`, my own new assertions: a typed `wr` first matched a built-in template, then the chooser's `q` mnemonic picked the template at once. The test now picks with the mnemonic `q`.

## Rows

`action:*` and `point.*` keys are absent from `docs/gpui-port/parity.json`, as is `context:EDITOR_WHEN`. The two `settings:` and six `unit:` keys exist; the unit rows are already done as models.

| Key                                          | State now         | Proving tests (full names) / what is missing                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:code.insertLiveTemplate`             | done              | Registry and ⌘J: `workbench::tests::proofs33_editor_templates::opens_a_lookup_restricted_to_live_templates_and_ends_with_it`, `…::a_dot_typed_in_the_template_lookup_ends_it_without_expanding`                                                                                                                                                                                                                                                      |
| `action:code.surroundWith`                   | done              | Registry: `workbench::tests::proofs33_surround::wraps_the_selected_lines`. ⌥⌘T: `…::lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks`. Also `…::a_caret_on_a_multi_line_statement_surrounds_the_whole_statement`, `…::offers_expression_surrounders_for_a_selection_inside_a_line`, `…::a_statement_answer_for_text_that_changed_meanwhile_is_dropped`, `…::surround_with_does_nothing_once_another_editor_is_active` |
| `action:code.surroundWithLiveTemplate`       | done              | `workbench::tests::proofs33_surround::surround_with_live_template_lists_the_selection_templates_and_configure_opens_the_settings`: ⌥⌘J picks a stored `$SELECTION$` template by mnemonic (text, caret, ⌘Z), registry, Escape, Configure Live Templates…                                                                                                                                                                                              |
| `point.registerEditorContribution:templates` | partial           | All tests in `workbench::tests::proofs33_editor_templates`, `workbench::tests::proofs33_template_session` and `editor::intelligence::tests::proofs33_template_keys`. Missing: the completion source (`editor.ts:526-555`), the Expressions chooser from Tab (needs a syntax tree at the dot), Emmet's turn of Tab, and the empty-field marker.                                                                                                       |
| `point.registerOverlay:templates.chooser`    | partial           | `…::several_templates_of_one_abbreviation_open_choose_template_at_the_caret`, the Surround tests, and the 7 tests in `templates::proofs33_chooser_view::*`. The Expressions variant is not reachable.                                                                                                                                                                                                                                                |
| `settings:editor.liveTemplates`              | partial           | The page's templates now expand: `workbench::tests::proofs32_settings_live_template_expands::a_live_template_added_on_the_page_expands_in_an_editor_of_the_window`, `…proofs33_editor_templates::tab_expands_the_live_templates_the_settings_stored`, `…::an_edited_built_in_expands_as_edited_and_a_disabled_template_does_not`. "Show in completion" reaches nothing.                                                                              |
| `settings:editor.general.postfixCompletion`  | partial           | `enabled` and the `disabled` list reach Tab: `…::expands_postfix_templates_replacing_the_expression`. `expandWithTab` off is untested; `showInCompletion` reaches nothing.                                                                                                                                                                                                                                                                           |
| `unit:…/editor.test.ts`                      | reached 11 of 14  | Missing: `wraps_compound_expressions_for_not_and_offers_a_chooser` and the 2 completion-source cases.                                                                                                                                                                                                                                                                                                                                                |
| `unit:…/surround.test.ts`                    | 2 of 2            |                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `unit:…/surrounders.test.ts`                 | 3 of 3            |                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `unit:…/live.test.ts`                        | most reached      | `suggests variable names` only through postfix `.for`.                                                                                                                                                                                                                                                                                                                                                                                               |
| `unit:…/postfix.test.ts`                     | partial           | Wider candidates from the syntax tree are not reached.                                                                                                                                                                                                                                                                                                                                                                                               |
| `unit:…/snippets.test.ts`                    | 0 of 2            | Nothing feeds plugin snippets into the native store.                                                                                                                                                                                                                                                                                                                                                                                                 |
| `EDITOR_WHEN`                                | not a context key | It is the constant at `src/features/templates/index.ts:28`, picked up by the inventory from a `when`. The condition is proven off by the read-only test and on by the action tests.                                                                                                                                                                                                                                                                  |

**Changes to ordinary completion**, both through the editor-intelligence and completion-view hooks the slice adds (in `editor/intelligence.rs` and `editor_lsp/`):

- A server's snippet item with several placeholders now starts a session, so Tab walks its placeholders: `editor::intelligence::tests::proofs33_template_keys::tab_walks_the_placeholders_of_a_servers_snippet_item`. This touches the LSP-completion snippet behaviour.
- Rows the templates list for Insert Live Template take no commit character.

The completion suite still passes (86 before and after, 87 with the new test).

## Which parts landed whole

- Parts 1 and 2 (Tab expansion with the session; Insert Live Template through the lookup) landed together, plus the session for server snippets.
- Surround With landed, reworked to ask through the structure service: a new `editor::structure::commands::statement_ranges` uses the service's cache and its `answer`/`store`, and drops an answer for a version the document has left. I followed the webview rather than the brief's wording "does nothing if the editor changed": a changed text gives no statement target but the chooser still opens on the line (`editor.ts:390`), and a changed active editor does nothing (`index.ts:209`).
- Postfix on Tab landed.
- The settings test that waited for this slice is now declared as `proofs32_settings_live_template_expands`. I trimmed the pending file to the New-file half, which another slice owns.

## Hypotheses: which way each went

- The `Edited` report arrives after the session is stored: held (`enter_moves_to_the_next_field_like_tab`).
- `cx.defer` in `complete_in` runs after the report: held. New test `…proofs33_template_keys::a_field_that_calls_complete_opens_the_lookup_when_the_caret_reaches_it`.
- The chooser's key claim gets the typed character: held (chooser tests).
- The bundled grammars highlight comments: held, once the wait moves the executor's clock.
- `syntax_structural_ranges` at column 0 returns the `if` statement: held.
- Indent units as the notes give them: held.
- Tab after an unknown word inserts spaces: held.
- A 0444 file opens read-only: held.
- `cmd-j`, `cmd-alt-t`, `cmd-alt-j` on macOS: held.
- The theme tokens parse: held.
- Rust `.for` stops on `item`; `synchronized` leaves `this` selected: held.
- The Rust expression surrounders are `1` and `2`: held.
- The template lookup commits on `.` after ↓: true, and a bug. `dtpli/dot_before.log` shows `console.log(":", );`. Fixed in `editor_lsp/completion_view.rs`.
- The brief's "an ordinary completion item's snippet is dropped by `accept`": true (`dtpli/snip_before.log` gave `call(x  , b)`). Fixed as above.
- The notes' "the webview closes the lookup and the selection with one Escape" is false by the CodeMirror precedence reading above. That reading was not run, so it is still a Hypothesis.
- The field background colour fits: seen in the shots below. Fields with no text are not drawn (`set_marks` drops empty marks), so the empty-field marker is still missing.
- Stale in the notes: they say to run `cargo fmt`, which the rules forbid; the two draft test-helper bugs above.

## Shots

All under `$S/insp8/shots/`. They were taken with the binary from before the Find-bar Escape change, which none of these screens exercises.

- **Template expanded:**
  - `dtpli2-01-fields-light-1.png`: `fori` expanded, the three `i` selected, the status bar says "3 carets".
  - `dtpli3-05-fields-dark-1.png`, cropped: the same in dark.
  - `dtpli5-01-fields-unselected-light-1.png` and `dtpli5-02-fields-unselected-dark-1.png`: after one Escape the selection is gone and each `i` keeps a visible field background (pale blue in light, dark blue in dark). The empty array and body fields are not drawn.
- **Insert Live Template lookup:**
  - `dtpli2-02-insert-live-template-light-1.png` and `dtpli3-06-insert-live-template-dark-1.png`: the completion lookup lists `log console.log` and `logv …` with the footer.
  - The selected row's highlight covers only the text, not the row's width. Hypothesis: that is the existing lookup's own drawing. Confirm with a `.mock` server lookup shot.
- **Surround With chooser:**
  - `dtpli2-03-surround-chooser-light-1.png`: "Surround With", items `1. if` … `0. # region…`, mnemonics underlined. The selected row's highlight was only as wide as its text; I fixed it in `templates/chooser_view.rs`.
  - `dtpli3-07-surround-chooser-dark-1.png`: after the fix, the selected row spans the width.
  - Python has no `$SELECTION$` templates, so no Live templates section and no Configure item, as in the webview.
- **Surround result:**
  - `dtpli2-04-surround-result-light-1.png`: `if :` around the two lines, indented one level, caret in the condition.
  - Hypothesis: `dtpli3-04-surround-result-light-1.png` and `dtpli3-08-surround-result-dark-1.png` show the same, since the script did not change between runs. Not opened.

The temporary projects under `/private/tmp/openidea-inspect-*` are removed.

## Files I edited

All under `app-gpui/src/`:

- Existing files: `editor/intelligence.rs`, `editor/marks.rs`, `editor/structure/commands.rs`, `editor/view.rs`, `editor_lsp/completion.rs`, `editor_lsp/completion_view.rs`, `templates/mod.rs`, `workbench/mod.rs`, `workbench/editor_intelligence.rs`, `workbench/tests/proofs32_settings_templates_pending.rs` (trimmed to the New-file half).
- Drafts handed to me, now declared and formatted: `templates/expand.rs`, `templates/chooser_view.rs`, `templates/proofs33_expand.rs` and `templates/proofs33_chooser_view.rs` (formatted only), `workbench/editor_templates.rs`, `workbench/tests/proofs33_editor_templates.rs`, `workbench/tests/proofs33_surround.rs`.
- New, untracked: `editor/intelligence/tests/proofs33_template_keys.rs`, `workbench/tests/proofs33_editor_keys.rs`, `workbench/tests/proofs33_template_session.rs`, `workbench/tests/proofs32_settings_live_template_expands.rs`.
- Created and deleted: the cost probe `workbench/tests/proofs33_cost_probe.rs` and its `mod` line.

I did not write the report file `docs/gpui-port/notes/reports/templates_editor_writer.md`, because `docs/` is off-limits.
