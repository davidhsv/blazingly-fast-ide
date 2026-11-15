I wrote both pieces, but nothing has been compiled or run (I have no compiler). Nothing below has been compiled or run; marked items are hypotheses.

## 1. Files written

| File                                                                                                                                                                                     | Lines | Contents                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- | ------------------------------------------------------------------------------------------------- |
| `app-gpui/src/app_views/keymap_reference_dialog.rs`                                                                                                                                      | 412   | the dialog view, 5 tests                                                                          |
| `app-gpui/src/workbench/keymap_reference.rs`                                                                                                                                             | 187   | action registration, handler, 3 end-to-end tests                                                  |
| `app-gpui/src/workbench/light_edit_tabs.rs`                                                                                                                                              | ~525  | LightEdit tab helpers, 8 end-to-end tests                                                         |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/keymap_reference_light_edit_tabs_integration.md` |       | notes: every hunk anchored on quoted text, a table of every `Content::Editor` site, compile risks |

**`KeymapReferenceDialog` view**

- **Constructor:** `new(KeymapReferenceInput { actions, bindings, preset, mac }, window, cx)`.
- **Event:** `KeymapReferenceEvent::Closed`, on Escape from the field or the dialog.
- **Accessors:** `filter_field()`, `groups()`, `categories()`, `count()`, `meta_line(cx)` ("Keymap: <preset> · N shortcuts").
- **Contents:** the shared `TextField` with a Clear button, the empty text, and the groups in two balanced columns.

**Workbench accessor:** `keymap_reference()`.

**Module lines:**

- `pub mod keymap_reference_dialog;` in `app_views/mod.rs`.
- `mod keymap_reference;` and `mod light_edit_tabs;` in `workbench/mod.rs`.
- `keymap_reference::register_actions(registry);` after `app_actions::register_actions(registry);`.
- Declare `proofs13_shell_pending` and `proofs13_windows_pending`.

## 2. Actions

- **`help.keymapReference`** (no `when` condition):
  - **What it does:** opens the dialog in the window's dialog slot (`open_vcs_dialog`, drawn through `dialog_frame`). This works in a project window, a single-file window and the Welcome screen. Closing gives the keyboard back.
  - **Tests:**
    - `keymap_reference::tests::{renders_the_overlay, filters_by_title_id_or_shortcut, opens_in_a_window_that_edits_a_loose_file}`;
    - `proofs13_shell_pending::renders_the_overlay`.
  - **IPC:** none.
- **`file.openFile`, `editor.closeTab`, `editor.closeOtherTabs`** were already registered. They now work on single-file windows.
  - **Tests:** the `light_edit_tabs` tests and the two `proofs13_windows_pending` tests.
  - **IPC:** `window_open_files` (done natively by `route_files`), `light_edit_open`, `fs_probe`, `doc_open`, and `doc_save` for a closed dirty tab.
- **Not registered:** none from this assignment.

## 3. Spec coverage

| Spec                     | Cases covered                                                                                     | Not covered, and why                                            |
| ------------------------ | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `shell.test.tsx`         | 8 of 9                                                                                            | "names the host of a remote project" has no native counterpart  |
| `windows.test.ts`        | cases 8 and 11 (pending tests); case 9 gains the empty-window → existing-single-file-window route | the rest unchanged                                              |
| `lightEditClose.test.ts` | 2 of 2                                                                                            | the existing tests, plus a two-tab version in `light_edit_tabs` |

Both pending files should pass unchanged. I found nothing wrong in them.

## 4. Design, gaps, assumptions, likely failures

**Design.** `Content::Editor` becomes a struct variant with exactly the tab fields of `Content::Project`. About 45 guards become one-line or-patterns. A single-file window therefore reuses the project machinery: the tab strip and editor area, `tabs_changed` (including its save-or-ask for a dirty tab), the tab menu, viewer and Markdown tabs, splits, caret reveal, and Find bars per tab.

- **Removed:** `light_find_bar` everywhere, and `light_editor_element`.
- **First file:** while the first file is still opening, the files after it wait in a new field, `light_pending`, and then open as tabs.
- **Close:** the window closes once its last tab is closed and any save has finished.
- **Session:** stored under the key `"<lightEdit>"`. It is restored into the next single-file window only when no other such window is open. Closing the last tab clears it.
- **Empty state:** an empty pane shows the two hints, Open File and the drop remark.
- **Title bar and status bar:** both follow the active tab.
- **Routing:** a loose file picked in a window that shows nothing goes to the existing single-file window, as the backend's `plan_open` does.

**Webview behaviour not built**

- **Keymap dialog placement:** centred, where the webview places it near the top and closes it when the window is deactivated. It lists a snapshot of the keymap taken when it opens.
- **Image first:** a window whose first file opened in a viewer (an image) still takes no tabs. A second file gets its own window, as before.
- **Failed tab:** a tab that fails to open is only logged; the webview raises an `openFailed` notification.
- **No drop handler:** dropping files on a window is not handled, so the drop hint is text only.
- **Empty pane untested end to end:** only its hint rows are tested. The state is reachable while a closed last tab waits on its Save question.

**Check first**

- **Titles of unregistered actions:** they come from `assets/gen/actions.json` rather than being humanised from the id.
- **Lint expectation:** the `#[expect(clippy::large_enum_variant)]` on `Content` may become unfulfilled. If so, delete it.
- **Unused imports:** some imports will be reported unused after the `editor_find.rs`, `editor_core.rs` and `editor_intelligence.rs` hunks. Drop what the compiler lists.
- **Concurrent edits:** another agent is editing `light_editor_element`. My notes delete the whole function whatever its body is, because the shared pane already opens the editor context menu.

**Most likely to fail on first run**

- **`light_edit_tabs::the_session_restores_its_tabs`:** depends on the tab order `merge_session_tabs` produces (`[a.txt, b.md, c.txt]`). I checked that by reading the code, not by running it.
- **`keymap_reference_dialog::tests::renders_the_overlay`:** Hypothesis: `debug_selector` is recorded on a div without an id. Confirm with `cx.debug_bounds("keymap-group:Navigate")`.
- **Existing single-file-window tests in `mod.rs`** (typing, find bar, status bar): the editor now sits under a tab strip, so a click or focus assumption could break.
- **`closing_the_last_tab_saves_its_changes_before_the_window_closes`:** relies on the `DocDirty` event arriving before Close Tab.
