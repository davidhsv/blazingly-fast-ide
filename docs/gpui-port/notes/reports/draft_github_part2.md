I've drafted three of the four items, but not all of them: the pending test's tab case should pass, and its registration case will still fail. Nothing has been compiled or run. Every claim about runtime behaviour below is a hypothesis until the integrator builds and runs it.

**1. Files written**

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/github/create_view.rs` (1213 lines)
  - `CreateForm` is a plain model of the form. It hands out a `BranchesRequest` with a ticket whenever the direction changes, and drops answers to an older ticket.
  - `CreatePanel` is the view. Constructor `CreatePanel::new(cx)`.
  - Its event enum is `CreateEvent`: Retry, RefreshBranches, LoadBranches, Create, OpenPullRequest, ShowMenu. Menu rows go back to `run_command` (`CreateCommand::{Base, Head, CreateDraft}`).
  - The workbench feeds it through `initialized`, `init_failed`, `template_loaded`, `branches_refreshed`, `set_branches`, `repository_changed`, `commits_loaded`, `existing_loaded`, `set_creation`.
  - Accessors for tests: `form`, `title_field`, `description_area`, `direction_texts`, `commit_rows`, `status_lines`.
  - 8 model tests and 5 view tests.
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/github_part2.rs` (949 lines): the actions, backend calls and 4 end-to-end tests with the fake `gh`.
  - It is declared as a child of `workbench/github.rs` (`#[path = "github_part2.rs"] mod part2;`), so it can use that file's private items without changing their visibility. `mod.rs` is not touched at all.
- The notes, with every hunk anchored on quoted text: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/github_part2_integration.md`. The hunks are:
  - `github/mod.rs`: add `pub mod create_view;`.
  - `branches.rs`: make `host_call` `pub(super)`.
  - `github.rs`:
    - Production code: the mod line, a slot field and its initial value, the actions call, context publishing, a repository-changed hook, a connection-state hook, and the `CreateOpened` / `TabClosed(Create)` arms.
    - Tests: declare `proofs14_github_window_pending`, and make 11 test helpers `pub(super)`.

**2. Actions registered (each run through the registry in an end-to-end test)**

| Action                              | What it does                                                                                                                  | Test (`workbench::github::part2::tests::`)                                                                                                                                                 | IPC commands                                                                                                                                                                                   |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `github.createPullRequest`          | Shows the window and opens the New Pull Request tab, or opens it once the window connects                                     | `create_pull_request_creates_the_current_branchs_pull_request_and_opens_its_tab`, `create_pull_request_before_the_window_connects_opens_the_tab_once_connected`, plus the pending tab test | `vcs_branches`, `vcs_branch_info`, `gh_api` (templates), `vcs_log` with a range, `gh_api` (REST open-pull-request search), `vcs_remote` `PushBranch` when needed, `gh_api` `createPullRequest` |
| `github.pullRequest.toggleViewed`   | Marks the selected files in the `#N` tab viewed, or not viewed if all already are                                             | `toggle_viewed_marks_the_selected_file_viewed_then_not_viewed`                                                                                                                             | `gh_api` `markFileAsViewed` / `unmarkFileAsViewed`; on failure `pullRequestFiles`                                                                                                              |
| `github.pullRequest.openInWorktree` | Adds the worktree `<root>_PR_<n>` on the pull request's head and opens it in a new window (a test records the folder instead) | `open_in_worktree_adds_the_pull_requests_head_as_a_worktree_and_opens_it`                                                                                                                  | `gh_api` `findPullRequest`, `gh_pr_prepare_diff`, `vcs_add_worktree`                                                                                                                           |

The webview call sites for each are quoted in the notes. The `createPullRequest` and `markFileAsViewed` calls are asserted on the fake's argument log. No push runs in the tests, because the head's remote-tracking ref is set to its tip with `git update-ref`.

Not registered:

- **`showTimeline`, `timeline.refresh`, `selectOpenedPullRequest`**: they need the timeline editor tab. Its model exists (`timeline.rs`, `TimelineInput`), but no view or editor tab was built.
- **`submitReview`**: it needs the Submit Review popup.
- **`deleteMergedBranch`**: only the timeline offers it.
- **`showInToolWindow`, `updateBranch`, `reviewMode`, `reviewMode.showDiff`**: they need review mode's branch widget and editor toolbar; nothing publishes their context keys.
- **The five `diff.*` comment actions**: they need review inlays in the diff window and the editor.

**3. Spec files**

- `toolwindow.test.tsx`: the pending "opens the New Pull Request tab once…" case is covered. "registers the window, the timeline editor and the actions" still fails on 4 ids: `github.pullRequest.showTimeline`, `github.pullRequest.timeline.refresh`, `github.selectOpenedPullRequest`, `github.pullRequest.submitReview`. Declaring the pending file means either accepting that failure or marking that one test `#[ignore]`; that is your call.
- `create.test.ts`: 14/14, already covered by `create.rs`.
- `RemoteBranchDialog.test.tsx`: 1/1, covered by the model only.
- `settingsPages.test.tsx`: 0/3, not reached.

**4. Not built, checks first, likely first-run failures**

Not built:

- The tab's changes panel and its diffs (only the commit list is shown).
- Reviewers, assignees and labels.
- The search fields in the base and head pickers (both are plain menus).
- The "Details" popup (the error shows as a second line instead).
- The push failure's own text: the form says only "Push failed", because `run_remote` returns a bool.
- Cleaning the remote branch name as it is typed.
- The per-repository template cache.
- Settings → Version Control → GitHub (item 4).

Hypotheses to check first:

- The `#[path]` attribute resolves to `workbench/github_part2.rs`; if not, use `"../github_part2.rs"`.
- `Entity<CreatePanel>.into()` converts to `AnyView`.
- `ide_vcs` names remote branches `origin/main` and gives a configured local branch `upstream: Some("origin/feature")`.
- Fields that only hold a `Task` do not trip `dead_code`; this matches `GithubSlot::refreshing`.

Tests most likely to fail on first run:

- The create test's `click_on("gh-create-submit")` may fail if the form is clipped in the test window.
- In the toggle test, `github.toggleViewed` is published on the render after the row click, so `run_action` may need a `run_until_parked` first.
- The worktree test's path assertion depends on `prepare_diff` returning the canonical repository root.
