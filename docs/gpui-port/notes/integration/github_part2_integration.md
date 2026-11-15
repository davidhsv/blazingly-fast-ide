# Integrating New Pull Request, Toggle Viewed and Open in Worktree into the Pull Requests window

New files, not compiled yet:

- `app-gpui/src/github/create_view.rs`: the New Pull Request form (`CreateForm`, a plain model) and its view (`CreatePanel`), with model `#[test]`s and view `#[gpui_kit::test]`s.
- `app-gpui/src/workbench/github_part2.rs`: actions, backend calls, end-to-end tests with the fake gh.

`github_part2.rs` is a **child module of `workbench::github`**, declared from `github.rs` with `#[path]`, not a sibling declared from `mod.rs`. As a child it sees every private item of `github.rs` it uses (`GithubSlot`/`PullRequestTab` fields, `gh_call`, `gh_dir`, `gh_api`, `find_pull_request`, `viewed_states`, `redacted_text`, `unexpected`, `number_arg`, `github_notice`, and the private methods `open_pull_request`, `reload_pull_request_list`, `context_pull_request`, `push_pull_request_inputs`). So `mod.rs` is not edited at all. Its test paths are `workbench::github::part2::tests::…`.

Every anchor below is quoted from the file as it is now. No line numbers are given.

## 1. `app-gpui/src/github/mod.rs`

After `pub mod create;` add:

```rust
pub mod create_view;
```

## 2. `app-gpui/src/workbench/branches.rs`

`fn host_call<T, F, W>(` becomes `pub(super) fn host_call<T, F, W>(` (the one at module level, "Runs `work` on the backend with the host and the window's label"). `github_part2.rs` uses it for `vcs_branches`, `vcs_branch_info`, `vcs_log` and `vcs_add_worktree`.

## 3. `app-gpui/src/workbench/github.rs`

### 3a. The module

After the import block's last line `use std::sync::Arc;` add:

```rust

#[path = "github_part2.rs"]
mod part2;
```

`#[path]` on a non-inline module in the non-mod-rs file `workbench/github.rs` resolves relative to `workbench/`, so it points at `app-gpui/src/workbench/github_part2.rs`.

### 3b. The slot's field

In `pub(super) struct GithubSlot`, after

```rust
    /// The list page on its way (a newer search makes the older answer void).
    list_fetch: Option<Task<()>>,
```

add:

```rust
    /// The New Pull Request tab, and Create Pull Request… waiting for the connection.
    part2: part2::Part2State,
```

### 3c. Its initial value

In `mount_pull_requests`, after `                list_fetch: None,` add:

```rust
                part2: part2::Part2State::default(),
```

### 3d. The actions

In `pub(super) fn register_actions`, the function ends with the `github.pullRequest.copyUrl` handler:

```rust
                workbench.with_pull_request_url(number, cx, |url, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                });
            }
        })
    });
}
```

Before that final `}` add:

```rust
    part2::register_part2_actions(registry);
```

### 3e. The context key

In `pub(super) fn publish_github_context`, after `        set_context(window, cx, "githubPrTab", tab);` add:

```rust
        self.publish_github_part2_context(window, cx);
```

This publishes `github.toggleViewed`. It is true while the window is visible and its active tab is a `#N` tab with a panel. It stands for the webview's `provideGithubHandler("toggleViewed")`, which the details panel's changes tree provides while shown.

### 3f. The repository was read again

In `pub(super) fn github_vcs_changed`, replace

```rust
        for number in numbers {
            self.push_pull_request_inputs(number, cx);
        }
    }
```

with

```rust
        for number in numbers {
            self.push_pull_request_inputs(number, cx);
        }
        self.github_part2_vcs_changed(cx);
    }
```

This is the webview's `CreatePanel` `useVcs.subscribe` → `onRepositoryChanged`. While the tab is open and ready, the branches are read again. A new check runs when the base or the head moved, or the head's upstream changed.

### 3g. A new connection state

