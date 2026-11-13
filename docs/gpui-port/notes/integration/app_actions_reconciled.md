# Application and window actions: reconciled integration notes (apply after the plugin stages, the context menus and the editor drafts)

This file replaces `docs/gpui-port/notes/integration/app_actions_integration.md` for the integration. Apply it after stages 1 to 3 of `plugins_all_integration.md`, `context_menus_reconciled.md`, `editor_core_integration.md` and `editor_language_integration.md`. Paths are relative to `app-gpui/src/` unless they start with `app-gpui/`, `src/`, `src-tauri/`, `crates/` or `docs/`.

**The drafted files.** `workbench/app_actions.rs` (1853 lines) was edited in this reconciliation; the five `app_views/*.rs` were checked and left as committed. The final versions, byte-identical to the worktree (checked with `cmp`), are under `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/app/files/app-gpui/src/` (`workbench/app_actions.rs`, `app_views/{mod,trust_dialog,about_dialog,new_project_dialog,invalidate_caches_dialog}.rs`). If `git status` shows `app-gpui/src/workbench/app_actions.rs` unmodified, copy it from there first. The committed draft (`860b143b`) is kept at `.../scratchpad/app/orig/`.

**Measured and read.** Nothing was compiled or run. Measured (commands run, output read): every anchor below was counted by `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/app/anchors.py` (run with `python3 -I`; it counts in the worktree and at HEAD `881ba91e`, and counts section 7's anchor in `plugins_all_integration.md`): `NOT ONE: 0`. At that moment the compiling agent had already applied stage 1, stage 2 and stage 3.5 (`menu.rs`) in the worktree, uncommitted. Also measured: the greps behind the test sweep and the trust survey (section 10), the production callers of `Workbench::open_project` (only `app_shell.rs` `OpenProject` and `windows.rs` 85 and 126, all three covered by 5b and 6), and the backend trust checks quoted in 10.4. Read, not measured: every API the drafted files call (checked against today's signatures in `app-gpui/src`, `crates/`, `gpui-pre-0.3.12/src` and `gpui-pre-macos-0.3.12/src`), and the webview behaviour quoted from `src/`.

Hunks: 20 edit sites in 7 files (`lib.rs` 1, `platform/storage.rs` 1, `workbench/mod.rs` 10, `workbench/chrome.rs` 1, `workbench/app_shell.rs` 4, `workbench/windows.rs` 2, `workbench/menu.rs` 1). 12 apply as the draft wrote them (1, 2, 3a to 3h, 5a.1, 5a.2). 4 are rewritten: 3i (anchor widened to the `roots` line), 4 (wrapped to 120 columns), 5b (the trust question moves from `RequestWindow` to `OpenProject`, after "Open Project" and the unsaved-changes question, as the webview asks it; Don't Open is a cancelled opening), 7 (`menu.rs`: on top of stage 3.5's line, without the draft's import edit, which pushed the import list past 120 columns). 4 are new: 3j (the Welcome-screen test now meets the question), 5c (a window that is asking keeps whoever waits for its opening waiting), 6.1 and 6.2 (`windows.rs`: a window opened for a project asks in that window, which covers New Window, Open in New Window and the command-line folder that bypassed the draft's question). Sections 1 to 7 are all required: `-D warnings` fails on dead code without 3, 4, 5, 6 and 7, and the tests fail without 3i, 3j, 5 and 6.

## 1. `lib.rs`

After `pub mod app_models;` add:

```rust
pub mod app_views;
```

## 2. `platform/storage.rs`

Export Settings… lists the stored entries. After

```rust
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
```

add:

```rust

    /// Every stored entry, in key order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }
```

## 3. `workbench/mod.rs`

### 3a. The module

Replace

```rust
mod agents;
mod app_shell;
```

with

```rust
mod agents;
mod app_actions;
mod app_shell;
```

### 3b. The actions

In `pub fn register_actions(cx: &mut App)`, after `    settings_window::register_actions(registry);` (stage 1.3.4 put `    plugins_window::register_actions(registry);` right after it; either order works) add:

```rust
    app_actions::register_actions(registry);
```

No declared module registers any of the 20 ids (measured: `grep -rl '"<id>"'` outside `app_actions.rs`, `menu_spec.rs`, `app_views/` finds only references: `app.about` in `welcome*.rs`, `app.restart` in `app_models/updates.rs`, `file.importSettings` in `welcome_pages.rs`, `editor.toggleZen` in `quick_switch.rs`, `title_bar.rs`, `features_a/vim/rc.rs`).

### 3c. The context keys

In `fn publish_context`, after `        self.publish_merge_context(window, cx);` add:

```rust
        self.publish_app_context(window, cx);
```

It publishes `toolWindowLayoutsSaved` (the `when` of `layout.rename` / `layout.delete`) and `distractionFree`.

### 3d. The Restricted Mode banner redraws when the trust changes

Replace

```rust
            IdeEvent::ProjectTrustChanged { trusted, .. } => {
                self.agents_trust_changed(*trusted, cx);
                return;
            }
```

with

```rust
            IdeEvent::ProjectTrustChanged { trusted, .. } => {
                self.agents_trust_changed(*trusted, cx);
                // The Restricted Mode banner comes or goes.
                cx.notify();
                return;
            }
```

(The comment line above the arm, `// The project's agents are listed again, …`, stays.)

### 3e. Distraction Free Mode hides the status bar

Replace

```rust
        (self.editor().is_some() || self.task_status().is_some())
            && crate::platform::settings::global(cx).appearance.show_status_bar
```

with

```rust
        (self.editor().is_some() || self.task_status().is_some())
            && crate::platform::settings::global(cx).appearance.show_status_bar
            && !self.distraction_free(cx)
```

### 3f. Distraction Free Mode hides the stripes

In `fn render`, replace

```rust
                // "Tool Window Bars" off hides the stripes; the windows stay reachable by action.
                let bars = crate::platform::settings::global(cx).appearance.show_tool_window_bars;
```

with

```rust
                // "Tool Window Bars" off hides the stripes, and so does Distraction Free Mode; the
                // windows stay reachable by action.
                let bars = self.shows_tool_window_bars(cx);
```

### 3g. The banner over a project in Restricted Mode

In `fn render`, replace

```rust
            .child(self.title_bar().clone())
            .child(div().flex_1().min_h_0().w_full().child(body))
```

with

```rust
            .child(self.title_bar().clone())
            .children(self.restricted_banner(cx))
            .child(div().flex_1().min_h_0().w_full().child(body))
```

### 3h. A project opened in Distraction Free Mode hides its tool windows

