I've drafted the Maven and Gradle tool windows, but none of it has been compiled or run, so every result below comes from reading the code. I wrote new files only and ran no git, cargo or fmt.

## Files written

- `app-gpui/src/build_tools/mod.rs` (7 lines)
- `app-gpui/src/build_tools/model.rs` (1565): the tree, labels, run configurations, completions, stored toggles and history. 17 tests, covering all 15 cases of `model.test.ts` plus 2 of my own.
- `app-gpui/src/build_tools/view.rs` (999): `BuildToolView` (toolbar, banners, tree, context menu). 4 view tests.
- `app-gpui/src/build_tools/execute.rs` (714): the Execute Maven Goal / Execute Gradle Task popup. 10 tests, covering all 9 cases of `ExecutePopup.test.tsx`.
- `app-gpui/src/workbench/build_tools.rs` (1577): the workbench module, with 8 end-to-end tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/build_tools_integration.md`

## Actions registered → end-to-end test → IPC

- `toolwindow.maven` / `toolwindow.gradle` → the lifecycle-phase run test / the Gradle reload test → `build_model` (`ops::build::build_model`).
- `maven.reload`, `maven.cancelReload` → `reload_reads_the_dependency_trees…` and `stop_cancels_a_running_sync…` → `build_sync` (`ops::build::build_sync` → `ide_build::sync_maven`) and `cancel_request` (`Host::cancel_request`).
- `gradle.reload`, `gradle.cancelReload` → the Gradle reload test → `build_sync` (→ `sync_gradle_model`) and `cancel_request`. Partial: reloading one chosen build is not asserted.
- Expand All / Collapse All (both tools), the three toggles, `maven.executeGoal`, `gradle.executeTask` and `maven.settings` are each run through the registry in some test. Running a phase, goal or task goes through `run.runConfig` (`ops::run::start`).
- `gradle.linkProjects` / `gradle.unlinkProjects` → `links_from_the_toolbar…` (which also unlinks the last build) and the untrusted-project test → `gradle_link_projects` / `gradle_unlink_projects`.

## The stand-in tools

- **Maven:** a `mvn` script in a temp tools folder, named as the Maven home in the project's `.openidea/settings.toml`. User settings and the local repository also point into that folder.
  - It touches `ran`, then prints `cwd` and one `arg` line per argument.
  - Run with the sync's goals, it reports building `core`, waits while a `hold` file exists, fails if `sync-fails` exists, and otherwise prints a dependency tree for `demo:core`.
- **Gradle:** the build's own `gradlew`. For `tasks --all` it prints the project-report lines the parser needs (without them the build reads as stale right after syncing) and a task list. It uses the same `hold` and `sync-fails` markers.
- No test sleeps; they wait on state with `wait_long`.

## Left out

- **Not registered:** Maven Add / Unlink Projects, and both Analyze Dependencies actions with their menu groups. There is no native model for the Dependency Analyzer, so their menu rows stay hidden.
- **Automatic syncs not built:** the first import in Deep mode (a test would start language servers), the reload after a profile toggle, and the reload after an external build-file change.
- **Also not built:** workspace members, the Run Anything `mvn` / `gradle` provider, the POM navigation and version-catalog editor features, and the Download JDK button on sync errors.
- **Already native:** the `gradle` / `maven` run configuration types are in `run/types.rs` and their existing test. The `build.maven` / `build.gradle` settings pages exist too.

## Edits needed in compiled files (exact hunks in the notes)

- `lib.rs`: `pub mod build_tools;`
- `workbench/mod.rs`:
  - the module line, the field and its initial value;
  - the `register_actions` call;
  - `mount_build_tools` after `mount_agents`;
  - `reset_build_tools` after both occurrences of `self.reset_build(window, cx);` (in `close_project` and `open_project`);
  - a mount arm before `FIND_TOOL_WINDOW`;
  - a call at the top of `backend_event`.
- `plugin_runtime.rs`: a cancel arm for status-bar tasks named `build-sync:*`.
- **Deliberately no `TOOL_WINDOWS` entry:** the webview registers the Maven window only while the project has a POM. I register and unregister both windows at runtime, as `plugin_tool_windows.rs` does.

## Check first

1. **Home reads:** `sync_maven` always calls `default_local_repo()`, which reads `~/.m2/settings.xml` through `$HOME`. The only seam is `HOME`, and the tests don't set it. `installed_jdks()` also scans the system's JDK folders, but no JDK runs.
2. **Trust:** these ops refuse on their own in Restricted Mode: `build_model`, link/unlink, `build_sync`, `gradle_dependencies`, `build_run` and `run::start`.
   - These four are refused only at the IPC layer: `gradle_published_catalog`, `maven_dependency_declaration`, `maven_goto_declaration` and `maven_exclude_dependency`. The last one writes the POM, so native callers must check trust first. My draft calls none of them.
   - The untrusted-project test asserts no Maven window, Reload unavailable, Link asking for nothing, the exact refusal text from `build_sync`, and a refused run that never starts the stand-in.
   - It also checks that trusting the project from the banner brings the model and the Maven window back.
3. **Toggle state:** toggles are read from window context keys, because the toggle callback can't read the workbench while a menu is being built.
4. **Unverified test assumptions:**
   - The root Gradle build's folder is `""`, and unlinking it empties the linked list.
   - A test task gets an extra init script, so the tests run `assemble` / `jar` instead.
   - A refused run ends as a failed session rather than a prompt.
   - The Execute popup's key claims see Enter before the text field does.
5. **Likely first failures:** the lifecycle-phase run test (row labels and argument order), the Maven reload test (dependency-tree parsing and the stale flag), the link test, and the Execute test.
