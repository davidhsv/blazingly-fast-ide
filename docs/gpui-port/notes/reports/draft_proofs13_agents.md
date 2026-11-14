I wrote 34 tests in 9 new test files plus a shared fixtures file. Nothing was compiled or run, and no existing file was edited. If they pass, 1 row becomes done and 4 move forward but stay partial. Detailed notes are at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs13_agents.md`. They list every spec case with its test, the `mod` lines and anchors, both hunks, and the specification of each missing view.

**Files** (1,328 lines)

- In `app-gpui/src/workbench/tests/`:
  - `proofs13_agents_fixtures.rs` (208): helpers only, including a scripted agent that reads images and logs each method, prompt and setting it receives.
  - `proofs13_agents_images.rs` (54): 2 tests.
  - `proofs13_agents_failed_send.rs` (137): 4 tests.
  - `proofs13_agents_stale_editor.rs` (29): 1 test.
  - `proofs13_agents_delete_failure.rs` (79): 1 test.
  - `proofs13_agents_config.rs` (257): 11 tests.
  - `proofs13_agents_new_task.rs` (33): 1 test.
  - `proofs13_agents_pending.rs` (46): 2 tests.
- In `app-gpui/src/agents/session_view/tests/`:
  - `proofs13_agents_session.rs` (361): 10 tests.
  - `proofs13_agents_session_pending.rs` (124): 2 tests.

Of the 34, 27 are expected to pass and 7 are expected to fail because the native app lacks the behaviour.

**Rows, if the tests pass**

- **Done:** `unit:src/features/agents/AgentsView.test.tsx`, 19 of 19. To fail one task's delete I move the project's `.git` away. The worktree removal then fails at once, before its retry loop that sleeps for real.
- **Partial:**
  - `messageEditing.test.tsx`: 17 of 18 (15 without hunk A).
  - `sessionState.test.tsx`: 29 of 31.
  - `ConfigPickers.test.tsx`: 7 of 13.
  - `composerCommands.test.tsx`: 1 of 12.
- **Unchanged:** `HomeEditor.test.tsx` 0 of 4 and `agentRegistry.test.tsx` 0 of 26, because their views are missing.

**Hunks the tests need**

- **A, `app-gpui/src/workbench/agents.rs`:** a test-only global `spawn_guard::FailingPrompts(Option<String>)` that makes a message send fail before it reaches the backend. It is needed because the backend's send returns success as soon as the turn starts, so an agent's error can never fail a send. Without hunk A, drop `failed_send` and `stale_editor`.
- **B, `app-gpui/src/vcs/prompt_dialogs.rs`:** a test-only `PromptDialog::message_text()`, so the delete test can read which task the failure dialog names.

**Item 4: which views exist natively**

- **Config pickers:** these already exist in the conversation's composer (`composer_view.rs:86-589`), so I wrote 11 tests through the docked composer. Still missing: a search field, the compact "Agent settings" popup, the description beside each entry, and the picker staying open after a choice.
- **New Task page:** it exists (`workbench/agents.rs:1368-1530`), but it has no settings chips, no slash-command popup, no image paste and no project selector. The logic for all of these already sits in models (`completion_at` in `composer.rs:93`, `draft_config` in `store.rs:786`); no view uses it.
- **Agents settings page:** no view is built (`custom_controls.rs` has no control for `tools.agents`). The notes describe what its Registry and Built-in tabs need, case by case.

**Expected to fail: native behaviour that differs**

- After an edit's send fails, the inline editor stays open and later draws over the message sent again.
- The composer's pickers close after each choice and the model list cannot be searched.
- A sent image shows as a name tile that does not open, and the question card shows no preview on hover.
- The docked changes panel does not exist.
- Probably: after a pointer drag, prepending earlier rows does not keep the first row in place on screen.

**Likely to fail on first run**

- `restores_after_entries_load_and_clears_positions_on_project_disconnect`: when the view opens before its entries load, the first layout seems to replace the kept offset with "follow the end". The fix is in the notes.
- The scroll-position test that moves the conversation between an editor tab and the tool window relies on closing a window to save the position, and on `gpui_kit::Size` being exported.

**Left out**

- Cases that test only the DOM or WebKitGTK (the paste with empty clipboard data).
- Pure-function cases already proven on the models.
- Each case whose control is missing is listed in the notes with exactly what it would need.
