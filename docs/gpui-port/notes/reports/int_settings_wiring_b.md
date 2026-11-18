# Report: trust step when attaching a folder (Part 0) and the Editor Tabs settings (Part 1)

Every gate is green. The source files have not changed since the build and clippy ran: their hashes still match the ones taken before the mutation runs.

## Gates (measured)

| Gate                                                                  | Result                                                                                                                                                         |
| --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whole suite, run 1 (`cargo test --lib`)                               | `test result: ok. 8354 passed; 0 failed; 0 ignored`, 225.84 s. `uptime` load averages before 3.52 3.57 4.17, after 30.68 12.24 7.44. Log: `$S/dswb/suite1.log` |
| Whole suite, run 2                                                    | `test result: ok. 8354 passed; 0 failed; 0 ignored`, 225.58 s. Load before 26.51 11.95 7.40, after 70.31 35.89 18.12. Log: `$S/dswb/suite2.log`                |
| Twenty runs of the added tests (filter `proofs44_`, 10 tests per run) | 20 of 20 runs passed all 10 tests (each test 20/20); no failure in any run. Log: `$S/dswb/repeat.log`                                                          |
| `cargo test --test web_view`                                          | `ok. 1 passed`. Log: `$S/dswb/web_view.log`                                                                                                                    |
| `cargo clippy --all-targets -- -D warnings`                           | Exit 0, no warning or error. Log: `$S/dswb/clippy.log`                                                                                                         |
| `cargo build`                                                         | Exit 0, 0 warnings. Log: `$S/dswb/build.log`                                                                                                                   |

- The eight tests the brief flags as flaky under load did not fail in either suite run.
- Free disk space was written to `$S/dswb/log.txt` before every heavy build; it was 332 GB or more each time.

## Part 0: attaching a folder answered its trust step without asking

### Before the fix (measured, log `$S/dswb/p0_before.log`)

A temporary measuring test, now removed, recorded what a user can reach:

- `project.attach` through the registry returns `Err("Action unavailable: project.attach")`. The native app registers no handler for it.
- The "Open project in" question shows only `["cancel", "new", "same"]`, because `carry_out_open` strips the `attach` button.

So no user path reaches the attach step today. The bypass is latent.

At the flow seam (the Attach answer fed straight into the open flow), with a trusted project window open and a second folder nobody decided about, holding a run configuration and a `.vscode/tasks.json`:

- **Question:** none was shown; the wait timed out after 30 s.
- **Notice:** "Project not attached" appeared.
- **Trust store:** `trust.status` read `(decided=false, trusted=false)` before and after.
- **Attachment:** nothing was attached.
- **Window:** it stayed unrestricted.

So the old line never trusted or attached an untrusted folder. It refused one that the webview would have asked about.

### What the webview does

`src/features/app/windows.ts:190-209` (`attachProject`) calls `confirmProjectTrust(path, "local")`:

- A folder whose trust is already decided gets no question (`src/features/project-trust/index.tsx:252`).
- Otherwise "Trust and Open Project?" is asked in the current window (`index.tsx:211-226`).
- **Trust Project** stores trusted, optionally with the parent folder (`index.tsx:195-209`), and then the folder is attached.
- **Preview in Restricted Mode** stores it as restricted, then shows the "Project not attached" warning and attaches nothing (`windows.ts:193-199`).
- **Don't Open** throws a cancellation, which `openFailed` (`windows.ts:45-46`) reports as nothing.

### Now

`app-gpui/src/workbench/app_shell.rs`, the `OpenStep::ConfirmTrust` arm, awaits `app_actions::confirm_project_trust`. `Ok(true)` moves the flow to the trust read and then to attaching. `Ok(false)` is a cancellation, so nothing is reported. `Err` is reported as a failure.

