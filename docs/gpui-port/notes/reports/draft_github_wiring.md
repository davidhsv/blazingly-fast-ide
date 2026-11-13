I've written `github.rs` and the integrator's instructions. Nothing was compiled or run (no cargo, rustfmt, git or gh); every "compiles" or "passes" below is a guess.

### 1. Files written; what `github.rs` holds

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/github.rs`
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/github_integration.md`

**The slot.** `GithubSlot` holds the project root, the primary repo, `RepositoryState`, `RepositoryData`, `FlowBusy`, the `PullRequestsWindow` entity, the `#N` tabs and the running tasks. Each `#N` tab keeps its `DetailsPanel` and what loaded for it: details, mergeability, commits, files, viewed states, the chosen commit's files, and a generation number that drops stale answers.

**Methods called from `mod.rs`.** `mount_pull_requests`, `sync_pull_requests_window`, `publish_github_context` (sets `githubConnected` and `githubPrTab`), `drop_github`, `github_vcs_changed`, and the free `register_actions`.

**Accessors for tests.** `pull_requests_window()`, `pull_request_details(n)`.

**Events handled:**

- **Connecting.** The window's `Refresh` lists each repo root's remotes through `vcs_roots` and `vcs_remotes`, because `VcsState` in `branches.rs` has no remotes. Then `remotes_listed` → `ide_github::availability` off the main thread → `availability_answered`. A `Connect` step, or the window's own `Connect`, runs `findRepository` plus REST `GET user` → `connected`. The saved repo choice and the data reset are honoured, and every state is pushed to the window with `set_repository`.
- **Other window events.** `OpenUrl`. `RunAction`: runs the action if it is registered; `terminal.runCommand` is not, so it opens the Terminal and logs. `PullRequestOpened`, `TabClosed`, `ActiveTabChanged`, `ShowMenu` → the window's `run_command`. `CreateOpened` and the timeline flag are logged.
- **The list.** `Fetch` → `ide_github::api` → `search_answered`. `LoadChooser` → paged REST through `RepositoryData`'s loaders (collaborators or contributors by permission, assignees, labels) → `set_chooser_items`. `ShowMenu` → the list's `run_command`. Search history and last search are stored by the window itself (checked).
- **A pull request tab.** Opening it starts five requests at once: `findPullRequest`, mergeability (`build_mergeability_state`), paged commits, REST `pulls/N/files`, and `pullRequestFiles` for viewed states. Results go to `set_details`, `set_inputs` (including `checked_out` and `resolve_error`) and the tab title, and refresh the list row.
  - `CommitSelected` loads REST `commits/{sha}`.
  - `OpenDiff` runs `ide_github::prepare_diff`, builds a `DiffData` with a `github.pullRequest` context, and calls `open_diff`.
  - `Reopen` and `Close` (`PATCH` state), `Post` (`markPullRequestReadyForReview`), `Flow(SetMyselfAsReviewer)` and `AdjustReviewers` (`POST`/`DELETE requested_reviewers`) go through `FlowBusy`, then reload the pull request and the list.
  - Checkout runs `save_all` → `ide_github::checkout` → `branches::refresh` → re-push of the tabs.
  - `Flow(other)`, `Merge`, `RequestReview`, `ResolveConflicts` and `ShowTimeline` are logged with `tracing::info!`.

**Actions.** `github.pullRequests`, `github.createPullRequest`, `github.checkoutPullRequest`, `github.pullRequest.checkoutBranch`, `github.refresh`, `github.changeRepository`, `github.selectOpenedPullRequest`, `github.pullRequest.show`, `github.pullRequest.refresh`, `github.pullRequest.openInBrowser`, `github.pullRequest.copyUrl`. Their `when`/`keyWhen` are copied from `index.ts`. They use `registry.register(ActionMeta…)` because none of these ids are in `actions.json`, and `registry.handle` panics on a missing id.

### 2. The drafts file

