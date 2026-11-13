# Integrating the editor's language actions into the workbench

New files, not compiled yet:

- `app-gpui/src/workbench/editor_language.rs`: the actions, their handlers, the refactoring undo history and the end-to-end tests.
- `app-gpui/src/editor_lsp/definition_view.rs`: the Quick Definition popup, with its own tests.

Every anchor below is quoted from the file as it is now. Line numbers are left out because `mod.rs` moves. Sections 1 to 7 are all required. Without them the new module does not compile (`show_hint_at`, `MarkLayer::Usages`, `apply_inspection`), or `-D warnings` fails on dead `pub(super)` items: `watch_workspace_edits`, `run_language_notification`, `run_intention_control`, `undo_refactoring` and `redo_refactoring` are only called from these hunks. Run `cargo fmt` after pasting, because a few signatures run past 120 columns.

## 1. `app-gpui/src/editor_lsp/mod.rs`

After `pub mod configuration_navigation;` add:

```rust
pub mod definition_view;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module and the Edit › Undo entry points

After `mod editor_intelligence;` add:

```rust
mod editor_language;
```

After the function `pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> { … }` add:

```rust
pub(crate) use editor_language::{redo_refactoring, undo_refactoring};
```

### 2b. The `Workbench` field

In `pub struct Workbench`, after

```rust
    /// The dialog a Git operation waits on.
    vcs_dialog: Option<branches::VcsDialog>,
```

add:

```rust
    /// The editor language actions: the refactorings undoable as one step, the highlighted usages,
    /// the rename waiting for its edit.
    language: editor_language::EditorLanguage,
```

### 2c. Its initial value

In `Workbench::new`, in `let mut workbench = Self { … }`, after `vcs_dialog: None,` add:

```rust
            language: Default::default(),
```

### 2d. The actions

In `pub fn register_actions(cx: &mut App)`, after `code_insight::register_actions(registry);` add:

```rust
    editor_language::register_actions(registry);
```

### 2e. The refactoring notification's Undo / Redo button

In `fn run_notification_action`, before

```rust
            EntryAction::Own(own) if own.command.starts_with("vcs.deleted.") => {
```

add:

```rust
            EntryAction::Own(own) if own.command == editor_language::WORKSPACE_UNDO_COMMAND => {
                self.run_language_notification(&own.command, cx);
            }
```

## 3. `app-gpui/src/workbench/editor_intelligence.rs`

### 3a. Watch the applied workspace edits once an editor opens

In `install_intelligence`, after `let intelligence = EditorIntelligence::install(editor, cx);` add:

```rust
        self.watch_workspace_edits(cx);
```

### 3b. A control chosen in a quick fix's → menu runs

This is the fix that `docs/gpui-port/notes/integration/proofs_editor.md` (test 7) asks for, routed through the new module. In `show_intentions`, replace

```rust
                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
                    workbench.close_popup(window, cx);
                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                }
```

with

```rust
                ListPopupEvent::Control { control, .. } => {
                    workbench.close_popup(window, cx);
                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                    workbench.run_intention_control(control, window, cx);
                }
                ListPopupEvent::Dismissed | ListPopupEvent::Pin => {
                    workbench.close_popup(window, cx);
                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                }
