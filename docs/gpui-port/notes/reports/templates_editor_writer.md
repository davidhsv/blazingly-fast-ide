I wrote seven new files and a notes file. Parts 1 to 3 are whole and Part 4 is partial. Nothing has been compiled or run, so every test result below is still unknown. Your brief was wrong on three points:

- **No snippet session to reuse:** the native editor has none. The completion code computes one (`AppliedCompletion::snippet`), but `EditorIntelligence::accept` drops it and nothing else uses it. I built the session new, in `TemplateSession`.
- **Insert Live Template is not the chooser:** in the webview it opens the completion lookup, restricted to templates (`editor.ts:343-347`). I built it that way, which needs hunks in `editor/intelligence.rs` and `editor_lsp/completion.rs`. The chooser is used for Choose Template, Surround With and Expressions.
- **`EDITOR_WHEN` is not a context key:** the inventory picks it up from the `when` of `emmet.updateTag` (`index.ts:229`). I tested the condition it stands for instead.

Also, none of the `proofs32_*` test files (fixtures included) are declared in `mod tests`. My tests use `trusted_project`, `settle` and `run_action` from `mod.rs`.

**Files written** (none existing touched, all untracked):

| File                                                                     | Lines |
| ------------------------------------------------------------------------ | ----- |
| `…/app-gpui/src/templates/expand.rs`                                     | 382   |
| `…/app-gpui/src/templates/chooser_view.rs`                               | 465   |
| `…/app-gpui/src/templates/proofs33_expand.rs` (6 tests)                  | 207   |
| `…/app-gpui/src/templates/proofs33_chooser_view.rs` (7 tests)            | 205   |
| `…/app-gpui/src/workbench/editor_templates.rs`                           | 472   |
| `…/app-gpui/src/workbench/tests/proofs33_editor_templates.rs` (11 tests) | 415   |
| `…/app-gpui/src/workbench/tests/proofs33_surround.rs` (6 tests)          | 199   |
| Notes: `$S/drafts/templates_editor_integration.md`                       | 372   |

`…` is `<worktree>`. The notes hold 23 hunks. Parts 1 and 2 have to land together (hunks 1–23), because `editor_templates.rs` calls the lookup hooks Part 2 adds. Keys are claimed with `input::claim_keys` on the editor's focus handle, so `editor/view.rs` needs only one paint line and `platform/input.rs` is untouched. Run `cargo fmt` after applying: some code lines are over 120 columns.

**Parity rows** (all expected states are unverified until the tests run):

| Key                                                           | Expected               | Proof / what is missing                                                                                                                                                                                  |
| ------------------------------------------------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:code.insertLiveTemplate`                              | done                   | `proofs33_editor_templates::opens_a_lookup_restricted_to_live_templates_and_ends_with_it` (registry and ⌘J)                                                                                              |
| `action:code.surroundWith`                                    | done                   | `proofs33_surround::wraps_the_selected_lines` (registry), `…::lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks` (⌥⌘T), plus the multi-line statement and expression tests |
| `action:code.surroundWithLiveTemplate`                        | done                   | `proofs33_surround::surround_with_live_template_lists_the_selection_templates_and_configure_opens_the_settings` (⌥⌘J and registry)                                                                       |
| `point.registerEditorContribution:templates`                  | partial                | 10 tests in `proofs33_editor_templates`. Missing: templates in the ordinary completion list, the Expressions chooser from Tab, Emmet's turn of Tab                                                       |
| `point.registerOverlay:templates.chooser`                     | done                   | chooser end-to-end tests plus 7 `proofs33_chooser_view` tests                                                                                                                                            |
| `context:EDITOR_WHEN`                                         | not a real key         | `proofs33_surround::the_template_actions_are_unavailable_in_a_read_only_editor`                                                                                                                          |
| unit `editor.test.ts`                                         | done as models (14/14) | the app now reaches 11; not reached: the `.not` Expressions case and both completion-source cases                                                                                                        |
| unit `surround`, `surrounders`, `live`, `postfix`, `snippets` | done as models         | the app reaches all of `surround` and `surrounders`, most of `live`, part of `postfix`; plugin snippets have no native caller                                                                            |

**Parts**

- **Part 1, Tab expansion: whole.** Tab, Shift+Tab, Enter and Escape walk the fields; linked fields take typing; fields are drawn (needs a new `MarkKind`). An expansion is one undo step. The `clipboard()` function is tested. Templates are read from the UI storage keys on each key press; tests write the keys directly, covering added, edited, deleted and disabled templates and the expand option.
- **Part 2, Insert Live Template: whole,** through the lookup.
- **Part 3, Surround With: whole.** With a single caret it asks the backend (`syntax_support`, then `syntax_structural_ranges`) off the main thread. With a selection it opens the chooser at once.
- **Part 4, postfix: partial.** Tab expansion honours `enabled`, `expandWithTab` and `disabled`. Not built: postfix and live templates in the ordinary completion list (that needs a contributed source in `completion_answered`), and the Expressions chooser from Tab (no synchronous syntax tree at the dot).

**Cost per key press:** a scan of the caret's line, one pass over 119 live or 98 postfix templates, and a binary search in the highlights. Stored templates are re-parsed only when their text changed.

**Differences from the webview**

- Escape drops the selection only while a template is being filled in.
- Undo always gives the abbreviation back first. CodeMirror joins the expansion with typing done within 500 ms.
- No marker is drawn for an empty field.

**Hypotheses** (each with how to settle it in the notes, section d):

- The editor's report of the expansion's own edit arrives after the session is stored.
- Completion opened from a `complete()` field survives that report.
- Typed characters reach the chooser's key claim.
- The TypeScript, Python, Rust and Java grammars are bundled and highlight comments.
- The backend finds the statement when asked at column 0.
- Indent units are 2 for TypeScript and 4 for Python, Rust and Java.
- A read-only file on disk opens as a read-only editor.
- The default test keymap is IntelliJ.
- The `bg-editor-selection-inactive` token is a fitting field background.
- The lookup row text is `label — description`.
- Once its rows are navigated with the arrows, the lookup may still commit a row on `.`, space and similar characters; the webview's only filters or closes.

Most likely to fail first: the multi-line statement surround test, the exact indentation in the expected texts, and the comment test (it waits for highlights).
