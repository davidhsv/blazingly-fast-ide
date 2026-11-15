# Integrating the language-server refactorings into the workbench

New file, not compiled yet: `app-gpui/src/workbench/refactor_actions.rs` (one child module of the workbench, with its end-to-end tests). No view file was needed: every popup is the existing `ListPopup` (`editor_lsp/intentions_view.rs`) and every dialog the existing `PromptDialog` (`vcs/prompt_dialogs.rs`). No existing file was edited. Every anchor below is quoted from the file as it is now; line numbers are left out because `mod.rs` is moving. All hunks are insertions after the quoted line.

All sections are required: without 1d the module's `pub(super) fn register_actions` is dead code (`-D warnings`), without 2 it does not compile.

## 1. `app-gpui/src/workbench/mod.rs`

### 1a. The module

After `mod project_view_options;` add:

```rust
mod refactor_actions;
```

### 1b. The `Workbench` field

In `pub struct Workbench`, after

```rust
    language: editor_language::EditorLanguage,
```

add:

```rust
    /// The Refactor menu's server refactorings, Refactor This and Safe Delete while they run, and
    /// Safe Delete's "Search in comments and strings" as last confirmed.
    refactorings: refactor_actions::RefactorActions,
```

### 1c. Its initial value

In `Workbench::new`, in the `Self { … }` literal, after

```rust
            goto_line: None,
            language: Default::default(),
```

add:

```rust
            refactorings: Default::default(),
```

(Anchor on both lines: `language: Default::default(),` alone may occur in other literals.)

### 1d. The actions

In `pub fn register_actions(cx: &mut App)`, after `    editor_language::register_actions(registry);` add:

```rust
    refactor_actions::register_actions(registry);
```

## 2. `app-gpui/src/workbench/editor_language.rs`

The refactorings apply a code action exactly as the code-action chooser does (with the reformatting of the lines it inserts, `reformat_generated`), which is the private `apply_action`. After

```rust
pub(super) type This = WeakEntity<Workbench>;
```

add:

```rust

/// Applies a chosen code action as the code-action chooser does (reformatting the lines it
/// inserted), for the Refactor menu's server refactorings.
pub(super) async fn apply_code_action(
    this: &This,
    editor: &Entity<Editor>,
    doc: DocId,
    action: CodeAction,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    apply_action(this, editor, doc, action, cx).await
}
```

(Every type it names is already imported there.)

## 3. Menus

Nothing to add: `menu_spec.rs` (Navigate › Super Method; Refactor › Refactor This…, Change Signature…, Introduce Variable / Constant / Field / Parameter…, Extract Method…, Inline…, Pull Members Up / Push Members Down…, Move…, Safe Delete…) and the editor context menu in `context_menus.rs` (Go To › Super Method; Refactor › …) already list every id. The editor context menu has no Introduce Field / Parameter rows, and neither has the webview's (`src/editor/core/controller.ts`, the "Refactor" submenu): faithful. Titles, `when` clauses and shortcuts come from `assets/gen/actions.json` / `keymaps.json` through `registry.handle`; all 13 ids are in the manifest (checked).

## 4. Not wired, and why