In `fn push_github_repository`, replace

```rust
        view.update(cx, |view, cx| view.set_repository(repo, cx));
        cx.notify();
    }
```

with

```rust
        view.update(cx, |view, cx| view.set_repository(repo, cx));
        cx.notify();
        self.github_part2_repository_pushed(cx);
    }
```

This covers Create Pull Request… run before the window connected. The webview does `if (await showPullRequests()) openCreateTab()`. Here the tab opens once the connection is published. The wait is dropped when the check ends without a connection (not checking, not connecting, availability known).

### 3h. The window's events

In `fn pull_requests_event`, replace

```rust
            PullRequestsEvent::CreateOpened => {
                tracing::info!("github: CreateOpened needs the New Pull Request view, which is not built");
            }
            PullRequestsEvent::TabClosed(GhTab::PullRequest(number)) => {
                if let Some(slot) = self.github.as_mut() {
                    slot.tabs.remove(&number);
                }
            }
            PullRequestsEvent::TabClosed(_) => {}
```

with

```rust
            PullRequestsEvent::CreateOpened => self.open_create_tab(window, cx),
            PullRequestsEvent::TabClosed(GhTab::PullRequest(number)) => {
                if let Some(slot) = self.github.as_mut() {
                    slot.tabs.remove(&number);
                }
            }
            // The form lives as long as its tab.
            PullRequestsEvent::TabClosed(GhTab::Create) => self.drop_create_tab(),
            PullRequestsEvent::TabClosed(GhTab::List) => {}
```

`open_create_tab` mounts the form synchronously with `set_tab_view(GhTab::Create, …)`, so the tab never shows `gh-tab-loading`. A second `CreateOpened` (the `+` button on an open tab) keeps the form.

### 3i. The tests: the pending file and the helpers' visibility

In `mod tests`, after `    mod proofs14_github_window;` add:

```rust
    mod proofs14_github_window_pending;
```

`github_part2.rs`'s `mod tests` is a descendant of `github` but not of `github::tests`. It needs these helpers widened from `fn` / `const` to `pub(super)`, which means visible in `github` and its descendants:

- `    const GITHUB_REMOTE: &str` → `    pub(super) const GITHUB_REMOTE: &str`
- `    const FAKE_GH: &str = r##"` → `    pub(super) const FAKE_GH: &str = r##"`
- `    fn github_project<'a>(` → `    pub(super) fn github_project<'a>(`
- `    fn pull_request(number: u32, title: &str, author: &str, draft: bool, label: &str) -> Value {` → `    pub(super) fn pull_request(…`
- `    fn github_answers(head: &str) -> Vec<(&'static str, String)> {` → `    pub(super) fn github_answers(…`
- `    fn gh_calls(root: &Path) -> Vec<String> {` → `    pub(super) fn gh_calls(…`
- `    fn calls_with(root: &Path, text: &str) -> usize {` → `    pub(super) fn calls_with(…`
- `    fn context_text(cx: &mut VisualTestContext, key: &str) -> Option<String> {` → `    pub(super) fn context_text(…`
- `    fn window_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<PullRequestsWindow>> {` → `    pub(super) fn window_of(…`
- `    fn show_list(` → `    pub(super) fn show_list(`
- `    fn open_first(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DetailsPanel> {` → `    pub(super) fn open_first(…`

**`proofs14_github_window_pending.rs` once declared:**

- `opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab` should pass.
- `registers_the_window_the_timeline_editor_and_the_actions` still fails. It names four ids that are not registered: `github.pullRequest.showTimeline`, `github.pullRequest.timeline.refresh`, `github.selectOpenedPullRequest` and `github.pullRequest.submitReview` (section 6).

To keep the build green, declare it and mark that one test `#[ignore = "the timeline editor and Submit Review are not built"]`. It is not my file to edit, so that is the integrator's call.

## 4. Menu entries

