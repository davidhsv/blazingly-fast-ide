I've landed Part 0 and all three parts of the slice. The full suite passed twice (8182 tests each run), and every test of the slice passed in all twenty repeated runs. Nothing is committed.

## Part 0: Debug on a configuration that runs on a run target

- **Check:** `Workbench::remote_debug_blocked` (`app-gpui/src/workbench/debug.rs`) calls the new model `app-gpui/src/run/remote_debug.rs`. It follows `src/features/run/remote.ts:70-95`: the same title, the same body, key `run.remoteDebug`, and Run / Edit Configuration… buttons carrying the configuration id. A compound configuration is never refused by its own name, as in `index.ts:168-174`.
- **Where it is called:** in `Workbench::debug_config` (`run.debug`, `run.debugConfig`, the run widget, Rerun, chooser, Services, gutter marks) and before `launch_produced` in `launch_production` (Debug Context), matching `index.ts:174` and `contextRun.ts:163`.
- **Tests:** they failed with the check switched off. The control test passes both ways.
  - `workbench::debug::tests::proofs34_remote_debug::debugging_a_configuration_on_an_ssh_target_is_refused_and_starts_no_adapter`: exact notice text, no session, the stand-in adapter's start file stays empty.
  - `…::debug_configuration_in_a_container_is_refused_and_its_button_edits_the_configuration`
  - `…::rerun_in_the_debugger_of_a_configuration_now_on_a_run_target_is_refused`
  - `…::the_run_widgets_debug_button_on_a_run_target_is_refused`
  - `workbench::run_context::tests::proofs20_run::debug_context_on_a_run_target_is_refused_and_starts_no_session`
  - Control: `…::a_configuration_on_this_machine_is_still_debugged`
  - Model tests: `run::remote_debug::tests::*` (3)
  - Logs: `$S/dtpl2i/part0_without.log` (0 passed, 5 failed), `$S/dtpl2i/part0_with.log` (16 passed).
- **Container exception:** `run/container_debug.rs` has no caller, so the native debugger has no container path. Python under debugpy in a container is therefore refused too, with the title "Debugging on run targets is not supported". That title for this case is native only.
- **Not proven by test:**
  - Hypothesis: the check before a compiled file's build matters on its own; today the context test also passes through `debug_config`. Check: a compiled-file Debug Context test with stand-in tools.
  - Hypothesis: a gutter mark's Debug goes through the check (read at `run_marks.rs:340`).
  - The notice's Run button is untested, because it would open SSH.
  - `debug.startConfig` called directly is not checked, as in the webview's debugger action. The existing `debugs_a_configuration_on_a_plugin_target_on_this_machine` stays valid for that reason.

## A created file never leaves its folder

All of these run on the real backend and assert the whole project tree afterwards. Without the workbench's own `stays_in` check none of them fails, because `plan_file`'s own checks refuse each case (log: `$S/dtpl2i/stays_without.log`).

