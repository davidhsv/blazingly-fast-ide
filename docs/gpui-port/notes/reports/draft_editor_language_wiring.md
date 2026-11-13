I wired 26 of the assigned actions into a new workbench module. Nothing was compiled or run: none of the tests below has been run, and the module needs hunks in 7 existing files before it builds. Three real gaps: Rename is dialog-only, eight refactor actions are left out, and `editor.showHint` is already registered by another draft.

## 1. Files written

| File                                                                                                                                                                    | Lines | Contents                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- | -------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/workbench/editor_language.rs`                                                                                                                             | 2258  | `impl Workbench` blocks, `register_actions`, async handlers, the refactoring undo history, 13 end-to-end tests |
| `app-gpui/src/editor_lsp/definition_view.rs`                                                                                                                            | 267   | `DefinitionPopup` view, 1 model test and 2 view tests                                                          |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/editor_language_integration.md` | 297   | Notes: every hunk, IPC table, what is not wired, assumptions, likely failures                                  |

**`DefinitionPopup`**

- Constructor: `new(targets, root, cx)`; targets come from `DefinitionTarget::read(location, text)`.
- Events: `DefinitionEvent::Open(Location)`, `DefinitionEvent::Closed`.
- Accessors: `index()`, `targets()`, `header()`, `lines()`.
- Keys: ←/→ switch definitions, F4 opens, Escape closes.

**Existing views reused:** `ListPopup` (the Generate and Go to Test choosers) and `PromptDialog` (the rename name and the rename preview, through `show_prompt_dialog`).

**Hunks the integrator must apply (all in the notes):**

- `editor_lsp/mod.rs`: `pub mod definition_view;`
- `workbench/mod.rs`: the `mod` line, the `language` field and its init, the register call, the re-export of `undo_refactoring`/`redo_refactoring`, and a route in `run_notification_action`.
- `editor_intelligence.rs`: call `watch_workspace_edits`, and route the → menu `Control` event to `run_intention_control`.
- `code_insight.rs`: `apply_inspection` becomes `pub(super)`.
- `editor/intelligence.rs`: new `pub fn show_hint_at` (hint with position, tone and timeout).
- `editor/marks.rs`: `MarkLayer::Usages`.
- `editor/actions.rs`: `edit.undo` / `edit.redo` fall back to the refactoring history.

## 2. Actions registered

| Action                                                                                   | Test that runs it                                                                                                                                       | IPC commands                                                                                                              |
| ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `refactor.rename`                                                                        | `rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each`                                                                              | `lsp_prepare_rename`, `lsp_rename`, `cancel_request`, `apply_workspace_edit`, `undo_workspace_edit`/`redo_workspace_edit` |
| `code.reformat`, `code.reformatFile`                                                     | `reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines`                                                                               | `lsp_format`                                                                                                              |
| `code.reformatWithPrettier`, `…WithOxfmt`, `…WithRuff`                                   | `reformat_with_a_named_formatter_runs_that_formatter` (Unix only)                                                                                       | `lsp_format`                                                                                                              |
| `code.optimizeImports`, `code.optimizeImportsFile`, `code.cleanupFile`                   | `optimize_imports_and_code_cleanup_apply_the_servers_source_actions`                                                                                    | `lsp_code_actions`, `lsp_apply_code_action`, `lsp_format`                                                                 |
| `navigate.nextError`, `navigate.prevError`, `code.showErrorDescription`                  | `next_error_moves_the_caret_to_the_next_problem_and_shows_its_message`                                                                                  | none (problems the editor already shows)                                                                                  |
| `code.applyQuickFix`                                                                     | `apply_quick_fix_applies_the_fix_of_the_problem_at_the_caret`                                                                                           | `lsp_code_actions` (quickfix only), `lsp_apply_code_action`                                                               |
| `find.highlightUsages`, `navigate.nextHighlightedUsage`, `navigate.prevHighlightedUsage` | `highlight_usages_marks_the_occurrences_and_the_caret_moves_through_them`                                                                               | `lsp_document_highlights`                                                                                                 |
| `code.quickDefinition`, `code.quickTypeDefinition`                                       | `quick_definition_shows_the_definitions_lines_and_f4_opens_it`                                                                                          | `lsp_goto`                                                                                                                |
| `code.generate`, `code.implementMethods`, `code.overrideMethods`                         | `generate_lists_the_generate_actions_and_applies_the_chosen_one`; ⌘N in a Markdown file still opens New: `cmd_n_in_a_markdown_file_opens_the_new_popup` | `lsp_code_actions`, `lsp_apply_code_action`                                                                               |
| `navigate.gotoTest`                                                                      | `opens_the_only_test_of_a_class` (also goes back to the subject)                                                                                        | `project_roots`, `find_files`                                                                                             |
| `navigate.nextMethod`, `navigate.previousMethod`                                         | `moves_by_line`                                                                                                                                         | `doc_outline`                                                                                                             |