In `pub fn open_project`, after

```rust
                        layout.load(&scope, stored.as_deref());
```

add:

```rust
                        // In Distraction Free Mode the project's tool windows are hidden like the others'.
                        if app_actions::zen_layout_loaded(&mut layout, cx) {
                            workbench.show_distraction_free_hint(cx);
                        }
```

### 3i. `projects_open_in_this_window_a_new_one_or_the_one_that_has_them` trusts its folders (test)

It opens three undecided folders through `file.openProjectPath`, `file.openProject` and `file.openRecentProject` (and a New Window); with section 5b and 6 each would wait on "Trust and Open Project?". Replace

```rust
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        for root in &roots {
            std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        }
```

with

```rust
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        for root in &roots {
            std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        }
        // Decided folders open without the trust question.
        for root in &roots {
            cx.update(|cx| cx.global::<Backend>().host.trust.decide(root, true, false)).expect("trusted");
        }
```

### 3j. `a_window_with_nothing_open_welcomes_with_the_recent_projects` trusts its folder (test, new)

It reopens its undecided folder from the Welcome screen three times (Enter, a click on `welcome-row-0`), which goes through `WelcomeEvent::Open` → `windows::open_project_with_policy` → the open flow. Replace

```rust
        let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
```

with

```rust
        let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
        // A decided folder reopens from the Welcome screen without the trust question.
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
```

## 4. `workbench/chrome.rs`: Distraction Free Mode makes the title bar compact

In `fn sync_title_bar`, replace

```rust
                (TitleBarKind::Project, title_bar_mode(false, show_main_toolbar), None, Some(project))
```

with

```rust
                let mode = title_bar_mode(self.distraction_free(cx), show_main_toolbar);
                (TitleBarKind::Project, mode, None, Some(project))
```

## 5. `workbench/app_shell.rs`

### 5a. Restart IDE quits the way Quit does, then relaunches

Replace

```rust
pub(super) fn quit(window: &mut Window, cx: &mut App) {
    if quitting(cx) {
        return;
    }
```

with

```rust
pub(super) fn quit(window: &mut Window, cx: &mut App) {
    quit_then(window, cx, |cx| cx.quit());
}

/// [`quit`], ending with `end` once every window agreed to close: quitting, or quitting and
/// starting again (Restart IDE).
pub(super) fn quit_then(window: &mut Window, cx: &mut App, end: fn(&mut App)) {
    if quitting(cx) {
        return;
    }
```

and, at the end of that function (the only occurrence in the file), replace

```rust
        cx.update(|cx| cx.quit());
```

with

```rust
        cx.update(end);
```

The doc comment above `quit` (`/// File › Exit: …`) stays on `quit`.

### 5b. "Trust and Open Project?" before an undecided project opens in this window

In `async fn carry_out_open`, replace

```rust
            OpenStep::OpenProject { path } => {
                this.update_in(cx, |workbench, window, cx| {
                    // Leaving what the window showed was decided first: it saves nothing more.
                    workbench.leave_decided = true;
                    workbench.open_project(PathBuf::from(path), window, cx);
                    // An open that did not reach `leave` must not spare a later one its autosave.
                    workbench.leave_decided = false;
                })?;
                let opened = project_opened(this, cx).await?;
                cx.update(|_, cx| flow.project_opened(opened, i18n(cx)))?
            }
```

with

```rust
            OpenStep::OpenProject { path } => {
                // A folder nobody decided about is asked about first. Don't Open is a cancelled
                // opening: nothing is reported and the window keeps what it shows.
                let confirmed = super::app_actions::confirm_project_trust(this, cx, Path::new(&path)).await?;
                let opened = match confirmed {
                    Ok(true) => {
                        this.update_in(cx, |workbench, window, cx| {
                            // Leaving what the window showed was decided first: it saves nothing more.
                            workbench.leave_decided = true;
                            workbench.open_project(PathBuf::from(path), window, cx);
                            // An open that did not reach `leave` must not spare a later one its autosave.
                            workbench.leave_decided = false;
                        })?;
                        project_opened(this, cx).await?
                    }
                    Ok(false) => Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")),
                    Err(error) => Err(error),
                };
                cx.update(|_, cx| flow.project_opened(opened, i18n(cx)))?
            }
```

Why here and not before `RequestWindow` (the draft): the webview asks inside `openProject` (`src/platform/project.ts:57`), which runs after the window was chosen (`windowOpenProject`) and after `confirmLeaveProject()` (`openHere`, `src/features/app/windows.ts:54-61`). `OpenFlow::project_opened(Err(Cancelled))` already gives `Failed { cancel_close: true, notice: None }` (`app_models/windows.rs:379-389`, `open_failed_notice` drops a `Cancelled` error), the webview's `openFailed` that returns on `isCancelled`. A trust store that cannot be read (a folder that vanished) is reported like a failed open (`app.window.openFailed`), as the webview's thrown error is. `Path`, `PathBuf`, `IdeError` and `ErrorKind` are imported in `app_shell.rs` already.

### 5c. A window that is asking is still opening its project

`open_once_opened` (files to open in a new window once its project is there) waits through `project_opened` for `open_result()` to say how the opening ended, and `Content::Empty` says "cancelled". With 6.1 a new window for an undecided folder shows the Welcome screen under the question, so the files would be dropped before the answer. In `fn open_result`, replace

```rust
            Content::Empty | Content::Editor(_) | Content::Viewer(_) => {
                Some(Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")))
            }
```

with

```rust
            // The trust question of a project opening in this window is still waiting.
            Content::Empty if self.trust_dialog().is_some() => None,
            Content::Empty | Content::Editor(_) | Content::Viewer(_) => {
                Some(Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")))
            }
```

Trust Project and Preview in Restricted Mode open the project in the same update that closes the question (`finish_trust_prompt` with `TrustAnswer::Open`), so the observer next sees `Content::Opening` (still `None`), then the project; Don't Open closes the question with nothing opening, and the observer gets "cancelled", as before. `project_opened` in 5b runs only after `open_project`, so it never meets this arm.

## 6. `workbench/windows.rs`: a window opened for a project asks there

The webview's new window opens its project through its launch (`consumePendingLaunch` → `openHere` → `openProject`, `windows.ts:231-251`), so the question shows in the new window. Natively a window for a project is made by `open_window(Target::Project(..))` (New Window in the "Open Project" question via `request_window`, File › Open in New Window via `open_project(path, true, …)`, the `for_window == None` fallbacks of `app_shell::open_project_with_policy` / `open_project_explicit`, and `main.rs`'s command-line folder), none of which reaches `carry_out_open`'s `OpenProject` step.

