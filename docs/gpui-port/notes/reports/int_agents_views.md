All three parts are finished. Every gate is green, nothing is committed, and the two `docs/` files and other agents' drafts were not touched.

## 1. Gates (final tree)

| Gate                                                | Result                                                             |
| --------------------------------------------------- | ------------------------------------------------------------------ |
| Suite, run 1 (nothing else running)                 | `test result: ok. 7564 passed; 0 failed` in 181.70s                |
| Suite, run 2 (beside the 20x loop)                  | `test result: ok. 7564 passed; 0 failed` in 224.16s                |
| 20x loop of every test I added, declared or changed | 20 of 20 runs `ok. 84 passed; 0 failed`. No test failed even once. |
| `cargo test --test web_view`                        | `ok. 1 passed`                                                     |
| `cargo clippy --all-targets -- -D warnings`         | rc=0                                                               |
| `cargo build`                                       | rc=0, 0 warnings                                                   |

Logs are in `$S/dagv/`: `suite_final_3.log`, `suite_final_4.log`, `loop20/progress.txt`, `web_view2.log`, `clippy_final2.log`, `build_final2.log`.

The 84 tests in the loop were: the proofs22 files, the 2 new store tests, all of `proofs11_agents`, `proofs13_agents_config`, `new_task_view::tests`, `composer_view::pickers::tests` and `proofs14_agents_views`, the moved question-card test, `a_task_with_the_default_agent_is_refused…` and `ui::popup_menu::tests`.

Not looped 20x: the other `proofs13_agents_*`, `proofs6_agents` and `proofs_agents` files, which use the fake agents I changed. They passed in every full suite run.

Failures seen along the way:

- **Part 0, typed search:** `finds_the_maven_toolchains_setting` and `finds_the_maven_active_profiles_setting` failed when the query was typed key by key. They landed on `languages.go.modules` and `keymap`.
- **Old binary loop:** 2 of 100 runs lost the agent's reply (Part 0b).
- **First fixed loop:** 3 of 200 runs failed at "the three tasks to be deleted" in `deletes_the_right_clicked_selection…`.
- **A load suite:** `proofs11_agents::rolls_back_before_sending_the_edited_text_when_send_is_clicked` failed once with "the edited turn did not happen".
- All of these are explained and fixed below.

## 2. Parts

### Part 0: Settings tests

- **What was declared.** I added one `mod` line each to `settings_view/pages/font_picker.rs`, `settings_view/pages/languages_controls.rs` and `workbench/mod.rs`. The files hold 23 tests; 22 compile on macOS, because `offers_both_main_menu_styles_on_a_frameless_title_bar` is gated to Windows and Linux.
- **First run.** 20 of 22 passed.
- **The two Maven searches.**
  - The dialog does not stay on Maven when the query is typed. Each prefix picks its own first page: "too" no longer matches Maven and jumps to Go Modules, and the full word then matches nothing, so it stays there.
  - The spec types nothing: it uses one `fireEvent.change` with the whole value (`dialog.test.tsx:165,172`).
  - I changed the test to paste the query in one step (`paste_search` with cmd-v). The assertions are unchanged, and both tests pass.
- **The writer's other doubts, settled by running (all passed):**
  - A click on the disabled UI fallback picker does not open it.
  - Escape right after typing does not cancel Settings.
  - Reset drops edits that were staged directly on the model.
  - The CMake round trip of an empty argument and a `C:\…` path works.
- **Rows.** Every test named in `proofs22_settings_rows.json` exists in `cargo test -- --list`.

Cases proven per spec file (my count of `it(`/`it.each` lines in brackets where it differs from the notes):

| Spec file               | Proven          | State   | Missing                                                                                                                 |
| ----------------------- | --------------- | ------- | ----------------------------------------------------------------------------------------------------------------------- |
| `cmakeEditor`           | 2 of 2          | done    | —                                                                                                                       |
| `mavenRunner`           | 1 of 1          | done    | —                                                                                                                       |
| `FontPicker`            | 5 of 5          | done    | —                                                                                                                       |
| `pages`                 | 5 of 5          | done    | —                                                                                                                       |
| `phpXdebug`             | 3 of 4          | partial | Detect again does not re-check Xdebug                                                                                   |
| `languageServerTargets` | 1 of 2          | partial | plugin run targets are not offered                                                                                      |
| `javaFormatter`         | 1 of 2          | partial | the Code Style language pages draw no controls                                                                          |
| `mainMenuStyle`         | 1 of 3 on macOS | partial | the other two run only on Linux/Windows or have no native counterpart                                                   |
| `mouseShortcut`         | 2 of 3          | partial | swipe recording is not built                                                                                            |
| `dialog`                | 29 of 41 [41]   | partial | 10 missing natively (Project Structure, Font preview, Run Targets, `toggleNativeToAscii`), 2 with no native counterpart |

