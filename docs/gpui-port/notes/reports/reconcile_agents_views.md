The reconciled notes are written and all 42 hunks check: the anchors script found each anchor exactly once in the current tree, re-run after the last `mod.rs` count. Nothing was compiled or run, so every runtime claim below is still a hypothesis.

**Hunks (42).** 35 come from the old notes and 7 are new. Since the draft commit `3a0f5b40` only 18 commits landed, none touching `app-gpui/src/agents`, so most anchors still hold.

- **Kept as they were:** 29.
- **Re-anchored (3):** the composer's chips block (H15) is one multi-line anchor, because `let muted = …` occurs 3 times. The `NewTaskView` import goes after `model`, where rustfmt sorts it. The old page's deletion anchors on the separator plus title, because `// The New Task page` occurs 4 times.
- **Rewritten (3):** the deletion of the composer's old picker methods is split in two, so the closing brace of `impl ComposerView` survives. The page-open spawn now logs a closed window instead of discarding the result.
- **New (7), which the old notes missed:**
  - Removing the parent's `MenuItem` import (H20).
  - Deleting `fn tx` in `workbench/agents.rs`, which becomes dead code and would fail `-D warnings` (H22).
  - Three hunks in a `workbench/mod.rs` test that still uses `super::agents::NewTaskEvent` and the old unit `Start` variant (H37–H39).
  - Two appends for the moved tests (H41, H42).
- **Dropped (1):** `mod proofs13_agents_config_pending;`. No old hunk had already been applied by another slice.

**Draft edits.**

- `config_pickers_view.rs`: the picker and Agent settings popups get the `shadow-popup` shadow.
- `new_task_view.rs`:
  - The command and member popups get the same shadow.
  - The agent menu now opens under the chooser through `popup_anchor::button_menu`, with the chooser measured by a `canvas` as `services/view.rs` does.
- `agents_views.rs`:
  - More agents… now runs `agents.addAgent` after the workbench's update has finished (`window.defer`).
  - Agents that are not installed can be chosen again, as in the webview.
  - No leading separator is drawn.
  - `MenuItem` is imported directly.
- `proofs14_agents_views.rs`: no longer requires Fake Agent to be the first row, since a built-in agent on the machine's PATH comes first. It checks Fake Agent comes before "Not installed".
- `new_task_view/tests.rs`: section comments no longer name the spec files.
- All six draft files parse and are already formatted under the repo's rustfmt settings. So are the nine files as the hunks leave them.

**Pending tests.**

- Moved: the two Model & effort tests go to `proofs13_agents_config.rs` (H41), whose helpers are identical; I deleted `proofs13_agents_config_pending.rs`. The question-card hover test goes to `proofs13_agents_session.rs` (H42) and needs the `cards.rs` and `session_view.rs` hunks.
- Left: the drag-prepend test still needs the conversation to hold back earlier rows during a scrollbar drag. `proofs13_agents_pending.rs` needs the image dialog and the docked changes panel.

**Expected `parity.json` rows.**

| Key                               | Now            | Expected                                                                             |
| --------------------------------- | -------------- | ------------------------------------------------------------------------------------ |
| `ConfigPickers.test.tsx`          | partial        | done, 13/13                                                                          |
| `HomeEditor.test.tsx`             | partial        | done, 4/4                                                                            |
| `composerCommands.test.tsx`       | partial        | partial, 12/12 (the isolation case doesn't read branches; that selector isn't built) |
| `sessionState.test.tsx`           | partial, 29/31 | partial, 30/31                                                                       |
| `point.registerToolWindow:agents` | partial        | partial, plus the three proofs14 tests                                               |
| `action:agents.addAgent`          | done           | done, with a second proof through the menu                                           |

No new action is registered. `settings:tools.agents` and `agentRegistry.test.tsx` are not earned.

**Top five risks.**

1. `starts_the_next_task_with_the_model_picked_on_the_new_task_page` assumes `agent_create` sends the chosen model with `session/set_config_option`; only that test settles it.
2. `completes_the_commands_the_agent_offered_in_the_project` relies on the backend remembering a session's offered commands for the project. I read that in `ide-agent` but it isn't measured.
3. Compile points to check first with `cargo check --tests` in `app-gpui/`: the borrow inside `refresh`, passing `&mut self` where `chip_elements` takes `&V`, and the `#[path]` module.
4. The New Task popups open downwards and can fall below a short tool window, so `click_on` would miss.
5. The `cmd-v` paste tests fail on a runner that isn't macOS, where the key is Ctrl.

Also noted: when `refresh` closes the Model & effort popup, the keyboard stays on the undrawn search field. And `spawn_guard` logs an error whenever the page opens with the default `claude` agent in a test that doesn't install the fake agent; that's noise only.

**Anchors script output** (`python3 -I anchors.py <worktree>`): H01–H42 all `ok`, ending in `42 hunks, 0 failed`. It also takes `--out DIR` to preview the result and `--apply` to write it, and exits 1 on any failure.

`git status` also shows `proofs12_diff_soft_wraps_pending.rs` modified; that isn't mine.

Files are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/`:

- `drafts/agents_views_reconciled.md`
- `agv/anchors.py`
- `agv/gen_notes.py` (regenerates the notes)
