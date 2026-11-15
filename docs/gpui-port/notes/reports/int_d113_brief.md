# Integrator brief: the build tools (backend seam first, then the native slice)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d113/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish it. Stop early only if blocked, and say what blocks you. Send one final report only, after every gate has finished.

## This brief names a seam under `crates/` (rule 3 of `$S/int_rules.md` applies in full)

Notes, reconciled against the tree: `docs/gpui-port/notes/integration/build_tools_reconciled.md`. Part 1 of the notes is a backend seam: `ide_build::maven::set_maven_user_home` (stands in for `~/.m2`), `ide_sdk::java::set_jdk_candidates` (limits the JDK scan), `ide_host::ops::build::isolate_from_user_home` (forwards to both), each defaulting to today's behaviour, each with its own test binary (`crates/ide-build/tests/maven_user_home.rs`, `crates/ide-sdk/tests/jdk_candidates.rs`). You may edit exactly the files that part names under `crates/`. No signature of an existing function changes, no IPC type changes, no `///` doc comment on a type that derives `specta::Type`, nothing under `src/` or `src-tauri/`.

After the seam, from the WORKTREE ROOT (not `app-gpui/`), one heavy build at a time: `cargo test -p ide-build`, `cargo test -p ide-sdk`, `cargo test -p ide-host` (if it has tests that the change can touch), `cargo clippy -p ide-build -p ide-sdk -p ide-host --all-targets -- -D warnings`, `cargo test -p openidea export_bindings`, and `git status --short src/ src-tauri/ e2e/ package.json` must print nothing. Put each result line in the report. (The main session runs the whole Tauri workspace suite afterwards.)

## Then the native slice

Part 2 of the notes: the `mod` lines and hunks for `app-gpui`, the actions it registers (`maven.*`, `gradle.*`, `toolwindow.gradle` and the build-tools ones), what is not built, the risks, the order. Draft files (committed, undeclared; the reconciler edited three of them): `app-gpui/src/workbench/build_tools.rs`, `app-gpui/src/build_tools/{mod,model,view,execute}.rs`. Re-check every anchor against the current tree before applying.

A guard the notes do not have and this slice must add, like the ones for `gh`, agents and shells: in a TEST BUILD the native code refuses to start any `mvn`, `mvnw`, `gradle`, `gradlew` or `java` that is not inside the test's temp home or temp project (find where the build-tools path spawns a process natively and where the backend resolves the tool; if the spawn is in the backend, the guard goes where `app-gpui` asks for the run, by checking the resolved tool path the backend reports, or by making the tests' project settings name a fake tool and asserting on that name before the run). The reconciler's own risk: "if the Maven home setting were ever ignored, the real `mvn` on `PATH` would run before any test assertion failed". Make that impossible, and prove it with a test that tries.

Each test isolates itself through project settings (Maven home, `userSettings`, `localRepository`, a stand-in JDK in `languages.java.jdk`) and fake `mvn` / `gradle` scripts in the temp folder that answer from files there. No real build tool, JVM or network; nothing under the user's home (`~/.m2`, `~/.gradle`, mise configs). Settle the notes' hypotheses as you go (whether the stand-in JDK is rejected with a notice; whether test windows start no Kotlin bridge or language server; the empty Gradle view after the last unlink).

After it passes, look at it in your own binary's window (inspection script: `$S/insp/USAGE.md`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; a temp project with a `pom.xml`; do NOT run a sync or a goal in the real binary: it would start the machine's real Maven; never open the Services tool window): shots of the Maven tool window as it opens on an unsynced project, and of the Execute popup. They must use the shared frames, sit inside the window and show no cut text.

## Gates (they replace rule 12 of `$S/int_rules.md`)

In `app-gpui/`: the whole suite twice to files, with nothing else beside it except, during the SECOND suite run, a loop of every test you added, declared or changed, twenty times by filter; `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings. Plus the crate checks above. Say how each run was started.

## Report

Besides the format of `$S/int_rules.md`: the result line of each crate check; one table of every `parity.json` key the notes name (the `action:maven.*`, `action:gradle.*`, tool-window and `ipc:` rows), with the state it reaches (done or partial), the proving tests by full name and, for partial, what is missing; for each `ipc:` row both call sites; the guard's test; the screenshots' paths and what you saw.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{diff_tab,agents_views}.rs`, `app-gpui/src/vcs_diff/{wrap,tab_view}.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/agents/{config_pickers_view,new_task_view}.rs`, `app-gpui/src/agents/composer_view/`, `app-gpui/src/agents/new_task_view/`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file, and every undeclared `proofs*` test file this brief does not hand you.
