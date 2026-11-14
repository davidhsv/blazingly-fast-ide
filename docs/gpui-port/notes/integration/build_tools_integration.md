# Integrating the Maven and Gradle tool windows into the workbench

New files, not compiled yet:

- `app-gpui/src/build_tools/mod.rs` (7 lines): `pub mod execute; pub mod model; pub mod view;`
- `app-gpui/src/build_tools/model.rs` (1565): the tree, labels, icons, run configurations, completions, stored settings and history; 17 `#[test]`s.
- `app-gpui/src/build_tools/view.rs` (999): `BuildToolView`, the tool window's view; 4 `#[gpui_kit::test]`s.
- `app-gpui/src/build_tools/execute.rs` (714): `ExecuteState` and `ExecutePopup`, Execute Maven Goal / Execute Gradle Task; 10 `#[test]`s.
- `app-gpui/src/workbench/build_tools.rs` (1577): `impl Workbench` blocks, the actions, the backend calls and 8 end-to-end tests.

No existing file was edited. Every anchor below is quoted from the file as it is now; line numbers are left out because `mod.rs` is moving. Several pasted lines run past 120 columns (56 in the new files), so run `cargo fmt` once after pasting.

## 1. `app-gpui/src/lib.rs`

After `pub mod build_window;` add:

```rust
pub mod build_tools;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod build_window;` add:

```rust
mod build_tools;
```

### 2b. The field

In `pub struct Workbench`, after

```rust
    /// The project's build.
    build: Option<build_window::BuildSlot>,
```

add:

```rust
    /// The Maven and Gradle tool windows: the project's build model, its syncs and toggles.
    build_tools: build_tools::BuildToolsSlot,
```

### 2c. Its initial value

In `Workbench::new`, after `            build: None,` (it occurs once) add:

```rust
            build_tools: Default::default(),
```

### 2d. The actions

In `pub fn register_actions(cx: &mut App)`, after `    build_window::register_actions(registry);` add:

```rust
    build_tools::register_actions(registry);
```

### 2e. Reading the model when a project opens

In `open_project`'s spawned continuation, after `                workbench.mount_agents(window, cx);` (it occurs once) add:

```rust
                workbench.mount_build_tools(window, cx);
```

### 2f. Forgetting it when the project goes

`        self.reset_build(window, cx);` occurs twice, once in `pub fn close_project` and once in `pub fn open_project`. After each of them add:

```rust
        self.reset_build_tools(window, cx);
```

### 2g. The mount arm

In `fn mount_panels`, add this arm directly before

```rust
                FIND_TOOL_WINDOW => {
                    use crate::find::view::{FindPanel, FindPanelEvent};
```

```rust
                crate::build_tools::model::MAVEN_TOOL_WINDOW | crate::build_tools::model::GRADLE_TOOL_WINDOW => {
                    match self.mount_build_tool(&id, window, cx) {
                        Some(panel) => panel,
                        None => continue,
                    }
                }
```

### 2h. Backend events

In `pub fn backend_event`, replace

```rust
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        let now = now_ms();
```

with

```rust
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        // Build files, project folders, Java settings and trust: the build model is read again.
        self.build_tools_backend_event(&event.event, cx);
        let now = now_ms();
```

It must come before the `match`, because the `FsChanged`, `SettingsChanged` and `ProjectTrustChanged` arms `return` early.

### 2i. `TOOL_WINDOWS` is not edited

This departs from the literal request, on purpose. In the webview, the Maven window is registered only while the project has a POM (`actions.test.tsx` › "registers the Maven tool window only while the project has a POM"), and the Gradle window only while it has a linked build or once it was shown to link one. An entry in `TOOL_WINDOWS` would register both windows, and their `toolwindow.*` actions, for every project.

Instead, `workbench/build_tools.rs` does what `plugin_tool_windows.rs` does:

- it calls `layout.register(tool_window_declaration(id))` and `layout.unregister(registration)` inside `with_layout`, and keeps the `Registration`;
- it registers `toolwindow.maven` and `toolwindow.gradle` itself, with `when: projectOpen && build.maven` and `when: projectOpen && build.gradle`. They match the `toolwindow.` dynamic entry of `smoke_registered_actions`.