- `refactor.pullMembersUp`, `refactor.pushMembersDown`: not registered. There is no native model or dialog for them (the members table with Make/Keep abstract, the destination select, "Problems Detected", the preview): `editor_refactor/` has nothing for `hierarchy.ts` / `HierarchyDialog.tsx`, and they run only through the JDT extension's `workspace/executeCommand` (`openidea.java.hierarchy.members` / `.refactor`, `ide_host::ops::lsp::lsp_hierarchy_members` / `lsp_hierarchy_refactor`). Refactor This leaves their rows out while the actions are not registered (it checks `ActionRegistry::contains`), so registering them later shows the rows with no change here.
- The in-place naming after Introduce / Extract (IDEA's rename of the introduced name with ↑/↓ suggestions, ⇧F6 for the dialog): not built. `editor_lsp/inplace_rename.rs` (`InplaceRename`) and `server_introduce::introduced_name` / `introduced_rename` exist as models but no editor session drives them (the same gap leaves `action:refactor.rename` partial). The server's edit lands; the name is not renamed afterwards.
- The syntax-based Introduce Variable / Constant when no server answers (`introduce.ts` `introduceFallback`): no native executor. The menu action shows the reason (`MenuStep::IntroduceFallback.otherwise`, the same hint the webview shows when the fallback declines); Refactor This leaves the fallback rows out.
- The temporary highlight while the "Expressions" and "Multiple occurrences found" choosers move: not drawn (there is no temporary-highlight mark layer; `MarkLayer::Usages` belongs to Highlight Usages).
- Safe Delete's preview of the mentions in comments and strings (`SafeDeleteStep::Preview`, the webview's preview popup): shown as a `PromptDialog::confirm` titled "Safe Delete Function 'x'" whose message is the warning and one `line: text` row per mention, OK labelled Delete. The existing `RefactoringPreview` (`project_view/file_dialogs.rs`, `project_files.rs::show_preview`) takes a `WorkspaceEdit`, not `editor_refactor::preview::Preview`. Reachable on `.mock` (a `// unused` comment), not tested.
- Super Method with several targets: a plain `ListPopup` of `file:line` rows with the folder as detail, not the webview's `locationItems` (symbol icon and the target's line).
- Refactor This and the Expressions chooser are not anchored at the caret (the popup slot's default place).

## 5. Assumptions (check first)

- **A1.** `FactRequest::LocalRename` (Rename without a server) is answered by `syntax_locals` finding a `Locals::Local`; the webview uses `planLocalRename` (syntax-fallback rename plan). Same outcome for a local, but not proven equal.
- **A2.** Safe Delete's "Search in comments and strings" is remembered per window (the webview keeps one module-level value).
- **A3.** `open_project_file_at` on the file already open in the active editor moves its caret (Super Method in the same file relies on it).
- **A4.** `syntax_expression_at` / `syntax_locals` on a `.mock` document (no grammar) answer an error that is not "cancelled" (or an empty answer), so Refactor This goes on to its rows.
- **A5.** `ListOptions { numbered: true }` puts `"1 "` before a row in `ListPopup::lines()` (as its doc says) and Escape on a `PromptDialog::ask` answers `CANCEL`.

## 6. Compile risks

- `refactor_actions.rs` reaches parent-private items: `Workbench.label`, `Workbench::open_popup`, `Workbench::close_popup`, `branches::run_action_later` / `show_prompt_dialog` (`pub(super)`), `editor_language::This`. All visible to a child module of `workbench`.
- `let plain = ListOptions::default;` (a fn item reused in two arms of `run_actions_step`); replace by `ListOptions::default()` at the call sites if it trips a lint.
- `with_checked` is called on the value `PromptDialog::ask` returns inside the dialog builder closure.
- `ide_host::ops::Channel::new(|_| {})` relies on inference from `lsp_references`' parameter, as `project_files.rs` does.
- `json!({ "symbol": …, "locations": Vec<Location> })` needs `Location: Serialize` (it is an IPC type).
- Tests: `MENU_CASES` and the outline fixtures are plain `json!` values; the long lines were wrapped by hand, run `cargo fmt`.

## 7. Tests most likely to fail first

- `apply_the_single_matching_action_or_offer_a_choice`: `asked_only` counts `params.context.only` of the logged `textDocument/codeAction`; if `lsp_code_action_query` widens or drops `only` the assertion fails although the edit lands.
- `looks_for_the_method_in_supertypes_level_by_level` (same file, A3) and `opens_the_super_method_in_another_file_or_says_there_is_none` (the backend opens `b.mock` with `doc::open` for its outline, then closes it before the editor opens it).
- `lists_only_the_refactorings_applicable_at_the_caret_in_idea_order` (A4, and the disabled action JSON `disabled: { reason }` mapping to `disabled_reason`).
- `asks_when_usages_exist_view_usages_delete_anyway_cancel`: the "Usages Detected" body is `"1 usage found\n…"`; `message_text()` may hold it differently if the dialog splits lines.

## 8. Actions

| Action id                   | Webview source                                                           | Native handler                                                                                                                               | Test                                                                                                                                                                                                                                                                                | IPC commands                                                                                                                                                              |
| --------------------------- | ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `refactor.extractVariable`  | `src/editor/lsp/actions.ts:302` → `refactorThis.ts` `runRefactoring`     | `refactor_actions::register_actions` → `Workbench::start_refactoring` → `refactor_menu` / `carry_out_menu` (`RefactorMenu`)                  | `apply_the_single_matching_action_or_offer_a_choice`, `offer_idea_s_occurrences_choice_when_the_server_has_both_variants`, `give_idea_s_caret_reason_the_server_s_disabled_reason_or_the_missing_server`, `the_editor_refactorings_do_nothing_while_a_tool_window_has_the_keyboard` | `syntax_support`, `syntax_expression_at`, `lsp_code_action_query`, `cancel_request`, `lsp_apply_code_action`, `lsp_format`                                                |
| `refactor.extractMethod`    | `actions.ts:305`                                                         | same                                                                                                                                         | same (minus occurrences)                                                                                                                                                                                                                                                            | `syntax_support`, `lsp_code_action_query`, `cancel_request`, `lsp_apply_code_action`, `lsp_format`                                                                        |
| `refactor.extractConstant`  | `actions.ts:306`                                                         | same                                                                                                                                         | same                                                                                                                                                                                                                                                                                | as extractVariable                                                                                                                                                        |
| `refactor.extractField`     | `actions.ts:309`                                                         | same                                                                                                                                         | same                                                                                                                                                                                                                                                                                | as extractVariable                                                                                                                                                        |
| `refactor.extractParameter` | `actions.ts:310`                                                         | same                                                                                                                                         | same                                                                                                                                                                                                                                                                                | as extractVariable                                                                                                                                                        |
| `refactor.move`             | `actions.ts:313`                                                         | same                                                                                                                                         | same + `say_the_server_doesn_t_support_it_when_its_advertised_kinds_can_t_produce_it`                                                                                                                                                                                               | `syntax_support`, `lsp_code_action_query`, `cancel_request`, `lsp_apply_code_action`, `lsp_format`                                                                        |
| `refactor.changeSignature`  | `actions.ts:314`                                                         | same                                                                                                                                         | same                                                                                                                                                                                                                                                                                | as move                                                                                                                                                                   |
| `refactor.inline`           | `actions.ts:330`                                                         | same                                                                                                                                         | same + `offer_idea_s_occurrences_choice_when_the_server_has_both_variants` (several actions)                                                                                                                                                                                        | as move                                                                                                                                                                   |
| `refactor.thisMenu`         | `actions.ts:289` → `refactorThis.ts` `refactorThis` / `showNothingHere`  | `Workbench::start_refactor_this` → `refactor_this` / `fact` (`RefactorThis`), `show_refactor_this`, `run_refactor_item`, `show_nothing_here` | `lists_only_the_refactorings_applicable_at_the_caret_in_idea_order`, `says_so_when_nothing_applies`                                                                                                                                                                                 | `lsp_code_action_query`, `cancel_request`, `lsp_prepare_rename`, `syntax_locals`, `syntax_expression_at`, `lsp_apply_code_action` (+ the chosen row's own)                |
| `refactor.safeDelete`       | `actions.ts:317` → `safeDelete.ts` `safeDelete`                          | `Workbench::start_safe_delete` → `safe_delete` / `carry_out_safe_delete` / `safe_delete_answer` (`SafeDeleteFlow`)                           | `confirms_first_delete_function_and_searches_nothing_when_cancelled`, `deletes_an_unused_declaration_as_one_undo_step_ignoring_recursive_calls`, `asks_when_usages_exist_view_usages_delete_anyway_cancel`, the `when` test                                                         | `lsp_goto` (definition), `doc_outline`, `syntax_locals`, `syntax_structural_ranges`, `lsp_references`, `syntax_identifiers`, `doc_edit` (the deletion through the editor) |
| `navigate.superMethod`      | `src/editor/navigation/index.ts:174` (`superMethod`, registered at :190) | `Workbench::start_super_method` → `super_method` / `super_locations` (`SuperMethodSearch`, `member_at`)                                      | `looks_for_the_method_in_supertypes_level_by_level`, `opens_the_super_method_in_another_file_or_says_there_is_none`, the `when` test                                                                                                                                                | `doc_outline`, `lsp_type_hierarchy` (incoming), `doc_open`, `doc_open_virtual` (library supertypes), `doc_close`, `lsp_goto` (declaration / implementation fallback)      |
| `refactor.pullMembersUp`    | `actions.ts:331` → `hierarchy.ts`                                        | not registered (section 4)                                                                                                                   | —                                                                                                                                                                                                                                                                                   | would be `lsp_hierarchy_members`, `lsp_hierarchy_refactor`, `apply_workspace_edit`                                                                                        |
| `refactor.pushMembersDown`  | `actions.ts:334` → `hierarchy.ts`                                        | not registered (section 4)                                                                                                                   | —                                                                                                                                                                                                                                                                                   | same                                                                                                                                                                      |

The IPC adapters are in `src-tauri/src/commands/lsp.rs` (`lsp_code_action_query`, `lsp_apply_code_action`, `lsp_prepare_rename`, `lsp_goto`, `lsp_references`, `lsp_type_hierarchy`, `lsp_format`), `doc.rs` (`doc_open`, `doc_open_virtual`, `doc_close`, `doc_outline`), `syntax.rs` (`syntax_support`, `syntax_expression_at`, `syntax_locals`, `syntax_structural_ranges`, `syntax_identifiers`) and `app.rs` (`cancel_request`); the native code calls the same `ide_host::ops::*` functions with the same arguments.

## 9. Models that differ from the webview

- None found in `refactor_this.rs`, `server_introduce.rs`, `catalog.rs`, `safe_delete.rs`, `structure_nav.rs` for what is wired. The models leave three requests without an executor here: `MenuStep::IntroduceFallback` / `RefactorRun::IntroduceFallback` (no `introduce.rs`), `RefactorRun::Hierarchy` (no hierarchy model) and `FactRequest::LocalRename` (answered per A1).

## 10. Spec coverage (end to end here; the rest by the models' own tests)

- `src/editor/refactor/refactorThis.test.ts`: 8 / 19 end to end — lists only…, says so when nothing applies, runs the mapped server action (Inline row; the Rename / Safe Delete rows run the registered actions), apply the single matching action or offer a choice, say the server doesn't support it, give IDEA's caret reason / the disabled reason (missing server: model only), never fall back to unrelated code actions (inside the caret-reason test), offer IDEA's occurrences choice. Model only: the 3 catalog cases, Pull/Push in Java files (no Java server in tests, not registered), the syntax Introduce (2, no executor), cancels a superseded query, choose the expression first (`.mock` has no grammar), rename the introduced name in place (not built).
- `src/editor/refactor/safeDelete.test.ts`: 3 / 26 end to end — deletes an unused declaration as one undo step ignoring recursive calls, asks when usages exist, confirms first and searches nothing when cancelled. Not end to end: the 12 pure cases (model tests in `safe_delete.rs`), the preview (substituted, untested), another file's declaration and "needs a symbol at the caret" (reachable, not written), the 2 no-server cases (need a grammar), the 6 project-view file cases (`FileUsagesFlow`, wired in `project_files.rs`).
- `src/editor/navigation/structureNav.test.ts`: 1 / 5 end to end (looks for the method in supertypes, one level, same file and another file); the other 4 are model cases (two of them already proven through Next / Previous Method in `editor_language.rs`).
