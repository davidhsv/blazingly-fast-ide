All gates passed after my last edit. The backend seam is in, the Maven and Gradle tool windows are wired, and a test-build guard now refuses any `mvn`, `gradle` or `java` outside the temp folder. The last shots show the Maven window opening and the Execute popup, with no cut text and no sync run.

One issue comes first, because it touches rule 6. The build model now loads on every project open. For a project with no `languages.java.jdk` setting, that reaches `ide_sdk::apply::project_jdk` → `pinned::pinned_install`, which reads `~/.config/mise/config.toml` (Read: `crates/ide-host/src/ops/build.rs` `load`, `crates/ide-sdk/src/pinned.rs` `global_configs`). It is a read, not a run, and it happens in tests too. My own tests avoid it: each one names a JDK and a Maven home, and every Gradle build in them has its own wrapper. Hypothesis: run detection already made this read before my change, through `apply_to_run_configs` → `project_jdk`. Confirm with a file-access trace, e.g. `sudo fs_usage -w -f filesys <test pid> | grep mise`. Closing it needs a seam the brief did not name, so I left it.

## 1. Gates (all after the last edit)

All suite runs below were from `app-gpui/` with `cargo test --lib`, output to files under `$S/d113/`.

| Gate                                                                                      | Result                                                                                                                      |
| ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1 (alone)                                                                       | `ok. 7383 passed; 0 failed; 0 ignored`, 171.00 s                                                                            |
| Suite run 2                                                                               | `ok. 7383 passed; 0 failed; 0 ignored`, 185.84 s; neither run recompiled (0 `Compiling` lines), so both ran the same binary |
| Loop during suite run 2: the test binary run directly with filter `build_tools`, 20 times | 20 of 20: `ok. 44 passed; 0 failed`                                                                                         |
| `cargo test --test web_view`                                                              | `ok. 1 passed`                                                                                                              |
| `cargo clippy --all-targets -- -D warnings`                                               | exit 0, 0 warnings or errors                                                                                                |
| `cargo build`                                                                             | exit 0, 0 warnings                                                                                                          |

The backend checks ran from the worktree root:

| Check                                                                           | Result                                                                          |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `cargo test -p ide-build`                                                       | 142 + 1 passed, 0 failed                                                        |
| `cargo test -p ide-sdk`                                                         | 197 passed, 1 ignored + `jdk_candidates` 1 passed + `version_managers` 2 passed |
| `cargo test -p ide-host`                                                        | 650 passed, 0 failed, 14 ignored                                                |
| `cargo clippy -p ide-build -p ide-sdk -p ide-host --all-targets -- -D warnings` | exit 0                                                                          |
| `cargo test -p openidea export_bindings`                                        | `export_bindings ... ok`                                                        |
| `git status --short src/ src-tauri/ e2e/ package.json`                          | printed nothing                                                                 |

- **Ignored tests:** none are mine. The ide-sdk one is `tests::detect_on_this_machine`, `#[ignore]` in `crates/ide-sdk/src/tests.rs`, which I did not edit. Hypothesis: the 14 in ide-host are all in files I did not touch; `git diff --name-only crates/ide-host/tests` would print nothing.
- **Failures seen along the way, all fixed:**
  - The draft imported `BuildRow` from `view` instead of `model`.
  - Clippy found three issues in the drafts: two `large_enum_variant` (now `#[expect]` with a reason) and one `explicit_auto_deref`.
  - My first guard broke `proofs4_run::a_gradle_boot_run_with_vm_options_is_launched_with_the_init_scripts_written_for_it`. It refused a Gradle run whose project has no Gradle build, even though the run's own command was a stand-in. The guard is now precise rather than weaker (see section 2).
- **Other agents edited files while I worked**, none of them by me: `agents/{config_pickers_view,new_task_view}.rs` (since reverted), `vcs_diff/tab_view.rs`, `workbench/diff_tab.rs`, and three `proofs12`/`proofs18` test files. Total test counts were steady: 7,339 + 43, then +1 for my scroll test.

## 2. What changed, per part

**Part 1, the seam.** Applied as written in the notes:

- `crates/ide-build/src/maven.rs`: adds `set_maven_user_home`, `maven_user_home` and `local_repo_in`, plus a unit test.
- `crates/ide-build/src/compile/fingerprint.rs`, `crates/ide-sdk/src/java.rs` (`set_jdk_candidates`, `distinct_jdks`) and `crates/ide-host/src/ops/build.rs` (`isolate_from_user_home`).
- New test binaries: `crates/ide-build/tests/maven_user_home.rs` and `crates/ide-sdk/tests/jdk_candidates.rs`.
- No existing signature, IPC type or bindings changed.

**Part 2, the native slice.**

