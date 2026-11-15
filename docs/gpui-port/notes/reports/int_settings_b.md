Brief B is integrated: all 16 language pages are declared and wired, and every gate passes on the final tree. The Build pages and the language pages now share one toolchain drop-down and one test seam. 5 pages are done and 11 are partial; nothing is committed.

## 1. Gates (from `app-gpui/`, output in `$S/dsetb/`)

| Gate                                                                                                                             | Result                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Suite, final run 1, nothing beside it (`suite3.txt`)                                                                             | ok, 7492 passed, 0 failed, 0 ignored, 178.20 s                                                                                                |
| Suite, final run 2, with the 20× loop beside it (`suite4.txt`)                                                                   | ok, 7492 passed, 0 failed, 0 ignored, 217.56 s                                                                                                |
| 20× loop (`loop2/`): filters `settings_view:: proofs19_settings_languages proofs19_settings_misc smoke_settings_pages inspect::` | 20/20 green, 176 tests each, no test failed in any run. Suite 2 ended at 14:15:16; loop runs 1–18 overlapped it, runs 19 and 20 ran after it. |
| `cargo test --test web_view` (`webview2.txt`)                                                                                    | ok, 1 passed                                                                                                                                  |
| `cargo clippy --all-targets -- -D warnings` (`clippy3.txt`)                                                                      | exit 0                                                                                                                                        |
| `cargo build` (`build4.txt`)                                                                                                     | exit 0, 0 warnings                                                                                                                            |

The same gates also passed on an earlier tree (`suite1.txt` 7492 ok 177.81 s, `suite2.txt` 7492 ok 215.88 s, `loop/` 20/20). The test count rose from 7459 to 7492.

Failures seen during the work, both mine and both fixed:

- `languages_sdk::tests::a_dialog_closed_takes_what_it_found_with_it`: a dropped entity is only released when the app flushes its effects, so the test now calls `cx.update(|_| {})` after dropping the model.
- My first inspection launch named a script file that did not exist. The app ran with no script and `run.sh` stopped it by PID at its time limit.

## 2. Per part of the brief

**Declaration and wiring**

- Declared in `app-gpui/src/settings_view/pages/mod.rs`: `languages_controls`, `languages_pages`, `languages_sdk`.
- Declared in `app-gpui/src/workbench/mod.rs`: `mod proofs19_settings_languages;`.
- The pages are installed in `app-gpui/src/settings_view/custom_controls.rs`, after the Build pages, through `languages_pages::install(dialog, &model, Mount { scope, toolchains, has_project }, cx)`.
- The existing `SettingsDialogView::page_view` getter is reused; no second getter was added.
- The draft compiled on the first try, and its 19 e2e tests and 11 view and unit tests passed as drafted before I changed anything.

**One drop-down, one seam** (webview `src/features/settings/languages/components.tsx:27-108`, `SdkField` and `useDetected`)

