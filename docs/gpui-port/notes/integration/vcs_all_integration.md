# Git wiring: one integration in three stages

This file replaces `docs/gpui-port/notes/integration/{vcs_operations,shelf,log_actions}_integration.md` for the integration. Apply the stages in order: 1 (repository operations), 2 (shelf, change lists, patches, commit checks), 3 (Git log actions). Build and run the stage's tests after each stage; each stage is meant to compile without warnings on its own.

Every anchor is quoted from the file as it is on 2026-11-13, or as the previous stage leaves it (said so where it matters). No line numbers: `workbench/mod.rs` is being edited by another agent. Check an anchor with `grep -nF` before pasting. Run `cargo fmt` after each stage. "Needed by" says which stage's tests (or compilation) depend on the hunk.

Rules every stage follows:

- **Who reads the repository again.** The three runners in `branches.rs` (`run_op`, `run_remote`, `run_git_operation`) call `refresh` before they report; callers never refresh after them. The Git log follows HEAD and branch tips through the backend's `VcsChanged` event (`vcs/log_view.rs`, `fn backend_event`). Explicit `reload_log` (stage 1, `vcs_operations.rs`) only where the log can miss a change: a new tag (the source does the same, `src/features/vcs/operations.tsx:430` and `:458`), and after the log's own operations and history rewrites (stage 3).
- **One operation runner.** `branches::run_git_operation(this, cx, GitRun { … })` is the only runner of `GitOperation`s (continue/skip/abort, reset, cherry-pick, revert, interactive rebase). It is modelled on `runGitOperation` (`src/features/vcs/ops.ts:386-460`): progress notice keyed `vcs.operation`, console lines, a stop on conflicts opening Resolve Conflicts instead of an error when asked, a "stopped for editing" text for a rebase that stops. Undo buttons are added by stage 3 by telling the same keyed notice again with its actions.
- **One dialog per kind.** Git Reset is `git_forms::ResetForm` (two layouts: Reset HEAD… with "To commit:", the log's Reset Current Branch to Here… with the commit line), New Tag is `git_forms::TagForm` (two layouts: Git › New Tag… with Force/Commit/Message, the log's name-only "Create New Tag On …"). Both are drawn by `GitFormDialog`. **Do not declare `vcs/log_reset_dialog.rs` nor `vcs/log_tag_dialog.rs`**: they stay on disk, unused.

---

## Stage 1: repository operations

Files to declare: `vcs/git_forms.rs`, `vcs/git_form_dialog.rs`, `workbench/vcs_operations.rs`.

Hunks: 20 edit sites (1.1: 1, 1.2: 4, 1.3: 4, 1.4: 2, 1.5: 3, 1.6: 6), in `vcs/mod.rs`, `vcs/prompt_dialogs.rs`, `workbench/branches.rs`, `workbench/commit.rs`, `workbench/mod.rs`.

### 1.1 `app-gpui/src/vcs/mod.rs` (needed by: compilation)

After `pub mod conflicts_dialog;` add:

```rust
pub mod git_form_dialog;
pub mod git_forms;
```

### 1.2 `app-gpui/src/vcs/prompt_dialogs.rs`: drawing helpers for the `vcs` module (needed by: compilation of `git_form_dialog.rs`; stage 2 uses them too)

`pub(super)` is enough: every user is a sibling in `crate::vcs`. Prefix each with `pub(super) `:

- `fn code_font(cx: &App) -> SharedString {`
- `fn checkbox(cx: &App, label: String, on: bool) -> Div {`
- `fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {`
- `fn field_frame(cx: &App, invalid: bool) -> Div {`

### 1.3 `app-gpui/src/workbench/branches.rs`: visibility (needed by: compilation of `vcs_operations.rs`)

Prefix with `pub(super) `:

- `fn i18n<R>(this: &This, cx: &mut AsyncWindowContext, f: impl FnOnce(&mut I18n) -> R) -> anyhow::Result<R> {`
- `async fn run_remote(`
- `async fn ask(this: &This, cx: &mut AsyncWindowContext, request: AskRequest) -> anyhow::Result<DialogAnswer> {`
- `async fn confirm(this: &This, cx: &mut AsyncWindowContext, request: ConfirmRequest) -> anyhow::Result<bool> {`

### 1.4 `app-gpui/src/workbench/branches.rs`: the final operation runner (needed by: stage 1 tests `reset_head_moves_the_branch_in_each_mode`, `a_stopped_*`; stage 3 compilation and tests)

Replace the whole function that starts with

```rust
/// A local Git operation that may take a while (continuing or aborting a stopped one): its progress
/// shows until it ends, then what it did or why it failed. Only one runs at a time.
async fn run_git_operation(
```

and ends with its closing `}` before `/// Continue, Skip or Abort of the operation `expected` stopped in the selected repository.`, with:

```rust
/// Key of the notification of a long Git operation: its progress, then what came of it.
pub(super) const OPERATION_KEY: &str = "vcs.operation";

/// A long local Git operation and what it says while it runs and after.
pub(super) struct GitRun {
    pub(super) op: GitOperation,
    /// Shown while it runs (English source text).
    pub(super) progress: &'static str,
    pub(super) success: String,
    /// What a rebase that stops for editing reports instead of `success`.
    pub(super) stopped: Option<String>,
    pub(super) failure: String,
    /// A failure that left conflicts opens them for resolving instead of an error notice.
    pub(super) conflicts_instead: bool,
}

/// A local Git operation that may take a while: its progress shows until it ends, then what it did
/// or why it failed. Only one runs at a time. Whether it succeeded.
pub(super) async fn run_git_operation(this: &This, cx: &mut AsyncWindowContext, run: GitRun) -> anyhow::Result<bool> {
    let GitRun { op, progress, success, stopped, failure, conflicts_instead } = run;
    let started = this.update(cx, |workbench, cx| {
        let slot = workbench.vcs.as_mut()?;
        if slot.state.remote_busy.is_some() {
            let warning = vcs_notice(NotificationLevel::Warning, "Another Git operation is in progress");
            workbench.notify(warning, cx);
            return None;
        }
        Some((slot.state.repo.clone(), slot.state.branch.as_ref().and_then(|branch| branch.operation.clone())))
    })?;
    let Some((repo, before)) = started else { return Ok(false) };
    save_all(this, cx).await?;
    this.update(cx, |workbench, cx| {
        let Some(slot) = workbench.vcs.as_mut() else { return };
        let progress = cx.default_global::<Localization>().0.tx(progress);
        slot.state.remote_busy = Some(progress.clone());
        slot.console.log(ConsoleKind::Cmd, &format!("git {}", describe_operation(&op, before.as_deref())), now_ms());
        let mut notice = vcs_notice(NotificationLevel::Info, format!("{progress}…"));
        notice.key = Some(OPERATION_KEY.to_owned());
        notice.sticky = Some(true);
        workbench.notify(notice, cx);
    })?;
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = Arc::clone(&lines);
    let path = repo_path(repo.as_deref());
    let ran = call(this, cx, move |host, label| async move {
        let channel = Channel::new(move |line: String| match sink.lock() {
            Ok(mut lines) => lines.push(line),
            Err(poisoned) => poisoned.into_inner().push(line),
        });
        git::vcs_operation(&host, &label, path, Backend::next_request_id(), op, channel).await
    })
    .await?;
    refresh(this, cx).await?;
    let output = match lines.lock() {
        Ok(lines) => lines.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let ok = ran.is_ok();
    this.update_in(cx, |workbench, window, cx| {
        let resolver = cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(RESOLVE_CONFLICTS));
        let Some(slot) = workbench.vcs.as_mut() else { return };
        slot.state.remote_busy = None;
        for line in &output {
            slot.console.log(ConsoleKind::Out, line, now_ms());
        }
        let rebasing = in_progress(slot.state.branch.as_ref().and_then(|branch| branch.operation.as_deref()))
            == Some(InProgress::Rebase);
        let notice = match &ran {
            Ok(()) => {
                let text = match stopped {
                    Some(stopped) if rebasing => stopped,
                    _ => success,
                };
                slot.console.log(ConsoleKind::Info, &text, now_ms());
                Some(vcs_notice(NotificationLevel::Info, text))
            }
            Err(message) => {
                slot.console.log(ConsoleKind::Err, message, now_ms());
                // An operation stopped on conflicts is not an error: they open for resolving.
                if conflicts_instead && resolver && !slot.state.conflicted_files().is_empty() {
                    run_action_later(RESOLVE_CONFLICTS.to_owned(), None, window, cx);
                    None
                } else {
                    let mut notice = vcs_notice(NotificationLevel::Error, failure);
                    notice.body = Some(message.clone());
                    Some(notice)
                }
            }
        };
        match notice {
            Some(notice) => workbench.notify(NotificationRequest { key: Some(OPERATION_KEY.to_owned()), ..notice }, cx),
            None => workbench.notifications.expire_notification(OPERATION_KEY),
        }
        workbench.vcs_state_changed(cx);
    })?;
    Ok(ok)
}
```

All names it uses are imported in `branches.rs` already (`ActionRegistry`, `RESOLVE_CONFLICTS`, `in_progress`, `InProgress`, `Channel`, `Mutex`, `Backend`, `describe_operation`, `ConsoleKind`, `now_ms`, `NotificationRequest`). `slot` is not used after the `match`, so `workbench.notify` borrows freely.

Then, in `pub(super) async fn control_operation`, replace

```rust
    run_git_operation(&this, cx, git_op, progress, success, failure).await?;
```

with

```rust
    // Continue and Skip that stop on the next conflict open them, as Abort cannot.
    let conflicts_instead = control != Control::Abort;
    let run = GitRun { op: git_op, progress, success: success.to_owned(), stopped: None, failure, conflicts_instead };
    run_git_operation(&this, cx, run).await?;
```

(`src/features/vcs/operations.tsx:205`: `onError: control === "abort" ? undefined : conflictsInstead`.)

### 1.5 `app-gpui/src/workbench/commit.rs` (needed by: compilation of `vcs_operations.rs`; test `amend_commit_toggles_amend_with_the_last_commits_message`)

a. In `impl Workbench`, after

```rust
    pub fn commit_box(&self) -> Option<&Entity<CommitBox>> {
        self.commit.as_ref()?.commit_box.as_ref().map(|(commit_box, _)| commit_box)
    }
```

add

```rust
    /// Whether Amend is checked in the window's commit state.
    pub(super) fn commit_amend(&self, cx: &App) -> bool {
        self.commit.as_ref().is_some_and(|slot| slot.state.read(cx).amend)
    }
```

and replace `use gpui_kit::{AppContext, AsyncWindowContext, Context, Entity, Subscription, Window};` with `use gpui_kit::{App, AppContext, AsyncWindowContext, Context, Entity, Subscription, Window};`.

b. `    fn remember_commit_message(&mut self, message: &str, cx: &mut Context<Self>) {` becomes `    pub(super) fn remember_commit_message(&mut self, message: &str, cx: &mut Context<Self>) {`.

### 1.6 `app-gpui/src/workbench/mod.rs`

a. Module (compilation). After `mod todo_window;` (the line before `use todo_window::TODO_TOOL_WINDOW;`) add `mod vcs_operations;`.

b. Field (compilation). In `pub struct Workbench`, after

```rust
    conflicts: conflicts::ConflictsSlot,
```

add

```rust
    /// The Git menu's repository operations: Show Commit in Git Log waiting for the log.
    vcs_operations: vcs_operations::VcsOperationsSlot,
```

c. Initial value (compilation). In `Workbench::new`, after `            conflicts: Default::default(),` add `            vcs_operations: Default::default(),`.

d. Actions (compilation; every stage 1 test). In `pub fn register_actions(cx: &mut App)`, after `    conflicts::register_actions(registry);` add `    vcs_operations::register_actions(registry);`.

e. The shared test helpers, once for all three stages (needed by: stage 1, 2 and 3 tests, which `use super::super::tests::{…}`). In `#[cfg(test)] mod tests`, prefix each of these with `pub(super) `:

- `    fn wait_long(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {`
- `    fn click_on(cx: &mut VisualTestContext, selector: &'static str) {`
- `    fn run_action(cx: &mut VisualTestContext, id: &str, arg: Option<serde_json::Value>) {`
- `    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {`
- `    fn init_repo(root: &std::path::Path) {`
- `    fn git_head(root: &std::path::Path) -> String {`
- `    fn git_project(`
- `    fn notice_titled(`
- `    fn branch_label(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {`
- `    fn wait_for_prompt(`
- `    fn retype(dialog: &Entity<crate::vcs::prompt_dialogs::PromptDialog>, text: &str, cx: &mut VisualTestContext) {`
- `    fn notification_titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {`
- `    fn wait_for_notice(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) {`
- `    fn commit_parts(`
- `    fn published(line: u32, col: u32, severity: ide_core::Severity, message: &str) -> ide_core::Diagnostic {`
- `    fn publish(`

