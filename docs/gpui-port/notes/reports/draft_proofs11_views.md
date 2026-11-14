I wrote 55 new tests in 8 new files, compiled and run by nobody yet. No existing file was edited. If they all pass, 3 rows become done and 6 stay partial.

**Files** (each has a `mod` line to add; the exact line and anchor are in the notes):

- `app-gpui/src/workbench/tests/proofs11_agents.rs` (629 lines, 13 tests): delete, cancel, archive, unarchive and worktree-cleanup batches through the Agents tool window with the fake agent. Also message editing and retracting, with a new `EDITING_AGENT` script that can roll back and logs every method it receives to `agent-calls.log`.
- `app-gpui/src/agents/task_list_view/tests/proofs11_task_list.rs` (356, 13): docked-conversation cases, menu labels for mixed selections, unread marks.
- `app-gpui/src/agents/session_view/tests/proofs11_session.rs` (202, 5)
- `app-gpui/src/agents/markdown_view/tests/proofs11_markdown.rs` (86, 4)
- `app-gpui/src/database/grid_view/tests/proofs11_grid.rs` (27, 1)
- `app-gpui/src/database/results_view/tests/proofs11_results.rs` (234, 9)
- `app-gpui/src/github/list_panel/tests/proofs11_list.rs` (230, 7)
- `app-gpui/src/workbench/github/tests/proofs11_github.rs` (84, 3): rewrites the fake `gh` script in the test's temp folder with one extra case so it can answer a second page. The real `gh` never runs.

**Cases covered on the view, per spec, if the tests pass:**

| Row                          | Covered                                | Status  |
| ---------------------------- | -------------------------------------- | ------- |
| `AgentsView.test.tsx`        | 18 of 19                               | partial |
| `Markdown.test.tsx`          | 7 of 18                                | partial |
| `messageEditing.test.tsx`    | 14 of 18                               | partial |
| `sessionState.test.tsx`      | 19 of 31                               | partial |
| `unread.test.tsx`            | 6 of 18                                | partial |
| `Grid.test.tsx`              | 12 of 12                               | done    |
| `results-view.test.tsx`      | 13 of 14                               | partial |
| `github/list/list.test.tsx`  | 15 of 15                               | done    |
| `action:github.pullRequests` | visible, keyboard, next page, choosers | done    |

**Hunks needed:** only the 8 `mod` lines. From reading the code, every helper, accessor and field the tests use exists.

**Left out, and why:**

- **AgentsView "continues after an error"**: I found no way to make one task's delete fail on purpose without adding a test hook.
- **Markdown images (8 cases) and diagrams (4)**: the view doesn't have this behaviour. It draws an image as a box with its alt text, and the image model `MessageImages` is used by no view. A Mermaid block stays as code and is never drawn. The unique-SVG-id case has no native counterpart.
- **unread**: "the view is not built" is no longer true for the unread dot, which the task list draws. Still missing:
  - The status count and the tool-window badge: `status_item` and `badge` have no caller outside tests, by grep.
  - 8 cases that need conversations as editor tabs (gap 17).
  - The 2 window-focus cases, which I did not write.
- **messageEditing**: 4 cases about images or a failed send, left out for time, not because a behaviour is missing.
- **sessionState**: 12 cases left out (the scroll-geometry and scroll-position ones, the question cards, tag kinds), mostly for time.
- **results-view "records the selected schema"**: that is the console's execution, not the result view.

**Possible differences from the webview** (each becomes an expected failure if true):

- Retracting a turn that has later messages may not ask for confirmation natively.
- `layout.show(…, true)` may not give the Pull Requests tool window the keyboard.
- An edited result that ran in schema `s2` may not ask for the `s2` table details.

**Most likely to fail on first run:** the worktree-cleanup test (it assumes how worktree tasks start and where their folder is), and the stop-timeout test, which is checked against wall-clock time and needs about 15 real seconds. The notes list six more with reasons.

Notes, with per-test rows: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs11_views.md`
