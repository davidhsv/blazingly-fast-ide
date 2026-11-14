I wrote 43 tests in 9 new files (1,366 lines). None of them has been compiled or run: in this task I never ran cargo, and I edited no existing file. The two test-only accessors and the `mod` lines are listed in the notes for whoever integrates them.

**Files** (under `app-gpui/src/workbench/`):

| File                                     | Lines | Tests |
| ---------------------------------------- | ----- | ----- |
| `tests/proofs13_close_guard.rs`          | 285   | 11    |
| `tests/proofs13_light_edit_close.rs`     | 105   | 2     |
| `tests/proofs13_ide_mode.rs`             | 59    | 2     |
| `tests/proofs13_shell.rs`                | 121   | 5     |
| `tests/proofs13_windows.rs`              | 290   | 9     |
| `tests/proofs13_tasks.rs`                | 141   | 4     |
| `tests/proofs13_focus.rs`                | 32    | 1     |
| `tests/proofs13_database.rs`             | 162   | 4     |
| `editor_extras/tests/proofs13_editor.rs` | 171   | 5     |

**`mod` lines to add:** eight `mod proofs13_*;` lines in `workbench/mod.rs` inside `mod tests`, after `    mod proofs11_vcs_windows;`. One `mod proofs13_editor;` in `editor_extras.rs` inside its `mod tests`, after `    mod levels_and_choosers;`.

**The two accessors the tests need:**

- **`PromptDialog::body()`** in `vcs/prompt_dialogs.rs`. The close guard tests need it to read the dialog's message text.
- **`TableEditorView::filter_options(kind)`** in `database/table_editor_view.rs`. The WHERE / ORDER BY completion tests need it to read which completions the popup lists.

**Cases proven through the application, per spec** (counting `it.each` as one case, as `parity.json` does):

| Spec            | Through the app                           | Row             |
| --------------- | ----------------------------------------- | --------------- |
| closeGuard      | 18 of 22 (one of them partly)             | partial         |
| lightEditClose  | 2 of 2 written, 1 expected to fail        | done once fixed |
| ideMode         | 3 of 3                                    | done            |
| shell           | 5 of 9 (6 once fixed)                     | partial         |
| windows         | 8 of 48, 2 of them partly (13 once fixed) | partial         |
| WorkspaceDialog | 0 of 9                                    | unchanged       |
| tasks           | 6 of 9                                    | partial         |
| focus           | 1 of 6                                    | partial         |
| ExportDialog    | 1 of 1                                    | done            |
| SqlFilterInput  | 3 of 3                                    | done            |
| distractionFree | 2 of 3                                    | partial         |
| rainbowBrackets | 5 of 6                                    | partial         |

The tasks test for backend task cancelling also proves `ipc:cancel_task`, and the focus test reads `context:editorFocus` off and on.

**Expected to fail, because the app lacks the behaviour:**

- **Closing a single-file window's tab:** Close Tab does nothing in a window that edits one loose file on its own.
- **Keymap Reference:** `help.keymapReference` is offered in the Welcome screen's Help menu but no handler is registered and no view exists.
- **Tabs in a single-file window:** a second loose file gets its own window instead of a tab (two tests).
- **Open Containing Folder as Project:** the status item clicks `lightEdit.openProject`, but no handler is registered (two tests).

**Left out, with the reason in the notes:**

- **WorkspaceDialog:** its actions have no handler and no view exists, so a test would need accessors I would have to invent.
- **No native counterpart:** remote and WSL windows, Attach, and workspace members (the app opens local projects only and does not offer Attach).
- **Launch cases (19):** the only launch the app handles is `main.rs` opening its first command-line argument at start, which a window test cannot reach.
- **Model only for now:** file drops (the workbench handles none), task queues (nothing in the app queues tasks), prepared custom documents at close, and the keymap grouping and filtering.
- **Browser-only cases:** four focus cases depend on the browser's caret, selection or element visibility.
- **Not drawn:** the editor draws no right margin.

**Where the app differs from the webview:**

- **Single-file windows:** each window edits one file and has no tabs.
- **Quit with Don't Save:** this may still autosave the file, because the quit path never marks leaving as decided. If `discards_without_saving` fails, treat it as a real bug, not a test fault.
- **Large exports:** if the table page arrives with the large value cut short, the export test will fail on a real difference.
- **Stale parity text:** the parity text for windows.test.ts is out of date. `app_shell.rs` now runs the shared models, including the leave guard.

The notes have the per-case tables, the exact hunks and the list of tests most likely to fail on first run: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs13_app.md`
