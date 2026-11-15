I've written the refactor module and its integration notes. Nothing has been compiled or run, so none of the 13 tests below has passed yet. 11 of the 13 actions are registered; Pull Members Up and Push Members Down are not.

## Report back

**1. Files written**

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/refactor_actions.rs` (1,653 lines; about 600 are tests).
  - It holds the per-window state `RefactorActions`: the menu flow, the Refactor This flow, the running Safe Delete, and the remembered "Search in comments and strings" choice.
  - It has one `register_actions` and the `impl Workbench` handlers.
  - It drives the existing models `RefactorMenu`, `RefactorThis`, `SafeDeleteFlow` and `SuperMethodSearch` off the main thread.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/refactor_actions_integration.md` (147 lines): the hunks, assumptions, risks, the action → source → handler → test → IPC table, and spec coverage.
- **No new view.** Every popup is the existing `ListPopup`, including the numbered Refactor This list and "Nothing here" (one disabled row). Every dialog is the existing `PromptDialog`.

**Hunks the integrator must apply** (all insertions after a quoted line; details in the notes):

- `mod.rs`:
  - `mod refactor_actions;`
  - the field `refactorings: refactor_actions::RefactorActions` plus its `Default::default()` initialiser
  - the `refactor_actions::register_actions(registry);` call
- `editor_language.rs`: a `pub(super) async fn apply_code_action` that wraps the private `apply_action`, so refactorings reformat inserted lines the same way the code-action chooser does.
- **Menus:** none to add. Every id is already in `menu_spec.rs` and `context_menus.rs`. The editor context menu has no Introduce Field / Parameter rows, and neither does the webview's.

**2. Actions registered** (one test covers all registered editor actions for the `when` clause: they do nothing while a tool window has the keyboard, and work again in the editor)

| Action                                                                                                                                                      | Tests                                                                                                                                                                                                                                                                                    | IPC commands                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| The eight menu refactorings: `extractVariable`, `extractMethod`, `extractConstant`, `extractField`, `extractParameter`, `move`, `changeSignature`, `inline` | `apply_the_single_matching_action_or_offer_a_choice`, `offer_idea_s_occurrences_choice_when_the_server_has_both_variants`, `give_idea_s_caret_reason_the_server_s_disabled_reason_or_the_missing_server`, `say_the_server_doesn_t_support_it_when_its_advertised_kinds_can_t_produce_it` | `syntax_support`, `syntax_expression_at`, `lsp_code_action_query`, `cancel_request`, `lsp_apply_code_action`, `lsp_format` |
| `refactor.thisMenu`                                                                                                                                         | `lists_only_the_refactorings_applicable_at_the_caret_in_idea_order`, `says_so_when_nothing_applies` (in the editor and with a tool window focused)                                                                                                                                       | `lsp_code_action_query`, `lsp_prepare_rename`, `syntax_locals`, `syntax_expression_at`, `lsp_apply_code_action`            |
| `refactor.safeDelete`                                                                                                                                       | `confirms_first_delete_function_and_searches_nothing_when_cancelled`, `deletes_an_unused_declaration_as_one_undo_step_ignoring_recursive_calls`, `asks_when_usages_exist_view_usages_delete_anyway_cancel`                                                                               | `lsp_goto`, `doc_outline`, `syntax_locals`, `syntax_structural_ranges`, `lsp_references`, `syntax_identifiers`             |
| `navigate.superMethod`                                                                                                                                      | `looks_for_the_method_in_supertypes_level_by_level` (same file), `opens_the_super_method_in_another_file_or_says_there_is_none`                                                                                                                                                          | `doc_outline`, `lsp_type_hierarchy`, `doc_open` / `doc_open_virtual` / `doc_close`, `lsp_goto`                             |

**Not registered:** `refactor.pullMembersUp` and `refactor.pushMembersDown`. There is no native model or dialog for them: no member table with Make/Keep abstract, no destination picker, no "Problems Detected". They run only through the Java extension's `workspace/executeCommand`. Refactor This hides their rows while the actions are unregistered.

**3. Spec coverage end to end** (the rest is covered by the models' own tests)

- `refactorThis.test.ts`: 8 / 19.
- `safeDelete.test.ts`: 3 / 26.
- `structureNav.test.ts`: 1 / 5.

The uncovered cases are listed in the notes, section 10. Most need a grammar, a Java server, or are pure model cases.

**4. Behaviour not built**

- **Naming after Introduce / Extract:** the in-place rename of the introduced name is not built; the server's edit lands and the name stays. The models exist (`InplaceRename`, `introduced_rename`), but nothing in the editor drives them.
- **Syntax-only Introduce Variable / Constant:** not built. The menu action shows the reason instead; Refactor This hides those rows.
- **Chooser highlights:** no temporary highlight while the "Expressions" or "Multiple occurrences" choosers move.
- **Safe Delete preview:** shown as a confirm dialog, because the existing `RefactoringPreview` takes a `WorkspaceEdit`. This step is reachable but untested.
- **Super Method with several targets:** a plain list of `file:line` rows.
- **Popup position:** popups are not anchored at the caret.

**Assumptions to check first**

- Rename without a server is approximated: `syntax_locals` finding a local stands in for the webview's local-rename plan.
- Super Method in the same file relies on `open_project_file_at` moving the caret of the file already open.
- `syntax_*` calls on `.mock` files (no grammar) must return a non-cancel error or an empty answer.
- "Search in comments and strings" is remembered per window.

**Most likely to fail first**

- The two Super Method tests: caret placement, and opening then closing `b.mock` in the backend.
- The Refactor This list test: whether the disabled action's JSON maps to its disabled reason.
- The text match on the "Usages Detected" message.

**Expected state after integration**

- `refactor.extractVariable`, `refactor.extractConstant`: partial (no in-place naming, no syntax fallback).
- `refactor.extractMethod`, `refactor.extractField`, `refactor.extractParameter`: partial (no in-place naming).
- `refactor.inline`, `refactor.move`, `refactor.changeSignature`: done.
- `refactor.thisMenu`: partial (fallback and Pull/Push rows hidden, no chooser highlight).
- `refactor.safeDelete`: partial (preview shown as a confirm dialog and untested; no-server paths not tested end to end).
- `navigate.superMethod`: partial (plain chooser for several targets).
- `refactor.pullMembersUp`, `refactor.pushMembersDown`: not done (no model or dialog).