All are used by `mod.rs`'s own tests, so none is dead before its stage. If the `private_interfaces` lint trips on one (it should not: every type in these signatures is `pub`), move them unchanged into `#[cfg(test)] pub(super) mod test_support` of `workbench` and import from there.

f. `init_repo`: the temporary repository decides what the backend's git does (needed by: stage 1, 2 and 3 tests). The tests' own `git_in` runs with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_SYSTEM=/dev/null`, but the backend's git does not (`crates/ide-vcs/src/cli.rs`, `pub(crate) fn command`), so it reads the machine's configuration. Setting process environment is `unsafe` in Rust 2024 and the tests run in parallel, so the repository's own config is the lever. Also, stage 2 writes `.openidea/.gitignore` (`keep_private`, `crates/ide-host/src/ops/vcs/commands.rs`) on the first change-list save; excluding `.openidea/` in `.git/info/exclude` keeps it out of `git status` without changing what any commit contains. Replace

```rust
        std::fs::write(root.join("a.txt"), "one\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "first"]);
    }
```

(the end of `fn init_repo`) with

```rust
        // What the backend's git reads from the machine elsewhere is the repository's own here.
        for (key, value) in [
            ("core.autocrlf", "false"),
            ("core.hooksPath", ".git/hooks"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("merge.ff", "true"),
        ] {
            git_in(root, &["config", key, value]);
        }
        // The IDE's per-user files (change lists, shelf) stay out of `git status`.
        std::fs::create_dir_all(root.join(".git/info")).expect("the folder is made");
        std::fs::write(root.join(".git/info/exclude"), "/.openidea/\n").expect("the exclude file is written");
        std::fs::write(root.join("a.txt"), "one\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "first"]);
    }
```

The keys are those the backend does not pin itself: it passes `--rebase`/`--no-rebase --ff`/`--ff-only` to pull (`crates/ide-vcs/src/lib.rs:1234`, `manage.rs:145-147`) and `--no-autosquash` to the interactive rebase (`ops.rs:368`), but not a merge's fast-forward mode, signing, line endings or the hooks folder (`a_refused_commit_…` writes `.git/hooks/pre-commit`). The CRLF test also sets `core.autocrlf` itself.

`vcs.ignoreLocal`'s test (`add_to_vcs_stages_an_unversioned_file_and_the_ignore_actions_write_their_own_file`) only checks that `.git/info/exclude` contains `local.log`, so it still holds. Do **not** apply the old shelf note §3h (committing `.openidea/.gitignore`): it is replaced by this hunk.

### 1.7 Menus

Nothing to add: `menu_spec.rs`, `project_view/context_menu.rs` and `vcs_ops/commit_view.rs` name the ids already.

---

## Stage 2: shelf, change lists, patches, before-commit checks

Files to declare: `vcs/change_dialogs.rs`, `vcs/patch_dialogs.rs`, `vcs/shelf_view.rs`, `workbench/shelf.rs`.

Hunks: 45 edit sites (2.1: 3, 2.2: 1, 2.3: 7, 2.4: 4, 2.5: 9, 2.6: 4, 2.7: 11, 2.8: 5, 2.9: 1), in `vcs/mod.rs`, `vcs/prompt_dialogs.rs`, `workbench/mod.rs`, `workbench/branches.rs`, `workbench/commit.rs`, `vcs/commit_box.rs`, `vcs/commit_view.rs`, `workbench/diff_window.rs`, `workbench/code_insight.rs`.

### 2.1 `app-gpui/src/vcs/mod.rs` (compilation)

Add in alphabetical order: `pub mod change_dialogs;` after `pub mod branches_popup;`, `pub mod patch_dialogs;` after `pub mod merge_view;`, `pub mod shelf_view;` after `pub mod prompt_dialogs;`.

### 2.2 `app-gpui/src/vcs/prompt_dialogs.rs` (compilation of `change_dialogs.rs`, `patch_dialogs.rs`)

`fn tx(cx: &mut App, text: &str) -> String {` becomes `pub(super) fn tx(cx: &mut App, text: &str) -> String {`. (`checkbox`, `button`, `field_frame` are `pub(super)` since 1.2.)

### 2.3 `app-gpui/src/workbench/mod.rs`

a. Module (compilation). After `pub use settings_window::apply_keymap_settings;` add `mod shelf;`.

b. Field (compilation). After the stage 1 lines

```rust
    /// The Git menu's repository operations: Show Commit in Git Log waiting for the log.
    vcs_operations: vcs_operations::VcsOperationsSlot,
```

add

```rust
    /// The project's change lists and shelf.
    shelf: shelf::ShelfSlot,
```

c. Initial value (compilation). After `            vcs_operations: Default::default(),` add `            shelf: Default::default(),`.

d. Actions (every stage 2 test). After `    vcs_operations::register_actions(registry);` add `    shelf::register_actions(registry);`.

e. Context key (Show Diff in the Shelf tab). In `fn publish_context`, after `        self.publish_merge_context(window, cx);` add `        self.publish_shelf_context(window, cx);`.

f. The Commit tool window's tree gets its lists and tabs (stage 2 tests on change lists and the Shelf tab). In `fn mount_panels`, `COMMIT_TOOL_WINDOW =>` arm, after `                    self.attach_commit_box(&changes, window, cx);` add `                    self.attach_changes_extras(&changes, cx);`.

g. The existing test `a_refused_commit_keeps_its_message_and_the_options_reach_the_commit` clicks Reformat code, which stage 2 disables (2.6c: it has no `on_click` any more), so the setting is never written. Replace

```rust
        click_on(cx, "commit-check-reformat");
        let reformat = cx.update(|_, cx| crate::platform::settings::effective("w1", cx).vcs.commit.reformat);
        assert!(reformat, "the check is written to the settings");
```

with

```rust
        // Analyze code is on by default: the click turns it off.
        click_on(cx, "commit-check-analyze");
        let analyze = cx.update(|_, cx| crate::platform::settings::effective("w1", cx).vcs.commit.analyze_code);
        assert!(!analyze, "the check is written to the settings");
```

### 2.4 `app-gpui/src/workbench/branches.rs`

a. The lists and shelf follow the project (every stage 2 test). In `pub(super) fn mount_vcs`, replace

```rust
        self.mount_conflicts(window);
        self.refresh_vcs(cx);
```

with

```rust
        self.mount_conflicts(window);
        self.mount_shelf(window, cx);
        self.refresh_vcs(cx);
```

and in `pub(super) fn drop_vcs(&mut self)`, after `        self.drop_conflicts();` add `        self.drop_shelf();`.

b. Every repository read feeds the lists (change-list tests). In `fn vcs_refreshed`, after `        self.conflicts_changed(conflicts, cx);` add `        self.changelists_follow_status(cx);` (before the waiters are settled, so a Commit that waited plans with the lists already following).

c. The Git console from the sibling module (compilation). In `impl Workbench`, after `    pub fn vcs_console(&self) -> Option<&VcsConsole> { … }` add

```rust
    /// Writes `text` to the window's Git console.
    pub(super) fn log_vcs_console(&mut self, kind: ConsoleKind, text: &str) {
        if let Some(slot) = self.vcs.as_mut() {
            slot.console.log(kind, text, now_ms());
        }
    }
```

### 2.5 `app-gpui/src/workbench/commit.rs` (all on top of stage 1.5)

a. Imports (compilation). Remove `use crate::vcs_ops::changelists::initial_changelists;`. Replace `use crate::vcs_ops::change_ops::{commit_precheck, commit_success, commit_title, committed};` with `use crate::vcs_ops::change_ops::{commit_candidates, commit_precheck, commit_success, commit_title, committed};`.

b. The plan uses the project's lists (commit tests). In `fn commit_box_input`, replace `            (Some(vcs), Some(slot)) => current_plan(vcs, &initial_changelists(), slot.state.read(cx)).included.len(),` with `            (Some(vcs), Some(slot)) => current_plan(vcs, self.changelists(), slot.state.read(cx)).included.len(),`, and after `            plugin_checks: Vec::new(),` add `            unavailable_checks: super::shelf::unavailable_commit_checks(),`.

c. The commit runs the checks (stage 2 commit-check tests). In `async fn commit`, replace

```rust
        let plan = current_plan(&vcs, &initial_changelists(), &state);
        let merging = vcs.branch.as_ref().and_then(|branch| branch.operation.as_deref()) == Some("merge");
        let commit = &settings::effective(&workbench.label, cx).vcs.commit;
        let skipped: Vec<&str> = [
            (commit.analyze_code, "analyze code"),
            (commit.check_todo, "check TODO"),
            (commit.reformat, "reformat"),
            (commit.optimize_imports, "optimize imports"),
            (commit.cleanup, "cleanup"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        if !skipped.is_empty() && !request.anyway {
            tracing::info!("before-commit checks are not run: {}", skipped.join(", "));
        }
        (state, vcs, plan, merging, commit.clear_initial_message)
```

with

```rust
        let plan = current_plan(&vcs, workbench.changelists(), &state);
        let merging = vcs.branch.as_ref().and_then(|branch| branch.operation.as_deref()) == Some("merge");
        let clear = settings::effective(&workbench.label, cx).vcs.commit.clear_initial_message;
        (state, vcs, plan, merging, clear)
```

then replace

```rust
    if let Some(notice) = commit_precheck(&request.message, amend, &plan, merging) {
        return notify(this, cx, notice);
    }
```

with

```rust
    if let Some(notice) = commit_precheck(&request.message, amend, &plan, merging) {
        return notify(this, cx, notice);
    }
    // Failed checks show in the box; the next Commit (Commit Anyway) skips them.
    if !request.anyway {
        let problems = super::shelf::run_commit_checks(this, cx, commit_candidates(&plan), amend).await?;
        if !problems.is_empty() {
            return this.update(cx, |workbench, cx| {
                let state = workbench.commit_state(cx);
                state.update(cx, |state, cx| {
                    state.check_problems = problems;
                    cx.notify();
                });
            });
        }
    }
```

`state.check_problems` exists (`vcs_ops/commit_store.rs`, `pub check_problems: Vec<CommitProblem>`). Check that `committed(…)` clears it; if not, add `state.check_problems.clear();` in the `state.update` after the commit.

d. Review (test `reported_errors_stop_the_commit_and_review_shows_the_problems`). In `fn commit_box_event`, replace

```rust
            // Only the before-commit checks report problems, and none run here.
            CommitBoxEvent::ProblemDetails(index) => tracing::debug!("no details for commit problem {index}"),
```

with `            CommitBoxEvent::ProblemDetails(index) => self.commit_problem_details(*index, window, cx),`.

e. The tabs follow the commit state (Shelf tab tests). In `pub(super) fn commit_state`, replace `        let observe = cx.observe(&state, |workbench, _, cx| workbench.sync_commit_box(cx));` with

```rust
        let observe = cx.observe(&state, |workbench, _, cx| {
            workbench.sync_commit_box(cx);
            workbench.sync_commit_tabs(cx);
        });
```

f. Module doc. Replace

```rust
//! The before-commit checks (code analysis, TODO, reformat…) have no runner here: a commit runs
//! without them whatever they are set to, and nothing reports them as passed.
```

with

```rust
//! Before the commit the before-commit checks run (`shelf::run_commit_checks`); what they find shows
//! in the box and the next Commit, Commit Anyway, skips them.
```

### 2.6 `app-gpui/src/vcs/commit_box.rs` (compilation; test of the disabled checks)

a. In `pub struct CommitBoxInput`, after

```rust
    /// Before-commit checks contributed by plugins.
    pub plugin_checks: Vec<PluginCheck>,
```

add

```rust
    /// Checks that cannot run here, with why: their rows are disabled with the reason.
    pub unavailable_checks: Vec<(CommitCheck, String)>,
```

b. After `pub fn input(&self) -> &CommitBoxInput { … }` add

```rust
    /// Why `check` cannot run here, when it cannot.
    pub fn unavailable_check(&self, check: CommitCheck) -> Option<&str> {
        self.input.unavailable_checks.iter().find(|(known, _)| *known == check).map(|(_, reason)| reason.as_str())
    }
```

c. In `pub fn option_rows`, replace **both** occurrences of `            rows.push((tx(cx, check.label()), check.on(&settings)));` with `            rows.push((tx(cx, check.label()), check.on(&settings) && self.unavailable_check(check).is_none()));`. In `fn render_options`, replace

```rust
        let check = |check: CommitCheck, id: &'static str, cx: &mut Context<Self>| {
            let label = tx(cx, check.label());
            row(checkbox(cx, label, check.on(&settings)), id)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_check(check, cx)))
        };
```

with

```rust
        let unavailable = self.input.unavailable_checks.clone();
        let check = |check: CommitCheck, id: &'static str, cx: &mut Context<Self>| {
            let label = tx(cx, check.label());
            let reason = unavailable.iter().find(|(known, _)| *known == check).map(|(_, reason)| tx(cx, reason));
            match reason {
                // A check that cannot run shows unticked and greyed out, with why under it.
                Some(reason) => div()
                    .flex()
                    .flex_col()
                    .child(row(checkbox(cx, label, false), id).opacity(0.5))
                    .child(
                        div()
                            .pl(px(24.0))
                            .text_color(theme::color(cx, "fg-muted"))
                            .text_size(theme::length(cx, "fs-small"))
                            .child(SharedString::from(reason)),
                    )
                    .into_any_element(),
                None => row(checkbox(cx, label, check.on(&settings)), id)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_check(check, cx)))
                    .into_any_element(),
            }
        };
