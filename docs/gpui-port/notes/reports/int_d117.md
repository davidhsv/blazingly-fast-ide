# Integrator report: D117 (parts 0c, 0d, 0, 0b)

All four parts are done and every gate passes. Nothing is committed. Part 0 changed more than the brief named: editors now handle left-button mouse shortcuts pressed on their text (point 2, Part 0). HEAD moved to `01407512` while I worked; `git diff --stat 7fae37f0 HEAD -- app-gpui crates src src-tauri` printed nothing, so only documents changed under me.

## 1. Gates (measured; logs in `$S/d117/`)

**App suite:** I ran it six times because I changed tests after the first gates. The final pair counts.

- Runs 5 and 6 (final code): 7391 passed, 0 failed, 0 ignored, 170.38 s and 195.00 s. Run 6 had the twenty-times loop beside it. The count is 7383 at HEAD plus 8 tests.
- Runs 1 to 4 (earlier versions of the tests): all 7391 passed, 0 failed.

**Twenty times, beside the suite:** I ran the already-built test binary (no second cargo) with the filters `build_tools::tests:: quick_evaluat proofs18_e2e_keymap inspect`, 82 tests per run.

- Final round (`repc_*.txt`): 20 of 20 passed.
- Round 1 (`rep_5.txt`): one failure. `workbench::debug::tests::alt_click_quick_evaluates_the_word_or_the_selection_while_paused` panicked with "the caret is at the click".
  - **Cause:** the stop's open-file at the execution line moved the caret after my click. Measured indirectly: with the caret put at line 0 before Debug, the test's new wait for "caret on line 1" passes, so the stop does move the caret. The run-5 failure itself was not instrumented.
  - **Fix:** the test waits for the execution line, the frame, and the caret moved by the stop before clicking. The assertion now prints the caret.

**Other failure seen:** the first version of the Alt-click test timed out. A debug print showed Quick Evaluate ran at the old caret, `start = value of start`. The window's capture listener took the press before the editor (see Part 0).

**Other gates:**

- `cargo test --test web_view`: 1 passed.
- `cargo clippy --all-targets -- -D warnings`: rc 0, no warnings.
- `cargo build`: rc 0, 0 warnings.

**Crate checks, from the worktree root:**

- `cargo test -p ide-sdk`: 201 passed, 0 failed, 1 ignored. The ignored one is the existing `tests::detect_on_this_machine`.
- `cargo test -p ide-host`: 650 passed, 0 failed, 14 ignored. `git diff HEAD -- crates | grep -c '#[ignore'` gives 0, so none of the ignores are mine.
- `cargo clippy -p ide-sdk -p ide-host --all-targets -- -D warnings`: rc 0.
- `cargo test -p openidea export_bindings`: 1 passed.
- `git status --short src/ src-tauri/ e2e/ package.json`: empty.

## 2. Parts

### 0c: tests read no global version-manager config

**Changed:**

- `crates/ide-sdk/src/pinned.rs`: new `pub fn set_global_configs(Option<Vec<PathBuf>>)`. `global_configs()` reads it first; `None` keeps today's behaviour.
- `crates/ide-host/src/ops/build.rs`: `isolate_from_user_home` also sets it to an empty list. Its signature is unchanged.
- New test binary `crates/ide-sdk/tests/pinned_global_configs.rs`, test `the_global_config_is_read_until_an_empty_list_is_named_and_again_after_none`:
  - It names `MISE_GLOBAL_CONFIG_FILE` through `ide_core::process::set_env_overlay`, which `host_var_os` reads first, so there is no `unsafe` and no change to the process environment.
  - It checks: the pin is found, then not found, the project's own `mise.toml` is still read, then found again after `None`.

**Native proof:** it cannot be asserted from the window without setting environment variables. The lookup can only be pointed at a temp folder through the environment (or the process-wide overlay, which would leak into parallel tests). Also, this machine has no `~/.config/mise`, so such a test could not tell the difference here. I rely on the crate test.

**What else in `pinned.rs` reads outside the project:**