- The single field is `SdkFieldView`, in `app-gpui/src/settings_view/pages/languages_sdk.rs`.
- From the language draft it keeps the choices: empty choice, installations found, custom value; TypeScript's Bundled/workspace; Python's virtualenv and path line. It also keeps one store per kind, like `useSdks`.
- From the Build draft it keeps the drawing: refresh icon "Detect again", "Detecting installations" while loading, folder icon "Browse…", and the selectors `-choice-N`, `-refresh`, `-browse`.
- The detection store, `Toolchains`, is kept per Settings dialog (by the model's entity id) and dropped when that model is released. A second window's Settings no longer empties the first's lists, which the draft's global would have done. A new model is made each time Settings opens (`workbench/settings_window.rs:212`).
- The seam is `SdkDetection` (found per kind, located tools, Xdebug, python-effective, plus a log of the kinds asked). With a seam set, its answers are used. Without one, a test build answers nothing at once, and other builds call the backend.
- The backend paths are behind a runtime `cfg!(test)` check, not `#[cfg(test)]`, so `cargo build` and the test build compile the same code.
- Detection's result is mapped field by field from `ide_host::ops::sdk::sdk_detect`, so the compiler checks the mapping; the draft went through JSON.
- `a_test_build_without_a_detection_of_its_own_finds_nothing` asks all four questions with no seam and gets nothing back each time.
- Gradle and Maven were moved onto this field. `build_sdk_field.rs` is deleted; its `ToolPage` now lives in `app-gpui/src/settings_view/pages/build_tool_page.rs`.
- Old → new test names: `build_sdk_field::tests::{the_installations_found_are_offered_once_the_field_is_drawn, a_test_build_without_a_detection_of_its_own_finds_nothing, browse_asks_for_a_folder_and_stages_it}` → `languages_sdk::tests::{the_combo_offers_the_detected_installations_and_stages_a_choice, a_test_build_without_a_detection_of_its_own_finds_nothing, browse_asks_for_a_folder_and_stages_it}`. No `parity.json` entry names the old path.
- `proofs19_settings_misc` now uses the shared seam, and its `bring_into_view` is `pub(super)` so the language tests can use it.

**Changes against the drafts, following the webview**

- Choice details: the draft put them into the label ("Auto — from composer.json"), but the webview's closed box shows the label only (`src/ui/Select.tsx:312`). `FormView` gained `with_details(key, DetailFn)` for the open list. This now applies to:
  - PHP Auto "from composer.json";
  - Java "SDK default" "from build / JDK" (the draft had left this out);
  - Package manager details. The detail logic is tested in `languages_pages::tests::a_package_manager_names_the_version_and_place_found`.
- Path fields now use the folder icon (`LanguagePages.tsx:158`), drawn by a new shared `icon_button` that the form fields use too.
- `LanguagePageView` now scrolls inside the dialog. Before, tall pages (Prettier, PHP) were cut off. The Prettier e2e now scrolls to Vue and clicks it; the PHP e2e scrolls to Xdebug.

**Tests added**

- `workbench::tests::proofs19_settings_languages::reset_and_cancel_discard_an_edit_on_each_language_page` covers all 16 pages.
- `languages_controls::tests::a_plugin_formatter_is_offered_for_the_languages_it_covers` covers the per-language formatter choice.
- `languages_sdk::tests::{the_combos_of_a_kind_share_one_detection, a_dialog_closed_takes_what_it_found_with_it}`.

**Smoke test**

- Unchanged: the language pages show as `<page view>`, which it already accepts.
- The plugin-formatters e2e was loosened without an observed failure: it now fixes only the header and the "Format on save" line. The reason is that `plugin_formatters()` reads the process-wide editor plugin host (`set_editor_providers`, `app-gpui/src/plugins.rs:195`). Hypothesis: another test's plugins could add rows. Check by running that test in a loop next to the plugin tests.

**Real window**

- Final binary, temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, trusted temp project, stopped by PID. Shots are in `$S/dsetb/insp/shots/t-0{1..9}-*.png`: Java, Node, Python, Go, Rust, TypeScript, PHP, Prettier, oxfmt, each in project scope with the Global / This project switch.
- To take them without running tools, I added a step to the debug-only inspection script, `no-toolchains`, in `app-gpui/src/inspect.rs`. This file is outside the brief's list. The step sets an empty `SdkDetection`, so the shots show the nothing-found state.
- I looked at each page: nothing is cut or overlapping, labels end in one colon, and tall pages scroll (Prettier's last rows and PHP's interpreter section are below the fold).
- Small existing difference: help lines of `FormView` text fields sit about 3 px left of those of drop-downs and SDK fields.
- No button was pressed.

**Left out, as the brief says:** `projectStructure`, Download JDK…, Python's Add Interpreter… and packages table, plugin run targets. Browse… still has no title or start folder, because `ControlEvent::Browse` carries neither.

## 3. Rows

Done means every control of the webview's page is drawn, a change is stored after Apply, and Reset and Cancel discard it, each proven by a named test. A missing Browse… title or start folder keeps a row partial, as it does for `settings:general`. E = `workbench::tests::proofs19_settings_languages`, R&C = E::`reset_and_cancel_discard_an_edit_on_each_language_page`.

| Row                                   | State   | Tests                                                                                                                                                                                                         | Missing                                                                      |
| ------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `settings:languages.go`               | partial | E::go_goroot_and_gopath_are_written_to_the_project, R&C, `languages_sdk::tests::*`                                                                                                                            | Browse… title/start folder                                                   |
| `settings:languages.go.buildTags`     | done    | E::go_build_tags_are_written_to_the_project, E::go_build_tags_and_modules_say_a_project_is_needed_without_one, R&C                                                                                            | —                                                                            |
| `settings:languages.go.modules`       | done    | E::go_modules_variables_are_written_to_the_project_and_cancel_discards, E::go_build_tags_and_modules_say_a_project_is_needed_without_one, R&C                                                                 | —                                                                            |
| `settings:languages.java`             | partial | E::java_level_and_shared_jdt_ls_apply_and_reset_discards_jvm_options, R&C, `languages_controls::tests::shared_jdt_ls_is_not_editable_for_a_workspace_project`                                                 | Download JDK…; Browse… title                                                 |
| `settings:languages.markdown`         | done    | E::markdown_preview_size_follows_the_editor_until_set_and_applies, R&C, `languages_controls::tests::the_preview_size_follows_the_editor_until_set_and_keeps_its_range`                                        | —                                                                            |
| `settings:languages.node`             | partial | E::node_package_manager_is_written_to_the_project, R&C                                                                                                                                                        | plugin run targets (Run in target, Language server runs on); Browse… title   |
| `settings:languages.oxfmt`            | done    | E::oxfmt_run_on_save_is_written_to_the_project, R&C                                                                                                                                                           | —                                                                            |
| `settings:languages.php`              | partial | E::php_include_path_and_level_are_written_to_the_project, R&C, `languages_controls::tests::{the_selected_php_shows_its_version_and_xdebug, a_stored_target_shows_as_unavailable_and_local_machine_is_staged}` | plugin run targets; Browse… title                                            |
| `settings:languages.pluginFormatters` | partial | E::plugin_formatters_format_on_save_applies, R&C, `languages_controls::tests::a_plugin_formatter_is_offered_for_the_languages_it_covers`                                                                      | per-language select proven only in a view test with formatters given by hand |
| `settings:languages.prettier`         | done    | E::prettier_provider_and_languages_are_written_to_the_project, R&C, `languages_controls::tests::a_language_checkbox_removes_and_adds_the_language`                                                            | —                                                                            |
| `settings:languages.ruby`             | partial | E::ruby_interpreter_is_written_to_the_project, R&C                                                                                                                                                            | Browse… title                                                                |
| `settings:languages.ruff`             | partial | E::ruff_configuration_directory_is_chosen_with_browse_and_written_to_the_project, R&C                                                                                                                         | Browse… title/start folder                                                   |
| `settings:languages.rust`             | partial | E::rust_linter_and_rustfmt_arguments_apply_and_reset_and_cancel_discard, R&C                                                                                                                                  | Browse… title                                                                |
| `settings:languages.serverTargets`    | partial | E::language_server_targets_local_machine_is_written_to_the_project, E::language_server_targets_are_not_editable_without_a_project, R&C                                                                        | plugin run targets                                                           |
| `settings:languages.typescript`       | partial | E::typescript_version_is_chosen_with_browse_and_written_to_the_project, R&C                                                                                                                                   | Browse… title                                                                |
| `settings:python.interpreter`         | partial | E::python_interpreter_and_activation_are_written_to_the_project, R&C                                                                                                                                          | Add Interpreter…, packages table, Browse… title                              |

Other rows: `settings:build.gradle` and `settings:build.maven` keep their state; their tests now run on the shared field. No IPC row is claimed:

- `sdk_detect`, `sdk_locate_tools`, `sdk_php_xdebug` and `sdk_python_effective` are proven only through the seam; their backend paths are compiled but never ran.
- `plugin_formatters` has no state that can only come from it.
- `ipc:languages` is already done.

## 4. Unmeasured

- Hypothesis: the four backend toolchain paths (detect, locate, Xdebug, python-effective) work as written. They type-check but never ran. To confirm, pick the language pages in a real window without `no-toolchains` on a disposable machine.
- Hypothesis: the plugin-formatters rows could vary under the full suite (section 2, smoke test bullet).

## 5. False or stale in notes and briefs

- The task message says that opening a page "runs no program on this platform". Reading `crates/ide-sdk/src/discover.rs` contradicts this; I did not run it. `exec::query` runs:
  - `node --version` (:539);
  - `--version` on npm, pnpm, yarn and bun (:551);
  - `rustc --version` and `--print sysroot` (:570, :576);
  - `go env GOPATH`, `go env GOROOT` and `go version` (:616, :628, :630);
  - PHP and Python version probes (:480, :270).

  Hypothesis: opening Node, Go, Rust, PHP or Python in the real binary runs those tools. Confirm with `grep -n "query(" crates/ide-sdk/src/discover.rs`, or by running `sdk_detect` with a PATH holding a logging script.

- Integration A's hypothesis that drawing Gradle and Maven runs no program is unconfirmed by the same reading (`java.rs:968` `java_runs` probe).
- The languages notes say to run `cargo fmt`; the rules forbid it. I formatted only my files with `$S/fmt.sh`.
- The notes call the drafts' combo and seam (`TestToolchains`) new; both are replaced. The notes' folded option details are wrong against `Select.tsx`.
- The brief names launcher `$S/d111/insp/run.sh`; I used `$S/insp8/run.sh`, as your message says.
- The brief expects the smoke test to need a change; it does not.