```

The call sites (`.child(check(CommitCheck::Reformat, "commit-check-reformat", cx))` …) take an `AnyElement` unchanged; add `AnyElement` to the `gpui_kit` import if it is not there.

### 2.7 `app-gpui/src/vcs/commit_view.rs` (change-list, tab and drag tests)

All anchors below were re-checked against the current file and match once each. 11 hunks.

#### 7a. Imports

- Add `ChangelistState` to the `use super::changes::{ … }` list.
- Add `AnyView` to the `gpui_kit::{ … }` list.
- Add `use crate::vcs_ops::git_tabs::ContentTab;`.

#### 7b. `kind_color` for the sibling views

`fn kind_color(kind: EffectiveKind) -> &'static str {` → `pub(super) fn kind_color(kind: EffectiveKind) -> &'static str {`

#### 7c. Events

In `pub enum CommitPanelEvent`, after `ShowMenu(gpui_kit::Point<gpui_kit::Pixels>),` add:

```rust
    /// Show content tab `id` (Commit, Shelf).
    SelectTab(String),
    /// Move the files at `paths` to change list `list` (they were dropped on its group).
    MoveToChangelist { paths: Vec<String>, list: String },
```

#### 7d. Rows know their list

In `struct Row`, after `expanded: bool,` add `changelist: Option<String>,`. In `fn flatten`, in the `rows.push(Row { … })` literal, after `expanded,` add `changelist: node.changelist.clone(),`.

#### 7e. The dragged changes

After `struct CommitParts { … }` add:

```rust
/// Changed files dragged onto a change list's group.
#[derive(Clone, Debug)]
pub struct DraggedChanges {
    pub paths: Vec<String>,
    label: String,
}

impl Render for DraggedChanges {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(theme::length(cx, "radius-small"))
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .text_color(theme::color(cx, "fg-strong"))
            .child(SharedString::from(self.label.clone()))
    }
}
```

#### 7f. Fields

In `pub struct CommitPanel`, after `commit: Option<CommitParts>,` add:

```rust
    /// The change lists the changes are grouped by.
    lists: Option<ChangelistState>,
    /// The content tabs above the tree, the chosen one, and what a tab other than Commit shows in
    /// place of the changes.
    tabs: Vec<ContentTab>,
    tab: String,
    content: Option<AnyView>,
