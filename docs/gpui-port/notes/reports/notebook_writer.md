The native notebook editor is drafted but has never been compiled or run, so none of the tests below has passed yet. The cells are edited in the app's multi-line text field (`ui::TextArea`), not its code editor: they have no syntax colours and no line numbers. `Editor` only works on a text document the backend opened from a file, and the only kind opened without a file (`open_virtual`) is read-only. In-memory editable documents would fix this; that lives in the backend, so I only describe it in the notes.

I wrote only new files and edited no existing file. Every change to existing files is a hunk in the notes, which also hold sections (a) to (f): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/notebook_integration.md` (331 lines).

## Files written (under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`)

| File                                                                                                    | Lines | Tests |
| ------------------------------------------------------------------------------------------------------- | ----- | ----- |
| `notebook/mod.rs`                                                                                       | 62    | 1     |
| `notebook/model.rs` (shared per-file model wrapping the existing session)                               | 295   | 0     |
| `notebook/view.rs` (toolbar, error bar, keys, cell editors)                                             | 700   | 0     |
| `notebook/cells.rs` (cells, Markdown preview, outputs)                                                  | 775   | 1     |
| `notebook/output_items.rs`                                                                              | 323   | 5     |
| `notebook/ansi.rs`                                                                                      | 201   | 3     |
| `notebook/markdown_cell.rs`                                                                             | 249   | 1     |
| `notebook/tests/proofs24_notebook_units.rs`                                                             | 257   | 11    |
| `notebook/tests/proofs24_notebook_view.rs`                                                              | 314   | 10    |
| `workbench/notebook.rs` (tab kind, all 11 backend calls, kernel events, close question, Save All, Undo) | 533   | 1     |
| `workbench/tests/proofs24_e2e_notebook.rs`                                                              | 334   | 9     |
| `workbench/tests/proofs24_e2e_notebook_pending.rs` (not declared)                                       | 47    | 1     |

There are 7 hunks: the `mod` lines in `lib.rs`, `workbench/mod.rs` and its `mod tests`; the tab kind and the `.ipynb` routing in `tab_views.rs`; Save All in `workbench/mod.rs`; Edit › Undo/Redo in `edit_actions.rs`. No action or menu entry is registered, because the webview registers none.

## Rows I expect