- No `~/.asdfrc` and no `ASDF_*` variables.
- The walk up from the project root reads `mise.toml`, `.config/mise/config.toml` and `.tool-versions` in every folder above the project. A project under the home folder still reaches the user's files with the switch on. Test projects are under `TMPDIR=/var/folders/hk/…/T/`, outside the home folder.
- **Left out, outside the named seam:** `ide_core::version_managers::managers()` reads `HOME/.local/share/mise` and `~/.asdf`, but only once a version is pinned. `discover.rs:160–172` and `:640–649` read `~/.pyenv`, `~/.local/share/uv`, `~/go` and the managers' installs. Hypothesis: neither is reached when a test opens a project. To confirm, log `version_managers::managers` calls during `cargo test --lib build_tools`.

### 0d: the Maven and Gradle conditions read on and off

**Changed:** tests only, in `app-gpui/src/workbench/build_tools.rs`. I split `maven_settings` (the stand-in Maven, settings, repository and JDK) out of `maven_project`. The webview rules (`src/features/build-tools/index.ts:59–60, 131–132, 269–305`) match the app, so the app needed no fix.

**New tests (`workbench::build_tools::tests::…`):**

- `the_build_conditions_are_off_without_a_build_and_a_new_pom_turns_on_only_maven`
- `a_new_gradle_build_turns_on_only_the_gradle_conditions`
- `link_gradle_projects_is_not_available_without_a_project` (the Welcome window)

**What they check:**

- Every registry action whose `when` reads `build.maven`, `build.gradle` or `build.gradleWindow` is unavailable, and `execute_action_or_throw` returns an error for each.
- Neither tool window is offered.
- Nothing ran: no Execute popup, no unlink question, no path prompt, no Maven run.
- `gradle.linkProjects` is available in a project with no build.
- After writing the build files and sending `FsChanged` through `Backend::emit`, only that half turns on.

**Changed slightly:** `does_not_link_or_start_gradle_imports_in_an_untrusted_project` lost a local `use` that moved to the module's imports.

### 0: Quick Evaluate, and the two keymap tests

**Webview source:** `src/features/debugger/actions.ts:195–211, 863–870` and `editor/quickEvaluate.ts:15–34`.

**Changed:**

- `workbench/debug.rs`: `debug.quickEvaluate` added to `DEBUG_ACTIONS`, and its result is shown.
- `workbench/debug_extras.rs`: `show_quick_evaluation` evaluates through `driver.evaluate(.., Hover, ..)`. It shows the value, or the error in the error tone, as a hint at the evaluated position; the value hint stays 60 s. "Nothing to evaluate" shows at the caret.
- `workbench/editor_language.rs`: `hint` is now `pub(super)` so it can be shared.
- `editor/intelligence.rs`: a Quick Evaluate press keeps a selection it lands in (`selection_after_click`) and passes `{"pos": caret}`.

**Widening beyond the brief:** the window's capture listener passed `in_editor_text = false` for every press, so it ran every left-click mouse shortcut before the editor, at the old caret.

- `editor/view.rs`: the editor now registers a hitbox for its text area and has `Editor::text_under(position, window)`.
- `workbench/mod.rs`: the capture listener asks the window's editors and passes `in_editor_text` to the existing dispatcher, so the editor runs the shortcut at the pressed position.
- I checked that gpui's hitbox ids are never reused: `next_hitbox_id` only increments (`window.rs:5550`).
- Hypothesis: ⌥⌘-click (`navigate.implementation`, `Meta+Alt+Button1`) and ⌥⇧-click (`editor.addOrRemoveCaret`) used to be taken by the window and now go to the editor. No test covers them. To confirm, add a workbench test that ⌥⌘-clicks a word.

**Tests:**

- `workbench::debug::tests::alt_click_quick_evaluates_the_word_or_the_selection_while_paused`. While paused it ⌥-clicks a word (hint `middle = value of middle`, caret at the click), ⌥-clicks inside a selection (`mid = value of mid`, the selection kept), and runs the action from the keyboard (`end = value of end`).
- `workbench::debug::tests::quick_evaluate_is_not_available_unless_a_session_is_paused`. The `when` is off before debugging: the registry refuses it and a ⌥-click shows nothing and goes to the editor. It is on while paused, and off again after the program ends.