```

In `CommitPanel::new`, after `commit: None,` add:

```rust
            lists: None,
            tabs: Vec::new(),
            tab: "changes".to_owned(),
            content: None,
```

#### 7g. Methods

After `pub fn selected_file_opens(&self) -> bool { … }` add:

```rust
    /// Groups the changes by `lists`.
    pub fn set_changelists(&mut self, lists: ChangelistState, cx: &mut Context<Self>) {
        if self.lists.as_ref() == Some(&lists) {
            return;
        }
        self.lists = Some(lists);
        self.rebuild(cx);
        cx.notify();
    }

    /// The change list of the selected row: a list's group or what is below it.
    pub fn selected_changelist(&self) -> Option<String> {
        let key = self.selected.as_deref()?;
        self.rows.iter().find(|row| row.key == key)?.changelist.clone()
    }

    /// The change list whose group row is selected.
    pub fn selected_list_group(&self) -> Option<String> {
        let key = self.selected.as_deref()?;
        self.rows.iter().find(|row| row.key == key && row.node_type == NodeType::Group)?.changelist.clone()
    }

    /// The paths, without a trailing slash, of the selected row's files.
    pub fn selected_paths(&self) -> Vec<String> {
        let Some(key) = self.selected.as_deref() else { return Vec::new() };
        let Some(row) = self.rows.iter().find(|row| row.key == key) else { return Vec::new() };
        row.files.iter().map(|file| strip_slash(&file.path).to_owned()).collect()
    }

    /// Shows `tabs` above the tree with `selected` chosen; `content` takes the place of the changes
    /// and the message box while a tab other than Commit is chosen.
    pub fn set_content_tabs(
        &mut self,
        tabs: Vec<ContentTab>,
        selected: &str,
        content: Option<AnyView>,
        cx: &mut Context<Self>,
    ) {
        let same = match (&self.content, &content) {
            (Some(shown), Some(new)) => shown.entity_id() == new.entity_id(),
            (None, None) => true,
            _ => false,
        };
        if same && self.tabs == tabs && self.tab == selected {
            return;
        }
        self.tabs = tabs;
        self.tab = selected.to_owned();
        self.content = content;
        cx.notify();
    }

    pub fn tabs(&self) -> &[ContentTab] {
        &self.tabs
    }

    pub fn selected_tab(&self) -> &str {
        &self.tab
    }

    /// What a tab other than Commit shows, while it is chosen.
    pub fn content(&self) -> Option<&AnyView> {
        self.content.as_ref()
    }
```

#### 7h. The tree is grouped by the lists

In `fn rebuild`, replace

```rust
        self.tree = build_change_tree(&self.status, &self.root, true, None, &[], false, i18n);
```

with

```rust
        self.tree = build_change_tree(&self.status, &self.root, true, self.lists.as_ref(), &[], false, i18n);
