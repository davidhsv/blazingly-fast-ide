I've finished Part 0 and Part 1, and everything is green. The one deviation from your brief: the GitHub commands in `src-tauri/src/commands/github.rs` don't check trust themselves. Tauri refuses them in one central place, and I gated the native side the same way.

## 1. Suite, clippy, scope

- **Suite ×5** (`cargo test --lib`): 6600 passed, 0 failed, 0 ignored, every run (67–88 s). Up from 6583: +1 Part 0b test, +16 GitHub tests.
- **Re-run:** after the final type-alias edit (for clippy) I ran the suite a sixth time and clippy again: 6600 passed, `cargo clippy --all-targets -- -D warnings` clean.
- **Compile errors:**
  - Part 0: 0 errors; 3 unused-import warnings in `terminal_actions.rs`, fixed.
  - Part 1: 0 compile errors on the first build. Clippy found 1: `type_complexity` in the drafted `github.rs` (`PullRequestTab.commit`), fixed with `type LoadedFiles`.
- **Scope:** files touched are `app-gpui/src/{backend.rs, terminal/{panel,test_support,view}.rs, workbench/{branches,conflicts,github,log_actions,mod,project_files,terminal_actions}.rs}`, `crates/ide-github/src/lib.rs`, and one new file, `crates/ide-github/tests/cli.rs`. That new file is beyond "only the seam": it is the notes' file, written out after I read it. I formatted only `mod.rs` and `github.rs`, with `rustfmt --edition 2024`; the check showed no other file would change.
- **Crate diff** (`lib.rs`, +127/−53; saved at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/crate_diff_gh.txt`): the 5 hunks of `crate_hunks.py`, applied in place. They add `GhCli` (`located`/`at`/`not_installed`/`arg`, plus `availability`/`api`/`checkout`); the free functions delegate to `GhCli::located()`. No specta derive on `GhCli`, no new dependency.
- **`cargo test -p ide-github`:** 4 + 3 (`cli.rs`) + 6 (`parse.rs`) passed. I read every test first: all are parse-only or run a fake `/bin/sh` program.
- **`cargo test -p openidea export_bindings`:** passed.
- **`git status --short src/ src-tauri/`:** empty.

## 2. Part 0

- **a. Plain shells.**
  - `terminal/test_support.rs` now holds `PLAIN_SHELL`, `plain_shell(home)` and `trusted_folder(window, root, cx)`.
  - `isolate_shells`, which every test start helper calls, now also sets `terminal.shellPath` to the plain shell and turns shell integration off.
  - The plain shell got `trap : INT` and re-reads after an interrupted read, so Ctrl+C stops the running command, not the shell.
  - A test-build guard, `assert_plain_shell` in `TerminalView::new`, panics if a terminal would start a known shell with no arguments or with `-l`/`--login`/`-i`, or would start `gh`. It applies the backend's rule: an untrusted window always gets the login shell.
  - The guard found 5 tests that would start a login shell; each was fixed:
    - `terminal/view.rs`: `a_shell_runs_in_the_terminal_and_reports_its_exit`
    - `terminal/panel.rs`: `tabs_are_shells_of_their_own`, `remembered_tabs_start_their_shells_when_first_shown`
    - `mod.rs`: `the_terminal_tool_window_runs_a_shell_at_the_bottom`, `a_new_terminal_tab_opens_the_terminal_with_one_tab` (folder trusted first), and `search_everywhere_gives_the_keyboard_back_and_runs_actions_where_it_was_opened` (via a new `search_project_with(cx, true)`).
  - What I checked:
    - Every PTY spawn in app-gpui goes through `ops::term::spawn` in `TerminalView::new`.
    - A grep for `Command::new("sh"|"bash"|"zsh")`, `-lc`/`-ilc`/`--login`/`-i` finds no test hits.
    - The login-environment capture (`shell_env -ilc`, `env_reload`) is never called from app-gpui, and `Backend` sets `project_environment: false`.
- **b. `remote_busy` left set.**
  - New test `workbench::tests::a_remote_operation_dropped_before_it_ends_lets_the_next_one_run`. Note its shape: `run_remote` is parked at the gate behind a hanging `run_op`, rather than given a never-ending backend call itself; that task is dropped, then Fetch runs.
  - Before the fix it failed: `busy=Some("Fetching")` and the notice "Another remote operation is in progress".
  - Fix in `branches.rs`: a `BusyMark` guard (oneshot release task plus a numbered `busy_mark`, the same mechanism as `GitCommand`) is used by `run_remote`, `run_git_operation` and the gh checkout.
- **c. Trash override.**
  - In test builds `Backend::start` asserts the home is isolated and sets the trash to `<test home>/trash`, with numbered names. That covers every test that builds a Backend.
  - All three delete callers (`project_files.rs`, `conflicts.rs`, `log_actions.rs`) now go through `crate::backend::delete_path`, which panics in test builds if the host has no live registered test trash.
  - Grep results: no `set_trash(None)` anywhere; `Host::new(` appears only in `backend.rs`.

## 3. Part 1

All test paths below are under `workbench::github::tests::`.

| Action                                                           | Test(s)                                                                                                                                      | IPC → backend                                                           | Native call same?                             | Status                                                               |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------- | -------------------------------------------------------------------- |
| github.pullRequests                                              | `the_window_connects_and_lists_two_pull_requests_with_their_state_author_and_labels`, `a_search_term_filters_the_list_through_a_new_request` | vcs_roots/vcs_remotes; gh_availability → `availability`; gh_api → `api` | yes, via `GhCli`                              | partial: list paging and chooser popups not tested                   |
| github.refresh                                                   | `refresh_list_checks_gh_again_and_asks_for_the_list_again`, `the_refresh_keys_work_only_in_the_pull_requests_window_on_their_tab`            | same as above                                                           | yes                                           | done                                                                 |
| github.changeRepository                                          | `change_repository_disconnects_then_connects_again`                                                                                          | same as above                                                           | yes                                           | done (one remote)                                                    |
| github.pullRequest.show                                          | `view_pull_request_opens_its_tab_with_its_title_branches_checks_and_changed_files`                                                           | gh_api ×5                                                               | yes                                           | partial: no timeline, threads or pending review                      |
| github.pullRequest.refresh                                       | `refresh_pull_request_loads_the_open_pull_request_again`, keys test                                                                          | gh_api ×5                                                               | yes                                           | done                                                                 |
| github.pullRequest.checkoutBranch                                | `checkout_branch_checks_out_the_pull_requests_branch_and_the_branch_widget_follows`                                                          | gh_pr_checkout → `checkout`                                             | yes; also takes the BusyMark and the Git gate | done                                                                 |
| github.checkoutPullRequest                                       | `checkout_pull_request_checks_out_the_number_it_is_given`, `in_restricted_mode_the_window_runs_no_gh_until_the_project_is_trusted_again`     | gh_pr_checkout                                                          | yes                                           | partial: no number prompt                                            |
| github.pullRequest.openInBrowser, .copyUrl, github.openInBrowser | `the_browser_and_copy_actions_use_the_pull_requests_page`                                                                                    | none (opens the URL)                                                    | —                                             | partial: the URL is built, not loaded; repository-page case untested |

Not registered: createPullRequest, selectOpenedPullRequest, timeline, review mode, comments, submitReview, toggleViewed, openInWorktree, deleteMergedBranch, clone. The details-panel mutations and the diff are wired but untested.

- **Context keys:** `githubConnected` is read off before the window shows, after Change Repository disconnects, and in Restricted Mode; on after connecting and reconnecting. `githubPrTab` is read as `list`, `pr`, then `list` again.
- **Shortcuts:** ⌘R on macOS, Ctrl+F5 on Windows.
  - Refresh List works in the list tab and does nothing with no tool window active.
  - In the `#1` tab, Refresh Pull Request starts exactly 5 calls and the list is not refreshed.
  - I added a negative: back in the list tab with `#1` open, the list refreshes and the pull request's details requests stay unchanged.
- **No real gh in a test build:**
  - The test `gh_program` returns only the stand-in, or an error when there is none.
  - `gh_call` asserts on the window thread that the program is never "the machine's gh", and the test `located_gh` panics.
  - The terminal guard refuses `gh`. The only `ide_github` free function app-gpui calls is `prepare_diff`, which runs git, not gh.
  - The fake gh unsets the token variables and fails loudly on anything unexpected. Test repositories set `protocol.allow never`; measured: an https fetch then fails with `fatal: transport 'https' not allowed`.
- **Login only recorded:** the test build's `run_in_terminal` only records the request; the sign-in test asserts the exact argument and that the Terminal stays closed.
- **Credentials masked:** the sentinel `{:?}` test passes as written.
- **Restricted Mode:** `gh_call` calls `host.authorize_command(label, "gh_*")`, as Tauri's central gate does. The new test asserts the checkout is refused with "Restricted Mode" in the notice and HEAD stays on `main`, a refresh shows the refusal with nothing connected, and no gh call starts. Once trusted again, the checkout runs.
- **Assumption 1 measured:** `git remote -v` prints the URL after the `insteadOf` rewrite.
- **Row and tool-window lists:** none needed changing. `every_menu_group_is_listed_once`'s `missing` list is the same 12 entries, `github.reviewMode` included, because no review-mode rows were added. The `context_menus.rs` and `a_tabs_menu_closes_pins_and_splits` lists have no GitHub rows. `TOOL_WINDOWS` is now 19; the tests iterate it (e.g. `mod.rs` ~5689), so they now check `toolwindow.pullRequests` too.
- **Registered ids:** 550 (previously 509; the 10 GitHub ids are among the new ones), saved to `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/registered_ids.txt`.

## 4. Fixes, changed tests, false notes

- **Product changes beyond the notes:**
  - The Restricted Mode gate in `gh_call` (source: `src-tauri/src/lib.rs:170`, `crates/ide-host/src/trust.rs:206`).
  - The PR checkout now holds the Git gate (`git_command_starts`/`git_command_ended`).
  - `begin_outside_git_operation` now returns a `BusyMark` (a new `OperationRuns` error).
  - `delete_path` replaces the three direct calls to the backend's delete.
- **Existing tests changed:** the 5 terminal tests above, to trust their folder or use the plain shell, and `plain_shells` in `terminal_actions.rs`, which now delegates to the shared helper.
- **False premise in the brief:** `src-tauri/src/commands/github.rs` has no trust check. Tauri refuses `gh_*` centrally because they are not in `restricted_command_allowed`.
- **Hypotheses:**
  - Tauri also refuses `vcs_roots`/`vcs_remotes` in Restricted Mode, while natively the remotes are still listed before gh is refused. What the user sees is the same. Check: grep `vcs_` in `trust.rs:206` (only `vcs_write_patch` is allowed).
  - Assumption 10, `classify_failure` on gh's full "no GitHub remote" message, is unchecked. Check: a unit test in `ide-github` on the full message.
- **Drafted tests left out:** none.