**Keymap tests:** moved into the declared `workbench/tests/proofs18_e2e_keymap.rs` with a new header, and `proofs18_e2e_keymap_pending.rs` deleted with `rm`. Apart from the header, the body matches the deleted file exactly (`diff` printed nothing), so no assertion was weakened.

**Left out:**

- The expandable value tree: the native hint is one line, `ValueHint::text()`.
- `textEditorActive` read off (the paused-but-no-editor case).
- Hypothesis: the hint text goes through `I18n::tx_template`, so a value that matches a catalog entry could show translated. To confirm, evaluate an expression whose value is a catalog key.

### 0b: modifiers step for the inspection script, and the real window

**New step:** `mods⇥<modifiers>`. The names are joined by `-`: `shift`, `ctrl`/`control`, `alt`/`option`, `cmd`/`command`/`meta`/`platform`/`super`/`win`, `fn`/`function`. Empty, or `mods` alone, releases all. It sends `PlatformInput::ModifiersChanged` through `Window::dispatch_event`. Example: `mods⇥shift`, `mods⇥`, `mods⇥shift`, `mods⇥` is double Shift.

**Tests:**

- Unit tests in `app-gpui/src/inspect.rs` cover parsing, including `hyper is not a modifier key`.
- `workbench::tests::inspect_script::an_inspection_script_double_shift_opens_search_everywhere`.

**Shots** (folder `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d117/insp/shots/`):

- They come from the debug binary built before my last two test-only edits; no non-test code changed after it.
- The app ran with a temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR` and project, and was stopped by its PID. There were no `fail` or `note` lines in the logs.
- `a-01-before-1-crop.png`: no popup.
- `a-02-double-shift-1.png` and its `-crop`: Search Everywhere is open, so double Shift works in the real window.
- `b-01-long-query-1-crop.png`: a query wider than the field. The start is clipped ("…erywhere field can show…") and the caret at the end is visible.
- `b-02-clicked-1-crop.png`: after a click at (700, 98) points, the caret sits between "t" and "he" of "the", where the click was.
- `b-03-selected-1-crop.png`: after five shift-left presses, " so t" is highlighted from just after "once,".

## 3. Rows (inventory not edited)

- `action:debug.quickEvaluate` (INVENTORY line 237) → done: the two `workbench::debug::tests::…` tests above.
- `e2e:e2e/keymap.spec.ts` (line 3494) → done: `workbench::tests::proofs18_e2e_keymap::keymap_double_click_offers_mouse_and_keyboard_shortcuts` and `…::keymap_enter_offers_mouse_and_keyboard_shortcuts`.
- `context:build.maven` (line 979) and `context:build.gradle` (line 977) → done: the first two 0d tests.
- The `[~]` rows whose only gap was "its condition read off" → done, with the first two 0d tests added:
  - Gradle: `gradle.cancelReload`, `gradle.collapseAll`, `gradle.expandAll`, `gradle.reload`, `gradle.toggleOffline`, `gradle.unlinkProjects`.
  - Maven: `maven.cancelReload`, `maven.collapseAll`, `maven.expandAll`, `maven.reload`, `maven.toggleOffline`, `maven.toggleSkipTests`.
- `gradle.linkProjects` (line 451) → done, adding `link_gradle_projects_is_not_available_without_a_project`.
- Still partial: `gradle.executeTask`, `maven.executeGoal` (the project chooser is not a select) and `maven.settings` (no Settings page yet). Their conditions are now read off.

## 4. False or stale

- The `workbench/mod.rs` capture listener always passed `in_editor_text = false`, contrary to the contract in `platform/dispatcher.rs:343–345` (the editor runs a left-click shortcut on its text itself). Fixed above.
- The `DEBUG_ACTIONS` doc comment listed Quick Evaluate as having no view; I updated it.
- `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md` and `$S/insp/USAGE.md` lack the `mods` step; I did not edit them.
- USAGE's advice to wait after a `mark` matters: my first "before" shot, taken without a wait, already showed the popup.
