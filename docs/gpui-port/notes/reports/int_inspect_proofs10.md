The script runner works in the real binary and the five final suite runs are green. Of the six defects, three are fixed (avatar colour, empty editor area and status bar, path gap). Fonts and Part 1's two fixture-dependent tests are blocked, and the main session has to decide what to do with the fonts.

## 1. Gates

- **Suite ×5** (after the last code edit): 6886 passed, 0 failed, 0 ignored, in each of the five runs (about 131 s each).
- **Earlier failures under heavy machine load (load 13–24, measured), in tests I did not write:**
  - `editor::intelligence::tests::proofs9_cancel_task::cancelling_…` failed once. It uses a helper with a 4-second budget, which I raised to 30 s (`src/editor/intelligence.rs`, `settle`). Two other helpers still say "four seconds" (`workbench/editor_extras.rs:635`, `workbench/editor_language.rs:1649`); I left them alone.
  - `workbench::plugin_java_debug::tests::stop_ends_the_process_and_the_session` failed in three runs, which took 258 s instead of 130 s. It passes alone and in all six runs since. The cause was not found (the full output was not kept). It uses the fixed port 50123.
- **Clippy** `--all-targets -D warnings`: clean. Its only finding during the work (`Iterator::last`, in `inspect.rs`) was fixed.
- **Binary build:** 0 warnings. The `__eh_frame too large` linker message is **suppressed, not fixed**: `[lints.rust] linker_messages = "allow"` in `app-gpui/Cargo.toml` hides every linker message for the whole package.
- **Compile errors:** Part 0 had two small ones (missing import, wrong return type), fixed. Part 1 had none.
- **Scope:** nothing under `docs/`, `src/`, `src-tauri/` or `crates/` was changed. `rustfmt` ran only on files I edited, and `git status` showed no other file changed.
- **No instance of the binary is running:** every launch was stopped by its PID and `ps` shows none.

## 2. Part 0a: the inspection script

- **Code:** `app-gpui/src/inspect.rs`, declared `#[cfg(debug_assertions)] pub mod inspect` and started from `main.rs` after the first window opens.
- **Variables:** `OPENIDEA_GPUI_INSPECT` (the script), `OPENIDEA_GPUI_INSPECT_LOG` (where marks and failures go).
- **Steps** (one per line, TAB-separated; `#` lines are comments):
  - `action⇥<id>[⇥<json>]`
  - `open⇥<abs path>`
  - `keys⇥<gpui keystrokes>`
  - `type⇥<text>`
  - `wait⇥<ms>`
  - `mark⇥<name>`, logged as `mark⇥name⇥unix-ms`
  - `quit`