`tool_window_declaration("maven")` and `tool_window_declaration("gradle")` read the existing `registerToolWindow` entries of `contributions.json`: Maven, `toolwindow-maven`, right, order 10; Gradle, `toolwindow-gradle`, right, order 11.

## 3. `app-gpui/src/workbench/plugin_runtime.rs`: Cancel on a sync's status bar task

A sync's task is `TaskId::Name("build-sync:maven")` or `TaskId::Name("build-sync:gradle")`, with `Cancel::Producer`. In `pub fn cancel_background_task`, add this arm before `            Some(CancelEffect::Producer(other)) => tracing::warn!("task {other:?} has no producer to stop it"),`:

```rust
            Some(CancelEffect::Producer(TaskId::Name(name)))
                if name.starts_with(super::build_tools::SYNC_TASK_PREFIX) =>
            {
                self.cancel_build_sync_task(&name, cx);
            }
```

Without this hunk, `cancel_build_sync_task` is dead code and `-D warnings` fails. If the hunk cannot be applied, delete that method, and set `cancel: Change::Clear` in `sync_build_tool` and in `build_sync_progress`.

## 4. Menu entries

None are added:

- The toolbar and the tree's context menus are the views' own.
- The `gradle.analyzeDependencies` and `maven.analyzeDependencies` menu groups of the editor, editor tab and Project view are not registered, because the Dependency Analyzer is not built. Their rows stay hidden, as an unregistered action's do.
- The menus' action rows (`maven.unlinkProjects`, `maven.analyzeDependencies`, `gradle.analyzeDependencies` and the like) are left out by `BuildToolView::context_menu` whenever the action is not registered.

## 5. The stand-in tools (tests only)

- **Maven:** `FAKE_MVN` is written as `<tools>/mvn` (tools is a temp dir outside the project) and made executable. The project's `.openidea/settings.toml` names it as `languages.java.maven.home`. It also sets `userSettings = <tools>/settings.xml` (`<settings/>`) and `localRepository = <tools>/m2`. `ide_sdk::apply::maven_command` returns the home itself when it is a file, so the sync and `prepare_maven_run` both start the stand-in.
  - It touches `ran`, then prints `stand-in mvn`, `cwd <pwd -P>` and one `arg <a>` line per argument.
  - With `help:effective-pom` in its arguments (the sync), it prints `[INFO] Building core 1.0 [1/2]` and waits while `<tools>/hold` exists. It fails with an `[ERROR]` line if `sync-fails` exists. Otherwise it prints a `--- …:tree … ---` section for `demo:core` with one test dependency.
- **Gradle:** `FAKE_GRADLEW` is the build's `gradlew`, which `gradle_command` picks over `gradle`.
  - It prints `stand-in gradle`, `cwd …` and one `arg …` line per argument.
  - With `tasks` in its arguments, it waits while `<root>/hold` exists. With `<root>/sync-fails` it prints Gradle's `FAILURE:` / `* What went wrong:` / `> …` lines and exits 1. Otherwise it prints two `@openidea-model\t{"path":…,"dir":<pwd -P>}` lines (without them a successful sync is `stale` at once) and a `Build tasks` section with `assemble` and `app:customJar`.
- **Waiting:** no test sleeps. The scripts poll a marker the test removes, and the tests wait with `wait_long`.

## 6. Isolation: what is read outside the temp folders (measured by reading code, not by running)

- **Maven's default local repository:** `ide_build::sync_maven` calls `maven::default_local_repo()` unconditionally (`crates/ide-build/src/sync.rs`, `load_with_linked(root, maven::default_local_repo(), …)`). That reads `~/.m2/settings.xml` when it exists, through `dirs::home_dir()`, which is `$HOME`. The only seam is `HOME`, which is process-wide; the tests do not set it. The read only looks for `<localRepository>`, used to find parent POMs outside the project; the fixtures have none. To make the sync fully hermetic, `sync_maven` should take the configured repository from `SyncOptions.maven_options` (`-Dmaven.repo.local=`), as it already does for plugin goals. That is a backend change, not made here.
- **Installed JDKs:** `ide_sdk::apply::gradle_build_env` and `maven_build_env` call `installed_jdks()`, which reads the system's JDK folders. No JDK is run. The fixtures declare no Java version, so `missing_jdk_message` returns `None` and never prefixes a sync error.
- **The Maven paths:** `load_linked` reads the user `settings.xml` and the local repository through the settings above, so the tests never touch `~/.m2`. Nothing reads `~/.gradle`: `GRADLE_USER_HOME` is only passed to the stand-in.