### 6.1 Replace

```rust
                Target::Project(root) => workbench.open_project(root, window, cx),
```

with

```rust
                Target::Project(root) => workbench.open_project_asking(root, window, cx),
```

### 6.2 In `pub fn open_project`, replace

```rust
        Some(workbench) => workbench.update(cx, |workbench, cx| workbench.open_project(path.to_path_buf(), window, cx)),
```

with

```rust
        Some(workbench) => {
            workbench.update(cx, |workbench, cx| workbench.open_project_asking(path.to_path_buf(), window, cx))
        }
```

`Workbench::open_project_asking` (in `app_actions.rs`) reads the folder's trust synchronously (`host.trust.status`: the store is in memory; the open flow's `ReadTrust` step reads it on the main thread the same way, `app_shell.rs:757-763`). A decided folder opens at once, exactly as today (so `open_once_opened` and the `Content::Opening` title bar behave as before); an undecided one shows the question at once, and its answer opens the project in the same update (5c); Don't Open leaves the window on the Welcome screen; an unreadable trust is notified as `app.window.openFailed`. In 6.1 it runs inside `cx.new(|cx| …)` of the window's workbench, where `open_project` already spawns today (assumption 2), and the question is created there with `cx.new` and `cx.subscribe_in`.

## 7. `workbench/menu.rs`: Window › Tool Window Layouts lists the saved layouts

Stage 3.5 of `plugins_all_integration.md` adds to `fn state`'s `HashMap::from([…])` the line quoted below (already applied in the worktree when this was counted: `file=1 (applied)`). After

```rust
        (super::menu_spec::DYNAMIC_PLUGIN_TOOLS.to_owned(), super::plugin_contributions::plugin_tool_items(cx)),
```

add:

```rust
        (super::menu_spec::DYNAMIC_LAYOUTS.to_owned(), super::app_actions::layout_menu_items(cx)),
```

The `use super::menu_spec::{…}` list is not touched. `layout_menu_items(cx: &App)` borrows `cx` shared, like `plugin_tool_items`. Each item runs `layout.restore` with the layout's name, checked for the active one. `menu_spec.rs` needs nothing: File has New Project…, Invalidate Caches…, Manage IDE Settings › Export / Import Settings…, Power Save Mode; View › Appearance has Enter Distraction Free Mode; Window › Tool Window Layouts has Save Current Layout as…, `Entry::Dynamic(DYNAMIC_LAYOUTS)`, Restore Default Layout, Rename, Delete; Help has About OpenIDEA. `app.restart` and the five `project.*` ids have no menu place in the webview either (Find Action, the notification button, the dialog and the banner).

## 8. Required by dead-code analysis

`confirm_project_trust` is used only by 5b; `open_project_asking` only by 6.1 and 6.2; `trust_dialog` by 5c and the tests; `layout_menu_items` only by 7; `quit_then` by 5a and `carry_out_caches`; `shows_tool_window_bars`, `distraction_free`, `restricted_banner`, `zen_layout_loaded`, `show_distraction_free_hint` and `publish_app_context` only by 3 and 4.

## 9. Edits made to `workbench/app_actions.rs` in this reconciliation

1. `relaunch` falls back to `restart_app`, which is `cx.restart()` under `#[cfg(not(test))]` and a `panic!` under `#[cfg(test)]`: a test build can never restart or quit the process, and a test that forgets the `Relaunch` recorder fails instead of reaching `App::restart` (whose test platform does nothing, but whose restart observers would run).
2. `confirm_project_trust` returns `anyhow::Result<Result<bool, IdeError>>` (outer: the window went away; inner: the trust could not be read), so 5b reports an unreadable trust as a failed opening instead of dropping the opening silently.
3. New `Workbench::open_project_asking` (section 6). The waiting questions are `TrustPrompts(HashMap<WindowId, TrustAnswer>)` with `TrustAnswer::Flow(oneshot::Sender<bool>)` (the open flow's question) or `TrustAnswer::Open(PathBuf)` (a window that opens the project itself on the answer, in the answering update: 5c); `show_trust_prompt` takes the answer instead of returning a receiver.
4. `set_trust` unloads the project plugins of a folder that is no longer trusted (Preview in Restricted Mode, Revoke Trust) through the plugin host (`super::plugin_runtime::plugin_host`, made `pub(super)` by stage 3.3.1) and `PluginHost::restrict_project(root)`, `root` being the trust store's canonical folder; `remove_trusted_location` does it for every open project that lost its trust. This is what the Tauri adapters `project_trust_set` (`src-tauri/src/commands/app.rs:328-338`) and `project_trust_remove_location` (`app.rs:350-357`) do; the draft said there was no native counterpart, but `plugins.rs` now holds the host (`pub host: Arc<PluginHost>`). Without a `Plugins` global (tests, a build without plugins) nothing is unloaded. New imports: `super::plugin_runtime::plugin_host`, `ide_core::IdeError`, `ide_host::plugins::PluginHost`, `std::sync::Arc`, `crate::app_models::windows::open_failed_notice`.
5. Tests: the module's own `start` is dropped for `crate::workbench::tests::start` (identical body); the trust tests no longer call `confirm_project_trust` directly but open undecided temp folders through `file.openProjectPath` and answer the dialog by clicking (Don't Open, Preview in Restricted Mode, Trust Project with "Trust projects in"); a new-window test; New Project answers the trust question instead of hiding it behind a trusted location, proposes the default project directory and checks the refused folder is untouched; three new tests (Invalidate Caches sentinels, a refused import, the ⇧F12 condition). Two over-long lines wrapped.

The five `app_views/*.rs` are unchanged: every call was found in today's code (also for `app_actions.rs`: `title_bar_mode(distraction_free: bool, show_main_toolbar: bool)` in `title_bar.rs:173`, `f32::from(window.viewport_size().width)` as `services_window.rs:285` writes it, `show_menu`'s signature unchanged by `context_menus_reconciled.md` 1i; and in the views: `input::claim_keys` / `release_keys`, `TextField::{new, set_text, text}` and `TextFieldEvent::{Changed, Submitted, Cancelled}`, `app_models::shell::{BuildInfo, about_rows}`, `project_actions::{join_path, project_name_error}`, `app_models::invalidate_caches::{CachesEffect, InvalidateCachesDialog, just_restart}`, `ui::tooltip::WithTooltip::with_tooltip`, `ClipboardItem::new_string`, `I18n::{t, tx, tx_built}`), the GPUI style calls each have uses elsewhere in the crate (`underline`, `truncate`, `opacity`, `whitespace_nowrap`, `FontWeight::SEMIBOLD`, `occlude`, `deferred(..).with_priority`), and every theme token they name is in `src/styles/tokens.css` (measured).