- `menu_spec.rs` already lists `act("github.createPullRequest", "Create Pull Request…")` under Git › GitHub. It is now backed by a handler.
- The details panel's changes-tree context menu already offers `MenuItem::action("github.pullRequest.toggleViewed")` (`details_panel.rs`, the row's right-click). It is now registered and enabled through `github.toggleViewed`.
- The list's row menu offers `github.pullRequest.openInWorktree` with the row's number (`ListPanel.tsx:35`). `list.rs` and `list_panel.rs` already carry that row (read).
- The window's `+` header button (`gh-create-pull-request`) already runs `github.createPullRequest`.

## 5. Actions registered: id → what it does → test → IPC commands

### `github.createPullRequest`

What it does: shows the Pull Requests window and opens or selects the "New Pull Request at {repo}" tab, at once when connected, else once connected.

Tests:

- `workbench::github::part2::tests::create_pull_request_creates_the_current_branchs_pull_request_and_opens_its_tab`
- `…::create_pull_request_before_the_window_connects_opens_the_tab_once_connected`
- `workbench::github::tests::proofs14_github_window_pending::opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab`

IPC commands:

- `vcs_branches` (`store.ts initCreate`: `call(commands.vcsBranches(connectedRoot()))`)
- `vcs_branch_info` (`call(commands.vcsBranchInfo(connectedRoot()))`)
- `gh_api`, `getPullRequestTemplates` (`loadPullRequestTemplate` → `api.pullRequestTemplates()`)
- `vcs_log` with `range` and `limit: 10_000` (`reloadBranchesState`: `call(commands.vcsLog(connectedRoot(), {… range: commitsRange(base, head) …}))`)
- `gh_api` REST `pulls?state=open&head=…&base=…&per_page=1` (`api.findOpenPullRequestsByHead(searchHead(headRepo, tracked.name), base.name)`)
- `vcs_remote` `PushBranch`, only when the push target lacks the commits (`pushHead`: `runRemote({ op: "pushBranch", … })`)
- `gh_api` GraphQL `createPullRequest` (`createPullRequest` → `api.createPullRequest({ repositoryId, baseRefName, headRefName, title, body, draft })`)

Native call sites: `init_create`, `load_create_branches`, `push_head`, `create_on_github` in `github_part2.rs`.

### `github.pullRequest.toggleViewed`

What it does: the files selected in the active `#N` tab's changes become viewed, or not viewed when all already are. The tab shows it at once. A failure reads the viewed states again.

Test: `…::toggle_viewed_marks_the_selected_file_viewed_then_not_viewed`. The fake's log shows `markFileAsViewed` then `unmarkFileAsViewed` with `"path":"src/main.rs"`, `"pullRequestId":"PR_1"`.

IPC commands:

- `gh_api` GraphQL `markFileAsViewed` / `unmarkFileAsViewed`, one per path (`operations.ts setViewed`: `viewed ? api.markFileAsViewed(id, p) : api.unmarkFileAsViewed(id, p)`)
- on failure, `gh_api` `pullRequestFiles` (`reloadPullRequestData(number, "viewedState")`)

The review diff's own toggle (`githubReviewDiff`, `toggleReviewDiffViewed`) is not part of it.

### `github.pullRequest.openInWorktree`

What it does, in order:

1. The details: the tab's, else gh's.
2. `prepare_diff`: the head and base are fetched when missing.
3. `vcs_add_worktree(revs.root, "<root>_PR_<n>", headRefOid, None)`.
4. `file.openInNewWindow` with that folder. In tests the folder is recorded instead (`OpenedWindows`).

Failures are notified as "Couldn't fetch #N" or "Couldn't create the worktree for #N", key `github-op`.

Test: `…::open_in_worktree_adds_the_pull_requests_head_as_a_worktree_and_opens_it`.

IPC commands:

- `gh_api` `findPullRequest` (`loadPullRequestData(number, "details")`)
- `gh_pr_prepare_diff` (`prepareDiff` → `commands.ghPrPrepareDiff(root, r, pr.number, pr.headRefOid, pr.baseRefOid, pr.baseRefName, repo, out)`)
- `vcs_add_worktree` (`local.ts openInWorktree`: `call(commands.vcsAddWorktree(revs.root, path, pr.headRefOid, null))`)