## 7. Project trust (read in `crates/ide-host/src/trust.rs` and `ops/build.rs`)

- **Refused by the op itself** (`host.require_trust` / `project.require_trust`): `build_model`, `gradle_link_projects`, `gradle_unlink_projects`, `build_sync`, `gradle_dependencies`, `build_run`, and `run::start` (so running a phase or task too).
- **Not refused by the op** (`gradle_published_catalog`, `maven_dependency_declaration`, `maven_goto_declaration`, `maven_exclude_dependency`): only the IPC layer refuses them, because `restricted_command_allowed` does not list them. Native callers must check `project_restricted` first. `maven_exclude_dependency` writes the POM. None of the four is called by this draft.
- **IPC allow-list:** `run_gradle_jvm_args_script` is on the Restricted Mode allow-list; `run_gradle_init_script` is not. Hypothesis, not read: the op `run_gradle_init_script` itself does not call `require_trust`. Check with `grep -n "require_trust" crates/ide-host/src/ops/run.rs` around `pub async fn run_gradle_init_script`.
- **Natively:** `build_model` is refused, so the model is `None`, no tool window is registered and `build.maven` / `build.gradle` stay false. `gradle.linkProjects` (`when: projectOpen && !workspaceOpen`) returns before asking for a file. The test `does_not_link_or_start_gradle_imports_in_an_untrusted_project` asserts each of these:
  - Reload is unavailable;
  - Link asks for nothing;
  - the op `build_sync` called directly returns exactly "Project is in Restricted Mode. Trust the project to enable execution.";
  - the stand-in never ran;
  - a generated Maven goal started through `run.runConfig` ends as a refused (not live) session, and the stand-in still has not run;
  - after Trust Project on the banner, `ProjectTrustChanged` reads the model and `build.maven` turns on.

  Assumption: a refused start ends in `start_failed` (a Failed session), not in a trust prompt through `RunDriver::ask`. If it prompts instead, answer it with `cx.simulate_prompt_answer("Cancel")`.

## 8. Assumptions (not confirmed by running; check these first)

1. `crate::workbench::tests::{start, wait_long, run_action, click_on, notification_titles}` are visible from `workbench::build_tools::tests`. They are `pub(super)` inside `workbench::tests`, the same way `github.rs` imports them.
2. **The static model of the fixtures.** If these do not hold, test 1's `labels` assertions fail first:
   - read in `maven.rs`, so this one is not an assumption: a project is named by `<name>`, falling back to `artifactId` (`display_name: pom.name … unwrap_or_else(|| artifact_id.clone())`), so the reactor is "Multi Demo" and the app module "Demo App";
   - read in `gradle.rs` `parse_settings`: Gradle's root project is named by `rootProject.name` ("demo");
   - the root build's `dir` is `""`;
   - `app/build.gradle` with `plugins { id 'java' }` gets the default `build`, `verification` and `help` tasks (`gradle::default_tasks`).