Behaviour worth knowing:

- **Rename is dialog-only.** "One undo" means a single Undo in the editor reverts the whole edit in both files, as in the webview. The notification's Redo re-applies it.
- **The → menu fix.** `opens_a_rows_menu_with_the_right_arrow_and_runs_its_items` tests it. With the two hunks applied, the drafted "fails by design" test `a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls` passes too.
- **Formatter tests run no real binary.** They point `languages.<tool>.path` at temp shell scripts.
- **External formatter tests use Light mode**, so no real language server starts.

**Not registered:**

- **`editor.showHint`:** `workbench/editor_core.rs`, another agent's draft, already registers it as a notification. Registering it again would silently replace that handler.
- **In-place rename:** `Editor` has no edit path that stays out of the undo history.
- **`code.reformatDialog`:** `PromptDialog` has no scope radio group.
- **`navigate.superMethod`:** needs `doc_open`/`doc_close` around `doc_outline` and the type hierarchy, plus a location chooser.
- **`refactor.thisMenu` and the Extract / Inline / Move / Change Signature actions:** the `runRefactoring` path (expression chooser, query grouping, `lsp_execute_code_action`, the Introduce session) does not exist natively.
- **`refactor.pullMembersUp` / `refactor.pushMembersDown`:** no hierarchy dialog.
- **`refactor.safeDelete`:** its usage-search and conflicts views do not exist.
- **`code.expressionTypeInfo`:** needs an `EditorIntelligence::show_documentation` API.
- **The six `editor.inlineCompletion.*` actions:** the editor cannot paint ghost text.
- **`code.applyPastedImports`:** the editor's paste has no hook.
- **The `~` actions:** left with `editor/intelligence.rs`, except `code.showIntentions`, which gains the → menu controls.

## 3. Spec cases covered

| Spec                    | Covered | Note                                                                                |
| ----------------------- | ------- | ----------------------------------------------------------------------------------- |
| `Popups.test.tsx`       | 2/13    | "switches between several definitions…" in the view, and the → menu case end to end |
| `gotoTest.test.ts`      | 2/9     | "opens the only test of a class", "goes back from a test to its subject"            |
| `structureNav.test.ts`  | 1/5     | "moves by line"                                                                     |
| `workspaceEdit.test.ts` | 1/6     | "applies single-file renames directly" (inside the rename test)                     |

The rest of those specs are model cases already tested natively, or belong to the actions left out.

## 4. What is not built, assumptions, likely failures

**Not built:**

- the syntax-tree local rename (no server, or broken code);
- the per-language Code Style indent override when formatting;
- re-indenting the code Generate or a quick fix inserts;
- interactive code-action commands that ask for input;
- the status bar drawing of the usages text (`usages_status()` holds it), and distinct read/write usage colours;
- the declaration extent and syntax colouring in Quick Definition.

**Check these first:**

- `WorkspaceEditApplied` carries this window's label (or none).
- Its `paths` match the editor paths.
- A `null` answer to `prepareRename` comes back as `Ok(None)`.
- The test keymap binds `Meta+N` and `F2` as in the IntelliJ presets.
- Light mode keeps every language server off.
- A hierarchical `documentSymbol` of kind 12 becomes a Function in the outline.

**Most likely to fail on first run:**

- the rename test: window label, paths and `prepareRename: null`;
- the ⌘N and F2 keystrokes, because of the keymap;
- the named-formatter test: settings keys, and Ruff's config lookup with an empty directory;
- `moves_by_line`: the outline conversion;
- Code Cleanup: the fake server answers both code-action requests with the same list, so the test relies on the two edits staying on separate lines.

**Side effect of the hunks:** every code action applied as a workspace edit now raises "Refactoring applied", and `edit.undo` reverts it. The webview behaves the same; by grep, no existing test depends on the old behaviour.
