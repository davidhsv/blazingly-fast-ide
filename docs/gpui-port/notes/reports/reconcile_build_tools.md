I wrote the reconciled notes and edited three of the five draft files. Nothing was compiled or run; outside the git and grep checks noted as measured, every claim comes from reading the code.

**The seam.** Two process-wide switches that default to today's behaviour:

- `ide_build::maven::set_maven_user_home` stands in for `~/.m2`. It is read by `default_local_repo`, by `user_settings_file` when that setting is empty, and by Build Project's fingerprint, so `sync_maven` needs no change.
- `ide_sdk::java::set_jdk_candidates` limits the JDK scan to the homes it is given plus the IDE's download folders, and drops the cached scan when it changes.

`ide_host::ops::build::isolate_from_user_home` forwards to both, because `app-gpui` does not depend on `ide-sdk`. The `app-gpui` test backend calls it once under `#[cfg(test)]`. The Maven folder it names is one path for the whole test binary, never created, not a temp folder per test. Each test isolates itself through project settings: Maven home, `userSettings`, `localRepository`, and a stand-in JDK in `languages.java.jdk`. That JDK setting also skips the mise-config read under the home. No signature, IPC type or `src-tauri` file changes, so `bindings.ts` should come out identical. Each switch has its own test binary: `crates/ide-build/tests/maven_user_home.rs` and `crates/ide-sdk/tests/jdk_candidates.rs`.

**Hunks.**

- **Kept on their quoted anchors:** `lib.rs`, the `mod` line, field, init, register, mount, reset, `backend_event`, and the `plugin_runtime.rs` cancel arm.
- **Re-anchored:** the mount arm now sits beside `BUILD_TOOL_WINDOW =>`, with a new `use` line.
- **New:** one hunk in `backend.rs`.
- **Dropped:**
  - The menu section, because the `analyzeDependencies` menu groups now exist with no rows.
  - "Run `cargo fmt`", because I wrapped the long lines by hand.
  - The old isolation section, which was wrong: `gradle_build_env` scans the machine's JDKs on every call.

**Draft edits.**

- `toolwindow.gradle` had an invented condition. It now depends on a new `build.gradleWindow` key that is true while the window has the Gradle tool window. Both `toolwindow.*` actions also copy `verbatim`.
- The tests now write a stand-in JDK, `gradle_project` takes the tools folder, and the links test runs `toolwindow.gradle` through the registry.
- The profile row uses the shared `app_views::checkbox`.
- The Execute popup's corners now match the popup slot's shadowed frame.
- Five over-long lines are wrapped.
- No API the drafts call has changed. Measured: `git log c0cdfe07..HEAD` on the backend files the drafts use printed nothing.

**Risks.**

- The new checkbox has an extra 8 px gap before its empty label.
- `cx` is borrowed inside `.when` closures in the view, as in the original draft.
- The stand-in JDK has no `javac`, so anything that validates it may reject it and show a warning.
- Fixed `px` lengths are kept as in the neighbouring views; replacing them with `theme::zoomed` would make them follow the zoom.
- If the Maven home setting were ever ignored, the real `mvn` on `PATH` would run before any test assertion failed.

**Hypotheses, with the check for each:**

- Test windows are not in Full mode, so the Kotlin bridge and jdtls do nothing: `grep -n "is_full\|enum IdeMode" -A6 crates/ide-core/src/settings.rs`.
- No `app-gpui` test depends on a JDK found on the machine: run `cargo test -p openidea-gpui` once after the backend hunk.
- The webview's main menu has no `maven.*` / `gradle.*` entries: `grep -rn "maven\.\|gradle\." src/workbench/ src/platform/ --include='*.ts'`.
- `toolwindow.gradle` after the last unlink shows an empty view: run the links test.
- The stand-in JDK gets rejected and raises a notice: `grep -n "project_jdk_is_usable" crates/ide-sdk/src/java.rs`.

Files are in /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01:

- `app-gpui/src/workbench/build_tools.rs`
- `app-gpui/src/build_tools/view.rs`
- `app-gpui/src/build_tools/execute.rs`

Notes: /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/build_tools_reconciled.md