3. **Detected run configurations.** `ide_run::detect` may list Maven or Gradle configurations for these fixtures. They show as a "Run Configurations" folder, so the tests assert the first and last rows rather than the whole list.
4. **The Run window.** `run.runConfig` with `{config, temporary: true}` shows the Run window, and `RunView::lines` lists the stand-in's output. A Maven `test` with Skip Tests on, a Gradle `assemble` or `jar`, and `clean` are not test runs, so `finalize_launch` adds no test-reporting arguments or init script. A Gradle `test` task would get `--init-script`, which is why the tests avoid it.
5. **Maven options.** `prepare_maven_run` resolves `mvn` to the configured home and splices Settings › Maven's options (`-s`, `-Dmaven.repo.local=`) in front of the goals. The tests therefore assert only the last arguments.
6. **The fake sync's dependencies.** After the fake sync the cache's hash matches the model's build files, so the build is not stale. `maven::build_files` is the same whichever repository `sync_maven` used. If test 2 sees a Stale banner, drop its `banners().is_empty()` assertion.
7. **Gradle linking.** `gradle::build_dirs` stops at the root's `settings.gradle`, so `libs/other` is not discovered before it is linked. Linking `<root>/libs/other/settings.gradle` adds it; its `dir` is `libs/other`, and the test reads it from the row.
8. **Dialogs and popups.**
   - `open_vcs_dialog` / `close_vcs_dialog` / `vcs_prompt` are used for the unlink confirmation; that layer is drawn whether or not the project is a repository.
   - `open_popup` / `close_popup` are used for the Execute popup.
   - Enter on the confirmation presses Unlink, and Escape cancels it.
9. **Key claims.** `input::claim_keys` sees Enter, Tab and the arrows before the `TextField` in the Execute popup. The window is activated at the start of each test.
10. **Unlinking the root build.** Unlinking the root build (`dir` `""`) through `gradle_unlink_projects([""])` leaves `linkedProjects` empty, so `load_linked` returns no build. This is checked in the links test.
11. **After a real Maven sync,** `ops::build::build_sync` calls `kotlin_bridge.configure_maven` when the project has a bridge, and `lsp.java_update_project_configs` for the updated POMs. Hypothesis: in Light mode neither starts a JVM or a language server. Check `crates/ide-host/src/kotlin_bridge.rs` `configure_maven`, and that no language server runs in Light mode.
12. **Toggle state.** It is read from window context keys (`build.maven.offline`, `build.maven.skipTests`, `build.gradle.offline`) that `publish_build_toggles` sets. `handle_toggle`'s closure cannot read the workbench while a menu is built inside its update.

## 9. Compile risks

- **Closure-call coercions:** `&String` → `&str` and `&&GradleModel` → `&GradleModel` in `model.rs::gradle_run_configs`. If the compiler rejects them, add `.as_str()` or `*`.
- **`BuildToolView::render_tree`:** captures `cx` inside `.when(checked, …)` within the `cx.processor` closure.
- **`futures::select!` in `sync_build_tool`:** a copy of `build_window.rs`'s loop.
- **`futures::executor::block_on`** in the restricted test: it needs the `executor` feature, which is a default feature of `futures`.
- **The `toolbar()` button list:** it uses `if … { "a" } else { "b" }.to_owned()` in a field initialiser. Wrap it in parentheses if the parser objects.
- **`Box::leak` selectors** in tests: this is the existing pattern.

## 10. Tests most likely to fail on first run, and why

1. **`runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options` (e2e):** it depends on assumptions 2 to 5. The first failure would be the row labels or the "arg" lines.
2. **`reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`:** it depends on `parse_dependency_tree` accepting the stand-in's section, with the root line `demo:core:jar:1.0` and no spaces, and on assumption 6.
3. **`links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`:** it depends on assumption 7, and on the reload after `SettingsChanged` (300 ms timer, advanced by `wait_long`) not reintroducing the build.
4. **`execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent`:** it depends on the key claims (assumption 9). If Enter reaches the `TextField` first, nothing runs.

## 11. What the webview does that is not built

- **Unregistered actions:**
  - Maven Add / Unlink Projects (`maven.addProjects`, `maven.unlinkProjects`): `settingsSet` of `languages.java.maven.unlinkedProjects` / `linkedProjects`, plus a file dialog.
  - The Dependency Analyzer (`maven.analyzeDependencies`, `gradle.analyzeDependencies` and the editor kind `maven.dependencyAnalyzer`): no native model exists, and the webview's analyzer is 645 + 239 lines with Exclude, Go to declaration and a `gradle_dependencies` run per configuration.
  - Their two menu groups.
- **Automatic syncs:**
  - the Deep-mode first import of a never-synced build (`firstImport.test.ts`): Full mode starts language servers, so a hermetic test cannot cover it;
  - the reload 800 ms after a profile toggle (`profileReload.test.ts`): toggles are kept and change `-P` on runs, but nothing reloads;
  - "Reload after external changes" of a stale build (`autoReload.test.ts`).