```

#### 7i. A menu on every row, row selectors, drag and drop

In `fn render_tree`, before `uniform_list(` add:

```rust
        let drop_bg = theme::color(cx, "bg-tree-selected");
```

In the row chain, replace

```rust
                        let (clicked, toggled, pressed) = (row.key.clone(), row.key.clone(), row.key.clone());
                        div()
                            .id(SharedString::from(format!("change:{}", row.key)))
```

with

```rust
                        let (clicked, toggled, pressed) = (row.key.clone(), row.key.clone(), row.key.clone());
                        let row_label = row.label.clone();
                        let dragged = (row.node_type != NodeType::Group && !row.files.is_empty()).then(|| DraggedChanges {
                            paths: row.files.iter().map(|file| strip_slash(&file.path).to_owned()).collect(),
                            label: row.label.clone(),
                        });
                        let target = row.changelist.clone().filter(|_| row.node_type == NodeType::Group);
                        div()
                            .id(SharedString::from(format!("change:{}", row.key)))
                            .debug_selector(move || format!("commit-row-{row_label}"))
                            .when_some(dragged, |el, dragged| {
                                el.on_drag(dragged, |dragged: &DraggedChanges, _, _, cx| cx.new(|_| dragged.clone()))
                            })
                            .when_some(target, |el, list| {
                                el.drag_over::<DraggedChanges>(move |style, _, _, _| style.bg(drop_bg)).on_drop(
                                    cx.listener(move |_, dragged: &DraggedChanges, _, cx| {
                                        let paths = dragged.paths.clone();
                                        cx.emit(CommitPanelEvent::MoveToChangelist { paths, list: list.clone() });
                                    }),
                                )
                            })
```

and in the right-button handler replace

```rust
                                    if panel.selected_file().is_some() {
                                        cx.emit(CommitPanelEvent::ShowMenu(event.position));
                                    }
```

with

```rust
                                    cx.emit(CommitPanelEvent::ShowMenu(event.position));
```

#### 7j. The tab strip and the tab's content

Replace the whole `impl Render for CommitPanel { … }` with:

```rust
impl CommitPanel {
    /// The content tabs (Commit, Shelf), drawn as `commit-tab-<id>`.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let accent = theme::color(cx, "bg-button-primary");
        let tabs: Vec<AnyElement> = self
            .tabs
            .iter()
            .map(|tab| {
                let selected = tab.id == self.tab;
                let label = cx.default_global::<Localization>().0.tx(&tab.label);
                let label = match tab.count {
                    Some(count) => format!("{label} {count}"),
                    None => label,
                };
                let (id, selector) = (tab.id.clone(), tab.id.clone());
                div()
                    .id(SharedString::from(format!("commit-tab-{}", tab.id)))
                    .debug_selector(move || format!("commit-tab-{selector}"))
                    .h(px(26.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_color(if selected { fg } else { muted })
                    .when(selected, |el| el.border_b_1().border_color(accent))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(CommitPanelEvent::SelectTab(id.clone()))))
                    .into_any_element()
            })
            .collect();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .children(tabs)
            .into_any_element()
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strip = (self.tabs.len() > 1).then(|| self.render_tabs(cx));
        if let Some(content) = self.content.clone() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .text_size(theme::length(cx, "fs-ui"))
                .children(strip)
                .child(div().flex_1().min_h_0().w_full().child(content))
                .into_any_element();
        }
        let tree = self.render_tree(window, cx);
        // Only the tree takes the keyboard's keys: what is typed in the message box stays there.
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .children(strip)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .child(tree),
            )
            .children(self.commit.as_ref().map(|parts| {
                div()
                    .flex_none()
                    .w_full()
                    .border_t_1()
                    .border_color(theme::color(cx, "border"))
                    .child(parts.commit_box.clone())
            }))
            .into_any_element()
    }
}
```

#### 7k. The view's own test

In `lists_the_repositorys_changes_by_group_and_directory`, replace

```rust
                CommitPanelEvent::ShowMenu(_) => {}
```

with

```rust
                CommitPanelEvent::ShowMenu(_) | CommitPanelEvent::SelectTab(_) | CommitPanelEvent::MoveToChangelist { .. } => {}
```

### 2.8 `app-gpui/src/workbench/diff_window.rs` (compilation; Show Diff tests)

Anchors re-checked against the current file. 3 hunks.

#### 8a. The tree's events

In `pub(super) fn commit_panel_event`, replace

```rust
            CommitPanelEvent::ShowMenu(position) => self.show_change_menu(*position, window, cx),
```

with

```rust
            CommitPanelEvent::ShowMenu(position) => self.show_changes_menu(*position, window, cx),
            CommitPanelEvent::SelectTab(tab) => self.select_commit_tab(tab, window, cx),
            CommitPanelEvent::MoveToChangelist { paths, list } => self.move_dropped_changes(paths, list, cx),
```

#### 8b. The old menu goes

Delete the whole `fn show_change_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) { … }` with its doc comment. Its rows (Show Diff, Jump to Source) are in `shelf::node_menu` now. Then remove what only it used: `use crate::vcs_ops::menu::MenuItem;`, and `Pixels, Point` from the `gpui_kit::{ … }` import (check with a build: `show_diff_settings` uses `gpui_kit::point`, not `Point`).

#### 8c. Show Diff in the Shelf tab

In `pub fn show_diff`, replace

```rust
        let paths = match show_diff_start(arg, context, false) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // There is no Shelf view to have the focus.
            ShowDiffStart::Shelf => return,
        };
```

with

```rust
        let paths = match show_diff_start(arg, context, self.shelf_focused(window, cx)) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // The Shelf tab has the keyboard: the diff of its selection.
            ShowDiffStart::Shelf => return self.show_shelf_diff(None, false, false, window, cx),
        };
```

and in the `ChangesView { … }` literal below it replace `lists: None,` with `lists: Some(self.changelists()),` (the chain of a diff stays within the file's change list, as the tree groups it).

### 2.9 `app-gpui/src/workbench/code_insight.rs` (compilation)

After

```rust
    pub fn problems_badge(&self) -> Option<usize> {
        error_badge(self.code_insight.problems.map())
    }
```

add

```rust
    /// Every file's problems as their producers reported them, before the inspection profile.
    pub(super) fn problems_map(&self) -> &problems::ProblemsMap {
        self.code_insight.problems.raw()
    }