Path: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/github_integration.md`

- **§1 `mod.rs`:** exact code anchored on lines that are unique today — the `mod`/`use` lines, the field and its `None` init, the `TOOL_WINDOWS` entry (15 → 16 today; re-count after the other drafts land), the `register_actions` line, the mount arm, the sync call, the context call, and `drop_github()` after both `drop_vcs()` calls.
- **§2 `branches.rs` (optional):** one line in `vcs_state_changed`, so open tabs notice checkouts made outside the window.
- **§3:** the methods and the field `github.rs` expects.
- **§4:** the gh seam, plus a complete `#[cfg(unix)] mod pull_requests` to paste into `mod tests`, with six tests named as sentences: connects and lists two PRs with state, author and labels; a search term filters through a new request; opening a PR shows title, branches, checks and changed files; Checkout checks out the branch and the branch widget follows; gh not signed in shows the sign-in state; no GitHub remote shows that state.
- **§5:** 17 assumptions, each with the file and what to check.

**The seam.** `ide_github::gh_executable()` reads `OPENIDEA_GH` once per process and caches it in a `OnceLock`; there is no per-call or per-project setting. The tests install a fake `sh` script through `set_var("OPENIDEA_GH")`, done once inside `TEST_GH.get_or_init`, and assert that `gh_executable()` now returns the fake.

**Why it is safe:**

- In test builds, `github.rs` runs gh only after `TEST_GH` is set (`gh_allowed()`), and no other app-gpui code runs gh. So the real gh is never run and no token is read, even if the assert fails.
- Each test's canned answers and call log live in its own repo's `.git/fake-gh/`, so the tests can run in parallel without a lock.
- One risk remains: during that single `set_var`, a C library on another test thread calling `getenv` could race it. §4.1 gives the alternative of exporting the variable from a nextest setup script.

### 3. Not wired, and why

- **Views that don't exist:** the merge dialog, the request-review picker, submit review, the timeline editor (`ShowTimeline`, the `timeline` flag), New Pull Request (`CreateOpened`) and ResolveConflicts. These are logged with `tracing::info!` only.
- **Actions not registered:** `github.openInBrowser`, `showTimeline`, `openInWorktree`, `deleteMergedBranch`, `toggleViewed`, `submitReview`. `github.selectOpenedPullRequest` only works with a number argument, because no timeline editor sets `githubTimelineActive`.
- **Run gh auth login:** `terminal.runCommand` has no handler in GPUI, so the button opens the Terminal without typing the command.
- **Not loaded:** review threads and the pending review (`threads` and `pending_comments` stay empty).
- **Checkout Pull Request… with no PR in context:** there is no number prompt; it logs instead.
- **Unproven end to end:** the "gh not installed" state. The gh lookup is cached for the whole process, so a test can't make gh look missing.

### 4. Most likely wrong, and most likely not to compile

- **Possible `ide-github` bug (from reading, unconfirmed):** gh's real "no GitHub remote" message also says "gh auth login". `classify_failure` checks that phrase first, so a project with no GitHub remote would show "Not logged in" instead of "No GitHub repository". The test sidesteps it by feeding only the first sentence of the message. An `ide-github` unit test on the full message settles it.
- **Test risks:** the checkout test assumes `githubConnected` is published by a frame drawn after connecting (needs §1.6 applied). The search test assumes clicking `gh-search-field` focuses the text field rather than a parent taking focus back. The fake gh picks its answer by matching text in the GraphQL document; if the documents are sent differently, a call falls through as "unexpected".
- **Compile risks:**
  - `gh_call`'s `impl Future + 'static + use<T, W>` return type.
  - `map_err(unexpected)`, where `unexpected` takes `impl Display`.
  - The nested `fn checkout(call: &ActionCall, …)`: an elided lifetime on `ActionCall`.
  - `#[cfg(test)]` on an `if` statement.
  - Task fields that are written but never read could trip dead-code warnings (the same pattern compiles in `branches.rs`).
  - Several test lines may need hand-wrapping to stay under 120 columns.
  - `gh_dir(slot.repo.connected_root(), slot)` borrowing while `slot` is `&mut`.
- **Strings not in the catalogs:** six new user-visible strings (the checkout and diff failure titles and four progress texts), plus the action titles.