The 13 new dialog cases (2, 4–8, 16, 25, 27–30, 32) are proven by the `proofs22_settings_units`/`proofs22_language_controls` tests named in the rows file.

### Part 0b: three agent tests that never got their reply

**Report on timeout.** When an agent wait runs out, the report (`window_agents_report` in `workbench/mod.rs`) now shows:

- each task as the store holds it and as the backend holds it (status, error, last entries);
- what happened to the reads of each conversation (a test-only trace in the store);
- the fake agent's processes (`ps`, filtered on its script path);
- the last lines the fake agent read and sent. `FAKE_AGENT` and `IMAGE_AGENT` now write `agent-transcript.log` beside their script. `EDITING_AGENT` writes none, so its report shows 0 lines.

Of the provider, only the script path is printed, never its environment (the key is never printed).

**Cause, measured.**

- I looped the three tests 100 times beside two suite runs, on the binary from before the fix. Runs 26 and 65 failed (load average about 26 and 34), both at "the agent at work".
- In both, the report shows the store holding 1 entry while the backend holds 2, the second being `"agent: Hello from the fake agent"`, and the agent process still alive. The window lost a reply the backend has.
- The race:
  - A reply that arrives while the conversation is being read is dropped, because the conversation is not loaded yet (`store.rs`, `on_event` Entry).
  - The read's answer, taken before the reply, then replaces the list (`entries_loaded`).
  - No later event brings the reply back.
- The webview has the same code: `src/features/agents/store.ts:331-345` (`if (!current) return;`) and `:376-379` (`loadEntries` overwrites unconditionally). That it loses replies the same way is a hypothesis, read from source, not run.

**Fix (app, `agents/store.rs` and `workbench/agents.rs`).**

- The store counts reads on their way (`reading_entries`).
- An entry, or a Truncated event, that arrives during a read marks it stale.
- `entries_read` then asks for the conversation again; `entries_unreadable` handles a failed read.

Unit test: `agents::store::tests::reads_the_conversation_again_when_a_reply_arrives_while_it_is_read`.

**After the fix (fixed binary, about 200–300 runs per loop; each loop had 2 full suites running alongside, load up to 104):**

- The reply-loss failure did not occur in 400 runs. It had appeared 2 times in 100.
- Measured: in 15 runs a read was answered with fewer entries than the store held ("answered with 1 entries (held Some(2))"). In each, the re-read brought the reply back. Without it, that reply would have been lost.
- Hypothesis: the original failures ("the reply", "agent: Answer 1") have the same cause. They are the same class of wait, and the original log showed the agent had already written its prompt log. To confirm, run the loop on `$S/dagv/bin/measure` (old) against `measure2`.
- Not improved: between a stale answer and the re-read, the list can briefly step back.

**Also found under load and fixed:**

1. **A deleted running task comes back as Failed.**
   - In 3 of 200 runs, the backend had removed the task ("no agent task") but the store still listed it as Failed, "The agent process exited."
   - Hypothesis: the agent's exit sends a task update after `Removed`. The order was not traced.
   - Fix: the store ignores task updates for ids the backend removed. Test: `agents::store::tests::keeps_a_deleted_task_deleted_when_its_agents_exit_is_reported_after_the_removal`.
   - The webview re-adds such a task the same way (`store.ts` "task" case).
   - A second 200-run loop with 2 suites beside it passed 200 of 200. A 0-of-200 result after a 3-of-200 rate can still be luck (roughly 5%).
2. **`rolls_back_before_sending_the_edited_text_when_send_is_clicked` compared whole conversation lines.**
   - Once a turn's end is recorded, each line carries the time the turn took (`"you: Original message edited | 337ms"`). The report shows the store and backend agreeing.
   - The test now compares the text before the `" | "`. Why the old check usually passed was not measured.

### Part A: setting pickers and the New Task page