I also rewrote the comment on `const ATTACH`, because its old reason ("no trust question to ask") is no longer true. Attach stays unoffered in this pass, for a different reason: `ReadCurrentProject` in `carry_out_open` only moves the flow on and does not read the project again. Hypothesis: the Project view would therefore not show an attached member. To confirm, wire `project.attach` and check the tree's rows after an attach.

A `#[cfg(test)]` entry point, `app_shell::attach_as_answered`, starts the flow at the Attach answer. The tests enter there because no user action reaches the step.

### Tests

All in `app-gpui/src/workbench/tests/proofs44_attach_trust.rs`, on the real backend with a temp home.

| Test                                                                            | What it proves                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `attaching_an_undecided_folder_asks_and_trust_project_attaches_it`              | The question is shown about that folder in this window. Before the answer, nothing is decided, attached or loaded, and no run session exists. Trust Project gives `(true, true)` and attaches the folder; the window keeps its project and is not restricted. |
| `attaching_an_undecided_folder_previewed_in_restricted_mode_attaches_nothing`   | The folder is stored as `(true, false)`, nothing is attached or loaded, "Project not attached" is shown, and the window's own project stays trusted and unrestricted.                                                                                         |
| `attaching_an_undecided_folder_answered_dont_open_decides_and_attaches_nothing` | `(false, false)`, nothing attached, no new notice.                                                                                                                                                                                                            |
| `attaching_a_trusted_folder_attaches_it_without_a_question`                     | No question; the folder is attached.                                                                                                                                                                                                                          |

- **Mutation M1** reverts the arm to the unconditional `Ok`. The first three tests fail and the fourth passes, as expected. The fourth test's guard is the `status.decided` early return in `confirm_project_trust`, which I did not change and did not mutate.
- Every mutation ran through `$S/dswb/mutate.sh` (copy, replace, trap restore, hash compare). Logs are `$S/dswb/M*.log`, and each reports `restored: yes`.

### Other trust steps read (`grep trust_confirmed|TrustAnswer|trust_read`)

- **`app_shell.rs`, `RequestWindow`:** asks via `confirm_project_trust` before a new window opens. This matches `windows.ts:139-141`.
- **`app_shell.rs`, `OpenProject`:** asks via `confirm_project_trust`. This matches the webview.
- **`app_shell.rs`, `ReadTrust`:** reads trust only and answers nothing.
- **`app_actions.rs`:** `open_project_asking`, `ask_before_new_window` and `finish_trust_prompt` all go through the question.
- **`app_models/workspace_dialog.rs:318`/`329`:** a model with no production caller (grep finds no user outside the file). It answers nothing itself.
- **`platform_more/project.rs:102` (`ProjectSession::trust_confirmed`):** no caller outside its own file.

No other step is answered without the user.

## Part 1: Editor › General › Editor Tabs

All keys are done; none is half applied. Before changing anything I ran the tests: all six failed (log `$S/dswb/p1_before.log`). They pass after the change (`$S/dswb/p1_after.log`).

Settings are changed in the tests through File › Settings… on `editor.general.editorTabs`, then Apply and OK. Tests are in `app-gpui/src/workbench/tests/proofs44_editor_tabs.rs`.

### Changes

- **`workbench/mod.rs`:**
  - New `TabLabel` and `tab_labels_of(pane, cx)`: what the strip draws, read from the effective settings at every render, so a change applies at once to open tabs, as the webview's `useEditorPrefs` does.
  - New `tab_rows_of(pane, cx)`.
  - `dirty_tabs` extracted from `run_tab_command`, which now uses it.
  - The strip in `pane_element` is built from the labels and rows, for every pane.
- **`workbench/tab_views.rs`:** new `TabOpening`, read when a tab opens. `open_text_tab` and `open_view_tab` place a new tab with it and then apply the existing `TabsState::enforce_tab_limit` model, keeping the tab just opened.

### Each key

**`tabLimit` and `closingPolicy`**