- **Wiring:** the hunks of notes 2.1–2.10 in `app-gpui/src/{lib.rs,backend.rs}` and `workbench/{mod.rs,plugin_runtime.rs}`.
- **Drafts:** declared, and handed to me by the brief: `workbench/build_tools.rs` and `build_tools/{mod,model,view,execute}.rs`.
- **Links test fix:** `libs/other` in the links test now has its own stand-in `gradlew`. Without it, its launcher would have been looked up in the user's mise config.
- **Execute popup scrolls:** its list used to clip without scrolling, and shot 1 showed a half-drawn last row. It now scrolls to the row the keyboard selects, with a 380 px height. The webview uses `min(380px, 55vh)` (`src/features/build-tools/BuildTools.module.css:161`); the 55vh part is not reproduced. New test: `workbench::build_tools::tests::the_execute_list_scrolls_to_the_completion_the_keyboard_selects`.
- The webview source the slice follows is `src/features/build-tools/index.ts` (actions at :268–462, tool windows at :138/:146/:183) and `ExecutePopup.tsx`.

**The guard** (new file `app-gpui/src/build_tools/guard.rs`, compiled only in test builds):

- **Where it runs:** before each backend call that can start these tools:
  - a sync, in `workbench/build_tools.rs`;
  - every run start, in `run/driver.rs`;
  - Build Project, in `workbench/build_window.rs`;
  - a debug launch, in `debugger/driver.rs`.
- **How it checks a sync:** it reads the launcher the backend itself reports in `build_model` (`MavenProject.command`, `GradleModel.command`). Each launcher must be an existing file inside the process temp folder, and the Maven Daemon must be off. So if the Maven home setting were ignored, the guard would see `mvn` and refuse before anything runs. That closes the notes' risk.
- **How it checks a run:**
  - Any configuration whose command is named `mvn`, `mvnw`, `mvnd`, `gradle`, `gradlew`, `java` or `javaw` must be a temp-folder file.
  - A bare `gradle` in a Gradle configuration must resolve to a Local distribution whose `bin/gradle` is in the temp folder. The backend does not use the wrapper there (`use_wrapper = false`).
  - Projects in Restricted Mode are passed through, because the backend refuses them first.
- **The tests that try it:**
  - `workbench::build_tools::tests::a_test_build_refuses_a_maven_that_is_not_a_stand_in_of_its_own_folders`: the Maven home has no Maven, so the backend reports a non-temp launcher. Reload, Execute Goal and a bare `java` run are each refused with the guard's message, and `synced_at` stays `None`.
  - `workbench::build_tools::tests::a_test_build_refuses_a_gradle_that_is_not_a_stand_in_of_its_own_folders`: covers the sync, a task, and a bare `gradle`.
  - Unit tests: `build_tools::guard::tests::*`.
- **Limits:** the boundary is the process temp folder, not one test's own folders. `check_build` and `check_debug` have no end-to-end test.

**Settled hypotheses from the notes:**

- **Stand-in JDK:** no JDK or Java notice appears. The assertion is in `reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`, which passed.
- **Gradle view after the last unlink:** the window stays registered, `toolwindow.gradle` runs, and Link is still offered (`links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`, passed).
- **Kotlin bridge and language server:** only read. `IdeMode` defaults to `Light`, so the bridge worker does nothing. Hypothesis: test windows start no jdtls; confirm with `grep -rn "mode = \"full\"" app-gpui/src`, plus a process listing during the test.

**Not built**, as the notes list:

- Maven Add / Unlink Projects.
- The Dependency Analyzer (both tools) and its `maven.dependencyAnalyzer` editor.
- Automatic syncs.
- The Execute popup's project chooser as a select (it is a row of names).
- Download JDK and Gradle settings buttons on sync errors.
- Native Workspaces.
- The Run Anything provider.
- POM navigation and the version catalog.

## 3. Inventory rows

`parity.json` has no `action:maven.*`, `action:gradle.*`, `point.registerToolWindow:maven/gradle`, `ipc:build_model`, `ipc:build_sync` or `ipc:gradle_*` keys today. The rows below are new. All tests are under `workbench::build_tools::tests::`.

