# GitHub Pull Requests tool window: integration

New file (already written, not compiled): `app-gpui/src/workbench/github.rs`. Everything below is a change to a compiled file, given as exact code anchored on a line that is unique in the file today (checked with `grep -c` while writing this). Nothing here was compiled.

Prerequisites already in place (checked): `app-gpui/src/lib.rs` has `pub mod github;`, `app-gpui/src/github/mod.rs` has `pub mod details_panel; pub mod list_panel; pub mod window_view;`, and `app-gpui/Cargo.toml` depends on `ide-github`. `tokio` and `serde` are already used by other workbench modules (`services_window.rs` calls `tokio::task::spawn_blocking`).

## 1. `app-gpui/src/workbench/mod.rs`

### 1.1 The module and the window id

After the line

```rust
mod editor_intelligence;
```

add

```rust
mod github;
```

After the line

```rust
use structure_window::STRUCTURE_TOOL_WINDOW;
```

add

```rust
use crate::github::window_view::PULL_REQUESTS_TOOL_WINDOW;
```

### 1.2 The field

In `pub struct Workbench`, after the line

```rust
    vcs: Option<branches::VcsSlot>,
```

add

```rust
    /// The Pull Requests tool window of the project: the GitHub connection and the opened pull requests.
    github: Option<github::GithubSlot>,
```

In `Workbench::new`'s struct literal, after the line

```rust
            services: None,
```

add

```rust
            github: None,
```

### 1.3 `TOOL_WINDOWS`

Today the array is `const TOOL_WINDOWS: [&str; 15] = [` and ends with `    HIERARCHY_TOOL_WINDOW,` then `];` (15 entries counted: PROJECT, COMMIT, TERMINAL, GIT, FIND, NOTIFICATIONS, RUN, SERVICES, BOOKMARKS, STRUCTURE, TODO, BUILD, PROBLEMS, USAGES, HIERARCHY). Other drafts (agents, debug, database…) add entries too: re-count when applying and add one to whatever the count is then. With only this change:

```rust
const TOOL_WINDOWS: [&str; 16] = [
```

and after the line

```rust
    HIERARCHY_TOOL_WINDOW,
```

add

```rust
    PULL_REQUESTS_TOOL_WINDOW,
```

`tool_window_declaration("pullRequests")` resolves from `assets/gen/contributions.json` (entry at line ~3322: title "Pull Requests", icon `toolwindow-pull-requests`, anchor left, order 2, stripe onUse), so the existing loop registers `toolwindow.pullRequests` and the layout registers the window.

### 1.4 `register_actions`

After the line

```rust
    services_window::register_actions(registry);
```

add

```rust
    github::register_actions(registry);
```

None of the ids it registers are in `assets/gen/actions.json` (only the `github.pullRequest.diff.*`, `reviewMode*`, `showInToolWindow`, `updateBranch` ids are), so `github.rs` uses `registry.register(Action::new(ActionMeta…))`, not `registry.handle` (which panics on an id missing from the manifest).

### 1.5 The mount arm and the visibility sync

In `fn mount_panels`, after the arm

