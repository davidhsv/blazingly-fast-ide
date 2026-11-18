# Modified text tab closed by the tab limit under LRU: no edit is lost, no code changed

I measured both cases through the window on the real backend. With autosave on, the closing tab's edit is written to disk without a question. With autosave off, a question is shown, and Cancel brings the tab back with the edit while the file on disk stays untouched. Neither case drops typed text without the user answering, so per your step 3 I changed no source. The only addition is two tests in `app-gpui/src/workbench/tests/proofs44_editor_tabs.rs`.

## What the webview does (read from the code, not run)

1. `enforcePaneTabLimit` (`src/editor/core/controller.ts:1400-1428`) does not protect a modified text tab, because `inputDirty` is `null` for text files (`src/editor/core/model/customDocuments.ts:50-53`). Under LRU the limit can close it.
2. Closing a text tab asks nothing. `removeGuarded` asks only for custom documents (`controller.ts:1594-1608`) and for untitled buffers (`controller.ts:1611-1640`).
3. Removing the tab disposes its slot (`controller.ts:515-519`, `src/editor/core/model/slot.ts:273-279`). When the document's last slot goes, `DocumentManager.release` (`src/editor/core/model/document.ts:858-890`) runs:
   - With `files.autoSave` on, it saves the document (`save("auto")`).
   - With `files.autoSave` off, it saves nothing, disposes the model and calls `backend.close(doc)`.

**Hypothesis:** with autosave off, the webview silently drops the edit of a modified text tab the limit closes. To confirm, run a vitest that types into a file tab, sets `files.autoSave: false` and a limit of 1, opens a second file, and checks the file's bytes and what reopening shows.

## The setting that governs the write on close

Natively, a tab's close writes it only when `files.autoSave` is on: "Save files when switching to a different application or a built-in terminal", on Settings › System Settings (`Workbench::auto_save`, used by `tabs_changed` in `app-gpui/src/workbench/mod.rs`). Idle saving ("Save files if the IDE is idle for") uses a timer and plays no part in a close.

## First run (before any change)

Both tests passed on their first run (`test result: ok. 2 passed`, log `$S/dswb/lru_first.log`):

| Test                                                                               | Setting                                                                                            | What the user ends with                                                                                                                                                                       |
| ---------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `a_modified_tab_the_limit_closes_is_written_with_autosave_on`                      | Defaults (`files.autoSave` true, closing policy `lru`), tab limit 1 set through Settings and Apply | The limit closes `a.txt` (tabs: `["b.txt"]`). No question is shown. The file on disk becomes `edited a\n`. Reopening `a.txt` shows `edited a\n`.                                              |
| `a_modified_tab_the_limit_closes_with_autosave_off_asks_and_cancel_keeps_its_edit` | `files.autoSave` switched off through System Settings and Apply; tab limit 1                       | Opening `b.txt` closes `a.txt` and asks "Unsaved Changes" (Save, Don't Save, Cancel). Cancel brings the `a.txt` tab back. The file on disk stays `a\n`. Reopening `a.txt` shows `edited a\n`. |

The native app differs from the webview with autosave off: it asks before dropping anything, through the close path every tab close already uses.

## Each test fails with its guard switched off

Same mutation script (`$S/dswb/mutate.sh`): copy, one replacement, restore in a trap, hash compared. Both runs printed `restored: yes`, and the hash of `mod.rs` matches the one taken before.

| Mutation | Guard switched off (in `tabs_changed`)                                    | Result                                                                                                                                                  |
| -------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M11      | Autosave close answers "Don't Save" (`None => Some(0)` becomes `Some(1)`) | `a_modified_tab_the_limit_closes_is_written_with_autosave_on` fails at line 277: the file is never written within the 30 s wait. Log `$S/dswb/M11.log`. |
| M12      | No question with autosave off (`(!auto_save && false)`)                   | `a_modified_tab_the_limit_closes_with_autosave_off_asks_and_cancel_keeps_its_edit` fails at line 307: no question is pending. Log `$S/dswb/M12.log`.    |

## Gates

| Gate                                        | Result                                                                                   |
| ------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Both new tests, twenty runs by filter       | 20 of 20 runs `ok. 2 passed`; no failure. Log `$S/dswb/repeat_lru.log`.                  |
| Whole suite (`cargo test --lib`)            | `test result: ok. 8356 passed; 0 failed; 0 ignored`, 227.81 s. Log `$S/dswb/suite3.log`. |
| `uptime` load averages around the suite     | Before: 4.47 5.53 8.56. After: 19.68 15.64 12.41.                                        |
| `cargo clippy --all-targets -- -D warnings` | Exit 0, no warning or error. Log `$S/dswb/clippy2.log`.                                  |
| `cargo build`                               | Exit 0, 0 warnings. Log `$S/dswb/build2.log`.                                            |

Free disk space was written to `$S/dswb/log.txt` before each heavy command; it was well above 40 GB each time.

## What I corrected in my earlier report

My earlier report gave "written with autosave on, asked with autosave off" for this path without a test. Both are now measured by the two tests above. Nothing is committed.