No destructive gh call is made by these three. `createPullRequest` and `markFileAsViewed` are asserted on the fake's `calls.log`. No push runs in the tests: the head's push target is set to its tip.

## 6. Actions of the assignment NOT registered, and why

- **`github.pullRequest.showTimeline`, `github.pullRequest.timeline.refresh`, `github.selectOpenedPullRequest`** (when `… && githubTimelineActive`; Refresh Timeline ⌘R / Ctrl+F5 with `keyWhen: editorAreaFocus`).
  - These need the timeline editor tab (editor kind `github.timeline`, `TimelineEditor.tsx` with `timeline/items.tsx`). The model exists (`github/timeline.rs`: `merge_timeline`, review statuses, hunks, suggestions, reactions, mentions; `toolwindow.rs`: `TimelineInput`, `timeline_title`, `timeline_icon`). Neither the view nor an editor-area tab of a custom kind is built.
  - gh calls: `gh_api` `pullRequestTimeline`, paged.
  - `selectOpenedPullRequest` → `openPullRequest(activeTimelinePullRequest(), { timeline: false })`.
  - `PullRequestOpened { timeline: true }` and `DetailsEvent::ShowTimeline` still only log.
- **`github.pullRequest.submitReview`** (when `… && (github.submitReview || githubCurrentBranchPullRequest)`).
  - This needs the Submit Review popup: a 500×200 text area, Approve / Request Changes (not for the author) / Comment, and discard of the pending review.
  - gh calls:
    - `gh_api` `findPendingReview`.
    - `submitReview(reviewId, event, body)` with a pending review, else `createReview({ event, body })`.
    - `deleteReview` for the discard.
  - No view is built. `DetailsEvent::Flow(FlowButton::SubmitReview)` still logs.
- **`github.pullRequest.deleteMergedBranch`** (hidden, `CONNECTED`).
  - gh call: `deleteRef(headRef.id)`, GraphQL.
  - The timeline's merged-state item offers it. It is not registered because no view offers it and no test could reach it from the UI.
- **`github.pullRequest.showInToolWindow`, `.updateBranch`, `.reviewMode`, `.reviewMode.showDiff`.**
  - These need review mode on the current branch: the branch widget's review group and the editor toolbar (`review_mode.rs` `branch_widget`, `review_toolbar`; `review_editor.rs`).
  - gh calls: REST `pulls?head=` lookup, `vcs_merge_base`, `gh_pr_prepare_diff` / `vcs_remote` update.
  - Their context keys (`github.showCurrentBranchPullRequest`, `github.updateReviewBranch`, `github.toggleReviewMode`, `githubCurrentBranchPullRequest`) are published by nobody.
- **`github.pullRequest.diff.addComment`, `.addMultilineComment`, `.nextComment`, `.previousComment`, `.refreshReview`** (`githubReviewDiff || githubReviewEditor`).
  - These need the review inlays in the diff window and the editor: `review_diff.rs`, `review_editor.rs`, `new_comments.rs`, `gutter.rs` hold the rules, but no view draws them.
- **Settings → Version Control → GitHub (item 4): not reached.**

## 7. Behaviours of the webview not built