- **Failures** are logged as `fail⇥line⇥step⇥reason` and the script goes on. The log ends with `done⇥ms`.
- **Frame wait:** each step runs from the runner's own task and waits for the next frame. That wait is capped at 1 s, because macOS stops drawing a covered window; when the cap is hit a `note` line is logged.
- **`click`: not possible.** GPUI records debug bounds only under `cfg(any(test, all(debug_assertions, feature = "test-support")))` (`gpui-pre-0.3.12/src/elements/div.rs:908`), which the binary does not enable, and `rendered_frame` is `pub(crate)`.
- **Tests:** four parser unit tests, and `workbench::tests::inspect_script::an_inspection_script_runs_an_action_types_into_the_editor_and_marks` (runs an action, types into the editor, fails one step, marks).
- **Proof run** (isolated home, temp Git project with one modified file): the script opened `src/main.rs`, ran `toolwindow.git` and `file.settings`, and logged the marks `file-open`, `git`, `settings` and `done`. The process stayed alive until killed by PID. The app's log had no errors; its warnings were only font ones (`failed to load font "Inter Variable"`, plus system fonts loaded by the Settings font picker).
- **Tour run:** the full example tour ran twice, the second time on a fresh home with the final binary, with all 13 marks and no `fail` or `note` lines. No screenshots were taken, so how the screens look is unverified.
- **Usage file:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp/USAGE.md`

## 3. Part 0b: the six defects

1. **Fonts: the bundled fonts cannot be used as they are. The tree is in neither state the task described, so you need to decide.**
   - **Measured** with the new `openidea-gpui --self-check=fonts`, which exits 1:
     - Before registering, "Inter" and "JetBrains Mono" resolve to Helvetica, so neither is installed.
     - `add_fonts` accepts the `.woff2` files, but they register as **"Inter"** and **"JetBrains Mono"**, so "Inter Variable" still misses.
     - Inter's `latin-ext` subset is rejected ("has no 'm' character").
     - Weight 400 and weight 600 resolve to the **same FontId** for both families (33 and 34), so all semibold and bold text would draw as regular.
   - **What stays in the tree:** the faces are **not registered at start**. `app-gpui/assets/fonts/{inter,jetbrains-mono}/` holds the `latin` woff2 files and the OFL licences. `theme/bundled_fonts.rs` embeds them with `include_bytes!` (not gated, so they ship in release), and the self-check uses them.
   - **Decision for you:** register them as they are, or remove them.
   - **Needed for real use:** one static face per weight. No `@fontsource` static package exists locally.
2. **Title bar avatar:** the webview colours the avatar from `project.name` (`Workbench.tsx:86`) and takes the initials from the label (`TitleBar.tsx:81`); the native app used the accent colour.
   - The avatar is now shared with the Welcome screen (`project_color::project_avatar` / `avatar_fill`), and the colour comes from the project's own name, carried as `color_name` through `Content::Project` and `ProjectWidget`.
   - The test covers `avatar_fill`, not the title bar's wiring.
3. **Trust dialog path:** the webview keeps the head and tail split, with the tail capped at `max-width: 70%` (`trust.module.css:17-27`).
   - GPUI's truncation also strips the trailing punctuation before the "…", which leaves a gap before the tail.
   - Change: the head is right-aligned and the tail is capped at 70% (`app_views/mod.rs`).
   - The test checks only the path split. **Hypothesis:** the gap is gone; confirm with a trust-dialog screenshot.
4. **`.openidea` in the Project view: matches the webview, no change.**
   - The webview's changelists save writes it after a modified file is assigned (`changelists.ts:151-165,204`, then `commands.rs:1271`).
   - The tree always shows dotfiles (`project-tree/model.ts:15`), and `.openidea` is not in `DEFAULT_IGNORED_NAMES` (`ignored.rs:18`).
5. **Empty editor and status bar:** the webview shows its empty-editor hints (`EditorArea.tsx:11-50`) in any pane without a tab, and always shows the status bar in a project window (`Workbench.tsx:97`).
   - New `workbench/empty_editor.rs` draws the same hints with the keymap's shortcuts; clicking one runs its action.
   - `shows_status_bar` now includes project windows.
   - Test: `workbench::tests::empty_project_window::…`.
   - Which status items show with nothing open was not compared to the webview item by item.
6. **"Hovered" status item:** the only background on a status item is the pointer `.hover` (`status_bar.rs:600`). Nothing else draws it highlighted, so nothing was changed. **Hypothesis:** the pointer was over it; confirm with a screenshot taken with the pointer elsewhere.

## 4. Part 1: plugin proof tests

- **21 tests declared and passing.** Inventory keys are as in the notes.

| Test (`workbench::…`)                                                                                                                      | Inventory key (notes)           | State   |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------- | ------- |
| `plugin_runtime::tests::proofs10_host_ui::{a_plugins_yes_no_question…, asks_multi_line_text…, a_plugins_folder_question…, a_context_key…}` | `wit/host.wit#ui`               | partial |
| `…proofs10_host_ui::executes_host_command_requests…`                                                                                       | `wit/host.wit#command-runner`   | done    |
| `…proofs10_host_ui::a_status_item_set_for_one_window…`                                                                                     | `ipc:plugin_subscribe`          | done    |
| `…proofs10_events::{a_file_whose_tab_closes…, a_file_created_on_disk…, a_plugin_on_a_schedule…}`                                           | `wit/exports.wit#event-handler` | done    |
| `…proofs10_events::a_status_item_the_plugin_removes…`                                                                                      | `wit/host.wit#ui`               | partial |
| `…proofs10_events::a_plugins_task_shows_the_progress…`                                                                                     | `wit/events.wit#tasks`          | done    |
| `…proofs10_host_api::a_plugins_output_channel…`                                                                                            | `wit/host.wit#output`           | partial |
| `…proofs10_host_api::a_plugins_secret…`                                                                                                    | `wit/io.wit#secrets`            | partial |
| `…proofs10_host_api::a_plugin_runs_its_granted_program…`                                                                                   | `wit/io.wit#process`            | partial |
| `…proofs10_host_api::{a_plugin_lists_the_windows_open_editors, a_plugin_opens_a_file_and_selects…}`                                        | `wit/workspace.wit#editor`      | partial |
| `…proofs10_host_api::a_plugin_searches_the_projects_text`                                                                                  | `wit/workspace.wit#workspace`   | partial |
| `…proofs10_host_api::a_plugin_reads_the_projects_log_blame_and_diff`                                                                       | `wit/workspace.wit#vcs`         | partial |
| `plugin_contributions::tests::proofs10_menus_settings::forwards_a_menus_path…`                                                             | `wit/workspace.wit#invocation`  | done    |
| `…proofs10_menus_settings::a_plugin_command_reads_the_projects_setting…`                                                                   | `wit/views.wit#settings`        | done    |

- **Left undeclared:**
  - `file_opened` moved to the new file `plugin_runtime/tests/proofs10_unsubscribe.rs`: its "unsubscribed hears nothing" half waits a fixed two seconds.
  - `a_plugin_reads_the_ui_language_the_settings_choose` moved to the new file `proofs10_locale.rs`: it needs the fixture and runtime language switching, which the native app does not have.
  - `plugins_window/tests/proofs10_lifecycle.rs`: needs the fixture.
- **Product fixes:**
  - `plugin_runtime.rs` `plugin_tabs_changed`: the window now reports its open editors (`mcp_reports.set_open_editors`) in every project window, like `src/features/mcp/index.ts:57-61`.
  - `plugins.rs` `Plugins::init`: `host.set_ui_locale(<start-up language tag>)`, like `src/features/app/locale.ts:20`. The native app has no runtime language (no non-test `set_locale` call), so this is always the language it starts with.
- **Fixture rebuild: not done.** `wasm32-wasip2` is installed, but `deactivate` and `locale` must first be added to `crates/ide-plugin/tests/fixtures/api14/src/lib.rs`, which is under `crates/`.

## 5. Notes and hypotheses

- **False in the notes:** none found among the 21 declared tests.
- **Unmeasured:** that `a_plugin_lists_the_windows_open_editors` failed before the fix. Check: revert the hunk in a scratch copy and run `cargo test --lib -- open_editors`.
- **Hypothesis:** a release build compiles without the `inspect` module. Check: `cargo check --release`.
- **Hypothesis:** `shift-f10` on the Welcome screen opens the selected row's menu right after closing the project. Check: the screenshot at the `welcome-context-menu` mark.
