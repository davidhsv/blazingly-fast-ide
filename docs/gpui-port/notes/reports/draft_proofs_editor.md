I wrote 64 tests in 20 new files (2,390 lines). I wrote the 7 code-intelligence tests myself; helper agents I briefed wrote the rest. None of it has been compiled or run, and no existing file was edited. The integrator has to add the `mod` lines, which are in one table at the top of the notes file. About 20 tests are meant to fail until a fix lands, and the notes list each one with its fix.

**Files** (under `app-gpui/src/`):

| Directory                    | Files                                                                            | Tests |
| ---------------------------- | -------------------------------------------------------------------------------- | ----- |
| `editor/intelligence/tests/` | `proofs_editor_lsp.rs` (332 lines, mine), `proofs_editor_navigation_modifier.rs` | 7 + 2 |
| `workbench/tests/`           | `proofs_editor_actions_{find,navigate,tabs,run_console,markdown}.rs`             | 11    |
| `workbench/tests/`           | `proofs_editor_search_{recent,everywhere,find,safe_delete}.rs`                   | 13    |
| `workbench/tests/`           | `proofs_editor_copy_reference.rs`                                                | 2     |
| `editor/view/tests/`         | `proofs_editor_highlight_{support,cmake,shell,interpolation,backend}.rs`         | 19    |
| `editor/view/editing_tests/` | `proofs_editor_{multi_caret,clipboard_history}.rs`                               | 8     |
| `find/popup_view/tests/`     | `proofs_editor_project_masks.rs`                                                 | 1     |

**Server attach.** My tests use their own fixture, `open_with`, which only returns once the fake server's log shows `textDocument/didOpen` for the opened file. Nothing is typed or run before that. The same one-line wait would fix the intermittent completion test. Why that test fails is a guess I have not measured, and how to check it is in the notes.

**Fake server.** The backend checks the server's capabilities before sending a request. The fake server advertises no references, hierarchy or execute-command support. My fixture therefore rewrites its script so capabilities come from the test's answers. A one-line alternative change to the script itself is in the notes.

**Rows, if the tests pass:**

- **Done (20):**
  - `ipc:lsp_execute_command`, `ipc:lsp_call_hierarchy`, `ipc:lsp_type_hierarchy`, `ipc:lsp_references`, `ipc:lsp_servers`, `ipc:lsp_restart`
  - `action:navigate.callHierarchy`, `action:navigate.typeHierarchy`
  - the four `find.popup.*` actions, `find.scrollToResults`, `navigate.gotoClass`, `navigate.prevBookmark`, `navigate.switcherBack`
  - `toggleActions`, `ipc:index_file_extensions`
  - `ipc:doc_close` and `ipc:doc_set_encoding`: done or partial is your call. Natively they call the backend directly instead of the `ide_host::ops` wrappers; the wrappers only pass through. `ipc:languages` is in the same position.
- **Done only after a fix lands:** `editor.closeAllTabs`, the run-console key claim, and three highlighting rows (cmake, shell interpolation, syntaxHighlight).
- **Partial (about 15):**
  - workspaceEdit, Popups and popupStore
  - the search dialogs (8 of 22 cases), codePreview, pathScope, safeDelete
  - copyReference, multiCaret, clipboardHistory, the navigation modifier, markdownLive, `feature:code-insight`
  - 13 code-intelligence modules, by citing the existing tests

**Code changes the tests depend on** (exact hunks in the notes):

- In the intentions popup, choosing an item from a fix's → menu only closes the popup; the webview runs it. The fix makes `apply_inspection` callable and handles that event in `editor_intelligence.rs`.
- Close All Tabs closes every pane and clears the reopen history.
- `project_files.rs` sends the shape `find.showUsageResults` expects.
- `focusedDirectory` gets set.
- There is also a sketch for keeping the IDE keymap out while the run console's find bar has focus.

**Left out:**

- Syntax fallback: the fake server can't send errors.
- The unavailable-server back-off: it can't be observed at the fake server.
- ⌘-click on a config key: the fake server can't serve `.properties`.
- `ipc:doc_open_virtual`.
- Numbered popup rows: nothing in the app uses them.
- The 3 syntax-tree usage rows: not reachable without a failing server.
- Breadcrumbs and colour schemes: not built natively.
- `catalog.ts`, templates and vim: not reached by the app.

**Native behaviour that differs from the webview:**

- Choosing an item from a fix's → menu does nothing.
- Close All Tabs closes only the active pane.
- ⌥-drag does not make a column selection.
- `editor.copyReference` is not registered as its own action.
- Several highlight queries colour differently, and Kotlin is nearly uncoloured.
- The Search Everywhere action rows show no on/off state for toggle actions.

**Most likely to fail first:**

- **By design:** the expected failures above.
- **Server status test:** whether the status item lists the server with root = the project folder.
- **References test:** whether the backend adds extra rows to the server's references.
- **Highlight tests:** timeouts while the grammars compile on first run.

Everything is in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs_editor.md`.