- **The changes panel of the New Pull Request tab** (`ChangesPanel.tsx`): the three-dot diff of all commits or one commit's files, `vcs_merge_base` + `vcs_diff_revisions` / `vcs_commit_files`, with diffs. The tab lists the commits only. Clicking a commit selects nothing.
- **The metadata grid** (`MetadataGrid.tsx`): reviewers, assignees and labels, then "Updating pull request fields". `CreationState::Metadata` is never entered.
- **The head popup's repository selector and search.** The head is a menu: local branches, then each GitHub remote's branches, labelled `owner/name: remote/branch` with several remotes. The base is a menu of the base remote's branches, with no search field.
- **"Details" popup of a failed check or creation.** The error shows as a second, small line under the status line.
- **Push failure detail.** `run_remote` notifies the push failure itself and returns only a bool, so the form says "Push failed" without `: {error}`.
- **"New Remote Branch for Pull Request".**
  - It uses `PromptDialog::prompt` with `FieldRule::NonEmpty`, label "Remote branch name:", and `clean_up_branch_name` on OK.
  - Missing: the typing clean-up of `RemoteBranchDialog` (spaces → `-` as typed; the model's `RemoteBranchDialog::set_text` exists) and the comment line "Your changes will be pushed…".
  - `FieldRule::Branch` needs a `BranchPrompt`, whose rule field is private.
- **The template cache per repository** (`template ??=`). The template is read again each time the tab opens.
- **`refreshCurrentBranchPullRequest()` after creating.** Review mode is not built.

## 8. Spec files

- `toolwindow/toolwindow.test.tsx`: "registers the window, the timeline editor and the actions" is still failing on 4 ids (section 3i). The other cases were covered before. The pending "opens … New Pull Request tab once …" is covered.
- `create/create.test.ts`: 14/14, already covered by `github/create.rs`'s model tests.
- `create/RemoteBranchDialog.test.tsx`: 1/1 model-covered (`create.rs`). The dialog shown does not clean as typed (section 7).
- `settingsPages.test.tsx`: 0/3, not reached.

The view tests in `create_view.rs` are "Not from a spec", since no spec covers `CreatePanel`.

## 9. Assumptions to check first

1. `#[path = "github_part2.rs"] mod part2;` in `workbench/github.rs` resolves to `workbench/github_part2.rs`. If it resolves to `workbench/github/github_part2.rs`, use `#[path = "../github_part2.rs"]`.
2. `Entity<CreatePanel>` → `AnyView` with `.into()`, as `github.rs` does for `DetailsPanel`.
3. `ide_host::ops::vcs::{vcs_branches, vcs_branch_info, vcs_log, vcs_add_worktree, RemoteOp}` are re-exported (`ops/vcs.rs`: `pub use commands::*;`). `LogQuery { range, limit, ..LogQuery::default() }`: `LogQuery` derives `Default`, and its `request` field defaults to `None`.
4. `ide_vcs::Branch` names remote branches `origin/main` with `remote: true`. A local branch with `branch.feature.remote/merge` config has `upstream: Some("origin/feature")`. `create.rs` assumes the same.
5. `PromptDialog::prompt` translates nothing itself, so title and label are passed translated.
6. The `when` parser accepts the dotted key `github.toggleViewed` (`platform/context.rs` tokenizer allows `.`; read, not run).
7. Fields holding only a `Task` (`CreateTab::init/branches/refresh/follow`) are written and never read, like `GithubSlot::refreshing`. If `dead_code` flags them, prefix them with `_`.

## 10. Tests most likely to fail on first run

- `create_pull_request_creates_…`:
  - The form must fit the test window: `click_on("gh-create-submit")` needs the bottom of the tab drawn. If it is clipped, call `panel.update(cx, |p, cx| p.run_command(CreateCommand::CreateDraft, cx))` instead, and assert `"draft":true`.
  - The `vcs_log` range `origin/main..feature` must list exactly one commit.
- `toggle_viewed_…`:
  - `github.toggleViewed` is published on the render after the row click. If `run_action` reports the action unavailable, add `cx.run_until_parked()` before it.
  - The row index comes from `tree_rows()`, which depends on how `changes_tree` compacts `src/main.rs`.
- `open_in_worktree_…`: the asserted path is `<root>_PR_1` from the canonical temp root. `revisions.root` must be the same canonical path (`Repo::discover` of the primary root).
- `create_view.rs` `a_typed_title_stays_…`: typing needs the field focused in an active window (`input::init`, `activate_window`), as in `prompt_dialogs.rs` tests.
