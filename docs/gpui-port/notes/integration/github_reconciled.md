# GitHub Pull Requests: reconciled integration

This file replaces `docs/gpui-port/notes/integration/github_integration.md` and its report `docs/gpui-port/notes/reports/draft_github_wiring.md` for the integration. Apply it **last**, after `plugins_all_integration.md`, `context_menus_reconciled.md`, `editor_all_integration.md`, `app_actions_reconciled.md` and `project_services_terminal_integration.md`. None of their hunks touches a line this file anchors on, and none inserts a line equal to one of these anchors (checked by `anchors.py`, see below). `project_services_terminal_integration.md` must be in: it registers `terminal.runCommand`, which "Run gh auth login" runs in production.

Every anchor is quoted from the file as it is on 2026-11-13 14:10 and occurs exactly once (as a substring and as a whole line). `workbench/mod.rs` is being edited by another agent while this is written, so re-check before pasting: `python3 -I /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/gh/anchors.py <worktree>` recounts every anchor below, checks the pending notes, recounts and re-applies the `ide-github` hunks to a copy, and compares the drafted file with its saved copy. It printed `ALL OK` when this file was written. No line numbers are used here. Run `cargo fmt` after applying.

Nothing here was compiled or run.

## 0. Files

| File                                 | State                                                 | What to do                                                                                                                                                                                                                                                                                                                                                                                 |
| ------------------------------------ | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `app-gpui/src/workbench/github.rs`   | drafted, rewritten by this reconciliation (section 9) | Declare it (1.1). If `git status` shows it unmodified compared with the draft commit `d7b69c6b`, or it differs from the saved copy, copy it from `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/gh/files/app-gpui/src/workbench/github.rs` (byte-identical to the worktree file at the time of writing). |
| `crates/ide-github/src/lib.rs`       | compiled                                              | Section 2 (5 replacements). The fully patched file is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/gh/files/crates/ide-github/src/lib.rs`, built from today's file by `crate_hunks.py`. Copy it only if `git diff crates/ide-github/src/lib.rs` is empty.                                              |
| `crates/ide-github/tests/cli.rs`     | new                                                   | Copy from `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/gh/files/crates/ide-github/tests/cli.rs`. Cargo finds integration tests on its own, so nothing needs declaring.                                                                                                                                 |
| `app-gpui/src/workbench/mod.rs`      | compiled                                              | Section 1 (12 edits).                                                                                                                                                                                                                                                                                                                                                                      |
| `app-gpui/src/workbench/branches.rs` | compiled                                              | Section 3 (2 sites).                                                                                                                                                                                                                                                                                                                                                                       |
| `docs/gpui-port/inventory.mjs`       | script                                                | Section 8 (1 site).                                                                                                                                                                                                                                                                                                                                                                        |

Prerequisites already in place (checked): `app-gpui/src/lib.rs` has `pub mod github;`, `app-gpui/src/github/mod.rs` declares every module `github.rs` uses, and `app-gpui/Cargo.toml` depends on `ide-github` (path) and `ide-vcs`. `ide-host` does **not** depend on `ide-github`, so the seam cannot live on `Host`. No new dependency anywhere.

Hunks: 20 in all: mod.rs 12, branches.rs 2, ide-github lib.rs 5, inventory.mjs 1. The draft had 13 (mod.rs 12, branches.rs 1) plus a test module to paste into `mod.rs`. Compared with it:

- **Rewritten (6), because the code changed shape:**
  - 1.1: the draft anchored after `mod editor_intelligence;`, which `editor_all_integration.md` also inserts after.
  - 1.4a and 1.4b: `TOOL_WINDOWS` is `[&str; 18]` now and ends with `DEBUG_TOOL_WINDOW`, not `HIERARCHY_TOOL_WINDOW`.
  - 1.6a: the mount arm now goes before the agents arm, a one-line anchor, instead of after a four-line block.
  - 1.8a and 1.8b: `self.drop_vcs();` occurs twice, so each site gets its own unique anchor.
- **Unchanged (7):** 1.2, 1.3a, 1.3b, 1.5, 1.6b, 1.7 and 3.2.
- **New (7):**
  - 3.1: `branches.rs` serialises gh's checkout with the window's Git operations (`88677f14`).
  - Section 2 (5 replacements): the per-call gh seam in `ide-github`.
  - Section 8: `inventory.mjs`.
- **Dropped:** the draft's test module pasted into `mod.rs`'s `mod tests`. The tests now live in `github.rs`'s own `mod tests`, as `log_actions.rs` does.

---

## 1. `app-gpui/src/workbench/mod.rs`

### 1.1 The module

Before the line `mod log_actions;` add:

```rust
mod github;
```

### 1.2 The window id

After the line `use structure_window::STRUCTURE_TOOL_WINDOW;` add:

```rust
use crate::github::window_view::PULL_REQUESTS_TOOL_WINDOW;
```

### 1.3 The field and its initial value

(a) In `pub struct Workbench`, after the line `    vcs: Option<branches::VcsSlot>,` add:

```rust
    /// The Pull Requests tool window of the project: the GitHub connection and the opened pull requests.
    github: Option<github::GithubSlot>,
```

(b) In `Workbench::new`'s struct literal, after the line `            services: None,` add:

```rust
            github: None,
```

### 1.4 `TOOL_WINDOWS`

Replace the line `const TOOL_WINDOWS: [&str; 18] = [` with the line below. If another integration has changed the count, add one to whatever it is.

```rust
const TOOL_WINDOWS: [&str; 19] = [
```

and replace

```rust
    DEBUG_TOOL_WINDOW,
];
```

with

```rust
    DEBUG_TOOL_WINDOW,
    PULL_REQUESTS_TOOL_WINDOW,
];
```

`tool_window_declaration("pullRequests")` resolves from `assets/gen/contributions.json`: "Pull Requests", icon `toolwindow-pull-requests`, anchor left, order 2, stripe `onUse`. The existing loop then registers `toolwindow.pullRequests`.

### 1.5 Registration

In `pub fn register_actions(cx: &mut App)`, after the line `    services_window::register_actions(registry);` add:

```rust
    github::register_actions(registry);