- Typed `../`, `..\` and `a/../../x`, a template file name with a separator, a template extension with one, a kind extension with one: `proofs34_template_files::no_name_template_or_kind_creates_a_file_outside_the_folder_new_was_used_in`. Each shows "Invalid name".
- An absolute typed name is read below the folder (`src/abs/inner.txt`): same test.
- A plugin's inferred package goes into the body only, never the path: `…::a_plugin_templates_package_goes_into_the_body_never_into_the_path`.
- A folder that is a symlink to outside the project: the file lands in the link's target, as `create.ts` and `fs_create_file` do. A name typed through the link does the same: `…::new_on_a_linked_folder_creates_the_file_in_the_folder_the_link_points_to`.
- No overwrite: `…::a_taken_name_is_reported_in_the_dialog_and_the_file_is_left_as_it_was`. For notebooks, which go through `create_file_bytes`: `…::an_existing_notebook_is_not_overwritten_by_a_new_one_of_the_same_name` (bytes kept).
- A read-only folder: `…::a_folder_that_cannot_be_written_reports_the_failure_and_creates_nothing`.
- Restricted Mode: creation needs no trust, but the backend refuses the external formatter. `…::in_restricted_mode_new_starts_no_formatter_after_creating_the_file` shows no formatter start and no notification; the control `…::in_a_trusted_project_new_reformats_the_file_with_the_formatter` shows one start.

## Parts

All three landed whole.

- **Part 1 (dialog and creation):**
  - The draft is wired.
  - Built-in templates added: Jupyter Notebook, Zig File, C# Class, Zig Test Function.
  - C# namespaces from the nearest `.csproj`, with a namespace block before C# 10 (`create.ts:76-94`, `:170-177`).
  - A notebook's body is written with the file and nothing waits for a text editor (`create.ts:141-151`, `:190-200`).
  - Template names and kind labels are translated.
  - The templates in effect come from `files_store`.
- **Part 2 (actions, New menu, New from Template):**
  - One action per template.
  - The New menu is filtered by project type and Tomcat (`index.ts:96-137`); the detection is a port of `projectType.ts` (`project_view/project_type.rs`).
  - The actions follow the stored templates and resync when a project opens or closes.
  - The three existing-test rewrites the notes predicted: two were needed, the third test passed unchanged.
- **Part 3 (Emmet's Update Tag):** the draft is wired. A new test clicks the editor menu's row and checks that one Undo reverts both tags.

## Existing tests whose expectations changed

- `context_menus::tests::every_menu_group_is_listed_once`: `templates.new` now has rows (`src/features/templates/index.ts:117`).
- `editor_extras::tests::go_to_test_offers_create_new_test_only_with_the_file_templates`: it now asserts Create New Test… is offered and opens the dialog on the test kind, then unregisters the action to prove the other branch (`src/editor/navigation/index.ts:154`, `index.ts:259`).
- `http_client::tests::new_http_request_creates_a_request_file_with_the_example_in_the_folder`: in a folder it now asks for the name (`src/features/http-client/index.ts:229-230`). The `requests_1.http` numbering moved to the new `new_http_request_without_a_folder_names_the_scratch_requests_then_requests_1`.
- `files_builtins::tests::package_rules_are_declared_per_language`: the code-template count goes from 7 to 8 (`files/builtins.ts:70`).
- `files_model::tests::applies_styles_and_extensions`: one Zig assertion was added; nothing was loosened.

## Gates

- **Suite run 1:** `$S/dtpl2i/suite1.log`: "test result: ok. 8182 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 218.81s". Load averages 4.43 before, 62.97 after.
- **Suite run 2:** `$S/dtpl2i/suite2.log`: "test result: ok. 8182 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 281.46s".
- **Twenty runs during suite 2:** 20/20 passed, 246 tests each, no failure in any run. Logs: `$S/dtpl2i/loop_summary.log` and `loop_1.log` to `loop_20.log`.
- **web_view:** `$S/dtpl2i/web_view.log`: "test result: ok. 1 passed; 0 failed".
- **Clippy** with `-D warnings`: exit 0 (`$S/dtpl2i/clippy.log`).
- **`cargo build`:** 0 warnings (`$S/dtpl2i/build.log`).
- No process of mine is left; the inspected app's PID is gone.

`$S` is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`.

## Rows

