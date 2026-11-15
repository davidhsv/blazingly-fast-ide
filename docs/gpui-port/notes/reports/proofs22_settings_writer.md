I wrote 24 tests in three new files, but none has been compiled or run, so every row I've marked "done" rests on tests nobody has run yet. The only check done is formatting: `rustfmt --edition 2024 --check`, run from the worktree, prints nothing for all three files, and no line passes 120 columns. That rustfmt exits 0 even when it finds a diff, so I checked its output rather than the exit code.

## Counts per spec file

Columns are cases already proven / newly written / missing natively / plumbing.

| Spec file                 | Before | New            | Missing                                               | Plumbing | Row     |
| ------------------------- | ------ | -------------- | ----------------------------------------------------- | -------- | ------- |
| cmakeEditor (2)           | 0      | 2              | 0                                                     | 0        | done    |
| languageServerTargets (2) | 1      | 0              | 1 (no plugin run targets)                             | 0        | partial |
| mavenRunner (1)           | 0      | 1              | 0                                                     | 0        | done    |
| phpXdebug (4)             | 0      | 3              | 1 (no re-check when installations are detected again) | 0        | partial |
| javaFormatter (2)         | 1      | 0              | 1 (Code Style language pages draw no controls)        | 0        | partial |
| mainMenuStyle (3)         | 0      | 2 (one per OS) | 1 (an invalid value isn't reset to hamburger)         | 0        | partial |
| mouseShortcut (3)         | 2      | 0              | 1 (swipe recording)                                   | 0        | partial |
| dialog (41)               | 16     | 13             | 10                                                    | 2        | partial |
| pages (5)                 | 5      | 0              | 0                                                     | 0        | done    |
| FontPicker (5)            | 3      | 2              | 0                                                     | 0        | done    |

- **pages.test.ts:** it needed no new test. All five cases already have tests in `settings_model::pages::tests` and `settings_model::builtin_pages::tests`; its current parity text is out of date.
- **dialog:** the 10 missing are the native-to-ascii toggle action, the Font preview in three font cases, the five Project Structure cases and Run Targets. The 2 plumbing are the separate window's Linux edge class and the hex colour sent to the window command.
- **mainMenuStyle:** the two new tests are compile-time OS alternatives. Only `hides_the_main_menu_setting_without_the_frameless_menu` exists on this macOS host, so its row lists only that one.

## New tests

`workbench::tests::proofs22_settings_units::`

- `round_trips_spaces_quotes_and_empty_values_in_the_cmake_and_build_arguments`
- `displays_each_profiles_arguments_when_switching_profiles`
- `chooses_an_installed_maven_runner_jre_in_project_scope_and_returns_to_project_jdk`
- `stages_properties_conversion_and_applies_the_checkbox`
- `edits_cmake_profiles_in_project_settings_and_persists_them_on_apply`
- `finds_gradle_jvm_and_applies_project_gradle_settings`
- `persists_maven_toolchains_and_profiles_in_project_settings`
- `finds_the_maven_active_profiles_setting`
- `finds_the_maven_toolchains_setting`
- `enables_saves_and_clears_the_custom_ui_fallback_font`
- `offers_both_main_menu_styles_on_a_frameless_title_bar` (Windows/Linux only)
- `hides_the_main_menu_setting_without_the_frameless_menu` (other OSes)
- `keeps_the_global_shared_jdt_ls_preference_separate_from_the_projects_override`
- `disables_sharing_for_a_java_language_server_target_only_in_project_scope`
- `keeps_sharing_available_when_java_runs_locally`
- `uses_a_staged_project_java_language_server_target_before_apply`
- `keeps_object_settings_stable_when_child_keys_are_staged`

`settings_view::pages::languages_controls::tests::proofs22_language_controls::`

- `shows_the_xdebug_version_of_the_selected_php_rechecked_when_the_interpreter_changes`
- `checks_the_php_on_the_search_path_for_the_default_interpreter`
- `hides_the_row_while_checking_and_when_the_check_fails`
- `disables_shared_jdt_ls_only_in_a_native_workspaces_project_scope`

`settings_view::pages::font_picker::tests::proofs22_font_picker::`

- `explains_each_font_list_state_without_declaring_saved_fonts_missing`
- `quietly_keeps_selections_and_manual_input_through_fallback_and_native_retry`

No accessor hunk is needed. The three `mod` lines are in the notes. Five tests depend on the `SdkDetection` seam as it is at HEAD (the Maven JRE test, the Gradle test and the three PHP tests). They all build it with `..SdkDetection::default()`, so new fields shouldn't break them.

## What I'm unsure of

- **Cited tests in a file being edited.** Hypothesis: the three tests I cite from `proofs19_settings_languages.rs` still exist under those names. Check: `grep -n "fn java_level_and_shared_jdt_ls_apply_and_reset_discards_jvm_options\|fn reset_and_cancel_discard_an_edit_on_each_language_page\|fn language_server_targets_are_not_editable_without_a_project" app-gpui/src/workbench/tests/proofs19_settings_languages.rs`
- **Maven search tests.** Hypothesis: no other page matches "Active profiles" or "Toolchains", so the dialog stays on Maven. The Maven page doesn't register those labels for the search. Check: run `cargo test finds_the_maven_` and look at `selected_page`.
- **UI fallback font test.** Hypothesis: clicking the picker while it's disabled opens no suggestions, and Escape after typing only closes the list rather than cancelling Settings. Check: run `enables_saves_and_clears_the_custom_ui_fallback_font`.
- **Proxy test.** Hypothesis: Reset drops the edits the test staged directly on the dialog's model. Check: run `keeps_object_settings_stable_when_child_keys_are_staged`.
- **CMake round trip.** Hypothesis: an empty argument and a `C:\…` path survive being written to the project's settings file and read back. Check: run `round_trips_spaces_quotes_and_empty_values_in_the_cmake_and_build_arguments`.
- **Main menu on Linux.** Hypothesis: the choice list near the bottom of the page stays inside the test window. Check: run `offers_both_main_menu_styles_on_a_frameless_title_bar` on Linux CI.
- **"While checking" in the PHP tests.** The test holds each question and answers it later through `languages_sdk::answer`, so the answer still comes from the seam; only the timing is controlled. The pending state is visible only that way. The tests also reset the view's private `xdebug` and `asked_path` fields, as the existing test does.
- **Thin citations.** javaFormatter case 2 is cited on a test that checks search keywords only; no language page draws the controls natively. dialog case 2's checkbox stages its value directly, not through the action, and that action (case 3) is missing.

Files are in the worktree, the notes and rows in the scratchpad:

- /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs22_settings_units.rs
- /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/settings_view/pages/languages_controls/tests/proofs22_language_controls.rs
- /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/settings_view/pages/font_picker/tests/proofs22_font_picker.rs
- /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs22_settings.md
- /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs22_settings_rows.json