- **Applying the hunks.** `anchors.py` reported "42 hunks, 0 failed". I reviewed the whole `--out` diff, then applied it.
- **Build.** One fix to compile: a lifetime on `open` in `new_task_view/tests.rs`.
- **Hypotheses.** All six are settled by their named tests, which passed on the first run, including hypothesis 2 (the model picked on the New Task page reaches `session/set_config_option`) and hypothesis 3 (offered commands become the project's commands).
- **Edits beyond the notes:**
  - **Popups stay inside the window.** `below_inside_window` and `place_popup` in `agents/config_pickers_view.rs` (not `ui::popup_anchor`) wrap downward popups in `anchored().snap_to_window_with_margin(4)`. This covers the model/mode picker, Agent settings, the command list and the member list. Test: `agents::new_task_view::tests::keeps_its_popups_inside_a_short_window`.
    - It fails without the change, measured: the command list ended at y=343 in a 320px window.
    - The model popup fits in that window either way, so the test proves placement for the command list only.
    - The webview flips as well as shifts (`src/ui/Popup.tsx:95`); this only shifts.
  - **Shared menu labels.** In `ui/popup_menu.rs`, a label wider than the menu now ends in an ellipsis; before, it ran past the frame. The webview's `Select` ellipsizes details too (`src/ui/Select.module.css:105-109`).
  - **`mod views;` moved** to the end of `workbench/agents.rs`. Clippy reported `items_after_test_module` with it at the top.

Screenshots (`$S/insp8/shots/`):

- **How they were taken.**
  - My own copy of the launcher, `$S/dagv/run_shots.sh`.
  - `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so no agent binary of the machine is found. Measured: the page showed "claude was not found on PATH", and no agent-related lines appeared in the app logs.
  - The temp home was seeded with a `fake.toml` agent whose command is `/usr/bin/true`, never started, and a `models.json` holding its settings, so that chips exist.
  - No send, no task, nothing started.
- **What they show.**
  - `dagv1-01-newtask-1.png`: the New Task page.
  - `dagv3-01-chooser-1.png`: the agent chooser. The menu is moved left to fit and sits flush at the window's right edge. Two long labels end in an ellipsis; the earlier `dagv2` shot showed them cut.
  - `dagv4-01-model-effort-1.png`: the Model & effort popup, below the box, inside the window, with its shadow, nothing cut. It extends slightly past the tool window's right edge.

## 3. `parity.json` keys

| Key                                                  | State                                 | Proving tests                                                                                                                                                                                                                                                                  |
| ---------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `unit:src/features/agents/ConfigPickers.test.tsx`    | done, 13 of 13                        | the existing proofs plus the 7 `agents::composer_view::pickers::tests::*` and the 2 moved `workbench::tests::proofs13_agents_config::*` named in notes section 4                                                                                                               |
| `unit:src/features/agents/HomeEditor.test.tsx`       | done, 4 of 4                          | the 4 `agents::new_task_view::tests::*` named in the notes                                                                                                                                                                                                                     |
| `unit:src/features/agents/composerCommands.test.tsx` | partial, 12 of 12 with one incomplete | 11 `agents::new_task_view::tests::*`, `agents::store::tests::defaults_to_the_active_member_last_used_member_then_first_member`, `workbench::tests::proofs14_agents_views::completes_the_commands_the_agent_offered_in_the_project`. The isolation case does not read branches. |
| `unit:src/features/agents/sessionState.test.tsx`     | partial                               | adds `agents::session_view::tests::proofs13_agents_session::picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape`. Still missing: the docked changes panel and the drag prepend. The notes say 31 cases; I count 30 `it` lines.             |
| `point.registerToolWindow:agents`                    | partial                               | add `workbench::tests::proofs14_agents_views::*` (3)                                                                                                                                                                                                                           |
| `action:agents.addAgent`                             | done                                  | add `workbench::tests::proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu`                                                                                                                                                                     |
| `feature:agents`                                     | partial                               | unchanged                                                                                                                                                                                                                                                                      |
| Settings unit rows                                   | as in the Part 0 table                | `docs/gpui-port/notes/integration/proofs22_settings_rows.json`                                                                                                                                                                                                                 |

Tests still pending:

- `defers_prepending_during_a_pointer_drag_and_preserves_the_first_rows_viewport_position`: waits for drag-aware prepending.
- `opens_an_image_of_a_sent_message_in_a_dialog`: waits for the image dialog.
- `keeps_docked_changes_opt_in_preserves_the_draft_and_returns_to_the_same_conversation`: waits for the docked changes panel.

These are in `proofs13_agents_session_pending.rs` and `proofs13_agents_pending.rs`, undeclared.

## 4. Stale or false in notes and briefs

- **Part 0 test count.** The brief says 24 Settings tests; the files hold 23.
- **Agent menu.** The notes' remark that an agent found on PATH comes first holds only when one is found. With the restricted PATH, Fake Agent was first.
- **Opening the New Task page can start an agent.**
  - With the default `claude` draft and claude on PATH, `agent_commands` → `fetch_settings` → `native::claude::discover` starts the user's `claude` binary. In tests the spawn guard blocks it; in the real binary only a PATH without it does.
  - Read in code, not run. Before the restricted PATH, `claude` was on this shell's PATH (`command -v` measured), so a launch with the normal PATH would likely have started it.
- **Scope of the Part 0b fix.** It is in the app (`agents/store.rs`, `workbench/agents.rs`), not in `crates/`. The webview has the same window for loss.

Files I changed are the 20 from `git diff --name-only`. They include the draft files handed to me, under `app-gpui/src/agents/{config_pickers_view.rs, new_task_view.rs, new_task_view/tests.rs}`.