| Key                                                                                                | State                   | Missing                                                                                                                                                                                                           |
| -------------------------------------------------------------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerEditorKind:notebook`                                                                | partial                 | session restore not tested; no tab draws a dirty mark anywhere in the app; Save All and the Edit menu path have no test                                                                                           |
| `point.registerFileEditorProvider:notebook`                                                        | done                    |                                                                                                                                                                                                                   |
| `feature:notebook`                                                                                 | partial                 | see "Left out"                                                                                                                                                                                                    |
| `ipc:notebook_open`, `_get`, `_edit`, `_save`, `_undo`, `_redo`, `_reload`, `_close`, `_interrupt` | done                    |                                                                                                                                                                                                                   |
| `ipc:notebook_execute`                                                                             | partial                 | the outputs of a successful run                                                                                                                                                                                   |
| `ipc:notebook_restart`                                                                             | partial                 | restarting a live kernel                                                                                                                                                                                          |
| `event:NotebookKernel`                                                                             | done                    |                                                                                                                                                                                                                   |
| `unit:…/OutputView.test.tsx`                                                                       | partial, 9 of 9 by name | two cases check a neighbour of the original: the link test checks the link reaches the window (the actual browser open isn't observed), and the SVG test checks a PNG is not decoded again, since SVG isn't drawn |
| `unit:…/security.test.ts`                                                                          | partial, 4 of 4 by name | the SVG case checks that SVG is never drawn                                                                                                                                                                       |
| `unit:…/outputs.test.ts` (8) and `session.test.ts` (16)                                            | done                    | the existing tests; the app now reaches that code                                                                                                                                                                 |

Every test name, prefix included, is in section (b) of the notes.

## Kernel tests and safety

The kernel talks ZeroMQ with signed messages, so a `/bin/sh` script cannot stand in for it, and no test can make a run succeed. The tests instead use a script in the temp folder as the interpreter, answered through the existing `SdkDetection` test global, so the machine's Pythons are never searched. The script writes its pid to a marker file, then either prints `No module named ipykernel_launcher` and exits 1, or runs `exec sleep 60`.

That proves:

- **Restricted mode:** the project refuses Run and Restart with its reason, and the script never starts.
- **Trusted project:** the script starts, the kernel's death is shown with the ipykernel install hint and the state "dead", and Restart clears it.
- **Interrupt:** while the kernel is still starting, Interrupt stops the batch and the process is gone.

The smallest backend change that would allow a real successful run is described in the notes, not written: a `KernelLauncher` trait in `crates/ide-notebook` with today's launcher as the default, plus a matching constructor in `ide-host`.

Untrusted output:

- PNG and JPEG are decoded only from the bytes in the file, with no size limit, because the webview has none.
- SVG is not drawn; its plain text shows instead.
- HTML shows as plain text with every tag, attribute and URL removed.
- JavaScript is shown as inert source under the webview's notice.
- Markdown cells keep only `attachment:` images and `data:` PNG/JPEG images; any other image shows its alt text.
- Links are opened by the window: file links in the editor, web links in the browser.
- Nothing named in a notebook is fetched.

## Left out, and why

- Syntax colours, line numbers and bracket matching in cells: explained at the top.
- A virtualised cell list and a remembered scroll position: all cells are drawn in one scrolling list.
- A spinner: there is no spinner component, so a static icon shows while busy.
- The file-path detail in the interpreter menu: menu rows have one label.
- Toggling `<details>` in Markdown previews: they are always open.
- Accessibility labels: there is no accessibility tree.
- Notebook autosave.
- The webview's limit of 256 output events per frame with recovery: the existing session has no public `recover()`.
- The "notebook changed while opening" error.
- Notebooks in LightEdit windows.

## What turned out wrong

- **Existing native models (`features_a/notebook/`):**
  - The session has no list of detected interpreters; `Answer::Python` drops it. I worked around it in the new model.
  - `select_python` doesn't remember the choice, unlike the webview. The new model stores it in `UiStorage`.
  - There is no public way to call the output reducer's `recover()`.
  - Undo or redo before the notebook has loaded fails instead of loading first, unlike the webview. Not worked around.
  - The selected cell, scroll position and Markdown-editing state are missing. The first and last are now in the new model.
  - `view.rs` says SVG goes "to sanitize", but no sanitiser exists.
- **The brief:**
  - It says to build a cell on the app's `Editor`, which cannot work without a backend document.
  - It suggests a `/bin/sh` kernel stand-in that speaks a line protocol, which cannot work with ZeroMQ.
  - It names `gpui-pre`'s sources; the crate actually imports `gpui-kit 0.9.1`, which sits on `gpui-pre-0.3.12`.
- **My own draft:** I first called `try_next` on the output channel, which is deprecated in futures-channel 0.3.34 and would fail clippy. It now uses `try_recv`.

## Hypotheses (unverified)

- Hypothesis: the `gpui_kit` API calls compile as written. I read them in `gpui-pre-0.3.12`, not in `gpui-kit` itself (section (d), items 1–7).
- Hypothesis: the Markdown renderer doesn't resolve reference-style images, so one input of the entity-encoded attachment test fails.
- Hypothesis: a test window redraws during `run_until_parked`, which the image-decode count test relies on.
- Hypothesis: Open File and Search Everywhere open files through `open_project_file`, so the new routing covers them.
- Hypothesis: Tab inserts a tab character in a cell, and input-method composition is not blocked by the key claims.
- Hypothesis: a source with `\r\n` line breaks is rewritten on every change and the caret jumps to the end.
- Hypothesis: GPUI's SVG decoder follows external references; this is why SVG is not drawn.
- Hypothesis: the kernel's binding of five 127.0.0.1 ports in two end-to-end tests doesn't count as "the network". You decide; if it does, those two tests move to the pending file.

The tests most likely to fail first, and what the real window must be checked for, are in sections (e) and (f) of the notes.