- **The Execute popup:** its project chooser is a row of clickable names instead of a select.
- **Notifications:** no Download JDK button on a sync error (`downloadJdkActions`). The Gradle "Open Gradle Settings" / "Choose Gradle JVM" buttons are left out, but the hint text is kept. `file.settings` does take the page as its argument; the limit is that a notification button (`NotificationAction`) has only `label` and `command`, with no argument. Maven's button runs `maven.settings`.
- **Other contributions:** native Workspace members (`workspace.test.tsx`); Run Anything's `mvn …` / `gradle …` provider (`runAnything.ts`); the POM navigation and version-catalog editor contributions.
- **Run configuration types (already native, not mine):** `gradle` and `maven` exist in `app-gpui/src/run/types.rs` (`gradle_type()` and `maven_type()` through `build_tool_type`, with wrapper and JVM fields). Both are in `TYPES` of `edit_configurations_adds_and_saves_a_configuration_of_every_built_in_type` (`workbench/mod.rs`) and in `registers_the_builtin_types`. This draft adds nothing there.
- **Shortcuts:** measured with `grep -rn "maven\.\|gradle\." src/platform/keymaps/`, which found nothing. No build tool action has a default shortcut, so there is no key condition to test.
- **`toolwindow.gradle` after the last unlink:** its `when` (`projectOpen && build.gradle`) is false while the window stays registered, so the stripe button still shows it but Find Action does not. In the webview `registerToolWindow` keeps the action while the window is registered. To match, the integrator can register it with `when: "projectOpen"`.
- **Settings pages:** `build.gradle` and `build.maven` already exist natively (`settings_model/builtin_pages/build.rs`); `maven.settings` opens the latter.

## 12. Parity table

