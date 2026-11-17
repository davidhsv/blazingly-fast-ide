Both slices are written as new, untracked files, plus an integration notes file with every hunk for the existing files. Nothing has been compiled or run, so every test result below is a prediction. I made no edits to existing files and ran no git command that changes anything.

## Files written (line counts)

- **Slice A**
  - `app-gpui/src/workbench/tests/proofs39_agent_undo.rs` (170, 4 tests)
  - `app-gpui/src/workbench/tests/proofs39_agent_undo_pending.rs` (79, not declared)
- **M1 (Enter in a completion's snippet)**
  - `app-gpui/src/templates/proofs39_completion_enter.rs` (55)
  - `app-gpui/src/editor/intelligence/tests/proofs39_completion_snippet.rs` (42)
- **M2 (Java `.var`)**
  - `app-gpui/src/templates/postfix_action.rs` (61)
  - `app-gpui/src/workbench/editor_templates/postfix_action.rs` (63)
  - `app-gpui/src/templates/proofs39_java_var.rs` (125)
  - `app-gpui/src/workbench/tests/proofs39_java_var.rs` (116)
  - `app-gpui/src/settings_view/pages/editor_postfix_completion/tests/proofs39_java_var_row.rs` (11)
- **M3 (Zig postfix)**
  - `app-gpui/src/templates/postfix_model/zig.rs` (30)
  - `app-gpui/src/templates/proofs39_zig_postfix.rs` (44)
  - `app-gpui/src/settings_view/pages/editor_postfix_completion/tests/proofs39_zig_tab.rs` (31)
- **M4 (Zig live templates)**
  - `app-gpui/src/templates/live_builtins/zig.rs` (38)
  - `app-gpui/src/templates/proofs39_zig_live.rs` (40)
  - `app-gpui/src/settings_view/pages/editor_live_templates/tests/proofs39_zig_group.rs` (46)
- **M5 (Zig surrounders)**
  - `app-gpui/src/templates/live_surrounders/zig.rs` (23)
  - `app-gpui/src/templates/proofs39_zig_surrounders.rs` (42)
- **M8 (Emmet in Django)**
  - `app-gpui/src/templates/proofs39_django_emmet.rs` (17)
- **Notes:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/t1a_integration.md` (1003). Every hunk is anchored on text from `git show HEAD:`, grouped so each item lands alone with its own `mod` lines.

## Status per slice and item

- **Slice A: complete.**
  - Hunks A1/A2 record an agent's edit for Undo and skip only the notification, as the webview does.
  - Hunk A5 starts the listener when the project opens. Today it starts only with the first editor, so an agent's edit made with no editor open is dropped. The webview starts its history with the window (`src/editor/core/index.ts:42-44` → `document.ts:761-763`).
  - The tests check bytes on disk and in both editors for Undo and Redo, and a refused Undo that keeps every file.
- **M1: complete.** A session flag lets Enter walk a template's fields but leaves Enter to the editor in a completion item's snippet.
- **M2: complete.**
  - Java `.var` now carries Introduce Variable and the description "T name = expr;".
  - Tab removes the key, ends the statement with `;`, selects the expression and runs the action one turn later.
  - Native "declines" is `execute_action` answering false (action not registered, or its `when` does not hold), the same as the webview.
- **M3, M4, M5: complete.** 12 postfix templates and a Zig tab, 19 live templates, and 10 surrounders, copied in the webview's order.
- **M8: complete on the model.** Nothing in the app reaches it yet, because Emmet's Tab expansion is not compiled.

## Parity rows

| Key                                         | Proposed                                                        |
| ------------------------------------------- | --------------------------------------------------------------- |
| `event:WorkspaceEditApplied`                | stays partial                                                   |
| `unit:…/workspaceUndo.test.ts`              | done, 4/4                                                       |
| `unit:…/editor.test.ts`                     | done, 17/17                                                     |
| `unit:…/postfix.test.ts`                    | done, 10/10 (after the hunk to the existing core-keys test)     |
| `unit:…/live.test.ts`                       | done, 18/18 (after the hunk to `ships_the_requested_built_ins`) |
| `unit:…/surrounders.test.ts`                | done, 4/4                                                       |
| `unit:…/emmet.test.ts`                      | partial, 5/8 (3 cases need the Emmet engine's tables)           |
| `settings:editor.general.postfixCompletion` | partial; new proofs added, its missing clause is unchanged      |
| `settings:editor.liveTemplates`             | partial; new proofs added, its missing clause is unchanged      |

`event:WorkspaceEditApplied` stays partial for two reasons, both fixed by wiring the already-written `editor_model/workspace_history.rs` into the editors:

- **Refusal message differs.** When Undo starts in one editor while another editor of the edit has newer typing, the webview says "Undo newer editor changes before undoing this refactoring". Natively the backend refuses with its own message. That case is in the `_pending` file.
- **Earlier typing loses its undo.** A workspace edit clears an open editor's own undo history (`editor/view.rs` `remote_patch`), so typing made before the edit can no longer be undone. The webview keeps it. The text itself is kept on both sides.

## Hypotheses

- Hypothesis: a background tab's editor keeps following its document after the agent's edit.
- Hypothesis: the file watcher leaves the undo record of a closed file alone when another program writes that file.
- Hypothesis: the default mode starts no language server for `.java` files.
- Hypothesis: the completion item `dependency` replaces the typed word `dep`.

## What turned out wrong

- **Parity keys.** The brief and `WORKPLAN.md` write `evt:WorkspaceEditApplied`; `parity.json` has `event:`. `WORKPLAN.md` writes `set:editor.general.postfixCompletion`; the key is `settings:`.
- **Zig live template count.** The reader's note says 20; the list has 19, as the commit message says.
- **Workspace history grep.** Grepping `workspace_edit_applied` does not find the editor model's workspace history. That model, `editor_model/workspace_history.rs`, is declared and tested but used by nothing.
- **"Edit only when `by_agent: false`".** The planner's measurement was right, and the reach was wider than it said: edits were also lost before any editor opened (fixed by A5).
- **Test location.** The M1 end-to-end test lives under `editor/intelligence/tests/`, not `workbench/tests/`. That is the only fixture with a fake language server that answers completions.
- **The live proof text** says "17 of 18, not covered: ships the requested built-ins" while listing that very test.