## 10. Project trust: what the webview does, the seam for the tests, what the native app does

### 10.1 The webview (read)

- **When it asks.** `confirmProjectTrust(path, scope)` runs first in `openProject(path)` (`src/platform/project.ts:56-57`), in the window the project opens in, and the prompt (`src/features/project-trust/index.tsx:238-243`) asks only when `projectTrustStatus(path).decided` is false. There is no condition on the project's content (plugins, run configurations, build files): every undecided folder is asked about. `decided` is `trusted || restricted.contains(root)` and `trusted` is `!restricted ∋ root && (projects ∋ root || a trusted location is a prefix || inherited)` (`crates/ide-host/src/trust.rs:50-57, 449-454`). Other askers: Attach (`windows.ts:179`, which then refuses an untrusted folder), the Workspace dialog (`WorkspaceDialog.tsx:106`), a remote connection (`remote.ts:186`).
- **Where it asks.** Every user opening reaches `openProject` through `openHere`: Open…, Open Recent, the Welcome screen's rows and Enter, New Project (`NewProject.tsx:108`, `openProjectInWindow`), This Window. A project that opens in a new window (New Window, Open in New Window, the CLI) is asked about in that new window (its launch, `consumePendingLaunch`). The Welcome screen itself asks nothing: no project, no question.
- **The dialog.** "Trust and Open Project?" with the folder (middle-cut path), two paragraphs, a "Trust projects in <parent>" checkbox and three buttons: Preview in Restricted Mode (focused), Don't Open, Trust Project (default); closing it is Don't Open. The webview's name for the mode is Restricted Mode (not Safe Mode).
- **The answers.** Trust Project: `projectTrustSet(root, true, folder)` (with the parent as a trusted location when checked). Preview in Restricted Mode: `projectTrustSet(root, false, false)`, which records the folder as restricted: decided, so it is never asked again, and the project opens untrusted. Don't Open: the opening is cancelled (`IpcError { kind: "cancelled" }`), nothing is decided, nothing is reported, the window keeps what it showed.
- **Where it is stored.** `TrustStore::load(config, data)` → `<data>/project-trust.json` (`trust.rs:28-44`), written atomically. When the file does not exist, the recent projects are migrated as trusted. Natively `data` is `AppDirs::data`: `$OPENIDEA_GPUI_HOME/data` when the variable is set (`backend.rs:39-43`), the platform data directory otherwise; in tests `AppDirs::under(tempdir)`, so `<temp home>/data/project-trust.json` (measured by reading `Backend::start` → `Host::new(HostConfig { data_dir, … })` → `trust: TrustStore::load(&config_dir, &data_dir)`, `crates/ide-host/src/lib.rs:132`). A fresh temp home has no recent projects, so the migration trusts nothing.
- **What Restricted Mode forbids.** The Tauri invoke handler refuses, for a window whose project is untrusted, every command that `restricted_command_allowed` does not list (`src-tauri/src/lib.rs:170`, `trust.rs:206-438`): run, debug, build, tests, coverage, tasks, Docker, database (`db_*` except `db_console_path` / `db_export_file`), deployment, tool installs, every VCS write (commit, checkout, merge, rebase, push, fetch, stage…), plugin commands other than listing; plugin changes (`plugin_install_dir`, `plugin_set_enabled`…) are allowed only where no project is open. Allowed: files, editing, search, navigation, syntax, history, terminals (`term_`), settings, SSH, VCS reads (with hooks, fsmonitor, external diff, attributes and filters neutralised by `ide-vcs`, `cli.rs:210-224`, `391-400`). On a transition to untrusted the backend itself (`Host::refresh_project_trust`, `trust.rs:151-201`) turns language-server execution off, kills the project's processes, shuts its agents down, restricts notebooks, stops the debugger, cancels the window's requests and withdraws plugin run targets; the adapter also unloads project plugins (`restrict_project`). The banner: "Restricted Mode: project commands and tools are disabled." with Trust Project.

### 10.2 The seam that keeps the existing tests green

`Workbench::open_project` stays the low-level opening that asks nothing, as the backend's `open_project` IPC command does; the question lives in the user's opening paths (5b, 6.1, 6.2), exactly where the webview's `openProject` sits. Tests that go through those paths trust their temp folder first with `host.trust.decide(root, true, false)`: the store write `Host::set_project_trust` (what Trust Project calls, `trust.rs:146-149`) begins with, followed only by `refresh_project_trust`, which changes nothing for a folder that is not open yet. The decision lands in `<temp home>/data/project-trust.json` of the test's own `Backend::init(AppDirs::under(home))`, never under the user's home. This is the pattern 15 existing tests already use (`grep -rn 'trust.decide(&root, true, false)'`).