| key                                                   | state                               | proving tests (crate-relative paths)                                                                                                                                                                                                                                                              |
| ----------------------------------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:templates.newFromTemplate`                    | done                                | `workbench::tests::proofs34_template_actions::new_from_template_lists_the_enabled_templates_and_creates_the_chosen_one`, `…::new_from_template_with_a_template_of_its_own_names_a_file_of_it`, `…::new_from_template_with_an_extension_opens_that_templates_dialog_with_the_kind_and_the_name`    |
| `action:(dynamic) src/features/templates/index.ts:81` | done                                | `…::the_template_actions_and_rows_follow_the_templates_in_effect`, `…::a_template_action_without_an_argument_uses_the_active_files_folder`, `…::two_windows_on_projects_of_different_schemes_each_offer_their_own_templates`, `…::the_template_actions_and_dialogs_are_titled_in_the_ui_language` |
| `point.registerMenuGroup:templates.new`               | done                                | `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`, `…::the_new_menu_offers_the_templates_of_the_projects_type`, `…::is_listed_in_the_new_menu_with_its_icon_without_duplicating_entries`                                                                                 |
| `point.registerMenuGroup:templates.newFromTemplate`   | done                                | `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`                                                                                                                                                                                                                        |
| `point.registerOverlay:templates.newFile`             | done                                | `workbench::tests::proofs34_template_files::*` (17), `templates::proofs34_new_file_dialog::*` (11)                                                                                                                                                                                                |
| `point.registerOverlay:templates.emmetUpdateTag`      | done                                | `workbench::tests::proofs34_emmet::*`, `templates::proofs34_update_tag_dialog::*`                                                                                                                                                                                                                 |
| `action:emmet.updateTag`                              | done                                | `workbench::tests::proofs34_emmet::the_editor_menus_row_opens_the_prompt_and_one_undo_reverts_the_update`, `…::update_tag_with_emmet_renames_the_element_and_sets_its_attributes`, `…::update_tag_is_off_outside_markup_and_a_caret_outside_any_tag_is_told`                                      |
| `point.registerMenuGroup:templates.emmet`             | done                                | `…::the_editor_menu_offers_update_tag_for_markup_files_only` and the click test above                                                                                                                                                                                                             |
| `point.registerOverlay:templates.chooser`             | unchanged (partial, not this slice) | the two chooser cases of `views.test.tsx` map to the existing `proofs33_chooser_view` tests                                                                                                                                                                                                       |
| `unit:…/views.test.tsx`                               | partial                             | 3 dialog cases in `templates::proofs34_new_file_dialog`; the chooser and Settings cases belong to other slices                                                                                                                                                                                    |
| `unit:…/pluginFiles.test.tsx`                         | done, 6/6                           | `templates::proofs34_new_file_dialog::*`                                                                                                                                                                                                                                                          |
| `unit:…/files.test.ts`                                | done                                | the 7 new cases, e.g. `templates::files_model::tests::come_from_the_projects_root_namespace_and_the_folders_below_the_csproj`. Hypothesis: the older cases keep their tests by name as the parity proof says                                                                                      |
| `unit:…/pluginPackages.test.ts`                       | done, 8/8                           | adds `templates::files_create::tests::uses_the_nearest_csproj_for_csharp`                                                                                                                                                                                                                         |
| `unit:…/emmet.test.ts`                                | partial, 4/8                        | the three expansion cases and the Django case need an expansion engine                                                                                                                                                                                                                            |
| `unit:…/templateCreate.test.ts`                       | partial, 5/6                        | "creates an empty file when the template renders nothing" is proven on the model only (`files_create::tests::a_file_a_viewer_opens_gets_its_body_on_creation_and_an_empty_body_an_empty_file`)                                                                                                    |
| `unit:…/builtinNames.test.ts`                         | done                                | `templates::files_builtins::tests::are_translated_where_the_new_menu_and_dialog_show_them`                                                                                                                                                                                                        |

`parity.json` has no keys for `templateCreate.test.ts`, `builtinNames.test.ts` or the action/menu rows above. The INVENTORY rows for `navigate.gotoTest` and `gotoTest.ts` still say "Create New Test… missing"; that is stale now.

## Hypotheses H1 to H12

- **H1 (input method):** held in tests. `does_not_submit_while_confirming_ime_text` passes on the test platform; no manual check with a Chinese input method was done.
- **H2 (quiet Reformat):** held. No notification after the refused Reformat (restricted test); the Java and HTTP creations end saved.
- **H3 (Save keeps the text):** held.
- **H4 (language ids):** held for html. Hypothesis for vue: the menu test is path-based.
- **H5 (caret side of the edit):** not measured.
- **H6 ("one set of actions per process"):** wrong as written. Each window's New menu shows its own scheme's templates. The actions are the union of all windows' templates. Running a template action from another window's scheme opens the template list.
- **H7 (project name is the folder name):** not measured.
- **H8 (project roots):** held for a folder below `src/main/java` (package `com.acme`).
- **H9 (opening a file outside the project):** held.
- **H10 (Enter presses OK in the Update Tag prompt):** held natively; the webview side is not checked.
- **H11 (translated action titles):** held in zh.
- **H12 (drafts compile as written):** wrong. Six fixes were needed: two `AppContext` imports, a lifetime in a test helper, a temporary borrow, a closure lifetime, and boxing the dialog's large state for clippy.

## Shots

All under `$S/insp8/shots/`. Seen:

- `dtpl2i-01-new-popup-light-1.png`: the Cargo project's New popup with icons and no Python, Go or Java rows.
- `dtpl2i-02-new-popup-closed-light-1.png`: the popup closed by a click outside. Escape had not closed it in the first run.
- `dtpl2i-03-new-file-invalid-light-1.png` and `-09-…-dark-1.png`: "New Rust File", three kinds with Struct selected, red border and "Invalid name".
- `dtpl2i-04-from-template-list-light-1.png` and `-10-…-dark-1.png`: the "New" list with the "src" line and all templates.
- `dtpl2i-05-created-light-1.png` and `-11-…-dark-1.png`: `user_card.rs` open with `pub struct user_card {`, caret at 2:5 (the snake-case name style, as the webview's).
- `dtpl2i-06-update-tag-light-1.png` and `-12-…-dark-1.png`: the Update Tag prompt for `<div>` with the example in the code font and OK / Cancel. It sits centred in the window. Hypothesis: the webview places it the same way.
- `dtpl2i-07-new-popup-dark-1.png` and `dtpl2i2-01-new-popup-dark-1.png`: after `view.toggleTheme`, `project.newFile` opened straight into File naming mode, both times. Hypothesis: this is not from this slice; check by running `$S/insp8/dtpl2i2.script` against the binary built at HEAD.

## Gaps and differences

- **Plugin file templates are never in effect natively:** `set_contributed` has no caller in the window.
- **Not built:** the "add the created file to Git" notice and the "Could not fill the new file" notification.
- **Main-thread read:** the New menu's project type is detected from the window's root (not a workspace member), by a synchronous directory listing on the main thread, as `run_context::peek` already does.
- **Resync on window close:** actions are not resynced when a window closes without closing its project.
- **File › New…:** this is `file.new`, the same popup. Hypothesis: same rows (read, not tested).
- **Stale in the notes:** "run `cargo fmt`"; "files.test.ts and pluginPackages.test.ts already done"; the third rewrite in hunk 2h was not needed.

## Files edited

- **Under `app-gpui/src/` (tracked):**
  - `project_view/mod.rs`, `run/mod.rs`
  - `templates/mod.rs`, `files_builtins.rs`, `files_create.rs`, `files_model.rs`, `new_file_dialog.rs`, `proofs34_new_file_dialog.rs`, `proofs34_update_tag_dialog.rs`
  - `workbench/mod.rs`, `context_menus.rs`, `debug.rs`, `editor_extras.rs`, `emmet.rs`, `http_client/tests.rs`, `run_context.rs`, `run_context/tests/proofs20_run.rs`, `template_actions.rs`, `template_files.rs`
  - `workbench/tests/proofs34_emmet.rs`, `proofs34_template_actions.rs`, `proofs34_template_files.rs`
- **New:** `app-gpui/src/run/remote_debug.rs`, `app-gpui/src/workbench/debug/tests/proofs34_remote_debug.rs`, `app-gpui/src/project_view/project_type.rs`
- **Left undeclared, as the brief allows:** `templates/proofs34_extension_guard_pending.rs` and `templates/proofs34_chooser_pending.rs`.
- **Scratch:** the screenshot scripts `$S/insp8/dtpl2i.script` and `$S/insp8/dtpl2i2.script`.