```rust
                SERVICES_TOOL_WINDOW => match self.mount_services(window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

add

```rust
                PULL_REQUESTS_TOOL_WINDOW => match self.mount_pull_requests(window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

In the same function, after the line

```rust
        self.sync_services_window(cx);
```

add

```rust
        self.sync_pull_requests_window(cx);
```

### 1.6 The context keys

In `fn publish_context`, after the line

```rust
        self.publish_vcs_context(window, cx);
```

add

```rust
        self.publish_github_context(window, cx);
```

This publishes `githubConnected` (the `CONNECTED` condition of the pull request actions) and `githubPrTab` (`list` / `pr` / `create`, the `keyWhen` of `github.refresh` and `github.pullRequest.refresh`).

### 1.7 Resets on project close and open

`        self.drop_vcs();` appears exactly twice in `mod.rs`: in `close_project` and in `open_project`. After each of them add

```rust
        self.drop_github();
```

## 2. `app-gpui/src/workbench/branches.rs` (optional, recommended)

So that an open `#N` tab's "branch checked out" state follows checkouts made elsewhere (the Branches popup, the terminal), in `fn vcs_state_changed`, after the line

```rust
        self.sync_log_panes(cx);
```

add

```rust
        self.github_vcs_changed(cx);
```

Without it, only Checkout started from the Pull Requests window updates the tabs (`run_checkout` calls `github_vcs_changed` itself).

## 3. What `github.rs` expects of the integrator

- `Workbench` field `github: Option<github::GithubSlot>` (1.2).
- `pub(super)` methods called from `mod.rs`: `mount_pull_requests(&mut self, &mut Window, &mut Context<Self>) -> Option<Panel>`, `sync_pull_requests_window(&mut self, &mut Context<Self>)`, `publish_github_context(&self, &mut Window, &mut App)`, `drop_github(&mut self)`, `github_vcs_changed(&mut self, &mut Context<Self>)`, and the free `register_actions(&mut ActionRegistry)`.
- Public accessors for tests: `Workbench::pull_requests_window() -> Option<&Entity<PullRequestsWindow>>`, `Workbench::pull_request_details(number) -> Option<&Entity<DetailsPanel>>`.
- `#[cfg(test)] pub(super) static TEST_GH: OnceLock<PathBuf>` in `github.rs`: in test builds gh is run only once a test has set it (see 4.1).

## 4. End-to-end tests

### 4.1 The seam that stands in for gh, and why it is safe

How `ide-github` finds gh (read in `crates/ide-github/src/lib.rs`): `gh_executable()` fills a process-wide `static GH: OnceLock<Option<PathBuf>>` the first time it is called, from `OPENIDEA_GH` when that names an existing file, else `which::which("gh")`, else a few fixed paths. Every call (`availability`, `api`, `checkout`, `prepare_diff`) goes through it and runs the program with `current_dir(root)`. There is no per-call or per-project setting. So the narrowest seam is: set `OPENIDEA_GH` to a fake script once, before the first lookup, in the test process.

Why it is safe here:

- The variable is written exactly once per process, inside `TEST_GH.get_or_init`, and never changed. The only reader is `ide_github::gh_executable`, and nothing in the crate reaches it before `TEST_GH` is set: `github.rs` refuses to run gh in test builds until then (`gh_allowed()`), and no other app-gpui code calls an `ide_github` function that runs gh (checked by grep: other modules only use its types). So no test ever runs the machine's real gh, and its account or token is never read.
- After the write, the installer asserts `ide_github::gh_executable() == Some(fake)`. If anything had looked gh up first, the assert fails, `TEST_GH` stays unset, and the guard keeps gh from running, so the real gh is still never run.
- Different tests never share answers: the fake reads its canned answers from `.git/fake-gh/` of the repository gh runs in (each test has its own temporary repository), and logs there too. So the GitHub tests can run in parallel threads without a lock; there is nothing to serialise but the one write, which the `OnceLock` serialises.
- What remains formally unsafe: `std::env::set_var` (an `unsafe fn` in Rust 2024) can race with a C library on another test thread calling `getenv` during that single write. Rust's own `std::env` readers and `Command::spawn` take std's environment lock and are safe. If the build owner wants no such window at all, run the GitHub tests with `OPENIDEA_GH` already set in the process environment (a nextest setup script that writes the fake and exports the variable) and drop the `set_var` line. The guard and the assert stay as they are.
- `gh` missing (the "Install gh…" state) cannot be shown in the same process, because the lookup is cached for the process. It stays unproven end to end (the model's `unavailable` covers its text).

The fake is a POSIX `sh` script (no python needed). It answers `repo view`, `api user --jq .login` (availability), `api --method … graphql` by the document in the request body (issueSearch, mergeability, commits, files, findPullRequest, findRepository), REST `user`, `pulls/N/files`, the repository lists, and `pr checkout N` (it really runs `git checkout -b <branch>` with the machine's git config ignored). Anything else exits 1 with "fake gh: unexpected call".

### 4.2 The tests

Paste this module at the end of `mod tests` in `app-gpui/src/workbench/mod.rs`, after the git helpers (`git_in`, `init_repo`, `git_head`, `git_project`, `branch_label`, `wait_long`, `run_action`, `click_on`, which it uses through `use super::*`).

```rust
    /// The Pull Requests window against a stand-in for gh.
    #[cfg(unix)]
    mod pull_requests {
        use super::*;
        use crate::github::details::head_branch_label;
        use crate::github::toolwindow::ToolWindowContent;
        use crate::github::window_view::PullRequestsWindow;
        use serde_json::{Value, json};
        use std::path::Path;

        const GITHUB_REMOTE: &str = "https://github.com/org/app.git";

        /// A stand-in for the GitHub CLI: it answers from the files a test writes into `.git/fake-gh`
        /// of the repository it runs in (gh runs in the connected repository), and logs every call,
        /// its arguments and the request body, to `calls.log` there.
        const FAKE_GH: &str = r##"#!/bin/sh
D="$(pwd -P)/.git/fake-gh"
[ -d "$D" ] || { echo "fake gh: no answers in $D" >&2; exit 1; }
body=""
case " $* " in *" --input - "*) body="$(cat)" ;; esac
printf '%s\t%s\n' "$*" "$body" >> "$D/calls.log"
answer() {
  if [ -f "$D/$1" ]; then cat "$D/$1"; exit 0; fi
  echo "fake gh: no answer $1" >&2
  exit 1
}
case "$1 $2" in
  "repo view")
    if [ -f "$D/repo-view.err" ]; then cat "$D/repo-view.err" >&2; exit 1; fi
    answer repo-view.json ;;
  "api user")
    if [ -f "$D/repo-view.err" ]; then cat "$D/repo-view.err" >&2; exit 1; fi
    echo me
    exit 0 ;;
  "pr checkout")
    branch="$(cat "$D/checkout-$3")" || exit 1
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git checkout -q -b "$branch" || exit 1
    exit 0 ;;
  "api --method") ;;
  *) echo "fake gh: unexpected call $*" >&2; exit 1 ;;
esac
case "$4" in
  graphql)
    case "$body" in
      *"query issueSearch"*)
        for f in "$D"/search-*.json; do
          [ -f "$f" ] || continue
          term="${f##*/search-}"
          term="${term%.json}"
          case "$body" in *"($term)"*) cat "$f"; exit 0 ;; esac
        done
        answer search.json ;;
      *"...pullRequestMergeability"*) answer mergeability.json ;;
      *'commits(first: '*) answer commits.json ;;
      *'files(first: '*) answer viewed.json ;;
      *"pullRequest(number:"*) answer pull-request.json ;;
      *"...repository"*) answer repository.json ;;
    esac ;;
  user) answer user.json ;;
  repos/*/pulls/*/files*) answer files.json ;;
  repos/*/labels*|repos/*/assignees*|repos/*/collaborators*|repos/*/contributors*) echo '[]'; exit 0 ;;
esac
echo "fake gh: unexpected call $*" >&2
exit 1
"##;

        /// Puts the fake gh in place of the GitHub CLI for this test process, once: `ide_github`
        /// reads `OPENIDEA_GH` the first time it looks for gh and keeps the answer, and the workbench
        /// runs gh only once a test has set its stand-in.
        fn fake_gh() {
            use std::os::unix::fs::PermissionsExt;
            crate::workbench::github::TEST_GH.get_or_init(|| {
                let dir = std::env::temp_dir().join(format!("openidea-fake-gh-{}", std::process::id()));
                std::fs::create_dir_all(&dir).expect("the fake gh's folder is made");
                let gh = dir.join("gh");
                std::fs::write(&gh, FAKE_GH).expect("the fake gh is written");
                std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("the fake gh can run");
                // SAFETY: written once per process, before anything looks gh up (the workbench runs gh
                // only once this lock is set), and read only by `ide_github::gh_executable`.
                unsafe { std::env::set_var("OPENIDEA_GH", &gh) };
                assert_eq!(ide_github::gh_executable().as_deref(), Some(gh.as_path()), "gh is the stand-in");
                gh
            });
        }

        /// A trusted project on a repository with `remote` as its `origin`, whose fake gh answers
        /// with `answers` (file name, content), given the repository's HEAD commit.
        fn github_project<'a>(
            cx: &'a mut TestAppContext,
            remote: &str,
            answers: impl FnOnce(&str) -> Vec<(&'static str, String)>,
        ) -> (Entity<Workbench>, &'a mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
            fake_gh();
            let remote = remote.to_owned();
            git_project(cx, move |root| {
                init_repo(root);
                git_in(root, &["remote", "add", "origin", &remote]);
                let head = git_in(root, &["rev-parse", "HEAD"]);
                let dir = root.join(".git").join("fake-gh");
                std::fs::create_dir_all(&dir).expect("the answers' folder is made");
                for (name, text) in answers(&head) {
                    std::fs::write(dir.join(name), text).expect("an answer is written");
                }
            })
        }

        fn actor(login: &str) -> Value {
            json!({
                "__typename": "User",
                "id": format!("U_{login}"),
                "login": login,
                "url": format!("https://github.com/{login}"),
                "avatarUrl": "",
                "name": null,
            })
        }

        fn repository() -> Value {
            json!({
                "id": "R_app",
                "owner": { "__typename": "Organization", "id": "O_org", "login": "org" },
                "nameWithOwner": "org/app",
                "viewerPermission": "WRITE",
                "mergeCommitAllowed": true,
                "squashMergeAllowed": true,
                "rebaseMergeAllowed": true,
                "defaultBranchRef": { "name": "main" },
                "isFork": false,
                "url": "https://github.com/org/app",
                "sshUrl": "git@github.com:org/app.git",
            })
        }

        /// An open pull request as the list's `pullRequestInfoShort` reads it.
        fn pull_request(number: u32, title: &str, author: &str, draft: bool, label: &str) -> Value {
            json!({
                "id": format!("PR_{number}"),
                "url": format!("https://github.com/org/app/pull/{number}"),
                "number": number,
                "title": title,
                "state": "OPEN",
                "isDraft": draft,
                "author": actor(author),
                "createdAt": "2026-11-01T10:00:00Z",
                "updatedAt": "2026-11-02T10:00:00Z",
                "isReadByViewer": true,
                "assignees": { "nodes": [] },
                "labels": { "nodes": [{ "id": format!("L_{label}"), "url": "", "name": label, "color": "d73a4a" }] },
                "reviewRequests": { "nodes": [] },
                "reviewThreads": { "nodes": [] },
                "reviews": { "nodes": [] },
                "reactions": { "nodes": [] },
                "mergeable": "MERGEABLE",
                "viewerCanUpdate": true,
                "viewerCanReact": true,
                "viewerDidAuthor": false,
            })
        }

        fn search(nodes: Vec<Value>) -> String {
            json!({ "data": { "search": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": nodes } } })
                .to_string()
        }

        fn in_pull_request(fields: Value) -> String {
            json!({ "data": { "repository": { "pullRequest": fields } } }).to_string()
        }

        /// A connected repository org/app with two open pull requests: #1 by alice (label bug, head
        /// `fix-crash` at `head`, one passing check, one changed file) and the draft #2 by bob (docs).
        fn github_answers(head: &str) -> Vec<(&'static str, String)> {
            let fix = pull_request(1, "Fix the crash on start", "alice", false, "bug");
            let docs = pull_request(2, "Document the setup", "bob", true, "docs");
            let mut details = fix.clone();
            for (key, value) in [
                ("body", json!("It no longer crashes.")),
                ("reviewDecision", Value::Null),
                ("baseRefName", json!("main")),
                ("baseRefOid", json!(head)),
                ("baseRepository", repository()),
                ("headRef", json!({ "id": "REF_1" })),
                ("headRefName", json!("fix-crash")),
                ("headRefOid", json!(head)),
                ("headRepository", repository()),
                ("viewerCanDeleteHeadRef", json!(false)),
            ] {
                details[key] = value;
            }
            let check = json!({
                "name": "build",
                "conclusion": "SUCCESS",
                "isRequired": false,
                "detailsUrl": null,
                "url": "https://github.com/org/app/runs/1",
            });
            let mergeability = json!({
                "mergeable": "MERGEABLE",
                "canBeRebased": true,
                "mergeStateStatus": "CLEAN",
                "baseRef": null,
                "commits": { "pageInfo": { "hasNextPage": false }, "nodes": [{
                    "id": "PRC_1",
                    "commit": {
                        "id": "C_1",
                        "oid": head,
                        "abbreviatedOid": "abc1234",
                        "status": null,
                        "checkSuites": { "nodes": [{ "checkRuns": { "nodes": [check] } }] },
                    },
                }] },
            });
            let commit = json!({
                "id": "C_1",
                "oid": head,
                "abbreviatedOid": "abc1234",
                "url": "",
                "messageHeadline": "Fix the crash",
                "author": null,
            });
            let commits = json!({
                "commits": { "pageInfo": { "hasNextPage": false }, "nodes": [{ "id": "PRC_1", "commit": commit, "url": "" }] },
            });
            let viewed = json!({
                "files": {
                    "pageInfo": { "hasNextPage": false },
                    "nodes": [{ "path": "src/main.rs", "viewerViewedState": "UNVIEWED" }],
                },
            });
            // `gh api --paginate --slurp`: an array of pages.
            let files = json!([[{
                "sha": "s1",
                "filename": "src/main.rs",
                "status": "modified",
                "additions": 2,
                "deletions": 1,
                "changes": 3,
                "patch": "@@ -1 +1,2 @@\n-a\n+b\n+c",
            }]]);
            let repo_view = json!({
                "nameWithOwner": "org/app",
                "url": "https://github.com/org/app",
                "defaultBranchRef": { "name": "main" },
                "mergeCommitAllowed": true,
                "squashMergeAllowed": true,
                "rebaseMergeAllowed": true,
                "viewerPermission": "WRITE",
            });
            let user = json!({
                "login": "me",
                "node_id": "U_me",
                "avatar_url": "",
                "html_url": "https://github.com/me",
                "name": "Me",
            });
            vec![
                ("repo-view.json", repo_view.to_string()),
                ("repository.json", json!({ "data": { "repository": repository() } }).to_string()),
                ("user.json", user.to_string()),
                ("search.json", search(vec![fix.clone(), docs])),
                ("search-crash.json", search(vec![fix])),
                ("pull-request.json", in_pull_request(details)),
                ("mergeability.json", in_pull_request(mergeability)),
                ("commits.json", in_pull_request(commits)),
                ("viewed.json", in_pull_request(viewed)),
                ("files.json", files.to_string()),
                ("checkout-1", "fix-crash".to_owned()),
            ]
        }

        /// gh's calls in the repository at `root`, each its arguments, a tab and the request body.
        fn gh_calls(root: &Path) -> Vec<String> {
            let log = std::fs::read_to_string(root.join(".git").join("fake-gh").join("calls.log"));
            log.unwrap_or_default().lines().map(str::to_owned).collect()
        }

        fn window_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<PullRequestsWindow>> {
            workbench.read_with(cx, |w, _| w.pull_requests_window().cloned())
        }

        fn content(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<ToolWindowContent> {
            let window = window_of(workbench, cx)?;
            Some(window.update(cx, |window, cx| window.content(cx)))
        }

        /// The numbers of the listed pull requests, top to bottom.
        fn listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<u32> {
            let Some(window) = window_of(workbench, cx) else { return Vec::new() };
            let list = window.read_with(cx, |window, _| window.list().clone());
            list.read_with(cx, |list, _| list.list().items.iter().map(|pr| pr.number).collect())
        }

        /// View Pull Requests, then the list as gh answers it.
        fn show_list(workbench: &Entity<Workbench>, count: usize, cx: &mut VisualTestContext) -> Entity<PullRequestsWindow> {
            run_action(cx, "github.pullRequests", None);
            wait_long(cx, "the pull requests are listed", |cx| listed(workbench, cx).len() == count);
            window_of(workbench, cx).expect("the window is open")
        }

        /// Opens #1's tab and waits for its details, checks and changes.
        fn open_first(
            workbench: &Entity<Workbench>,
            window: &Entity<PullRequestsWindow>,
            cx: &mut VisualTestContext,
        ) -> Entity<crate::github::details_panel::DetailsPanel> {
            window.update(cx, |window, cx| window.open_pull_request(1, false, cx));
            cx.run_until_parked();
            let panel = workbench.read_with(cx, |w, _| w.pull_request_details(1).cloned()).expect("#1 has a tab");
            wait_long(cx, "the pull request loads", |cx| {
                panel.update(cx, |panel, cx| {
                    let checks = panel.status_lines(cx).iter().any(|line| line == "All required checks have passed");
                    panel.details().is_some() && checks && !panel.tree_rows().is_empty()
                })
            });
            panel
        }

        /// View Pull Requests checks gh, connects to the repository of the project's GitHub remote and
        /// lists its open pull requests with their state, author and labels.
        #[gpui_kit::test]
        async fn the_window_connects_and_lists_two_pull_requests_with_their_state_author_and_labels(
            cx: &mut TestAppContext,
        ) {
            let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
            let window = show_list(&workbench, 2, cx);
            assert_eq!(content(&workbench, cx), Some(ToolWindowContent::Connected));
            let list = window.read_with(cx, |window, _| window.list().clone());
            let rows = list.update(cx, |list, cx| list.rows(cx));
            let shown: Vec<(u32, &str, Option<&str>)> =
                rows.iter().map(|row| (row.number, row.state_icon, row.row.tag)).collect();
            assert_eq!(shown, [(1, "github-pr-open", None), (2, "github-pr-draft", Some("DRAFT"))]);
            assert!(rows[0].row.info.ends_with(", by alice"), "{}", rows[0].row.info);
            assert!(rows[1].row.info.ends_with(", by bob"), "{}", rows[1].row.info);
            let labels: Vec<Vec<String>> = list.read_with(cx, |list, _| {
                list.list().items.iter().map(|pr| pr.labels.nodes.iter().map(|l| l.name.clone()).collect()).collect()
            });
            assert_eq!(labels, [vec!["bug".to_owned()], vec!["docs".to_owned()]]);
            let calls = gh_calls(&root);
            assert!(calls.iter().any(|call| call.starts_with("repo view")), "{calls:?}");
            assert!(calls.iter().any(|c| c.contains("query issueSearch") && c.contains("repo:org/app")), "{calls:?}");
        }

        /// A term typed in the list's search field and applied with Enter asks gh again with it, and
        /// the list shows what that search finds.
        #[gpui_kit::test]
        async fn a_search_term_filters_the_list_through_a_new_request(cx: &mut TestAppContext) {
            let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
            show_list(&workbench, 2, cx);
            click_on(cx, "gh-search-field");
            cx.simulate_input("crash");
            cx.simulate_keystrokes("enter");
            wait_long(cx, "the search answers", |cx| listed(&workbench, cx) == [1]);
            let searches: Vec<String> =
                gh_calls(&root).into_iter().filter(|call| call.contains("query issueSearch")).collect();
            assert!(searches.len() >= 2, "{searches:?}");
            assert!(!searches[0].contains("(crash)") && searches.last().is_some_and(|s| s.contains("(crash)")));
        }

        /// Opening a pull request shows its tab with its title, its branches, the checks' summary and
        /// the files it changes.
        #[gpui_kit::test]
        async fn opening_a_pull_request_shows_its_title_branches_checks_and_changed_files(cx: &mut TestAppContext) {
            let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
            let window = show_list(&workbench, 2, cx);
            let panel = open_first(&workbench, &window, cx);
            let (title, base, head) = panel.read_with(cx, |panel, _| {
                let pr = panel.details().expect("the details loaded");
                (pr.short.title.clone(), pr.base_ref_name.clone(), head_branch_label(pr))
            });
            assert_eq!((title.as_str(), base.as_str(), head.as_str()), ("Fix the crash on start", "main", "fix-crash"));
            let files: Vec<Option<String>> =
                panel.read_with(cx, |panel, _| panel.tree_rows().into_iter().map(|row| row.path).collect());
            assert!(files.contains(&Some("src/main.rs".to_owned())), "{files:?}");
            assert_eq!(panel.update(cx, |panel, cx| panel.commit_label(cx)), "Fix the crash");
            let titles = window.update(cx, |window, cx| window.tab_titles(cx));
            assert_eq!(titles.last().map(String::as_str), Some("#1"), "{titles:?}");
            let active = window.read_with(cx, |window, _| window.tabs().active);
            assert_eq!(active, crate::github::toolwindow::GhTab::PullRequest(1));
        }

        /// Checkout Branch on an open pull request runs `gh pr checkout`, and the branch widget of the
        /// title bar follows to the pull request's branch.
        #[gpui_kit::test]
        async fn checkout_checks_out_the_pull_requests_branch_and_the_branch_widget_follows(cx: &mut TestAppContext) {
            let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
            let window = show_list(&workbench, 2, cx);
            open_first(&workbench, &window, cx);
            assert_eq!(branch_label(&workbench, cx), "main");
            run_action(cx, "github.pullRequest.checkoutBranch", None);
            wait_long(cx, "the branch widget follows", |cx| branch_label(&workbench, cx) == "fix-crash");
            assert_eq!(git_head(&root), "fix-crash");
            assert!(gh_calls(&root).iter().any(|call| call.starts_with("pr checkout 1")));
        }

        /// With gh not signed in, the window says so and offers Run gh auth login.
        #[gpui_kit::test]
        async fn with_gh_not_signed_in_the_window_shows_the_sign_in_state(cx: &mut TestAppContext) {
            let message = "To get started with GitHub CLI, please run:  gh auth login\n".to_owned();
            let (workbench, cx, _folders, _root) =
                github_project(cx, GITHUB_REMOTE, move |_| vec![("repo-view.err", message)]);
            run_action(cx, "github.pullRequests", None);
            wait_long(cx, "gh is checked", |cx| {
                matches!(content(&workbench, cx), Some(ToolWindowContent::Unavailable(_)))
            });
            let Some(ToolWindowContent::Unavailable(shown)) = content(&workbench, cx) else {
                panic!("the window explains why it cannot connect");
            };
            assert_eq!(shown.title, "Not logged in to GitHub");
            assert!(shown.login && !shown.install);
            cx.run_until_parked();
            assert!(cx.debug_bounds("gh-login").is_some(), "Run gh auth login is offered");
        }

        /// A project whose remotes are not on GitHub shows that there is no GitHub repository.
        #[gpui_kit::test]
        async fn without_a_github_remote_the_window_shows_that_state(cx: &mut TestAppContext) {
            let message = "none of the git remotes configured for this repository point to a known GitHub host\n";
            let (workbench, cx, _folders, _root) = github_project(cx, "https://gitlab.com/org/app.git", move |_| {
                vec![("repo-view.err", message.to_owned())]
            });
            run_action(cx, "github.pullRequests", None);
            wait_long(cx, "gh is checked", |cx| {
                matches!(content(&workbench, cx), Some(ToolWindowContent::Unavailable(_)))
            });
            let Some(ToolWindowContent::Unavailable(shown)) = content(&workbench, cx) else {
                panic!("the window explains why it cannot connect");
            };
            assert_eq!(shown.title, "No GitHub repository");
            assert!(!shown.login && !shown.install);
            let repo = workbench.read_with(cx, |w, cx| w.pull_requests_window().map(|v| v.read(cx).repository().clone()));
            assert!(repo.is_some_and(|repo| repo.remotes.is_empty() && repo.connection.is_none()));
        }
    }
```

Some test lines may exceed 120 columns once nested (`fn show_list…`, `fn window_of…`, the `commits` json line, the `repo` line of the last test). `cargo fmt` reflows the code ones; the `json!` lines it may leave, so wrap those by hand if rustfmt does not.

## 5. Assumptions not confirmed by reading (file, what to check)

1. **`gh_call`'s return type** (`github.rs`): `-> impl Future<Output = Result<T, String>> + 'static + use<T, W>`. This copies `branches.rs::host_call`'s shape. Check that `Backend::call`'s future captures only `T`/`W` (`app-gpui/src/backend.rs:206`) and that `tokio::task::spawn_blocking` inside `backend.call(async move { … })` is accepted (same as `services_window.rs::docker_blocking`).
2. **`git::vcs_roots` / `git::vcs_remotes`** (`crates/ide-host/src/ops/vcs/commands.rs:269,996`): `vcs_roots(&Host, &str) -> IdeResult<Vec<VcsRoot>>` is sync, and `vcs_remotes(&Host, &str, Option<PathBuf>) -> IdeResult<Vec<ide_vcs::Remote>>` is async. Check that the `Remote` there is `ide_vcs::Remote`, which `RootRemotes.remotes` holds (the module does `use ide_vcs::*`).
3. **`list_remotes`'s future is `Send`**: it holds `&host` across `.await`, as `branches.rs::read_snapshot` does.
4. **Fields written but never read** (`GithubSlot::{refreshing, connecting, list_fetch}`, `PullRequestTab::{loading, commit_loading}`): they keep `Task`s alive. `branches.rs::VcsSlot::{refreshing, debounce}` and `services_window::poll` do the same and compile under `-D warnings`. If `dead_code` fires anyway, prefix them with `_`.
5. **The two `registry.register(Action::new(…, checkout))` calls** pass the nested `fn checkout` item twice, which works because fn items are `Copy`. Its parameter `call: &ActionCall` has an elided lifetime on `ActionCall<'_>`; if the lint `elided_lifetimes_in_paths` is denied, write `&ActionCall<'_>`.
6. **`map_err(unexpected)`** with `fn unexpected(error: impl Display)`: passing an impl-Trait-argument fn as a value. If inference rejects it, change it to `fn unexpected<E: std::fmt::Display>(error: E)`.
7. **`#[cfg(test)]` on the `if` statement inside `gh_allowed`**: attributes on expression statements are stable for `cfg`. Check that clippy's `needless_return` does not fire in the test build (the `return` is inside an `if`, so it should not).
8. **`ide_core::CancellationToken`** is `tokio_util::sync::CancellationToken` (`crates/ide-core/src/lib.rs:40`), and `::new()` exists.
9. **`DetailsPanel::run_command`, `PrListPanel::run_command` and `PullRequestsWindow::run_command`** are called through `WeakEntity::update(cx: &mut Context<Workbench>, …)`, whose result is ignored with `let _ =` (the same as `welcome_event`'s menu).
10. **`DiffData` for a pull request** (`pull_request_diff`): the sides are `DiffSide::Rev` at the merge base and head (or `oid^` and `oid` for one commit), with absolute paths under `PrDiffRevisions.root`. This assumes `ide_host::ops::vcs::vcs_file_at` resolves an absolute path inside a project repository at any local revision (`diff_window.rs::load_text`). The `context` (`kind: github.pullRequest`, `number`, `path`, `commit`) matches `review_diff::review_diff_context`, but nothing reads it yet: the diff shows no review threads.
11. **`github.pullRequest.checkoutBranch` in the test** runs through `execute_action_or_throw`, so `githubConnected` must already be published. That needs 1.6 applied and a frame drawn after the connect (`push_github_repository` calls `cx.notify()` on the workbench). If the test reports "unavailable", call `cx.run_until_parked()` once more after `open_first`, or drive the window directly.
12. **`click_on(cx, "gh-search-field")` focusing the text field**: it relies on `TextField`'s mouse-down focusing (`ui/text_field.rs:262`) not being taken back by a parent's `track_focus`. This is the view-rules trap. If the search test sees no new request, focus the field through the window's `Focusable` and Tab, or store a last search before connecting instead.
13. **The fake gh's dispatch on the GraphQL body**: it matches substrings of the documents in `github/graphql.rs` as `with_fragments` sends them inside JSON: `query issueSearch`, `...pullRequestMergeability`, `commits(first: `, `files(first: `, `pullRequest(number:`, `...repository`. If `with_fragments` reorders or rewrites spreads, a call falls through to "unexpected call". `calls.log` shows the body.
14. **`ide_github::classify_failure` and gh's real "no GitHub remote" message (hypothesis: a bug in `crates/ide-github/src/model.rs:578`).** gh's actual message (2.x) is "none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use `gh auth login`". The `gh auth login` check comes first, so that message would classify as `NotLoggedIn`, not `NotGitHub`. The test feeds the message without its second sentence. To confirm: `classify_failure("none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use `gh auth login`")` in an `ide-github` unit test.
15. **REST `GET user`** stands for the current user, as the TS `currentUser` does (`src/features/github/api/requests.ts:101`). `findRepository`'s answer is read as `{ repository: GhRepository | null }`.
16. **The REST mutations** follow the TS (`requests.ts:581-596`): `PATCH pulls/N {state}` for Close and Reopen, `POST`/`DELETE pulls/N/requested_reviewers {reviewers, team_reviewers}`. `markPullRequestReadyForReview` (GraphQL) is used for Post. "Set myself as reviewer" posts only the current user. IntelliJ's `setMyselfAsReviewer` should be equivalent; check `src/features/github/details/flow.ts`.
17. **New user-visible strings that are not in the catalogs**: "Cannot check out", "Cannot show the pull request diff", "Reopening pull request…", "Closing pull request…", "Marking pull request ready for review…", "Requesting review…", and the action titles and descriptions (from `index.ts`, so the TS catalog may already have them as `action.<id>`).