```

With 3b and 4 applied, the drafted test `editor::intelligence::tests::proofs_editor_lsp::a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls` (expected to fail by design) passes at `settle(cx, |cx| inspections(cx) != before)`. So does this module's own copy of it, `opens_a_rows_menu_with_the_right_arrow_and_runs_its_items`.

## 4. `app-gpui/src/workbench/code_insight.rs`

Replace

```rust
    fn apply_inspection(&mut self, command: InspectionCommand, window: &mut Window, cx: &mut Context<Self>) {
```

with

```rust
    pub(super) fn apply_inspection(&mut self, command: InspectionCommand, window: &mut Window, cx: &mut Context<Self>) {
```

## 5. `app-gpui/src/editor/intelligence.rs` (hints with a place, a tone and a timeout)

After the method `fn show_hint_text(&mut self, text: &str, cx: &mut Context<Self>) { … }` add:

```rust
    /// Shows `text` (English source text) at `pos`, the caret when `None`, with `tone`, for
    /// `timeout_ms` (the hint's usual time when `None`).
    pub fn show_hint_at(
        &mut self,
        text: &str,
        pos: Option<u32>,
        tone: Tone,
        timeout_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let caret = self.editor.upgrade().map_or(0, |editor| editor.read(cx).caret());
        let timer = self.hints.show(text, pos.unwrap_or(caret), tone, timeout_ms);
        self.hint_timer(timer, cx);
    }
```

`Tone` is already imported there.

## 6. `app-gpui/src/editor/marks.rs` (the highlighted usages' layer)

In `pub enum MarkLayer`, after

```rust
    /// The line where the debugger is suspended.
    Execution,
```

add:

```rust
    /// The usages Highlight Usages in File marks, until Escape.
    Usages,
```

No paint arm is needed. The usages are `MarkKind::FindOccurrence` marks, which the painter already draws in every layer (`editor.marks.values().flatten()`).

## 7. `app-gpui/src/editor/actions.rs` (one Undo takes a refactoring back)

Replace

```rust
    registry.handle("edit.undo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
```

with

```rust
    registry.handle("edit.undo", |_, window, cx| {
        // With nothing of its own left to undo, the editor undoes the refactoring of its file.
        if workbench::undo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        if workbench::redo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
```

Why this is needed: a workspace edit reaches an open editor as `DocPatched { workspace_edit: true }`, and `Editor::remote_patch` then clears the editor's history. So ⌘Z after a rename did nothing. In the webview, one Undo in any affected editor reverts the whole edit through `undo_workspace_edit` (`src/editor/core/model/workspaceHistory.ts`). The native equivalent:

- an editor with history of its own undoes that first;
- after that, it undoes the newest refactoring that touched its file;
- redo works the same way in reverse.

## Menus

Nothing to add. `menu_spec.rs` already lists Reformat Code, Optimize Imports, Generate, Implement / Override Methods, Rename, Quick (Type) Definition, Test, Next / Previous Method, Next / Previous Error and Error Description. The context menus already offer `code.reformatFile`, `code.optimizeImportsFile`, `code.generate`, `refactor.rename` and `navigate.gotoTest`. In the webview, Highlight Usages, Apply Quick Fix and the highlighted-usage moves exist only in the keymaps.

## IPC commands each action stands for

| Action                                                                                                               | IPC commands                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `refactor.rename`                                                                                                    | `lsp_prepare_rename`, `lsp_rename`, `cancel_request`, `apply_workspace_edit`; then `undo_workspace_edit` / `redo_workspace_edit` (Edit › Undo / Redo, notification) |
| `code.reformat`, `code.reformatFile`, `code.reformatWith{Prettier,Oxfmt,Ruff}`                                       | `lsp_format`                                                                                                                                                        |
| `code.optimizeImports`, `code.optimizeImportsFile`, `code.generate`, `code.implementMethods`, `code.overrideMethods` | `lsp_code_actions`, `lsp_apply_code_action`                                                                                                                         |
| `code.cleanupFile`                                                                                                   | `lsp_code_actions` (twice), `lsp_apply_code_action`, `lsp_format`                                                                                                   |
| `code.applyQuickFix`                                                                                                 | `lsp_code_actions` (`only: ["quickfix"]`), `lsp_apply_code_action`                                                                                                  |
| `find.highlightUsages`                                                                                               | `lsp_document_highlights`                                                                                                                                           |
| `code.quickDefinition`, `code.quickTypeDefinition`                                                                   | `lsp_goto` (definition / typeDefinition)                                                                                                                            |
| `navigate.gotoTest`                                                                                                  | `project_roots`, `find_files`                                                                                                                                       |
| `navigate.nextMethod`, `navigate.previousMethod`                                                                     | `doc_outline`                                                                                                                                                       |
| `navigate.nextError`, `navigate.prevError`, `code.showErrorDescription`, `navigate.next/prevHighlightedUsage`        | none: the problems and marks the editor already shows                                                                                                               |

## Not wired, and why

- **In-place rename.** `Editor` has no edit path outside its undo history. Typing in the name records, and so would the session's mirrored edits, so Escape would leave undo entries behind. Rename therefore always asks in the dialog, as the webview does for an empty range and for ⇧F6 during an in-place session. Two `Editor` additions are needed:
  - `apply_edit_unrecorded(changes, selections, main, cx)`;
  - a "history suspended" switch while a session runs.

  `editor_lsp::inplace_rename::InplaceRename` is ready for them.

- **The syntax-tree local rename** (no server, or broken code) is not built: `planLocalRename`, `renameRisks` and `commitSyntaxRename`. Without a server, Rename shows the server error, or renames the file (`project.rename`) when no server covers it.
- **`code.reformatDialog`.** It needs a dialog with a scope choice (Selected text / Whole file) and Optimize imports. `PromptDialog` has no radio group. It is not registered, and the "Formatted N lines" hint has no "Show the Reformat dialog" link.
- **Reformat:**
  - the Code Style "Tabs and Indents" override per language is not sent (`format_options(editor, None)`);
  - contributed formatters (SQL) are not consulted;
  - generated code is not re-indented after Generate or a quick fix (`reformatGenerated`, `indentUnderBrace`, `formatLines`).
- **Generate / Implement:** the syntax-based Implement fallback (`languageSemantics.implementMethods`) is not built.
- **Interactive commands** (code actions whose command asks for input, `lsp_resolve_command` forms) are not built. Actions are applied through `lsp_apply_code_action` as the intentions list already does.
- **Contributed local quick fixes** (spelling and others, `contributedQuickFixes`) are not consulted by Apply Quick Fix.
- **Highlight Usages:**
  - the status text is kept (`Workbench::usages_status`), but no status bar item draws it;
  - read and write usages share the find-occurrence colour;
  - the automatic highlight of the usages at the caret (Code Editing › Usages of element at caret) is not built;
  - the syntax fallback without a server is not built;
  - `navigate.nextOccurrence` / `prevOccurrence` (the fallback of the moves) are not registered natively.
- **Quick Definition:**
  - the lines are a window around the name, not the declaration's extent (`structuralRanges` of a document that may not be open);
  - the code is not syntax-highlighted;
  - the PHP construct documentation is not shown.
- **Go to Test:**
  - Create New Test… runs `templates.newFromTemplate`, which is not registered natively;
  - the Rust test module insertion is built but untested.
- **Not registered:**
  - `editor.showHint`: another draft, `workbench/editor_core.rs`, already registers it. Registering it here too would silently replace that handler. To show it at the caret as the webview does, that handler can call `EditorIntelligence::show_hint_at` (section 5) instead of a notification.
  - `navigate.superMethod`: `SuperMethodSearch` asks for the outline and the supertypes of files that may not be open. Each needs a backend document: `ops::doc::doc_open` and `doc_close` around `doc_outline` and `lsp_type_hierarchy` (incoming), as the webview's `withDoc` does. Its several results also need the location chooser (`locationItems`). Not built.
  - `refactor.thisMenu`, `refactor.extractVariable`, `refactor.extractMethod`, `refactor.extractConstant`, `refactor.extractField`, `refactor.extractParameter`, `refactor.inline`, `refactor.move`, `refactor.changeSignature`: `runRefactoring` (expression chooser first, `lsp_code_action_query` grouping, `lsp_execute_code_action`, the Introduce fallback) is not built. That covers the expression chooser (`occurrences`), the grouping of the query's actions by refactoring (`refactor_this`) and the Introduce in-place session, which needs the same unrecorded `Editor` edits as in-place rename.
  - `refactor.pullMembersUp`, `refactor.pushMembersDown`: `lsp_hierarchy_members` / `lsp_hierarchy_refactor` and the hierarchy dialog are not built.
  - `refactor.safeDelete`: `editor_refactor::safe_delete` needs the usage search (`lsp_references` streamed), its conflicts dialog and the delete through `apply_workspace_edit`. None of these views exist. Not built.
  - `code.expressionTypeInfo`: the webview shows the type as a documentation popup at the caret. The native popup (`HoverView`) belongs to `EditorIntelligence`, which has no way to show given markup. It needs `EditorIntelligence::show_documentation(markup, pos)`; after that, `lsp_hover` and `type_info::extract_type_info` are enough.
  - The six `editor.inlineCompletion.*` actions: the editor has no ghost-text painting or hook.
  - `code.applyPastedImports`: the editor's paste has no hook for the question.
  - The `~` rows (`code.completion`, `code.parameterInfo`, `code.quickDocumentation`, `navigate.declaration`, `navigate.implementation`) stay with `editor/intelligence.rs`. `code.showIntentions` gains its inspection controls through section 3b.

## Assumptions to check first

1. `BackendEvent.window` of `WorkspaceEditApplied` is the project's window label or `None` (both are accepted).
2. `lsp_prepare_rename` answers `Ok(None)` when the server's `textDocument/prepareRename` result is `null` (not the word range).
3. The test keymap in `input::init` binds `Meta+N` to `project.newFile` and `code.generate`, and `F2` to `navigate.nextError`. `simulate_keystrokes("cmd-n")` reaches the dispatcher with `keyWhen` (`is_action_key_enabled` in `platform/input.rs`).
4. `ide.mode = "light"` keeps every language server off, so the Python and Markdown tests start no real server.
5. A formatter path set in `languages.<tool>.path` is run directly when it is not a `.js` file (`resolve_formatter_command`'s `custom` branch). The fake formatter reads stdin and prints the formatted text.
6. `ide_lsp`'s `document_symbols` turns a hierarchical `DocumentSymbol` of kind 12 into an `OutlineItem` of `SymbolKind::Function`.

## Compile risks

- `on_language_editor` hands the background work a clone of the task's `AsyncWindowContext` (it is `Clone` in gpui-pre-0.3.12), so the work's future owns its context.
- `#[expect(clippy::too_many_arguments)]` on `choose_action` assumes clippy counts `self` (8 inputs).
- `basename(found) == name` compares `Cow<str>` with `String`.

## Tests most likely to fail on first run

1. **`rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each`.** It depends on three things:
   - assumption 1 (the event's window label);
   - the event's `paths` being file paths that equal `to_ipc(editor.path())` (`AppliedEdit::affects`);
   - assumption 2 at the last step (`prepareRename: null`).
2. **`cmd_n_in_a_markdown_file_opens_the_new_popup`** and the `f2` step of **`next_error_moves_the_caret_to_the_next_problem_and_shows_its_message`.** Both depend on the test keymap (assumption 3).
3. **`reformat_with_a_named_formatter_runs_that_formatter`.** It depends on the `languages.<tool>.path` keys, on Light mode, and on `ruff_configuration` accepting an empty configuration directory.
4. **`moves_by_line`.** It depends on assumption 6.
5. **`optimize_imports_and_code_cleanup_apply_the_servers_source_actions`.** The fake server answers every `codeAction` request with the same list, so Code Cleanup's second request returns the actions computed for the first text. The test keeps the two edits on separate lines so both still apply. The test also assumes `lsp_code_actions` sends `context.only` and the whole-file range end `(2, 0)`.
6. **Existing tests after hunk 3a.** Every code action or rename the backend applies as a workspace edit now raises "Refactoring applied" (the webview does the same). A test that counts notifications after a code action would see one more. A test that runs `edit.undo` after a code action now reverts the action, where before it did nothing. By grep, no current test does either.