- **Webview rule:**
  - After every open, `enforcePaneTabLimit` (`src/editor/core/controller.ts:1359`, `1400-1428`; model in `src/editor/core/model/tabs.ts:369-388`) closes tabs beyond the limit, least recently used first.
  - Pinned tabs, the active tab, the opened tab, busy views and dirty custom-document views are spared.
  - "unchanged" closes an unmodified tab first. A modified text tab is not protected: `inputDirty` is `null` for text files.
  - Closed tabs go on the reopen stack (`tabs.ts:277`).
- **First run:** fail. All five tabs stayed (`["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"]`), and with policy "unchanged" `a`, `b`, `c` all stayed.
- **Change:** the limit is applied in both open paths. Dirty views are protected; dirty text tabs only steer the "unchanged" policy.
- **Tests:**
  - `a_tab_limit_of_three_closes_the_least_recently_used_tabs_and_reopen_brings_them_back`: tabs `[c, d, e]`, `e` active, backend documents released down to `[c, d, e]`, Reopen Closed Tab brings back `b`. Mutation M2 (no limit call) makes it fail.
  - `past_the_limit_close_non_modified_files_first_keeps_the_modified_tab`: tabs `[a*, c]`, no question asked, file not written. Mutation M10 (policy forced to LRU) makes it fail.

**`openAtEnd`**

- **Webview rule:** `controller.ts:1343` opens a new tab at the end instead of after the active tab.
- **First run:** fail; the new tab went after the active tab.
- **Change:** `OpenTabOptions.index` is set to the end.
- **Test:** `open_new_tabs_at_the_end_puts_a_new_tab_last_instead_of_after_the_active_one`. Mutation M3 makes it fail.

**`showExtension`, `showDirectoryForNonUnique`, `markModified`**

- **Webview rule:** `src/editor/core/view/EditorArea.tsx:197-215`.
  - The extension is cut at the last dot when that dot is not the first character, for file tabs only.
  - With the directory setting off, no hint is shown.
  - A modified tab gets a `*` prefix on the already-cut name.
- **First run:** fail; titles stayed `["mod.rs — x", "a.txt", "mod.rs — y"]`.
- **Test:** `the_titles_of_open_tabs_follow_the_extension_directory_and_modified_settings`, expecting `["mod", "*a", "mod"]`. Mutations M4, M5 and M6, one per key, each make it fail.

**`showFileIcon`, `showFullPathOnHover`**

- **Webview rule:** `EditorArea.tsx:220`, `234`. No icon when off. The tooltip is the tab's path (or its name) when on.
- **First run:** fail at the default; native drew no tooltip at all.
- **Test:** `tabs_follow_the_file_icon_and_full_path_on_hover_settings`. It reads the label the strip draws, not a real hover. Mutations M7 (icon) and M8 (tooltip) each make it fail.

**`pinnedSeparateRow`**

- **Webview rule:** `EditorArea.tsx:266`, spec `src/editor/core/view/pinnedTabsRow.test.tsx:49-76`. Pinned tabs get their own row only while the pane has both pinned and other tabs.
- **First run:** fail; one row of 3 tabs.
- **Change:** the strip is split into rows. The title-slot buttons go on the last row; I did not check where the webview places them.
- **Test:** `pinned_tabs_take_a_row_of_their_own_when_the_setting_asks_for_it`, with rows `[3]`, then `[1, 2]`, then `[2, 1]`, then `[3]` once every tab is pinned. Mutation M9 makes it fail.

### When changes take effect

- Label, icon, tooltip and row settings apply at once to open tabs.
- The limit, the closing policy and open-at-end apply at the next opening, as in the webview.

### Left out

- The pin icon on a pinned tab (`EditorArea.tsx:209`) is not drawn natively. This is not a key of this batch.
- `editor.tabs.activateOnClose` is written by the page and, by grep, not read by any native code. It is not in this batch.
- Webview tab limit on session restore (`controller.ts:673`) is not applied natively.

## Existing tests changed

None. `TabTitle` and `tab_titles_of` are unchanged; the new data is a separate `TabLabel`.