| action id                                                 | end-to-end test (`workbench::build_tools::tests`)                                                                                                  | IPC (adapter → backend fn)                                                                                  | status                                          |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| `toolwindow.maven`                                        | `runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options`                                                                          | `build_model` → `ide_host::ops::build::build_model`                                                         | done                                            |
| `toolwindow.gradle`                                       | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`                                                                                 | `build_model` → `ops::build::build_model`                                                                   | done                                            |
| `maven.reload`                                            | `reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`                                                                              | `build_sync` → `ops::build::build_sync` (→ `ide_build::sync_maven`)                                         | done                                            |
| `maven.cancelReload`                                      | `stop_cancels_a_running_sync_without_an_error`                                                                                                     | `cancel_request` → `Host::cancel_request`                                                                   | done                                            |
| `gradle.reload`                                           | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`                                                                                 | `build_sync` → `ops::build::build_sync` (→ `ide_build::sync_gradle_model`)                                  | partial (Reload of one `buildDir` not asserted) |
| `gradle.cancelReload`                                     | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`                                                                                 | `cancel_request` → `Host::cancel_request`                                                                   | done                                            |
| `maven.expandAll` / `maven.collapseAll`                   | `runs_a_lifecycle_phase_…`                                                                                                                         | —                                                                                                           | done                                            |
| `gradle.expandAll` / `gradle.collapseAll`                 | `gradle_reload_lists_…`                                                                                                                            | —                                                                                                           | done                                            |
| `maven.toggleSkipTests`                                   | `runs_a_lifecycle_phase_…`                                                                                                                         | — (UiStorage `openidea.buildTools:<root>`)                                                                  | done                                            |
| `maven.toggleOffline`                                     | `execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent`                                                                             | —                                                                                                           | done                                            |
| `gradle.toggleOffline`                                    | `a_task_runs_by_its_path_with_the_builds_wrapper_and_offline`                                                                                      | —                                                                                                           | done                                            |
| `maven.executeGoal`                                       | `execute_maven_goal_…`                                                                                                                             | `run_start` (through `run.runConfig`) → `ops::run::start`                                                   | done                                            |
| `gradle.executeTask`                                      | `a_task_runs_by_its_path_…`                                                                                                                        | `run_start` → `ops::run::start`                                                                             | done                                            |
| `maven.settings`                                          | `runs_a_lifecycle_phase_…`                                                                                                                         | —                                                                                                           | done                                            |
| `gradle.linkProjects`                                     | `links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`, `does_not_link_or_start_gradle_imports_in_an_untrusted_project` | `gradle_link_projects` → `ops::build::gradle_link_projects`                                                 | done                                            |
| `gradle.unlinkProjects`                                   | `links_from_the_toolbar_…`                                                                                                                         | `gradle_unlink_projects` → `ops::build::gradle_unlink_projects`                                             | done                                            |
| `maven.addProjects`, `maven.unlinkProjects`               | —                                                                                                                                                  | (`settings_set`)                                                                                            | not registered                                  |
| `maven.analyzeDependencies`, `gradle.analyzeDependencies` | —                                                                                                                                                  | `gradle_dependencies`, `maven_dependency_declaration`, `maven_exclude_dependency`, `maven_goto_declaration` | not registered                                  |

The other build IPC commands in `src-tauri/src/commands/` are not called by this draft:

- `gradle_published_catalog` → `ops::build::gradle_published_catalog`, used by the version catalog editor contribution;
- `build_run` → `ops::build::build_run`, which the Build window owns;
- `run_gradle_jvm_args_script` → `ops::run::run_gradle_jvm_args_script` and `run_gradle_init_script` → `ops::run::run_gradle_init_script`, which the run driver already calls.

| inventory row                                                                                               | status                                            | covered by                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerToolWindow:maven`                                                                            | done                                              | e2e 1, 2, 3, restricted                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `point.registerToolWindow:gradle`                                                                           | done                                              | e2e 4, 5, links                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `point.registerRunConfigType:gradle` / `:maven`                                                             | done before this draft (`run/types.rs`)           | `edit_configurations_adds_and_saves_a_configuration_of_every_built_in_type`                                                                                                                                                                                                                                                                                                                                                                                     |
| `point.registerMenuGroup:gradle.analyzeDependencies` / `maven.analyzeDependencies`                          | not built                                         | —                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `point.registerOverlay:build-tools.execute`                                                                 | done                                              | execute e2e, `execute.rs` tests                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `point.registerEditorKind:maven.dependencyAnalyzer`                                                         | not built                                         | —                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `settings:build.maven` / `settings:build.gradle`                                                            | existing, already native                          | `maven.settings` opens `build.maven`                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `unit:src/features/build-tools/model.test.ts`                                                               | 15/15                                             | `build_tools::model::tests` (+2 not from a spec)                                                                                                                                                                                                                                                                                                                                                                                                                |
| `unit:…/ExecutePopup.test.tsx`                                                                              | 9/9 (at state level; the project Select is chips) | `build_tools::execute::tests` (+1 not from a spec)                                                                                                                                                                                                                                                                                                                                                                                                              |
| `unit:…/actions.test.tsx`                                                                                   | 2/4 done, 2 partial                               | view tests `runs_a_lifecycle_phase_…`, `does_not_mark_gradle_test_tasks_…`; e2e 1 covers registration and toggles only in part (Maven window removal when the POMs go is not asserted)                                                                                                                                                                                                                                                                          |
| `unit:…/linkage.test.tsx`                                                                                   | 3/16 (the final-build unlink included)            | `links_from_the_toolbar_…` (it also covers "leaves links unchanged when unlink is cancelled or a module is targeted") and `does_not_link_or_start_gradle_imports_in_an_untrusted_project`. "runs duplicate task paths with the selected build's wrapper, cwd and offline setting" is covered at model level (`keeps_identical_project_paths_…`). The 7 Maven link cases, the Workspace case, the late link result and the script-change reload are not covered. |
| `unit:…/workspace.test.tsx`                                                                                 | 0/4                                               | native Workspaces are not wired                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `unit:…/firstImport.test.ts`                                                                                | 0/2                                               | not built                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:…/autoReload.test.ts`                                                                                 | 0/3                                               | not built                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:…/profileReload.test.ts`                                                                              | 0/3                                               | not built                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:…/analyzer.test.ts`, `DependencyAnalyzer.test.tsx`, `pomNavigation.test.ts`, `versionCatalog.test.ts` | 0                                                 | not built                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