```

`github::register_actions` uses `registry.handle(id, …)`: all ten ids it registers are in `assets/gen/actions.json`, so title, category, icon, `when`, `keyWhen` and keywords come from the manifest. The draft copied that metadata by hand and it had already drifted: the keywords of `github.checkoutPullRequest`, `github.createPullRequest` and `github.refresh` differed.

### 1.6 Mounting and visibility

(a) In `fn mount_panels`, before the line `                crate::agents::model::TOOL_WINDOW => match self.mount_agents_panel(window, cx) {` add:

```rust
                PULL_REQUESTS_TOOL_WINDOW => match self.mount_pull_requests(window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

(b) In the same function, after the line `        self.sync_services_window(cx);` add:

```rust
        self.sync_pull_requests_window(cx);
```

### 1.7 Context keys

In `fn publish_context`, after the line `        self.publish_vcs_context(window, cx);` add:

```rust
        self.publish_github_context(window, cx);
```

It publishes `githubConnected` (boolean, the `when` of the pull request actions) and `githubPrTab` (`list` / `pr` / `create`, in the `keyWhen` of `github.refresh` and `github.pullRequest.refresh`).

### 1.8 Close and open project

(a) In `pub fn close_project`, before the line `        cx.global::<Backend>().host.close_project_detached(&self.label);` add:

```rust
        self.drop_github();
```

(b) In `pub fn open_project`, before the line `        self.files = Default::default();` add:

```rust
        self.drop_github();
```

---

## 2. `crates/ide-github/src/lib.rs`: a per-call gh (the seam)

### How gh is located today (read in `crates/ide-github/src/lib.rs`)

`gh_executable()` fills a process-wide `static GH: OnceLock<Option<PathBuf>>` the first time it is called. It takes `OPENIDEA_GH` when that names an existing file, else `which::which("gh")` on `PATH`, else `/opt/homebrew/bin/gh`, `/usr/local/bin/gh`, `/usr/bin/gh` or `/snap/bin/gh` (`C:\Program Files\GitHub CLI\gh.exe` on Windows). Every command (`command()`, `availability()`'s early `NotInstalled`, `account_repositories()`) goes through it. There is no per-call, per-project or per-`Host` setting. So a test could only point it at a fake through `std::env::set_var("OPENIDEA_GH")` before the first lookup anywhere in the process. That is process-wide, races with parallel tests (another test could lock in the real gh first, or run the fake), and is `unsafe` in Rust 2024. A thread-local override would not work either: `availability()` runs gh on two scoped threads.

### The seam

A value `GhCli { exe: Option<PathBuf>, leading: Vec<String> }` carries the program, and its methods do the work:

- `GhCli::located()` is what [`gh_executable`] finds; `GhCli::at(exe)` and `.arg(a)` name a program and its leading arguments; `GhCli::not_installed()` has no program.
- `.availability(root)`, `.api(root, req)` and `.checkout(root, n, cancel, on_output)` are the three calls the native window makes.
- The free functions keep their signatures and delegate to `GhCli::located()`, so the Tauri app (`src-tauri/src/commands/github.rs`) and `account.rs` behave exactly as before.

`leading` lets a test run `sh <script> <gh args>`. A script that is executed directly right after it is written can fail with `ETXTBSY` while another test thread forks.

Apply these five replacements. Each quoted old text occurs exactly once (`crate_hunks.py` checks and applies them).

**2.1** Replace the whole function

```rust
fn command(root: &Path, args: &[&str]) -> IdeResult<Command> {
    let exe =
        gh_executable().ok_or_else(|| IdeError::unavailable("gh executable not found; install the GitHub CLI"))?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(root)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    tracing::debug!(?args, "gh");
    Ok(cmd)
}
```

with

```rust
/// The gh program the calls run: the one this machine has ([`gh_executable`]), or a program the
/// caller names (with leading arguments of its own, e.g. a script and its interpreter). The free
/// functions of this crate run the located one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhCli {
    exe: Option<PathBuf>,
    leading: Vec<String>,
}

impl GhCli {
    /// The gh [`gh_executable`] finds; none when it finds none.
    pub fn located() -> Self {
        Self { exe: gh_executable(), leading: Vec::new() }
    }

    /// The program at `exe`, whatever is installed.
    pub fn at(exe: impl Into<PathBuf>) -> Self {
        Self { exe: Some(exe.into()), leading: Vec::new() }
    }

    /// No gh: every call answers as on a machine without it.
    pub fn not_installed() -> Self {
        Self { exe: None, leading: Vec::new() }
    }

    /// `arg` goes before gh's own arguments on every call.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.leading.push(arg.into());
        self
    }

    fn command(&self, root: &Path, args: &[&str]) -> IdeResult<Command> {
        let exe = self
            .exe
            .as_deref()
            .ok_or_else(|| IdeError::unavailable("gh executable not found; install the GitHub CLI"))?;
        let mut cmd = Command::new(exe);
        cmd.current_dir(root)
            .args(&self.leading)
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("GH_PAGER", "cat")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("GIT_TERMINAL_PROMPT", "0");
        tracing::debug!(?args, "gh");
        Ok(cmd)
    }

    fn run(&self, root: &Path, args: &[&str]) -> IdeResult<String> {
        let out = process::run(self.command(root, args)?, QUICK, None)?;
        if out.success { Ok(out.stdout) } else { Err(failure(&out.stderr, &out.stdout)) }
    }

    fn run_streaming(
        &self,
        root: &Path,
        args: &[&str],
        cancel: &CancellationToken,
        on_output: &dyn Fn(String),
    ) -> IdeResult<String> {
        let mut stdout = String::new();
        let (ok, stderr) = process::stream(self.command(root, args)?, cancel, &mut |stream, line| {
            if stream == process::Stream::Stdout {
                stdout.push_str(&line);
                stdout.push('\n');
            }
            if !line.trim().is_empty() {
                on_output(line);
            }
        })?;
        if ok { Ok(stdout) } else { Err(failure(&stderr.join("\n"), &stdout)) }
    }

    /// Whether gh is installed and signed in, and which GitHub repository `root` maps to.
    pub fn availability(&self, root: &Path) -> GhAvailability {
        if self.exe.is_none() {
            return GhAvailability::NotInstalled;
        }
        let (repo, login) = std::thread::scope(|s| {
            let repo = s.spawn(|| {
                let cmd = self.command(root, &["repo", "view", "--json", REPO_FIELDS])?;
                process::run(cmd, QUICK, None)
            });
            let login = s.spawn(|| self.run(root, &["api", "user", "--jq", ".login"]));
            (repo.join(), login.join())
        });
        let login = login.ok().and_then(Result::ok).map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
        match repo {
            Ok(Ok(out)) if out.success => match parse_repo(&out.stdout) {
                Ok(repo) => GhAvailability::Ready { repo, login },
                Err(e) => GhAvailability::Error { message: format!("unexpected gh output: {e}") },
            },
            Ok(Ok(out)) => classify_failure(if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr }),
            Ok(Err(e)) => GhAvailability::Error { message: e.message },
            Err(_) => GhAvailability::Error { message: "gh check panicked".into() },
        }
    }

    /// `gh pr checkout <n>`: creates / updates a local branch for the pull request and switches to it.
    pub fn checkout(
        &self,
        root: &Path,
        number: u32,
        cancel: &CancellationToken,
        on_output: &dyn Fn(String),
    ) -> IdeResult<()> {
        let n = number.to_string();
        self.run_streaming(root, &["pr", "checkout", &n], cancel, on_output).map(drop)
    }

    /// Runs `req`; returns the response body (JSON text, empty for `204 No Content`).
    pub fn api(&self, root: &Path, req: &GhApiRequest) -> IdeResult<String> {
        let args = api_args(req)?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = process::run_with_input(self.command(root, &refs)?, req.body.as_deref(), QUICK, None)?;
        if out.success { Ok(out.stdout) } else { Err(failure(&out.stderr, &out.stdout)) }
    }
}

fn command(root: &Path, args: &[&str]) -> IdeResult<Command> {
    GhCli::located().command(root, args)
}
```

**2.2** Replace

```rust
fn run(root: &Path, args: &[&str]) -> IdeResult<String> {
    let out = process::run(command(root, args)?, QUICK, None)?;
    if out.success { Ok(out.stdout) } else { Err(failure(&out.stderr, &out.stdout)) }
}

fn run_streaming(
    root: &Path,
    args: &[&str],
    cancel: &CancellationToken,
    on_output: &dyn Fn(String),
) -> IdeResult<String> {
    let mut stdout = String::new();
    let (ok, stderr) = process::stream(command(root, args)?, cancel, &mut |stream, line| {
        if stream == process::Stream::Stdout {
            stdout.push_str(&line);
            stdout.push('\n');
        }
        if !line.trim().is_empty() {
            on_output(line);
        }
    })?;
    if ok { Ok(stdout) } else { Err(failure(&stderr.join("\n"), &stdout)) }
}
```

with

```rust
fn run(root: &Path, args: &[&str]) -> IdeResult<String> {
    GhCli::located().run(root, args)
}

fn run_streaming(
    root: &Path,
    args: &[&str],
    cancel: &CancellationToken,
    on_output: &dyn Fn(String),
) -> IdeResult<String> {
    GhCli::located().run_streaming(root, args, cancel, on_output)
}
```

**2.3** Replace (the doc comment above it stays)

```rust
pub fn availability(root: &Path) -> GhAvailability {
    if gh_executable().is_none() {
        return GhAvailability::NotInstalled;
    }
    let (repo, login) = std::thread::scope(|s| {
        let repo = s.spawn(|| {
            let cmd = command(root, &["repo", "view", "--json", REPO_FIELDS])?;
            process::run(cmd, QUICK, None)
        });
        let login = s.spawn(|| run(root, &["api", "user", "--jq", ".login"]));
        (repo.join(), login.join())
    });
    let login = login.ok().and_then(Result::ok).map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
    match repo {
        Ok(Ok(out)) if out.success => match parse_repo(&out.stdout) {
            Ok(repo) => GhAvailability::Ready { repo, login },
            Err(e) => GhAvailability::Error { message: format!("unexpected gh output: {e}") },
        },
        Ok(Ok(out)) => classify_failure(if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr }),
        Ok(Err(e)) => GhAvailability::Error { message: e.message },
        Err(_) => GhAvailability::Error { message: "gh check panicked".into() },
    }
}
```

with

```rust
pub fn availability(root: &Path) -> GhAvailability {
    GhCli::located().availability(root)
}
```

**2.4** Replace

```rust
pub fn checkout(root: &Path, number: u32, cancel: &CancellationToken, on_output: &dyn Fn(String)) -> IdeResult<()> {
    let n = number.to_string();
    run_streaming(root, &["pr", "checkout", &n], cancel, on_output).map(drop)
}
```

with

```rust
pub fn checkout(root: &Path, number: u32, cancel: &CancellationToken, on_output: &dyn Fn(String)) -> IdeResult<()> {
    GhCli::located().checkout(root, number, cancel, on_output)
}
```

**2.5** Replace

```rust
pub fn api(root: &Path, req: &GhApiRequest) -> IdeResult<String> {
    let args = api_args(req)?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = process::run_with_input(command(root, &refs)?, req.body.as_deref(), QUICK, None)?;
    if out.success { Ok(out.stdout) } else { Err(failure(&out.stderr, &out.stdout)) }
}
```

with

```rust
pub fn api(root: &Path, req: &GhApiRequest) -> IdeResult<String> {
    GhCli::located().api(root, req)
}
```

`crate_hunks.py` holds the same five pairs and applies them to a copy. The crate's tests are the new `crates/ide-github/tests/cli.rs` (unix):

- `a_given_program_answers_with_the_arguments_gh_would_get`: `GhCli::at("/bin/sh").arg(script)` gets `api --method GET user`.
- `a_failing_program_is_classified_like_gh`: "gh auth login" on stderr gives `NotLoggedIn`.
- `without_gh_every_call_says_it_is_not_installed`.

They write into their own folder under the temporary directory and remove it; no real gh runs.

### Natively (in `github.rs`)

- **Production** (`#[cfg(not(test))] fn gh_program`): the window passes `None`, and `GhCli::located()` is resolved inside the blocking task, because the lookup reads the disk. That is the same lookup and cache as the Tauri app.
- **Tests** (`#[cfg(test)]`): the program comes from the gpui global `GhStandIn { cli, started, terminal }`. Each test has its own `App` and sets its own stand-in: `GhCli::at("/bin/sh").arg(<repo>/.git/fake-gh/gh)`. Without a stand-in, `gh_program` fails, so the window runs no gh at all. There is no environment variable and no process-wide state. `started` counts the gh calls the window began, synchronously, on the window's thread.

---

## 3. `app-gpui/src/workbench/branches.rs`

### 3.1 Serialising gh's checkout with the window's Git operations

Commit `88677f14` serialised Git operations and repository reads: the runners set `slot.state.remote_busy`, and a read that a backend event asks for does not start while it is set. `gh pr checkout` runs git too, so it must do the same. `VcsSlot.state` is private to `branches.rs`.

After

```rust
    /// Writes `text` to the window's Git console.
    pub(super) fn log_vcs_console(&mut self, kind: ConsoleKind, text: &str) {
        if let Some(slot) = self.vcs.as_mut() {
            slot.console.log(kind, text, now_ms());
        }
    }
```

add

```rust

    /// Marks a Git operation the window does not run itself (gh's checkout of a pull request) as
    /// running, as the window's own runners do: no other operation starts and no read of the
    /// repository the backend's events ask for begins meanwhile. False (with a warning) while another
    /// one runs; true when the project has no repository to guard.
    pub(super) fn begin_outside_git_operation(&mut self, progress: &str, cx: &mut Context<Self>) -> bool {
        let Some(slot) = self.vcs.as_mut() else { return true };
        if slot.state.remote_busy.is_some() {
            let warning = vcs_notice(NotificationLevel::Warning, "Another Git operation is in progress");
            self.notify(warning, cx);
            return false;
        }
        slot.state.remote_busy = Some(progress.to_owned());
        true
    }

    /// The operation [`Self::begin_outside_git_operation`] marked has ended.
    pub(super) fn end_outside_git_operation(&mut self) {
        if let Some(slot) = self.vcs.as_mut() {
            slot.state.remote_busy = None;
        }
    }
```

`run_checkout` in `github.rs` follows `run_git_operation`'s order:

1. Mark the operation as running, with the progress text "Checking out #N…".
2. `save_all`.
3. `refresh`: a read still running would hold `index.lock`.
4. Run gh.
5. `refresh` again.
6. Clear the mark.
7. Notify as the webview does (`src/features/github/data/local.ts:70-85`): "Checked out pull request #N" (info, key `github-checkout`, group `vcs`), or "Couldn't check out #N" (error, key `github-op`, gh's message as the body, credentials masked).

### 3.2 Open tabs follow checkouts made elsewhere (needed by: compilation of nothing; recommended)

In `fn vcs_state_changed`, replace

```rust
        self.sync_log_panes(cx);
        cx.notify();
```

with

```rust
        self.sync_log_panes(cx);
        self.github_vcs_changed(cx);
        cx.notify();
```

---

## 4. Menus and context menus

- The Git › GitHub entries are already in `app-gpui/src/workbench/menu_spec.rs` (View Pull Requests, Create Pull Request…, Checkout Pull Request…, Open on GitHub). No hunk is needed.
- `github.createPullRequest` stays unregistered, so its entry shows as the menu shows any unavailable action.
- The window's own menus (tab menu, list rows, details) are drawn by the views and carried out through `Workbench::show_menu`, as before.

## 5. "Run gh auth login"

**In the webview** (`src/features/github/toolwindow/PullRequestsToolWindow.tsx:54-78`): in the "Not logged in to GitHub" state, the primary button runs `executeAction("terminal.runCommand", { program: "gh", args: ["auth", "login"], name: "gh auth login" })`. The Terminal's action (`src/features/terminal/index.ts:118-135`) opens a new tab at the project root running that program instead of the shell, then shows and focuses the Terminal. The terminal looks up `gh` on its own `PATH`, not through `OPENIDEA_GH`.

**Natively:** `PullRequestsWindow` emits `PullRequestsEvent::RunAction { id: "terminal.runCommand", arg }` with the same argument (`github/window_view.rs`, its `gh-login` button). The workbench routes that id to `run_in_terminal`:

- **Production:** `run_action_later("terminal.runCommand", arg)`. That action is registered by `terminal_actions.rs` (pending `project_services_terminal_integration.md`), and it opens a tab running `gh auth login` exactly as the webview does.
- **Test builds:** the stand-in records `arg` in `GhStandIn.terminal`, and nothing starts. Without a stand-in, the request is logged and dropped.

The draft's fallback, which opened the Terminal and logged `?arg` when the action was missing, is gone. The only test that clicks `gh-login` (`with_gh_not_signed_in_…`) asserts:

- the recorded request is exactly the webview's argument;
- the Terminal tool window did not open.

No test can start an interactive login.

## 6. Tokens

- **Measured** (grep in this session, no match): nothing in `crates/ide-github`, `src-tauri/src/commands/github.rs`, `src/features/github` or `app-gpui/src/github` reads a token, calls `gh auth token`, or touches a keychain, `GH_TOKEN`, `GITHUB_TOKEN` or `hosts.yml`. The pattern was `auth token|GH_TOKEN|GITHUB_TOKEN|keychain|keyring|hosts\.yml|oauth_token`. Authentication is gh's own: every call is a `gh` subprocess, and the IDE never holds a credential.
- **Hypothesis** (not checkable here: no gh, no network): gh keeps the token in the OS keyring, or in `~/.config/gh/hosts.yml`, and `GH_TOKEN` / `GITHUB_TOKEN` in the environment override it. The fake gh `unset`s those variables first anyway.
- **The real leak path** was a remote URL carrying a credential (`https://user:token@github.com/…`). It flowed through `list_remotes` into `RootRemotes`, `GhRemote.url`, `RepositoryState` and `GhConnection`. All of these derive `Debug`, and the window copies them. It could also reach gh's and git's error text, which became notification bodies and log lines.

**Fixed in `github.rs`:**

- `list_remotes` masks every remote URL with `crate::vcs::git_forms::redacted_url` (from 88677f14) before anything keeps it: `ada:***@`, or `***@` for a bare HTTP(S) user.
- `GITHUB_REMOTE`'s `(?:[^@/]+@)?` and the path-suffix matches (`remote_of_repository`, `ide_github::remote_for`) still parse the masked URL. A plain test checks this.
- Nothing native needs the credential: gh signs in with its own account, and git reads the repository's real config.
- `redacted_text` (every word, between spaces, quotes and brackets, masked as a URL, so `'https://TOKEN@…'` inside a git error is caught) is applied to:
  - the checkout failure body;
  - the diff failure body;
  - the review-flow failure bodies (`pull_request_mutated`);
  - the `tracing` lines of checkout and diff output;
  - the remotes-listing warning and the load warnings.
- No field that holds a credential exists. `GhStandIn` does not derive `Debug`. `GhCli` derives `Debug` and holds only a path and arguments.

The tests:

- `a_token_in_the_remote_url_is_never_kept_shown_copied_or_notified` (end to end): the remote is `https://ada:<SENTINEL>@github.com/org/app.git`. It connects, lists and opens #1, copies the URL, then formats with `{:?}`: the workbench's `RepositoryState`, the window's repository, selector options and tabs, `content()`, the tab titles, every notification, and the clipboard. It asserts that the sentinel appears in none of them.
- `remotes_keep_no_credentials_and_still_name_their_repository` (plain test): `without_credentials`, the repository names `github_remotes` still finds, and `redacted_text` of a git error line.

---

## 7. Actions, keys, context keys

**Registered** (each run through the registry by an end-to-end test on the fake gh, ids and conditions from the manifest): `github.pullRequests`, `github.refresh`, `github.changeRepository`, `github.checkoutPullRequest`, `github.openInBrowser`, `github.pullRequest.show`, `github.pullRequest.refresh`, `github.pullRequest.checkoutBranch`, `github.pullRequest.openInBrowser`, `github.pullRequest.copyUrl`.

**Behaviour now matching `src/features/github/runtime.ts`:**

- `github.refresh` checks the connection again, then reloads the list.
- `github.checkoutPullRequest` shows the window first.
- An action's argument is a pull request number only.
- `github.openInBrowser` opens the active pull request's page, else the repository's page.

**Removed from the draft's registrations:**

- `github.createPullRequest`: there is no New Pull Request view.
- `github.selectOpenedPullRequest`: its `when` needs `githubTimelineActive`, which nothing native publishes, so no test can run it.

**Keys** (`src/platform/keymaps/intellij-*.ts`): `github.refresh` and `github.pullRequest.refresh` are on ⌘R (macOS) and Ctrl+F5 (Windows). `the_refresh_keys_work_only_in_the_pull_requests_window_on_their_tab` tests both directions:

- In the list tab: the list refreshes.
- With no tool window active: nothing happens. No check starts and `started` is unchanged, both read synchronously.
- In the `#1` tab: exactly 5 gh calls start (the pull request's details, status, commits, changes and viewed states), and the list's refresh does not run.
- Elsewhere again: nothing happens.

Not tested and not registered: ⌘N `github.createPullRequest` and ⌘⇧S `github.pullRequest.toggleViewed`.

**Context keys:**

- `githubConnected` (boolean) is read **off** before the window is shown and after Change Repository disconnects, and **on** after connecting and after reconnecting.
- `githubPrTab` (text) is read as `list`, then `pr`.
- `githubTimelineActive` is not published natively; the actions that need it are not registered.

---

## 8. `docs/gpui-port/inventory.mjs`: list the 18 actions

`inventory.mjs` builds the Actions rows from `tsmodel.mjs actions()`, which keeps an object literal only when it has both `id` and a literal `run` (`tsmodel.mjs:193`). `src/features/github/index.ts` declares 21 action specs, and only 3 of them have a literal `run`: `showInToolWindow`, `updateBranch` and `reviewMode`. The other 18 get theirs from `registerActions(actions().map((a) => ({ ...a, run: a.run ?? … })))`. `gen-data.mjs actionManifest()` follows that wrapper (`wrapperOf`/`wrapped`), so `actions.json` has all of them.

Measured, against the committed `INVENTORY.md`, in Python: the manifest's ids minus the inventory's Actions rows are exactly these 18 `github.*` ids. The other difference runs the other way: 18 `(dynamic)` rows that are only in the inventory.

In `docs/gpui-port/inventory.mjs`, replace the line

```js
const actionMap = actions();
```

with

```js
const actionMap = actions();
// Specs a wrapper completes (`registerActions(specs.map((a) => ({ ...a, run })))`) carry no literal
// `run`; the generated manifest resolves them, so its ids fill the gaps.
const generated = JSON.parse(readFileSync(join(root, "app-gpui/assets/gen/actions.json"), "utf8"));
for (const { id, source } of generated.actions) if (!actionMap.has(id)) actionMap.set(id, source);
```

`readFileSync`, `join` and `root` are already imported. Its effect (not run, since node is not allowed here):

- The Actions section grows from 763 to **781** rows (+18).
- Five ids leave "Keymap ids with no statically registered action" (51 → 46): `github.createPullRequest`, `github.refresh`, `github.pullRequest.refresh`, `github.pullRequest.timeline.refresh` and `github.pullRequest.toggleViewed`.
- The inventory total grows by 13.

No `parity.json` key names those five `boundid:` rows, so the "unknown row" check stays quiet. Run `node docs/gpui-port/gen-data.mjs` first if `actions.json` is stale.

---

## 9. Edits made to the drafted `app-gpui/src/workbench/github.rs`

1. **The gh seam:**
   - `TEST_GH` and `gh_allowed` are replaced by `GhStandIn` (a test-only gpui global) and `gh_program`.
   - `gh_call` now takes `W: FnOnce(&GhCli) -> Result<T, String>`.
   - Every helper that runs gh takes `cli: &GhCli` and calls `cli.api` / `cli.availability` / `cli.checkout` instead of the free functions.
   - `pull_request_pages` takes `(repo, number)` as a pair, to keep six parameters.
2. **Login:** `RunAction { id: "terminal.runCommand" }` goes to `run_in_terminal` (production: the action; test: recorded), and the old fallback is removed. `TERMINAL_TOOL_WINDOW` is no longer imported by the module.
3. **Credentials:**
   - `without_credentials` is applied in `list_remotes`.
   - `redacted_text` is applied to failure bodies and log lines.
   - `failure()` takes `&str`, sets group `vcs` and key `github-op`, and masks.
   - `pull_request_mutated` masks the error.
4. **Actions:**
   - All ten use `registry.handle` (manifest metadata). The `meta` helper and the hand-written `ActionMeta`s are removed.
   - `github.createPullRequest` and `github.selectOpenedPullRequest` are removed.
   - `github.openInBrowser` is added (`open_github_in_browser`).
   - `github.checkoutPullRequest` shows the window first.
   - `github.refresh` goes through `refresh_pull_request_list`: a new `GithubSlot.reload_on_connect` flag makes the list reload after the connect, and it is cleared on disconnect or failure.
   - `number_arg` takes a number only, as `targetPullRequest` does.
5. **Checkout:** `run_checkout` follows section 3.1 and uses the webview's catalog strings, through `tx_built`: "Checking out #{number}…", "Checked out pull request #{number}" and "Couldn't check out #{number}". The draft's "Cannot check out" string is gone.
6. **New accessor** `Workbench::github_repository()`, for the `{:?}` test.
7. **`unexpected` is generic** (`fn unexpected<E: Display>(error: E)`), which removes the draft's risk of passing an `impl Trait`-argument function as a value. Lines are wrapped to at most 120 columns.
8. **Tests are in `github.rs`'s own `#[cfg(all(test, unix))] mod tests`.** It uses `super::super::tests::{branch_label, click_on, git_head, git_in, git_project, init_repo, notification_titles, run_action, wait_for_notice, wait_long}` and local `context_on` / `context_text` helpers (`mod.rs`'s are private).
   - **The remote** is a local bare repository at `<repo>/.git/fake-github/org/app.git` (`main` pushed to it). `origin` is that path, rewritten to look like the GitHub URL by `url.<GitHub URL>.insteadOf <bare path>`. This is set in the test repository's config only, together with `protocol.allow never` and `protocol.file.allow always`, so git can reach local files only and never the network (assumption 1).
   - **The fake gh** is `<repo>/.git/fake-gh/gh`, run as `sh <file>`. It finds its answers beside itself (`dirname "$0"`), logs each call to `calls.log`, unsets the token variables, answers only the subcommands the tests use, and exits 1 with "fake gh: unexpected call" on anything else.
   - **The tests** (15: 14 end to end plus 1 plain): the 6 from the draft (two renamed, with more asserted), plus refresh list, refresh pull request, the keys, checkout by number (failure and success), browser and copy, change repository (`githubConnected` on and off), gh not installed (now provable: `GhCli::not_installed()`), and the token test with its plain companion.

Tests wait with `wait_long` / `wait_for_notice`. The synchronous "does nothing" assertions follow `run_until_parked`, and there are no fixed sleeps.

---

## 10. Assumptions to check, in order

1. **`git remote -v` shows the `insteadOf` form.** `github_project` adds `origin` as the bare path and sets `url.<GitHub URL>.insteadOf <bare path>`. Hypothesis: `git remote -v` (which `ide_vcs::Repo::remotes` parses, `(fetch)` lines only) prints URLs after `insteadOf` rewriting, so the IDE sees the GitHub URL (`git remote get-url`'s documentation says it expands `insteadOf`; not checked here).
   - Two assertions fail loudly if this is wrong. `the_window_connects_…` expects the window's remotes to be exactly `[("origin", "org/app")]`. `a_token_in_the_remote_url_…` expects the kept state to contain `https://ada:***@github.com/org/app.git`.
   - If they fail: rewrite the other way round in `github_project`, with `remote add origin <GitHub URL>` and `url.<bare>.insteadOf <GitHub URL>`. That is the two lines in `github_project`, swapping `remote`/`bare`. Then a fetch goes to the bare repository, and `remote -v` shows the GitHub URL only if it does not rewrite.
   - Do not rely on `availability_answered`'s fallback: with no GitHub remote found, `probe` is `None` while every listed root is `Some`, so the fallback remote has an empty name.
   - Check: `git -c "url.https://github.com/org/app.git.insteadOf=/tmp/x/org/app.git" remote -v` in a scratch repository whose `origin` is `/tmp/x/org/app.git`.
   - In this direction a fetch would go to github.com. `protocol.allow never` (with `protocol.file.allow always`) in the test repository's config refuses it before any connection, and no test fetches.
2. **`publish_context` runs after a `cx.notify()`** in the test window, so `githubConnected` reaches the context once a frame is drawn. `show_list` waits for it with `wait_long`.
3. **⌘R / Ctrl+F5 dispatch picks the binding whose `when` and `keyWhen` hold** among the many actions on that key (`run.rerun`, `debug.rerun`, `find.rerun`, `find.replaceInFile`, `db.refresh`, …), and a keystroke with nothing focused still goes through the IDE keymap. Also, `window.blur` leaves `activeToolWindow` unset: `blur()` asserts it.
4. **`PullRequestsWindow::focus_handle`** (the list's handle on the list tab, the window's own on a `#N` tab) is the handle `mount_panels`' `Panel` reports, so focusing it publishes `activeToolWindow == 'pullRequests'`. `focus_window()` asserts it.
5. **The fake's dispatch on GraphQL bodies.** It matches substrings of the `github/graphql.rs` documents as `with_fragments` sends them inside JSON: `query issueSearch`, `...pullRequestMergeability`, `commits(first: `, `files(first: `, `pullRequest(number:` and `...repository`. Checked by reading: no fragment holds an earlier pattern. A miss shows in `calls.log` as "unexpected call".
6. **`gh pr checkout` in the fake** runs `git checkout -q -b fix-crash` in the repository while the window may be reading it. 3.1's serialisation, plus the refresh before gh, should keep `index.lock` free.
7. **`github.checkoutPullRequest` with 2** fails because the fake has no `checkout-2` answer. The failure text is the fake's `cat` error, classified `internal`, and the window says "Couldn't check out #2".
8. **The `tx_built` titles** come out as "Checked out pull request #1" in English: `js_number(1.0)` is `1`, and there is no English shape override.
9. **`GhCli` resolution inside `spawn_blocking`.** `GhCli::located()` locks the same `OnceLock` as the Tauri path, and is never reached in tests.
10. **`ide_github::classify_failure` with gh's real "no GitHub remote" message.** Hypothesis carried over from the draft: if the message ends with "please use `gh auth login`", it classifies as `NotLoggedIn` (that check comes first). The webview has the same behaviour, so parity holds. The test feeds only the first sentence. Check: a unit test in `ide-github` on the full message.

## 11. Compile risks

- `PullRequestsEvent::RunAction { id: TERMINAL_RUN_COMMAND, arg }` matches a `&'static str` field against a `const &str` (allowed in patterns), before the general `RunAction` arm.
- `gh_call`'s `impl Future + 'static + use<T, W>`: the closure now captures `program: Result<Option<GhCli>, String>`, which is `Send`. Closures passed as `move |cli| …` and `move |_| …` take their `&GhCli` signature from the `W` bound.
- `run_in_terminal(arg, window, cx)` is called with `cx: &mut Context<Workbench>` where `&mut App` is expected (deref coercion). The test version's unused `_: &mut Window` is fine.
- `#[cfg(test)] impl gpui_kit::Global for GhStandIn {}`: `Global` is assumed re-exported at the crate root, as `platform/actions.rs` uses it.
- `Remote { url: redacted_url(&remote.url).into_owned(), ..remote }`: the borrow ends before the base moves.
- `others.iter().any(|o| c.contains(o))` with `o: &&str` relies on `Pattern for &&str`; write `c.contains(*o)` if it is refused.
- Test helpers:
  - `cx.read_from_clipboard()` on `VisualTestContext` (deref to `TestAppContext`) and `window.blur(cx)` (gpui-pre 0.3.12 `Window::blur(&mut self, cx: &mut App)`), both checked.
  - `use gpui_kit::Focusable` for `view.focus_handle(cx)`.
- Write-only `Task` fields (`GithubSlot::{refreshing, connecting, list_fetch}`, `PullRequestTab::{loading, commit_loading}`) keep their tasks alive. `branches.rs`' `VcsSlot::{refreshing, debounce}` are likewise only assigned and compile today under `-D warnings`, which is the precedent. If `dead_code` fires anyway, prefix the five with `_`.
- In `ide-github`, the free `run`, `run_streaming` and `command` are still used (by `list`, `view`, `review_comments`, `create`, `submit_review`, `add_comment` and `account.rs`), so there are no dead-code warnings.
- In the draft-era model calls (unchanged, written against the same `app-gpui/src/github` files, none changed since `d7b69c6b` except `platform/actions.rs` by one line), the riskiest are:
  - `Connection { page_info, nodes }` destructuring (fields assumed public);
  - `failed.key.to_owned()`;
  - `ChooserSource::of(chip, permission)` with `permission: Option<Permission>`.

## 12. Tests most likely to fail first, and why

1. `the_refresh_keys_work_only_in_the_pull_requests_window_on_their_tab`: assumptions 3 and 4 (the key dispatch among many ⌘R actions, focus and `activeToolWindow`). The exact `+5` also assumes nothing else starts a gh call when the tab gets the keyboard.
2. `a_search_term_filters_the_list_through_a_new_request`: `click_on("gh-search-field")` must leave the text field focused (the parent `track_focus` trap from the view rules).
3. Every test, on its first run, through `github_project`: `git push` to the bare repository, then `protocol.allow never`, then `insteadOf` (assumption 1). If `remote -v` does not rewrite, every connecting test shows "No GitHub repository" or connects through the fallback, and the two positive remote assertions fail first.
4. `checkout_*`: the `index.lock` timing (assumption 6), and `wait_for_notice` on the exact catalog text (assumption 8).
5. `the_window_connects_…`: the exact row icons, tags and `info` suffix, carried over from the draft.
6. Compiled tests that could notice the new tool window or actions: a grep of `app-gpui/src` (outside `github/`, `workbench/github.rs` and `menu_spec.rs`) for `"github.`, `TOOL_WINDOWS` and `pullRequests` finds only:
   - `platform/keymap/tests/proofs_presets.rs`, which works on manifest bindings and is unchanged by registration;
   - `run_configs.rs`' tool-window list for Edit Configurations (gains "Pull Requests", as the webview's does);
   - `menu.rs`' View › Tool Windows (gains "Pull Requests").
     No test asserts an exact list there. A layout or stripe snapshot test could see the new left-side window, but its stripe is `onUse`, so it stays hidden until it is used.

## 13. Action → test → IPC → status

| Action id                                                                                                | End-to-end test (`workbench::github::tests::…`)                                                                                              | IPC commands it stands for (`src-tauri/src/commands/` adapter → backend function)                                                                                                                                                                    | Status                                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `github.pullRequests`                                                                                    | `the_window_connects_and_lists_two_pull_requests_with_their_state_author_and_labels`, `a_search_term_filters_the_list_through_a_new_request` | `vcs.rs::vcs_roots` → `ide_host::ops::vcs::vcs_roots`; `vcs.rs::vcs_remotes` → `ops::vcs::vcs_remotes`; `github.rs::gh_availability` → `ide_github::availability`; `github.rs::gh_api` → `ide_github::api` (findRepository, `GET user`, issueSearch) | partial: connect, list, search and choosers' REST lists are wired. Next pages of the list (scrolling) and the chooser popups are not tested end to end.                                            |
| `github.refresh`                                                                                         | `refresh_list_checks_gh_again_and_asks_for_the_list_again`, `the_refresh_keys_…`                                                             | as above                                                                                                                                                                                                                                             | done                                                                                                                                                                                               |
| `github.changeRepository`                                                                                | `change_repository_disconnects_then_connects_again`                                                                                          | as above                                                                                                                                                                                                                                             | done (with one GitHub remote; the selector with several remotes is tested only at view level)                                                                                                      |
| `github.pullRequest.show`                                                                                | `view_pull_request_opens_its_tab_with_its_title_branches_checks_and_changed_files`                                                           | `github.rs::gh_api` → `ide_github::api` (findPullRequest, findPullRequestMergeability, pullRequestCommits, REST `pulls/N/files`, pullRequestFiles)                                                                                                   | partial: details, status and checks, commits, changes and viewed states are shown. Missing: the timeline editor (the webview opens it unfocused), review threads and comments, the pending review. |
| `github.pullRequest.refresh`                                                                             | `refresh_pull_request_loads_the_open_pull_request_again`, `the_refresh_keys_…`                                                               | as above                                                                                                                                                                                                                                             | done (same missing parts as `show`)                                                                                                                                                                |
| `github.pullRequest.checkoutBranch`                                                                      | `checkout_branch_checks_out_the_pull_requests_branch_and_the_branch_widget_follows`                                                          | `github.rs::gh_pr_checkout` → `ide_github::checkout`, then `VcsChanged` (native: `refresh`)                                                                                                                                                          | done                                                                                                                                                                                               |
| `github.checkoutPullRequest`                                                                             | `checkout_pull_request_checks_out_the_number_it_is_given`                                                                                    | `github.rs::gh_pr_checkout` → `ide_github::checkout`                                                                                                                                                                                                 | partial: without a number or a selected pull request, the webview asks for one (`numberDialog`); natively it logs                                                                                  |
| `github.pullRequest.openInBrowser`                                                                       | `the_browser_and_copy_actions_use_the_pull_requests_page`                                                                                    | none for a loaded or listed pull request (`openExternalUrl`); the webview loads the details (`gh_api` findPullRequest) when neither is known                                                                                                         | partial: natively the URL is built from the repository instead of being loaded                                                                                                                     |
| `github.pullRequest.copyUrl`                                                                             | `the_browser_and_copy_actions_use_the_pull_requests_page`                                                                                    | as above (clipboard)                                                                                                                                                                                                                                 | partial: as above                                                                                                                                                                                  |
| `github.openInBrowser`                                                                                   | `the_browser_and_copy_actions_use_the_pull_requests_page`                                                                                    | none (`openExternalUrl`)                                                                                                                                                                                                                             | partial: the repository-page case (no pull request in context) is not tested                                                                                                                       |
| `github.createPullRequest`                                                                               | — (not registered)                                                                                                                           | `github.rs::gh_pr_create` → `ide_github::create`                                                                                                                                                                                                     | missing: New Pull Request view                                                                                                                                                                     |
| `github.selectOpenedPullRequest`                                                                         | —                                                                                                                                            | —                                                                                                                                                                                                                                                    | missing: needs the timeline editor (`githubTimelineActive`)                                                                                                                                        |
| `github.pullRequest.showTimeline`, `github.pullRequest.timeline.refresh`                                 | —                                                                                                                                            | `gh_api` (timeline documents)                                                                                                                                                                                                                        | missing: timeline editor                                                                                                                                                                           |
| `github.pullRequest.openInWorktree`                                                                      | —                                                                                                                                            | `github.rs::gh_pr_prepare_diff` → `ide_github::prepare_diff`; `vcs.rs::vcs_add_worktree`                                                                                                                                                             | missing                                                                                                                                                                                            |
| `github.pullRequest.deleteMergedBranch`                                                                  | —                                                                                                                                            | `gh_api` (deleteRef)                                                                                                                                                                                                                                 | missing                                                                                                                                                                                            |
| `github.pullRequest.submitReview`                                                                        | —                                                                                                                                            | `gh_api` (review mutations)                                                                                                                                                                                                                          | missing: Submit Review popup                                                                                                                                                                       |
| `github.pullRequest.toggleViewed`                                                                        | —                                                                                                                                            | `gh_api` (markFileAsViewed / unmark)                                                                                                                                                                                                                 | missing                                                                                                                                                                                            |
| `github.pullRequest.reviewMode`, `.showInToolWindow`, `.updateBranch`, `.reviewMode.showDiff`, `.diff.*` | —                                                                                                                                            | `gh_api`, `gh_pr_prepare_diff`                                                                                                                                                                                                                       | missing: review mode and review in the diff (models exist in `github/review_mode.rs`, `review_diff.rs`)                                                                                            |
| (details panel) Open diff of a changed file                                                              | not tested end to end                                                                                                                        | `github.rs::gh_pr_prepare_diff` → `ide_github::prepare_diff`                                                                                                                                                                                         | partial: wired (`open_pull_request_diff`); no test fetches `refs/pull/N/head` from the bare remote yet                                                                                             |
| (details panel) Close / Reopen / Ready for review / Set myself as reviewer / adjust reviewers            | not tested end to end                                                                                                                        | `gh_api` (REST `PATCH pulls/N`, `POST`/`DELETE requested_reviewers`, markPullRequestReadyForReview)                                                                                                                                                  | partial: wired, untested; merge dialog, request-review picker and resolve-conflicts are not built                                                                                                  |
| (clone source) Get from VCS › GitHub                                                                     | —                                                                                                                                            | `github.rs::gh_account_repos` → `ide_github::account_repositories`                                                                                                                                                                                   | not part of this file (model `github/clone.rs`)                                                                                                                                                    |

There is no gists feature in `src/features/github`.