Tests in the build today that would meet the question (measured: `grep -rn` over `app-gpui/src` for `file.openProjectPath`, `file.openProject"`, `file.openRecentProject`, `file.openInNewWindow`, `open_project_with_policy`, `open_project_explicit`, `windows::open_project`, `Target::Project`, `WelcomeEvent::Open`, `welcome-row`, `welcome.recentProjects`, `vcs.clone`, plus the pending drafts' tests):

| Test                                                                                         | Path that asks                                                                                                                      | Hunk                                                                               |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `workbench/mod.rs` `projects_open_in_this_window_a_new_one_or_the_one_that_has_them`         | `file.openProjectPath` (here and New Window), `file.openProject` + path prompt, `file.openRecentProject` on three undecided folders | 3i                                                                                 |
| `workbench/mod.rs` `a_window_with_nothing_open_welcomes_with_the_recent_projects`            | Welcome screen Enter and row click on an undecided folder                                                                           | 3j                                                                                 |
| `workbench/mod.rs` `opening_another_project_here_asks_about_the_unsaved_changes_of_this_one` | `file.openProjectPath`                                                                                                              | none: both folders are trusted already (`trusted_project`, `decide(&other_root…)`) |

No other test reaches those paths. Every helper that opens a project in a test window calls `Workbench::open_project` directly and is unaffected: in `workbench/mod.rs` `trusted_project`, `search_project`, `git_project`, `run_project` and the bodies of the 28 other tests listed by the sweep (`a_project_shows_its_tree_and_opens_files_in_the_editor`, `tool_window_actions_show_focus_and_hide_the_project_view`, `context_keys_follow_the_project_the_editor_the_tool_windows_and_the_repository`, …); `workbench/tests/proofs_editor_search_safe_delete.rs` `usages_project`; `editor/intelligence.rs` `open`; `editor/intelligence/tests/proofs_editor_lsp.rs` `open_with`; `workbench/plugin_runtime.rs` `the_focused_editor_and_a_checkout_reach_the_plugin_which_shows_the_branch`; the uncommitted test code of `workbench/debug.rs` (two fixtures) and `workbench/plugins_window.rs` (one test); the drafts `workbench/context_menus.rs` `open`, `workbench/editor_core.rs` `project`, `workbench/editor_language.rs` `open`; `workbench/app_actions.rs` `project`. `vcs_operations.rs` (worktree Open) and `plugin_runtime.rs` (`PluginEvent::OpenProject`) run `file.openProjectPath` / `file.openInNewWindow`, but no test drives them; the constants `OPEN_RECENT_ACTION` and `welcome_model::OPEN_PROJECT` appear in tests only as menu or button lookups (`menu.rs` tests 682 and 703, `welcome.rs` view tests that record events), never run. No shared helper gets a hunk: none of them goes through the open flow.

### 10.3 The trust tests (in `app_actions.rs`)

- `offers_three_choices_cancels_without_a_trust_decision_and_supports_restricted_preview`: an empty window opens an undecided temp folder through `file.openProjectPath`; the dialog names it and draws its three buttons; Don't Open leaves the Welcome screen, decides nothing and reports nothing; the second opening answers Preview in Restricted Mode: the project opens restricted with the banner, stored `(decided, untrusted)`; after Close Project the third opening asks nothing.
- `saves_the_parent_folder_choice_and_reuses_a_persisted_decision`: Trust Project with "Trust projects in <parent>" checked opens the project trusted and stores both the project and its parent; a sibling then opens without a question, trusted.
- `a_project_opening_in_a_new_window_is_asked_about_in_that_window`: `{path, newWindow: true}` from a window without a project opens `project-1`, which asks; the first window asks nothing; Trust Project there opens the project trusted.
- `updates_the_running_projects_banner_after_a_backend_trust_transition` (kept): the banner's Trust Project turns execution on.
- `revoking_trust_or_removing_its_location_restricts_the_project`, `removing_a_trusted_location_restricts_the_projects_inside_it` (kept).
- `new_project_creates_the_folder_and_opens_it`: the created folder is undecided and is asked about, as in the webview.

### 10.4 What an untrusted project can do natively (read, except where marked measured)

| Capability                  | Webview in Restricted Mode                                                                            | Native today (with this file applied)                                                                                                                                                                                                                                                                                                                                                                                                      | Decision                                                                                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| Opening an undecided folder | asked first                                                                                           | asked first (5b, 6)                                                                                                                                                                                                                                                                                                                                                                                                                        | done                                                                                                                       |
| Plugins                     | project plugins not loaded (host policy) and unloaded on untrust; plugin commands refused by the gate | the host's project policy is set (`plugins.rs:179-181`); plugin calls pass `Host::authorize_command` (`plugin_runtime.rs:376, 818, 1508`, `plugin_contributions.rs:864, 919`); unload on untrust added (edit 4)                                                                                                                                                                                                                            | done                                                                                                                       |
| Language servers            | backend: execution off at open and on untrust (`project.rs:137`, `trust.rs:168`)                      | same backend code                                                                                                                                                                                                                                                                                                                                                                                                                          | done                                                                                                                       |
| Run configurations          | `run_*` refused by the gate; backend ops check too                                                    | the run driver lists none (`run/driver.rs:268`); `ops::run` checks trust (7 sites)                                                                                                                                                                                                                                                                                                                                                         | done                                                                                                                       |
| Debugger                    | refused; backend checks                                                                               | `debugger/driver.rs:160`; `ops::debug` checks trust (4 sites)                                                                                                                                                                                                                                                                                                                                                                              | done                                                                                                                       |
| Agents                      | refused; backend checks                                                                               | `agents.rs:356`; `ops::agent` checks (14 sites)                                                                                                                                                                                                                                                                                                                                                                                            | done                                                                                                                       |
| Git hooks and VCS writes    | writes refused by the gate; reads with hooks neutralised                                              | measured: every repository call of `ide-vcs` runs under the repository's execution flag (`ExecutionScope::enter`, 99 sites), which turns hooks, fsmonitor, external diff, attributes and filters off; the writes are refused by the backend ops themselves (34 `require_trust` calls in `ops/vcs/commands.rs`, among them `vcs_commit`, `vcs_checkout`, `vcs_merge`, `vcs_rebase`, `vcs_stage`, and `vcs_remote` for fetch, pull and push) | done; the unchecked ones (patch, shelf and change-list files, below) write files, not run git hooks                        |
| Database                    | `db_*` refused                                                                                        | measured: no trust check in `database/`, `workbench/database.rs` or `ops::db` (`ops::db::{test, databases, schemas, table_details, trust_ssh_host}` are called natively); a restricted project can connect and browse                                                                                                                                                                                                                      | difference for the integrator: gate `workbench/database.rs` on `project_restricted` or have `ops::db` call `require_trust` |
| Terminals, editing, search  | allowed                                                                                               | allowed                                                                                                                                                                                                                                                                                                                                                                                                                                    | done                                                                                                                       |
| Attach to workspace         | asked, then refused when untrusted                                                                    | Attach is not offered (`app_shell.rs` `ATTACH`)                                                                                                                                                                                                                                                                                                                                                                                            | not built                                                                                                                  |

The native app links the backend directly, so the IPC gate (`authorize_command` in `src-tauri/src/lib.rs:170`) never runs for the native UI's own calls; a restriction holds natively only where the backend operation or the native caller checks it. A heuristic sweep (measured: a Python scan of `crates/ide-host/src/ops/**` for `pub fn`s with neither `require_trust` nor `is_trusted` whose IPC name is not in `restricted_command_allowed`) lists, besides `db`: `vcs::{vcs_create_patch, vcs_shelf_delete, vcs_shelf_rename, vcs_shelf_import, vcs_changelists_save, …}`, `deploy::*`, `tools::{tools_install, tools_uninstall}`, `run::{save_configs, run_save_templates, …}`, `build::{gradle_link_projects, maven_exclude_dependency, …}`. Of those the native UI calls `ops::run::save_configs` (`run/driver.rs:583`, writing `.openidea` files, no execution) and none of deploy/tools/build-link (measured with `grep -rhno 'ops::\(deploy\|tools\|build\)::[a-z_]*' app-gpui/src`). To confirm per operation: `grep -n 'require_trust' crates/ide-host/src/ops/<area>.rs` against the native call sites.

## 11. Restart, Invalidate Caches, export / import, New Project

- **Restart.** Production: `relaunch` → `App::restart` → the macOS platform spawns `/bin/bash -c 'while kill -0 $pid; do sleep 0.1; done; open "$app_path" …'` and quits (`gpui-pre-macos-0.3.12/src/platform.rs:598-643`); `app_path` is the `.app` bundle when bundled, else `current_exe()`. Test double: the `Relaunch` global, a counter installed by `record_relaunches` (which also turns `general.confirmExit` off in the temp home); without it a test build panics (edit 1). Hypothesis to decide: `open` goes through LaunchServices, so a relaunched app may not inherit `OPENIDEA_GPUI_HOME` (a dev build would come back on the user's real directories) and an unbundled executable opens in Terminal. Check: run a dev build with `OPENIDEA_GPUI_HOME=/tmp/x`, Restart IDE, then `ps -E -p <new pid> | tr ' ' '\n' | grep OPENIDEA`. Tauri relaunches through its own restart flag (`windows.set_restart()` + `app_quit`, `src-tauri/src/commands/caches.rs:25-28`). Risk (read, not fixed): `quit_then` leaves `Quitting(true)` set once every window agreed; if `end` returns without quitting (the restart script fails to spawn, which GPUI only logs), later Exit and Restart IDE return at once. A reset would need GPUI to report the failure.
- **Invalidate Caches.** Scheduling writes `<data>/invalidate-caches.json` (`ide_workspace::caches::schedule`, in `Backend::dirs.data`; Tauri uses `app_data_dir()`); the next start applies it in `Backend::start` before any project opens (`backend.rs`, `apply_pending(&dirs.data, &dirs.local_data)`). It removes exactly (`caches::cache_dirs`, data ≠ local data): `<local-data>/index`; `<data>/history` (Local History checked); `<local-data>/lsp` (language servers); `<local-data>/cache` (build caches); then `<data>/index`, and `<data>/lsp` / `<data>/cache` for the same options. With `OPENIDEA_GPUI_HOME=H`: `H/local-data/{index,lsp,cache}`, `H/data/{history,index,lsp,cache}`. `invalidating_every_cache_removes_only_the_apps_cache_folders` checks all three boxes through the dialog, records the restart, runs `apply_pending` as the next start would, and asserts the removed list is exactly those seven while sentinels survive in `H`, `H/config`, `H/data`, `H/local-data/tools` and in a second temp directory (including an `index/` folder there).
- **Export / import.** The test names the file in a temp dir through `simulate_new_path_selection` / `simulate_path_prompt_response`; export writes only there (`SettingsStore::export_bundle(path, storage)`), import reads only it. Validation is the backend's (`import_bundle`, `crates/ide-workspace/src/settings.rs:316-330`): unparsable JSON → "not an OpenIDEA settings file: <serde error>"; another format or a newer version → "not an OpenIDEA settings file, or one from a newer version"; settings that do not merge → "invalid settings: …"; shown as the webview does (title "Cannot import settings", body the message, key `app.settingsImport`). `importing_a_file_that_is_not_a_settings_bundle_changes_nothing` covers the first two and asserts `config/settings.toml`, the setting and the UI state are untouched. Oversized: neither the webview nor the backend has a size limit for a `.json` bundle (`import_bundle` reads the whole file; the only limit, `JETBRAINS_MAX_TEXT`, is for JetBrains `.zip` entries), so none is added; a limit would be a product decision. The save dialog is offered `dirs::home_dir()` as its starting folder (only a suggestion; the test platform ignores it and nothing is read or written there).
- **New Project.** Creates `<location>/<name>` only after `fs_read_dir` says it is absent (created) or empty (reused), refusing a non-empty one with "<path> already exists and is not empty" in the dialog (`CreateProject::listed`, the webview's `createProjectFolder`). The test sets `general.defaultProjectDirectory` to a temp folder first, so even the proposed location is temp (otherwise the dialog proposes the user's home, as the webview does, without writing there), and leaves "Create Git repository" unchecked (the backend's `git init` would read this machine's git configuration).

## 12. Ids, conditions, labels, keys, menu places, layouts

Every id, title and `when` comes from `app-gpui/assets/gen/actions.json` through `registry.handle` (the manifest's metadata; `editor.toggleZen` is re-registered with the title Enter / Exit Distraction Free Mode as the mode flips, `distractionFree.ts:102-110`): `project.trust` / `project.revokeTrust` (shown), `project.previewRestricted` / `project.dontOpen` / `project.removeTrustedLocation` (hidden); `layout.save`, `layout.restore`, `layout.restoreDefault` (`when: projectOpen`), `layout.rename`, `layout.delete` (`when: toolWindowLayoutsSaved`); `file.togglePowerSave` (toggle); the rest without `when`. Default keys (`app-gpui/assets/gen/keymaps.json`, generated from `src/platform/keymaps/`, measured): only `layout.restoreDefault` ⇧F12 (IntelliJ and Eclipse keymaps, macOS and Windows) and `editor.toggleZen` ⌘K Z / Ctrl+K Z (VS Code keymaps only). `the_restore_default_layout_key_works_only_with_a_project` presses ⇧F12 in a window without a project (nothing happens: the saved layout stays active, and the action is unavailable through the registry) and then in a project (the default layout comes back). `editor.toggleZen` has no `when`, so there is no false condition for its chord. Execution goes through `execute_action`, which checks `when` (`platform/actions.rs:227-237`).

Window layouts (read from `src/workbench/toolwindows/layouts.ts` and `actions.ts`): the named layouts are shared by every project in the UI storage key `openidea.toolWindowLayouts.v1` as `{layouts: [{name, layout}], active}`; Save Current Layout as… replaces a layout of the same name and makes it active; Restore Tool Window Layout applies a saved one (leaving Distraction Free Mode first) and makes it active; Restore Default Layout applies `defaultLayoutSnapshot()` and clears `active`; Rename refuses an empty or taken name; Delete asks first. The project's own current layout stays under `openidea.layout.v1:<root>` (`tool_windows.rs:18, 44`). The webview has no "Store Current Layout as Default" action (measured: `grep -rni 'storeDefault\|Store Current Layout\|layout as default' src` finds only `menu.layout.restoreDefault` in the locales), so the default layout is the built-in one and nothing persists it.

## 13. Not wired, and why

- The Trust settings page (trusted projects and locations with Revoke / Remove / Add folder…): it belongs to the Settings dialog's pages; the actions it runs are registered.
- The banner and Trust Project for an attached workspace member that lost its trust: workspace attachment is not built.
- The layout chooser is a popup menu of the saved names (no title row). Rename with a taken name says "A layout with this name exists" after OK, not under the field (`FieldRule` has no such rule).
- About: no logo (no app-logo icon in `icons.json`); the Runtime row says `GPUI` without a version; Keymap Reference and Show Log links appear only when those actions are registered (they are not).
- New Project's Browse… cannot start in the typed folder (`PathPromptOptions` has no starting folder).
- Import of a JetBrains `.zip` reads it as a bundle and fails with the backend's message; the file dialogs have no type filter. The Tauri adapter's MCP re-apply and update-checker wake on import have no native counterpart.
- The Power Save Mode banner above editors; the menu label of Distraction Free Mode stays "Enter" (`menu_spec::Entry` labels are fixed).
- Database access in a restricted project (section 10.4).

## 14. Anchors in regions the compiling agent is likely to touch

It is applying the plugin stages in this worktree now (stage 1, stage 2 and stage 3.5 were in the worktree, uncommitted, when counted). Re-run `python3 -I /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/app/anchors.py` right before pasting. In `workbench/mod.rs`: `register_actions` (3b, next to stage 1.3.4's line), the `Render` layer list right under the title bar (3g; stages 1.3.5, 2.2.8 and 3.4.6 edit the lines below it), the file-opening task of `open_project` (3h; stage 3.4.5 edits `watch_debug_editor` in the same task), `publish_context` (3c), the backend-event match (3d; stage 2 edits `plugin_backend_event`, not this match) and the two tests in `mod tests` (3i, 3j). `workbench/menu.rs` `fn state` (7) is in the compiling agent's list. `chrome.rs` (4) has uncommitted edits (stage 2.3, `status_bar_event`, not `sync_title_bar`). `app_shell.rs`, `windows.rs`, `lib.rs` and `platform/storage.rs` are outside the compiling agent's list.

## 15. Assumptions to check, in order

1. Stage 3.5 is in `menu.rs` as quoted (section 7 anchors on it; it was applied in the worktree when counted).
2. Creating the question inside `cx.new(|cx| { Workbench::new(…); … })` of `open_window` (6.1) works: `show_trust_prompt` there calls `cx.new` (the dialog), `cx.subscribe_in` and `open_vcs_dialog` (`window.focused`, `window.focus`) on the workbench being built. Partly settled by reading: the line 6.1 replaces, `workbench.open_project(root, window, cx)`, already calls `cx.spawn_in(window, …)` and `cx.new` there today.
3. `crate::workbench::tests::{start, click_on, run_action, wait_long, notification_titles}` (`pub(super)` in the private `workbench::tests`) and the private `Workbenches` with its field are reachable from `workbench::app_actions::tests` (a descendant of `workbench`).
4. An empty window opening a folder reaches `OpenStep::OpenProject` without any question (the existing multi-window test shows it does), and `file.closeProject` with nothing unsaved goes back to the Welcome screen synchronously.
5. The default keymap of the test build binds ⇧F12 (`shift-f12`) to `layout.restoreDefault` (IntelliJ on both hosts), and a key in a window without a focused editor reaches the keymap dispatcher (`the_new_agent_task_and_debugger_shortcuts_need_a_project` relies on the same).
6. In the layouts test, Enter chooses the first row of the popup `show_menu` opens, and `TextField::set_text` leaves the caret at the end (read: it does), so the backspaces clear the prompt.
7. `TitleBar::mode()` follows `set_mode` once the window has redrawn after the toggle (section 4 runs in `sync_title_bar`).
8. `Host::apply_proxy` in the import test sets the process-wide proxy from the temp home's default settings (no proxy), which is what every other test process already has.

## 16. Compile risks

- 5b: `super::app_actions::confirm_project_trust` returns `anyhow::Result<Result<bool, IdeError>>`; the `match` arms all evaluate to `Result<(), IdeError>` (`project_opened(this, cx).await?` is that type).
- `set_trust` and `remove_trusted_location`: the `call` closures end in `ide_core::IdeResult::Ok(())` so that `?` inside the `async move` infers its error type; they capture `Option<Arc<PluginHost>>` (Send) and move `path` after `host.trust.status(&path)` has returned.
- `if !trusted && let Some(plugins) = plugins` (a let chain, Rust 2024).
- `open_project_asking`: `return self.open_project(root, window, cx)` in a guarded arm moves `root`, which the later arms and the tail use (each path moves it once); `open_failed_notice(&shown, …)` with `shown: Cow<str>`, the notice bound before `self.notify(notice, cx)` so the `Localization` borrow has ended.
- `finish_trust_prompt`'s `match` on `TrustAnswer` (`Some(TrustAnswer::Open(root)) if open` moves `root` out of the removed entry); `show_trust_prompt` destructures the replaced entry with `if let Some(TrustAnswer::Flow(previous)) = …insert(…)`.
- 5c: `self.trust_dialog()` inside `open_result(&self)` (it needs no `cx`).
- `restart_app` exists twice under complementary `cfg`s; `cx.restart()` needs `&mut App` (it is `pub fn restart(&mut self)`).
- `AsyncApp::update(end)` with `end: fn(&mut App)` (5a).
- `subscribe_in` handlers that destructure `*event` (`TrustDialogEvent::Run(action)`): the field is `&'static str`, so it copies.
- In the new-window test, `VisualTestContext::from_window(handle, cx)` takes `&TestAppContext` and clones it, so two of them and `cx` are used in turn.

## 17. Tests most likely to fail first, and why

1. `a_project_opening_in_a_new_window_is_asked_about_in_that_window`: depends on 6.1 and assumption 2, on `windows::windows` listing the new window as `project-1`, and on clicking in a window that is not the active one. No test covers files opened with a new-window opening of an undecided folder (5c): measured, every caller of `app_shell::open_project_with_policy` passes `Vec::new()` today (`windows.rs:138`) and `OpenFlow::explicit` takes none, so 5c guards a path no action reaches yet.
2. `saves_and_restores_visible_windows_and_sizes_by_name`: assumption 6 and the popup taking the keyboard.
3. `the_restore_default_layout_key_works_only_with_a_project`: assumption 5.
4. `offers_three_choices_…` and `saves_the_parent_folder_choice_…`: they depend on 5b and on `file.closeProject` (assumption 4); the notice check looks for the folder name in notification titles.
5. `invalidate_and_restart_…`, `just_restart_…`, `invalidating_every_cache_…`: they depend on 5a; the quit loop decides every window's close with `confirmExit` off; a leftover process in a window would ask first.
6. `exports_templates_macros_layouts_and_view_options_not_per_project_state`: `import_bundle` rewrites the temp home's `settings.toml` and `apply_proxy` runs in the test process (assumption 8).
7. `hides_tool_windows_and_restores_the_previous_layout_on_exit`: depends on 3e, 3f and 4, and assumption 7.
8. The two `mod.rs` tests of 3i and 3j fail (time out on the dialog) if 5b or 6 is applied without them.

## 18. Actions: end-to-end test, IPC commands, status

| Action id                       | End-to-end test                                                                                                                                                                                                                                            | IPC commands it stands for (`src-tauri/src/commands/` adapter → backend function)                                                                                                                | Status                                                                                                                                     |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `project.trust`                 | `saves_the_parent_folder_choice_and_reuses_a_persisted_decision`, `a_project_opening_in_a_new_window_is_asked_about_in_that_window`, `updates_the_running_projects_banner_after_a_backend_trust_transition`, `new_project_creates_the_folder_and_opens_it` | `project_trust_set` (`app.rs:328`) → `Host::set_project_trust`; the question: `project_trust_status` (`app.rs:316`) → `TrustStore::status`                                                       | partial: no attached-member path (workspace attachment not built)                                                                          |
| `project.previewRestricted`     | `offers_three_choices_cancels_without_a_trust_decision_and_supports_restricted_preview`                                                                                                                                                                    | `project_trust_set(path, false, false)` → `Host::set_project_trust` + `PluginHost::restrict_project`                                                                                             | done                                                                                                                                       |
| `project.dontOpen`              | `offers_three_choices_…`                                                                                                                                                                                                                                   | none (the opening is cancelled)                                                                                                                                                                  | done                                                                                                                                       |
| `project.revokeTrust`           | `revoking_trust_or_removing_its_location_restricts_the_project`                                                                                                                                                                                            | `project_trust_set(path, false, false)` → `Host::set_project_trust` + `restrict_project`                                                                                                         | partial: no Trust settings page to run it from a row                                                                                       |
| `project.removeTrustedLocation` | `removing_a_trusted_location_restricts_the_projects_inside_it`                                                                                                                                                                                             | `project_trust_remove_location` (`app.rs:350`) → `TrustStore::remove_location` + `Host::refresh_project_trust` + `restrict_project` per untrusted project                                        | partial: no Trust settings page; Add folder… (`project_trust_add_location`) not offered                                                    |
| `layout.save`                   | `saves_and_restores_visible_windows_and_sizes_by_name`                                                                                                                                                                                                     | none (UI storage `openidea.toolWindowLayouts.v1`)                                                                                                                                                | done                                                                                                                                       |
| `layout.restore`                | `saves_and_restores_…`, `restores_by_name_through_the_action_window_tool_window_layouts_items`                                                                                                                                                             | none                                                                                                                                                                                             | partial: the chooser is a popup menu, not a titled list dialog                                                                             |
| `layout.restoreDefault`         | `saves_and_restores_…`, `the_restore_default_layout_key_works_only_with_a_project`                                                                                                                                                                         | none                                                                                                                                                                                             | done                                                                                                                                       |
| `layout.rename`                 | `saves_and_restores_…`                                                                                                                                                                                                                                     | none                                                                                                                                                                                             | partial: a taken name is reported after OK, not under the field                                                                            |
| `layout.delete`                 | `saves_and_restores_…`                                                                                                                                                                                                                                     | none                                                                                                                                                                                             | done                                                                                                                                       |
| `editor.toggleZen`              | `hides_tool_windows_and_restores_the_previous_layout_on_exit`                                                                                                                                                                                              | none (UI storage `openidea.distractionFree`)                                                                                                                                                     | partial: menu label stays "Enter"; 3 of about 12 `distractionFree.test.tsx` cases                                                          |
| `app.about`                     | `about_shows_the_version_of_the_build`                                                                                                                                                                                                                     | `app_build_info` (`window.rs:375`), `app_info` (`app.rs:27`)                                                                                                                                     | partial: no logo, no runtime version, no Keymap Reference / Show Log links                                                                 |
| `file.newProject`               | `new_project_creates_the_folder_and_opens_it`                                                                                                                                                                                                              | `fs_read_dir` (`fs.rs:42`) → `ops::fs::read_dir`; `fs_create_dir` (`fs.rs:62`) → `ide_vfs::create_dir`; `vcs_init` (`vcs.rs:509`) → `ops::vcs::vcs_init`; `window_open_project` (`window.rs:19`) | partial: Git init not run end to end (machine git config); Browse… cannot start in the typed folder                                        |
| `file.invalidateCaches`         | `invalidate_and_restart_schedules_the_chosen_caches_and_restarts`, `invalidating_every_cache_removes_only_the_apps_cache_folders`                                                                                                                          | `caches_schedule_invalidation(options)` (`caches.rs:11`) → `ide_workspace::caches::schedule`; `app_restart` (`caches.rs:25`)                                                                     | done (restart through the seam)                                                                                                            |
| `app.restart`                   | `just_restart_cancels_a_pending_invalidation_cancel_does_nothing`                                                                                                                                                                                          | `caches_schedule_invalidation(null)` → `caches::cancel`; `app_restart` → `set_restart` + `app_quit`                                                                                              | partial: the production relaunch (`App::restart`) is not exercised; the environment of the relaunched process is a hypothesis (section 11) |
| `file.exportSettings`           | `exports_templates_macros_layouts_and_view_options_not_per_project_state`                                                                                                                                                                                  | `settings_export` (`app.rs:216`) → `SettingsStore::export_bundle`                                                                                                                                | partial: no file-type filter in the save dialog                                                                                            |
| `file.importSettings`           | `exports_templates_…`, `importing_a_file_that_is_not_a_settings_bundle_changes_nothing`                                                                                                                                                                    | `settings_import` (`app.rs:236`) → `SettingsStore::import_bundle`, then each project's settings, file defaults and language servers, SSH configurations, proxy, `SettingsChanged`                | partial: JetBrains `.zip` import, MCP re-apply and update-checker wake missing                                                             |
| `file.togglePowerSave`          | `power_save_mode_toggles_the_global_setting`                                                                                                                                                                                                               | `settings_set("global", "general.powerSaveMode", …)` (`app.rs:181`)                                                                                                                              | partial: no Power Save banner above editors                                                                                                |