## Shots

Real binary, run through `$S/insp8/run.sh` with a neutral `INSPECT_BASE` (removed afterwards). Temp project with `tabLimit=3`, `markModified`, `pinnedSeparateRow` and autosave off in its settings file. Opened `a`, `b`, `c`, `x/mod.rs` (pinned), `y/mod.rs` and `d` (then typed into).

- `$S/insp8/shots/dswbtabs-01-tabs-light-1.png` (light) and `$S/insp8/shots/dswbtabs-02-tabs-dark-1.png` (dark) show the same thing:
  - Top row: pinned `mod.rs x`.
  - Second row: `mod.rs y`, `*d.txt` (active), `c.txt`.
  - `a` and `b` were closed by the limit: five files opened, three unpinned kept.

## Rows (`parity.json` not written)

| Row                                                | Status today | What changed or what I found                                                                                                                                                                                                               |
| -------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:project.trust`                             | partial      | Unchanged. The four `proofs44_attach_trust` tests add that Trust Project from the attach question trusts and attaches.                                                                                                                     |
| `ipc:project_trust_status`                         | done         | Its claim that an undecided folder is asked about was false for the attach step until Part 0 (the attach step was unreachable by a user). It now holds at the seam, by `attaching_an_undecided_folder_asks_and_trust_project_attaches_it`. |
| `point.registerOverlay:project.trustDialog`        | partial      | Now also proven for attach, by the three undecided-folder tests. Its "Missing" text about a new window is stale: that is asked in the current window since HEAD.                                                                           |
| `module:src/platform/projectTrust.ts`              | partial      | `confirmProjectTrust` with Don't Open cancelling without a report is proven for attach by `attaching_an_undecided_folder_answered_dont_open_decides_and_attaches_nothing`.                                                                 |
| Attach action                                      | —            | No `action:project.attach` row exists. `unit:src/features/app/__tests__/windows.test.ts` lists Attach as missing; that is still true for users, since attach is not offered.                                                               |
| Settings page row for Editor › General             | —            | Found no `settings:editor.general…` row besides `postfixCompletion`. The six `proofs44_editor_tabs` tests prove the nine keys.                                                                                                             |
| `unit:src/editor/core/cm/prefs.test.ts`            | partial      | The tab-limit clause is now proven through the window by the two tab-limit tests.                                                                                                                                                          |
| `unit:src/editor/core/view/pinnedTabsRow.test.tsx` | partial      | The row split is now proven in the window by `pinned_tabs_take_a_row_of_their_own_when_the_setting_asks_for_it`. Middle-click close is still missing.                                                                                      |

## What turned out false in the brief or the audit

- **The attach action the brief asks to test through does not exist natively.** `project.attach` has no handler, and the Attach answer is stripped from the Open question. The bypass was unreachable, and the old step refused rather than trusted.
- **Brief rule 4 versus the webview.** The brief says "an unsaved tab is never closed by the limit". The webview protects only dirty custom-document views: under LRU it can close a modified text tab (`controller.ts:1405-1407` with `inputDirty` null for text). I followed the webview. Natively, closing such a tab writes it with autosave on, or asks with autosave off, so no edit is lost; no test covers that close.
- **The audit's text match.** It was right that no native code read the nine keys. But the tab-limit model `enforce_tab_limit` (and `editor_model::removal`) existed with no production caller.
- **Missing brief file.** `$S/tests_brief.md` (and `$S/wiring_brief.md`, named in `int_rules.md`) do not exist.

## Files

- **Edited:** `app-gpui/src/workbench/app_shell.rs`, `app-gpui/src/workbench/mod.rs` (also declares the two new test modules), `app-gpui/src/workbench/tab_views.rs`.
- **New:** `app-gpui/src/workbench/tests/proofs44_attach_trust.rs`, `app-gpui/src/workbench/tests/proofs44_editor_tabs.rs`.
- Nothing is committed.