```

(`ProblemsStore::raw` exists, `code_insight/problems.rs`.)

### 2.10 What changed in the stage 2 files since `shelf_integration.md`

- The CRLF check reads the file through `ide_host::ops::fs::read_text`, the same path as the source's `fsReadText`; in a project window that text is LF-normalised, so the check does not fire there, as in the source. Its test now asserts a detached HEAD alone and no "Line separators".
- `PatchMade::Changes { repo }` is `PatchMade::Repo { repo, commits }` (empty `commits`: the local changes checked in the dialog), and `open_create_patch` and `PatchMade` are `pub(super)`, for stage 3's Create Patch of commits. Stage 2 constructs it with `commits: Vec::new()`, so nothing is dead before stage 3.
- `shelf.rs` imports `RESOLVE_CONFLICTS` from `super::conflicts` instead of its own copy.

---

## Stage 3: Git log actions

Files to declare: `vcs/interactive_rebase_dialog.rs`, `vcs/log_message_dialog.rs`, `workbench/log_actions.rs`. **Not** `vcs/log_reset_dialog.rs`, **not** `vcs/log_tag_dialog.rs`.

Hunks: 26 edit sites (3.1: 2, 3.2: 5, 3.3: 6, 3.4: 1, 3.5: 3, 3.6: 2, 3.7: 4, 3.8: 3), in `vcs/mod.rs`, `workbench/mod.rs`, `workbench/branches.rs`, `workbench/conflicts.rs`, `workbench/log_window.rs`, `workbench/commit.rs`, `vcs/prompt_dialogs.rs`, `vcs/log_view.rs`.

### 3.1 `app-gpui/src/vcs/mod.rs` (compilation)

`pub mod interactive_rebase_dialog;` after `pub mod graph;`; `pub mod log_message_dialog;` after `pub mod log_details;` (before `pub mod log_options;`).

### 3.2 `app-gpui/src/workbench/mod.rs`

a. Module. Before `mod log_window;` add `mod log_actions;`.

b. Field. After the stage 2 lines

```rust
    /// The project's change lists and shelf.
    shelf: shelf::ShelfSlot,
```

add

```rust
    /// What the Git log's actions keep for their notifications' buttons (Undo of a history rewrite).
    log_actions: log_actions::LogActionsSlot,