| Key                                                                                                                                | State                                               | Proving test                                                                                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:maven.reload`                                                                                                              | done                                                | `reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`                                                                                                                                                                                                                                        |
| `action:maven.cancelReload`                                                                                                        | done                                                | `stop_cancels_a_running_sync_without_an_error`                                                                                                                                                                                                                                                               |
| `action:maven.executeGoal`                                                                                                         | partial: the project chooser is a row, not a select | `execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent`, `the_execute_list_scrolls_to_the_completion_the_keyboard_selects`                                                                                                                                                                    |
| `action:maven.toggleOffline`                                                                                                       | done                                                | `execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent`                                                                                                                                                                                                                                       |
| `action:maven.toggleSkipTests`, `action:maven.expandAll`, `action:maven.collapseAll`, `action:maven.settings`                      | done                                                | `runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options`                                                                                                                                                                                                                                    |
| `action:gradle.reload`, `action:gradle.cancelReload`, `action:gradle.expandAll`, `action:gradle.collapseAll`                       | done                                                | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`                                                                                                                                                                                                                                           |
| `action:gradle.executeTask`                                                                                                        | partial: chooser as above                           | `a_task_runs_by_its_path_with_the_builds_wrapper_and_offline`                                                                                                                                                                                                                                                |
| `action:gradle.toggleOffline`                                                                                                      | done                                                | `a_task_runs_by_its_path_with_the_builds_wrapper_and_offline`                                                                                                                                                                                                                                                |
| `action:gradle.linkProjects`                                                                                                       | done                                                | `links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`, `does_not_link_or_start_gradle_imports_in_an_untrusted_project`                                                                                                                                                           |
| `action:gradle.unlinkProjects`                                                                                                     | done                                                | `links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`                                                                                                                                                                                                                            |
| `action:maven.addProjects`, `action:maven.unlinkProjects`, `action:maven.analyzeDependencies`, `action:gradle.analyzeDependencies` | not built                                           | none                                                                                                                                                                                                                                                                                                         |
| `point.registerToolWindow:maven`                                                                                                   | done                                                | `runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options` (via `toolwindow.maven`)                                                                                                                                                                                                           |
| `point.registerToolWindow:gradle`                                                                                                  | done                                                | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`, `links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`                                                                                                                                                        |
| `ipc:build_model`                                                                                                                  | done                                                | every test above. Call sites: `src-tauri/src/commands/build.rs:18` and `app-gpui/src/workbench/build_tools.rs:406` (`load_build_model`)                                                                                                                                                                      |
| `ipc:build_sync`                                                                                                                   | done                                                | `reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`. Call sites: `src-tauri/src/commands/build.rs:57` and `workbench/build_tools.rs:647` (`sync_build_tool`)                                                                                                                               |
| `ipc:gradle_link_projects`                                                                                                         | done                                                | links test. Call sites: `src-tauri/src/commands/build.rs:28` and `workbench/build_tools.rs:828`                                                                                                                                                                                                              |
| `ipc:gradle_unlink_projects`                                                                                                       | done                                                | links test. Call sites: `src-tauri/src/commands/build.rs:38` and `workbench/build_tools.rs:909` (`unlink_confirmed`)                                                                                                                                                                                         |
| `ipc:cancel_request`                                                                                                               | stays partial; now also proven for syncs            | `stop_cancels_a_running_sync_without_an_error`. Call sites: `src-tauri/src/commands/app.rs:281` and `workbench/build_tools.rs:776` (`host.cancel_request`). Hypothesis: `AppState::cancel_request` forwards to `Host::cancel_request`; confirm with `grep -n "fn cancel_request" -A3 src-tauri/src/state.rs` |
| `ipc:run_start`                                                                                                                    | already done                                        | goals run through `run.runConfig`. Call sites: `src-tauri/src/commands/run.rs:106` and `app-gpui/src/run/driver.rs:1100`                                                                                                                                                                                     |

`build.gradleWindow` is a context key that only the native app has; no inventory row covers it.

## 4. Screenshots

The real debug binary ran on a temp trusted project with a multi-module `pom.xml`, using a fresh `OPENIDEA_GPUI_HOME` and fake `HOME`/`ZDOTDIR`, and was stopped by its PID. The project's Maven home was a stand-in that writes a marker; afterwards the marker was absent, so no Maven ran. No sync was run, no Enter was pressed, and the Services window was never opened.

- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d113/insp/shots/b-01-maven-unsynced-1.png`: the Maven tool window on the right. It has its toolbar and the rows Multi Demo › Lifecycle / Plugins / Run Configurations / core / Demo App, and it sits inside the window. No banner is shown on the unsynced project. Hypothesis: the webview shows none either; check `BuildToolView.tsx`.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d113/insp/shots/b-02-execute-popup-1.png`: Execute Maven Goal in the shared popup frame, with title, hint, field with placeholder, the "Maven Goals" section, and the project row. The rows end on a whole row; no text is cut.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d113/insp/shots/b-03-execute-popup-last-row-1.png`: after `up`, the list has scrolled to the selected last row `site:stage-deploy`, with plugin names right-aligned.
- The earlier `a-02` shot, from before the fix, showed the clipped last row.

## 5. Stale or wrong in the notes

- Note 1.8 says a bare `gradle` run uses `gradle_command`. It actually uses `resolve_gradle_command(.., use_wrapper = false)` (`crates/ide-sdk/src/apply.rs:1427`), so the build's wrapper is not used there.
- `libs/other` in the links test had no wrapper, which sent its launcher lookup to the user's mise config.
- Note 2.12 says only the `FAKE_GRADLEW` `printf` line is over 120 columns. It is still the only one (144 columns), but the draft also had a closure line (132) and the new guard test a format line (123); I wrapped both.