```

c. Initial value. After `            shelf: Default::default(),` add `            log_actions: Default::default(),`.

d. Actions. After `    log_window::register_actions(registry);` add `    log_actions::register_actions(registry);`. The 39 ids (22 commit actions with `vcs.log.createPatch`, 5 Branches pane, 6 changed files, goToRef, revealBookmark, collapseLinear, expandLinear and the two toggles) are all in `assets/gen/actions.json`.

e. Undo button (test `edit_commit_message_rewords_the_head_and_an_older_commit_and_undo_restores_it`, `interactive_rebase_lists_the_commits_oldest_first_and_applies_the_plan`). In `fn run_notification_action`, before `            EntryAction::Own(own) if own.command.starts_with(conflicts::FOLLOW_UP_PREFIX) => {`, add

```rust
            EntryAction::Own(own) if own.command.starts_with(log_actions::UNDO_PREFIX) => {
                if !self.run_log_undo(&own.command, window, cx) {
                    tracing::debug!("the notification command {} has no handler any more", own.command);
                }
            }
```

### 3.3 `app-gpui/src/workbench/branches.rs`

a. Visibility (compilation): `enum Success {` becomes `pub(super) enum Success {`; `async fn checkout(` becomes `pub(super) async fn checkout(`. The old log note's `VcsSlot.state` / `VcsSlot.console` and `vcs_state_changed` visibility changes are **dropped**: only the removed `run_log_operation` needed them.

b. The Undo buttons go with the project. In `pub(super) fn drop_vcs(&mut self)`, after `        self.vcs_dialog = None;` add `        self.log_actions = Default::default();`.

c. Push All up to Here (test `push_all_up_to_here_pushes_the_branch_to_the_bare_remote`; the source passes `{ branch, revision }` to `vcs.push`, `historyActions.tsx:392-400`). Anchors re-checked. 3 hunks; item 4 is 3.6b.

1. Replace the `vcs.push` registration

```rust
    registry.handle("vcs.push", |call, window, cx| {
        let branch = call.arg.and_then(|arg| arg.get("branch")).and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, |workbench, window, cx| workbench.start_push(branch, window, cx))
    });
```

with

```rust
    registry.handle("vcs.push", |call, window, cx| {
        let branch = call.arg.and_then(|arg| arg.get("branch")).and_then(Value::as_str).map(str::to_owned);
        // A commit id, or a commit object with one.
        let revision = call.arg.and_then(|arg| arg.get("revision")).and_then(|revision| {
            revision.as_str().or_else(|| revision.get("id").and_then(Value::as_str)).map(str::to_owned)
        });
        on_workbench(window, cx, |workbench, window, cx| workbench.start_push(branch, revision, window, cx))
    });
```

2. `fn start_push(&mut self, branch: Option<String>, window: &mut Window, cx: &mut Context<Self>) {` becomes

```rust
    fn start_push(&mut self, branch: Option<String>, revision: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
```

In its body, `cx.spawn_in(window, async move |this, cx| finished(push(this, cx).await)).detach();` becomes `cx.spawn_in(window, async move |this, cx| finished(push(this, cx, revision).await)).detach();`.

3. `pub(super) async fn push(this: This, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {` becomes `pub(super) async fn push(this: This, cx: &mut AsyncWindowContext, revision: Option<String>) -> anyhow::Result<()> {`. In its body, `let input = PushDialogInput { repos, selected, has_hook, protected_branches: protected };` becomes:

```rust
    let input = PushDialogInput { repos, selected, has_hook, protected_branches: protected, revision };
```

The dialog still lists every outgoing commit of the branch, not only those up to the commit.

### 3.4 `app-gpui/src/workbench/conflicts.rs` (compilation)

`async fn save_exact(host: &Host, label: &str, path: PathBuf, text: String) -> IdeResult<()> {` becomes `pub(super) async fn save_exact(…`.

### 3.5 `app-gpui/src/workbench/log_window.rs`

a. (compilation) `    fn set_log_options(&mut self, options: LogOptions, cx: &mut Context<Self>) {` becomes `    pub(super) fn set_log_options(…`.

b. (test `the_changed_files_actions_act_on_the_files_of_the_selected_commit`) In `pub(super) fn run_log_changes_command`, replace

```rust
            ChangesCommand::Revert | ChangesCommand::GetFromRevision | ChangesCommand::ShowHistory { .. } => {
                tracing::info!("{command:?} on the log's changes is not available");
            }
```

with

```rust
            ChangesCommand::Revert => self.revert_log_changes(changes, window, cx),
            ChangesCommand::GetFromRevision => self.get_from_revision(changes, window, cx),
            ChangesCommand::ShowHistory { .. } => {
                tracing::info!("{command:?} on the log's changes is not available");
            }
```

c. (compilation: `show_commit_menu` is otherwise called only from tests) the `let GitLogEvent::SelectionChanged = event;` is irrefutable only while there is one variant. Replace

```rust
        let selected = cx.subscribe_in(&log, window, |workbench, _, event: &GitLogEvent, _, cx| {
            let GitLogEvent::SelectionChanged = event;
            workbench.feed_log_details(cx);
        });
```

with

```rust
        let selected = cx.subscribe_in(&log, window, |workbench, _, event: &GitLogEvent, window, cx| match event {
            GitLogEvent::SelectionChanged => workbench.feed_log_details(cx),
            GitLogEvent::ShowMenu { position } => workbench.show_commit_menu(*position, window, cx),
        });
```

### 3.6 `app-gpui/src/workbench/commit.rs` (compilation: `rebase_after_fixup` is otherwise unused)

a. Replace

```rust
    if request.rebase_onto.is_some() {
        tracing::info!("Commit and Rebase is not available: the fixup commit was made without the rebase");
    }
```

with

```rust
    if let Some(target) = request.rebase_onto.clone() {
        super::log_actions::rebase_after_fixup(this.clone(), cx, target).await?;
    }
```

The checks (2.5c) run before the fixup commit, the rebase after it, as in the source. `push` is never combined with it (`push: push && rebase_onto.is_none()` in `commit_box.rs`).

b. With 3.3c, `        push(this.clone(), cx).await?;` becomes `        push(this.clone(), cx, None).await?;`.

### 3.7 `app-gpui/src/vcs/prompt_dialogs.rs` (test `push_all_up_to_here…`)

Anchors re-checked, once each. 4 hunks.

1. In `pub struct PushDialogInput`, after `pub protected_branches: String,` add:

```rust
    /// Push All up to Here…: the commit the selected repository's branch is pushed up to.
    pub revision: Option<String>,
```

2. In `pub struct PushDialog`, after `    protected: String,` add:

```rust
    /// The repository pushed only up to a commit, and that commit.
    revision: Option<(String, String)>,
```

3. In `PushDialog::new`, in the returned `Self { … }`, after `protected: input.protected_branches,` add:

```rust
            revision: input.selected.zip(input.revision),
```

`input.repos` is moved earlier, which is a partial move. `selected` and `revision` are still whole, and `selected` was only borrowed before.

4. In `fn choice`, replace `            revision: None,` with:

```rust
            revision: self.revision.as_ref().filter(|(root, _)| *root == row.repo.push.root).map(|(_, id)| id.clone()),
```

Test constructors use `..PushDialogInput::default()`, so they compile unchanged.

### 3.8 `app-gpui/src/vcs/log_view.rs` (right-click menu)

3 hunks. In item 2, use the **first** `use gpui_kit::{` (the module's; the second one is in its tests).

1. Replace

```rust
/// What the log tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitLogEvent {
    /// Another commit is selected, or none.
    SelectionChanged,
}
```

with

```rust
/// What the log tells its window.
#[derive(Debug, Clone, PartialEq)]
pub enum GitLogEvent {
    /// Another commit is selected, or none.
    SelectionChanged,
    /// Show the commit context menu of the selected commit at `position`.
    ShowMenu { position: Point<Pixels> },
}
```

`Pixels` is not `Eq`, so `Eq` goes. Nothing compares `GitLogEvent`s (`grep -rn GitLogEvent app-gpui/src`).

2. Add `MouseButton, MouseDownEvent, Point` to the `use gpui_kit::{ … }` list.

3. In `fn commit_list`, replace

```rust
                            .on_click(cx.listener(move |log, _, window, cx| {
                                window.focus(&log.focus, cx);
                                log.select(index, cx);
                            }))
```

with

```rust
                            .on_click(cx.listener(move |log, _, window, cx| {
                                window.focus(&log.focus, cx);
                                log.select(index, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |log, event: &MouseDownEvent, window, cx| {
                                    window.focus(&log.focus, cx);
                                    log.select(index, cx);
                                    cx.emit(GitLogEvent::ShowMenu { position: event.position });
                                    cx.stop_propagation();
                                }),
                            )
```

### 3.9 What changed in the stage 3 files since `log_actions_integration.md`

- `LogOperation` and `run_log_operation` are gone: cherry-pick, revert, reset and rebases go through `branches::run_git_operation(GitRun { … })`; `run_rebase` reads HEAD before and after and tells the `vcs.operation` notice again with Undo. That is two notifies with the same key and title; a test that checks the Undo button of a rebase waits until the notice has it (`interactive_rebase_lists_the_commits_oldest_first_and_applies_the_plan` does).
- Reset Current Branch to Here… is `ResetForm::on_commit` in `GitFormDialog` through `vcs_operations::ask_form`; New Tag… is `TagForm::on_commit` with `NewTagOn`'s prompt texts.
- `git_log_view` and `reload_log` come from `vcs_operations.rs` (they were defined twice: two `git_log_view` in `impl Workbench` is a hard error).
- `vcs.log.createPatch` is registered (`CommitAction::CreatePatch`) on stage 2's `CreatePatchDialog`, with an end-to-end test.
- The commit menu reads `@{u}..HEAD` before it shows; it is asynchronous now (tests wait for it).
- The tests use the shared helpers of `workbench::tests` (1.6e), so they get 1.6f too.

---

## Assumptions to check, in order

1. `run_git_operation` (1.4): `workbench.notifications` is reached from `branches.rs` (a child module of `workbench`: private fields of `Workbench` are visible there). The old `log_actions.rs` did the same.
2. `vcs_ops/operations.rs` `Control` derives `PartialEq` (used as `control == Control::Continue` already).
3. `commit_takes_exactly_the_included_files_with_the_typed_message` asserts tree lines `["Changes (2)", …]`. With stage 2 the tree is grouped by the lists; with only the default list it should still read `Changes (2)` (`DEFAULT_LIST_NAME`). Check on the first stage 2 run.
4. Every existing commit test now runs Analyze code, which opens the committed `.txt` files and waits up to 2 s on the executor's clock. All of them wait with `wait_long` after the commit (checked: `grep -n 'click_on(cx, "commit-commit")\|COMMIT_KEYS' app-gpui/src/workbench/mod.rs`), so they pass, 2 s slower. The source does the same (`src/features/vcs/commitChecks.tsx:315`).
5. `log_actions.rs` imports `crate::agents::conversation::decode_uri_component`, and `agents/**` is being edited by another agent. If it moves, `database/model.rs` has a private copy to share.
