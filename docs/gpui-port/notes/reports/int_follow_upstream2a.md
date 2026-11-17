Items 1 to 5 have landed and every gate passes, but item 4 is only partly done. The composer still shows the effort inside the model button, not as its own picker. I wrote the code changes and the test rewrites together, so no test was watched failing before its fix. Nothing is committed and no git command that changes anything was run.

## 1. Gates

- **Suite run 1** (nothing else running): `test result: ok. 8234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 220.46s`. Load average before 2.74 and after 13.99. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/suite_try1.log`
- **Suite run 2**, with the 20× loop alongside: `test result: ok. 8234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 219.28s`. Load average before 9.39 and after 28.68. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/suite_run2.log`
- **20× loop**: 137 tests per run, covering every test I added or changed plus their whole modules. All 20 runs passed (`test result: ok. 137 passed; 0 failed …`) and no test failed even once. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/loop20.log`
- **`cargo test --test web_view`**: `test result: ok. 1 passed; 0 failed …`. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/web_view.log`
- **`cargo clippy --all-targets -- -D warnings`**: exit 0, 0 warnings. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/clippy.log`
- **`cargo build`**: exit 0, 0 warnings. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol2/build.log`
- **Failures seen along the way**: one, in a module run before the gates. `run_testing::services::tests::uses_the_saved_display_order_and_leaves_other_types_alone` failed with `left: [] right: ["c","a","b"]` because it relied on Spring Boot being added by default. I added Spring Boot explicitly, as `services.test.ts:638` does. No `pgrep` match for cargo or the app is left.
- **Failed first: not measured, for every test below.** Hypothesis: each rewritten test fails on the old code. To confirm, rebuild the test against the code at `efc778614` and run it.

## 2. Items

### Item 1: Services

- **Spec at HEAD** (`src/features/run/services.test.ts`):
  - :287 starts empty and persists the user's list per project
  - :234 lists added types fully and keeps other types out
  - :264 shows nothing for a type that was never added
  - :301 keeps the type out of Services
  - :305 drops a deleted configuration once its run has ended
  - :335 hides a configuration even while it runs
  - :351 keeps an added type's group whose configurations are all hidden
  - :366 does not duplicate the Docker runs
  - :796 does not apply a picker result after switching projects
- **Upstream**: `d5cb379ca`, "show only services the user added".
- **Before**: Spring Boot was in Services by default. Every configuration that ever ran was remembered per project and listed. Types of running sessions were listed even if never added. A hidden configuration came back while it ran. Docker kinds were not filtered.
- **Now**:
  - The list of added types starts empty.
  - Only added types are listed, and Docker kinds are left out (`services.tsx:340`).
  - A hidden configuration stays hidden even while it runs.
  - The remembered-runs list is removed everywhere. That covers the `ran` key, `record_run` and its retain step, the `seen_runs` tracking in the Services window, and the `ran` field of the stored settings.
  - The run-start effect `RecordRun` stays, because the chooser's Recent list uses it.
  - D143 had already changed the Docker side of Services. This finishes the run-configuration side.
- **Rewritten (old → new expectation)**:
  - `defaults_to_spring_boot…` → `starts_empty_and_persists_the_users_list_per_project`: `["spring-boot"]` → `[]`.
  - `lists_added_types_fully_and_other_types_only_with_their_runs` → `lists_added_types_fully_and_keeps_other_types_out`: npm and Python groups → only Cargo; npm shows once it is added.
  - `shows_spring_boot_by_default…` → `shows_nothing_for_a_type_that_was_never_added`.
  - `hides_a_configuration_unless_it_runs` → `hides_a_configuration_even_while_it_runs`: `[a,b]` while running → `[b]`.
  - `keeps_an_empty_default_group…` → `keeps_an_added_types_group…`.
  - `drops_a_deleted_configuration_once_its_run_has_ended`: npm added, so the result is an empty group (:307) instead of no group.
  - `does_not_apply_a_picker_result_after_switching_projects`: `DEFAULT_TYPES` → `[]` (:811).
  - `shows_configured_debug_roots_without_a_run_session`: the kind is now added (:109).
  - `uses_the_saved_display_order…`: Spring Boot added explicitly (:638).
  - `stops_and_forgets…`: no longer asserts the remembered runs.
  - `a_picked_selection_hides_the_rest…`: `["spring-boot","npm"]` → `["npm"]`.
- **Deleted**, because their spec cases were removed upstream: `orders_remembered_configurations_when_their_type_was_not_explicitly_added`, `persists_the_ran_configuration_ids_per_project`, `lists_previously_run_configurations_without_a_session`, `ignores_ran_ids_whose_configuration_is_gone_and_merges_sessions`.
- **Added**: `keeps_the_type_out_of_services`, `does_not_duplicate_docker_compose_docker_dockerfile_or_docker_image_runs_in_the_run_configurations_provider`.
- **Left**:
  - Grouping by Run/Debug folders (case 5).
  - The running-services badge (cases 20–22).
  - The sbt and jar-application categories (case 23).
  - Hypothesis: the detail pane's console strip still has its Run/Debug/Stop buttons, which `d5cb379ca` removed. To check, read `workbench/services_window.rs`, where the detail is built.

### Item 2: Run configuration chooser

- **Spec at HEAD** (`src/features/run/chooserPinned.test.tsx`):
  - :54 lists pinned configurations first, in pin order
  - :111 pins and unpins from the row's actions menu
  - :127 offers the executors, Edit and Delete in the actions menu, also on →
  - :212 lists the configurations started last after the pinned ones
  - Also `runConfigTypes.test.tsx:894`.
- **Upstream**: `6c301a7b6`.
- **Before**: Current File was the first row and there were no separators. The selected row had an inline Pin button.
- **Now**:
  - Current File comes after the configurations, and separator rows (shown as `---`) sit before Current File and before Edit Configurations (`ConfigChooser.tsx:90-97`).
  - The row's buttons are Run/Debug (or Rerun/Stop), then More Actions.
  - More Actions, or →, opens a menu inside the chooser: Run, Debug, a separator, Edit…, Pin or Unpin, and Delete. Pin keeps the chooser open; Edit closes it and selects the configuration.
- **Rewritten**:
  - `pins_and_unpins_from_the_rows_button` → `pins_and_unpins_from_the_rows_actions_menu`.
  - `lists_pinned_configurations_first…`: `[current,[Pinned],serve,App,[npm],build,edit]` → `[[Pinned],serve,App,[npm],build,---,current,---,edit]`.
  - `proofs8_chooser::lists_the_configurations_started_last…`: same move of Current File.
  - `is_always_offered_in_the_chooser`, `the_keyboard_moves_over_rows_and_acts_on_them`, `config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind`, `offers_rerun_and_stop…` and `keeps_run_and_debug…`: the button lists now end with "More Actions" and Pin is gone from them.
  - `workbench::tests::proofs9_run_chooser::keeps_pins_per_project_across_reloads`: now pins through → and the menu, and asserts the chooser is still open in the workbench and that the store has `chooser_open` (:119).
- **Added**: `offers_the_configurations_executors_edit_and_delete_in_its_actions_menu_also_on_right`.
- **Left**:
  - All Configurations folding (cases 7 and 8) and the Unity cases (1–3).
  - The menu's "Configuration" header is drawn as a separator, because the native menu has no header rows.
  - Separator rows are full row height, because the list draws every row at the same height.

### Item 3: Version control

- **Spec at HEAD**:
  - `ignore.test.ts:62`
  - `historyOps.test.ts:87`
  - `gitOps.test.ts:136` (the protected-branch refusal)
  - `branchFilterPopup.test.tsx:39`, `:50` and `:81`
- **Upstream**: `a58e3a45f` (ignore), `f7a507a5d` (pushed history), `8faedce17` (branch filter).
- **Local exclude file**: it is now opened after the rule is written (`ignore.ts:22-24`). The unused destination parameter of `ignore_written` is gone. Rewritten: `opens_nothing_for_the_local_exclude_file` → `opens_the_local_exclude_file_the_rule_was_written_to`, from `open == None` to `Some("/p/.git/info/exclude")`.
- **History rewriting**:
  - The menu no longer disables Edit Message, Fixup, Squash Into, Drop, Interactive Rebase, Squash Commits or Drop Commits for pushed commits. Push All up to Here keeps its own rule.
  - A rewrite now counts as touching pushed history when HEAD is pushed or, otherwise, when its oldest commit is at or below the merge base with the upstream. This uses the backend's merge-base and log operations, as `rebase.tsx:62-84` does.
  - New `protected_branch_problem` follows `rebasePlan.ts:47-55`: the branch matches the Git "Protected branches" setting (default `master; main`) and has an upstream.
  - On a protected branch, every rewriting action and Undo Commit of a pushed HEAD refuse with "Pushed commits of the protected branch 'main' cannot be rewritten", before anything else is asked. Elsewhere a danger confirmation is shown first, and declining it rewrites nothing.
- **Rewritten**:
  - `disables_history_rewriting_for_pushed_commits` → `offers_history_rewriting_for_pushed_commits_too_idea_allows_it_the_run_confirms`: the rewriting actions go from disabled to `[]`, and Push All up to Here stays disabled.
  - `the_commit_menu_disables_history_rewriting_for_a_pushed_commit` → `the_commit_menu_offers_history_rewriting_for_a_pushed_commit_too`.
- **Added**:
  - `rebase_plan::tests::refuses_history_rewriting_on_a_protected_branch_with_an_upstream`.
  - `workbench::log_actions::tests::refuses_every_rewrite_of_pushed_commits_of_a_protected_branch`. Covers Edit Message, Drop Commit, Drop Commits, Squash Commits, Interactive Rebase and Commit-and-Rebase after Fixup, plus Undo Commit of a pushed HEAD. Each shows the exact title and message, and the commit ids are unchanged. A local commit can still be reworded.
  - `…asks_before_rewriting_pushed_commits_and_rewrites_nothing_when_declined`. Same actions on a non-protected branch: the exact warning is shown, declining leaves the ids unchanged and opens no further dialog, and Continue then rewords the commit.
  - These tests use temp repositories and a local bare repository only, with no network.
- **Branch filter**:
  - Shows HEAD, then a Favorites section (main by default, or the user's favorites, by name), then Local and Remote collapsed.
  - A click or Enter opens or closes a group, → opens it and ← closes it; inside, the current branch comes first, then favorites. A search lists every match under Local and Remote.
  - The favorites are passed in from the workbench.
  - Rewritten: `groups_head_local_and_remote_branches_and_leaves_out_the_remote_head` → `lists_head_and_the_favorites_with_local_and_remote_collapsed_until_opened_no_remote_head`, from Local and Remote expanded to the collapsed rows. `moves_over_section_headers…`: only its comment changed (Favorites header).
  - Added: `shows_the_users_favorite_branches_local_and_remote_under_favorites_by_name`.
  - Left: the favorite star and the indentation of branches inside a group.

### Item 4: Agents (partly done)

- **Spec at HEAD**: `AgentsView.test.tsx:336`, `agents.test.ts:296`.
- **Upstream**: `9e46d61dc` (menu), `e54580265` (collaboration mode), `0ce7f6e43` (model parameters), `3580cbacd` (usage meter).
- **Now**:
  - The task menu no longer has Open; Rename… comes first.
  - The composer's main settings are model, mode, collaboration mode, permissions, approval, effort, then every model parameter (such as Fast). When a model option exists, those parameters are kept out of the row.
  - The usage label reads "24k/200k (12%)" and carries the percentage; the meter text still shows "12%".
- **Rewritten**:
  - `task_actions::tests::keeps_placement_out_of_session_menus`: `[Open, Rename, …]` → `[Rename, …]`.
  - `proofs11_task_list::keeps_placement_out_of_session_menus`: contains Open → no Open, and Rename first.
  - The task-list menu test: asserts Rename instead of Open.
  - `composer::tests::picks_the_main_settings_and_cycles_the_mode`: `[model,mode]` → `[model,mode,fast]`, plus collaboration mode and the new usage label.
  - `session::tests::shows_the_providers_usage_limits…`: now reads the percentage field (still "12%").
- **Not done: effort as its own picker beside the model** (`ConfigPickers.test.tsx`, case 11). At HEAD the effort is a plain dropdown and the model popup has no effort steps. Natively, `config_bar` still builds the combined model-and-effort picker, and the model popup still has the effort steps.
  - Doing it means reworking `config_pickers_view.rs` and these tests: `config_bar::tests::shows_the_model_with_the_agents_effort_name_and_the_mode_with_allow_all`, `steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup` (its spec case was removed), `filters_the_model_list…`, `new_task_view/tests.rs:177`, `composer_view/pickers.rs:200` and `proofs13_agents_config.rs:168,233`.
  - I left it out to stay inside one pass, so this row must not be counted.
- **Also left**: Terminal hidden for local tasks at the project root, the "Fork" label, and the arrange entries of the task menu. `keeps_placement_out_of_session_menus` still asserts `OpenInTerminal("/project")` in its exact list. Fast inside the model popup is also left.

### Item 5: Deployment model rule

- **Spec at HEAD**: `deploymentSettings.test.ts:34`. At `3662ca0ea` the case normalised ftp and ftps to SFTP too.
- **Upstream**: `ae3973f11`.
- **Before**: the native test asserted that an unknown type was refused.
- **Now**: new `read_deployment_servers` reads an unknown or missing type as SFTP. FTP and FTPS stay types of their own, and a value that is not a list reads as none. The Settings › Deployment page now reads its servers through it.
- **Rewritten**: `normalizes_unsupported_types_to_sftp`, from "is_err" to `Sftp` for missing, `"webdav"`, `"inPlace"` and `null`.
- **Limit**: `workbench/deployment.rs:582` and `workflow_settings.rs:157` still read typed settings parsed by the backend in `crates/`, which I may not edit. Hypothesis: there a server with an unknown type makes the backend fall back to its default settings. To check, put a `"kind":"webdav"` server in a test settings layer and read `settings::effective(...).deployment`.

## 3. Spec cases that gained a native test (by full name)

| Spec case                                                                                                      | Native test                                                                                                                                                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| services.test.ts › lists added types fully and keeps other types out                                           | `run_testing::services::tests::lists_added_types_fully_and_keeps_other_types_out`                                                                                                                                                                                                                                       |
| › shows nothing for a type that was never added                                                                | `run_testing::services::tests::shows_nothing_for_a_type_that_was_never_added`                                                                                                                                                                                                                                           |
| › starts empty and persists the user's list per project                                                        | `run_testing::services::tests::starts_empty_and_persists_the_users_list_per_project`                                                                                                                                                                                                                                    |
| › keeps the type out of Services                                                                               | `run_testing::services::tests::keeps_the_type_out_of_services`                                                                                                                                                                                                                                                          |
| › drops a deleted configuration once its run has ended (#1497)                                                 | `run_testing::services::tests::drops_a_deleted_configuration_once_its_run_has_ended`                                                                                                                                                                                                                                    |
| › hides a configuration even while it runs                                                                     | `run_testing::services::tests::hides_a_configuration_even_while_it_runs`                                                                                                                                                                                                                                                |
| › keeps an added type's group whose configurations are all hidden                                              | `run_testing::services::tests::keeps_an_added_types_group_whose_configurations_are_all_hidden`                                                                                                                                                                                                                          |
| › does not duplicate docker-compose, docker-dockerfile or docker-image runs…                                   | `run_testing::services::tests::does_not_duplicate_docker_compose_docker_dockerfile_or_docker_image_runs_in_the_run_configurations_provider`                                                                                                                                                                             |
| › does not apply a picker result after switching projects                                                      | `run_testing::services::tests::does_not_apply_a_picker_result_after_switching_projects`                                                                                                                                                                                                                                 |
| chooserPinned › lists pinned configurations first, in pin order, and not again in their group                  | `run::chooser_view::tests::lists_pinned_configurations_first_in_pin_order_and_not_again_in_their_group`                                                                                                                                                                                                                 |
| › pins and unpins from the row's actions menu                                                                  | `run::chooser_view::tests::pins_and_unpins_from_the_rows_actions_menu`, `workbench::tests::proofs9_run_chooser::keeps_pins_per_project_across_reloads`                                                                                                                                                                  |
| › offers the configuration's executors, Edit and Delete in its actions menu, also on →                         | `run::chooser_view::tests::offers_the_configurations_executors_edit_and_delete_in_its_actions_menu_also_on_right`                                                                                                                                                                                                       |
| › lists the configurations started last after the pinned ones, newest first                                    | `run::chooser_view::tests::proofs8_chooser::lists_the_configurations_started_last_after_the_pinned_ones_newest_first`                                                                                                                                                                                                   |
| ignore › opens the local exclude file the rule was written to                                                  | `vcs_ops::ignore::tests::opens_the_local_exclude_file_the_rule_was_written_to`                                                                                                                                                                                                                                          |
| historyOps › offers history rewriting for pushed commits too (IDEA allows it; the run confirms)                | `vcs_ops::git_log::tests::offers_history_rewriting_for_pushed_commits_too_idea_allows_it_the_run_confirms`, `workbench::log_actions::tests::the_commit_menu_offers_history_rewriting_for_a_pushed_commit_too`, `workbench::log_actions::tests::asks_before_rewriting_pushed_commits_and_rewrites_nothing_when_declined` |
| gitOps › Undo Commit › refuses history rewriting on a protected branch with an upstream                        | `vcs_ops::rebase_plan::tests::refuses_history_rewriting_on_a_protected_branch_with_an_upstream`, `workbench::log_actions::tests::refuses_every_rewrite_of_pushed_commits_of_a_protected_branch`                                                                                                                         |
| branchFilterPopup › lists HEAD and the favorites, with Local and Remote collapsed until opened; no remote HEAD | `vcs::log_filters::tests::lists_head_and_the_favorites_with_local_and_remote_collapsed_until_opened_no_remote_head`                                                                                                                                                                                                     |
| › shows the user's favorite branches, local and remote, under Favorites by name                                | `vcs::log_filters::tests::shows_the_users_favorite_branches_local_and_remote_under_favorites_by_name`                                                                                                                                                                                                                   |
| AgentsView › keeps placement out of session menus                                                              | `agents::task_actions::tests::keeps_placement_out_of_session_menus`, `agents::task_list_view::tests::proofs11_task_list::keeps_placement_out_of_session_menus`                                                                                                                                                          |
| agents.test.ts › picks the main settings and cycles the mode                                                   | `agents::composer::tests::picks_the_main_settings_and_cycles_the_mode`                                                                                                                                                                                                                                                  |
| deploymentSettings › normalizes unsupported types to SFTP                                                      | `platform_more::remote_settings::tests::normalizes_unsupported_types_to_sftp`                                                                                                                                                                                                                                           |

ConfigPickers › "shows the effort as its own picker beside the model, and the mode with Allow All" gained no test.

## 4. Files edited (no new files)

All under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`:

- `run_testing/services.rs`, `workbench/services_window.rs`, `run/store.rs`
- `run/config_chooser.rs`, `run/chooser_view.rs`, `run/chooser_view/tests/proofs8_chooser.rs`, `workbench/tests/proofs9_run_chooser.rs`
- `vcs_ops/ignore.rs`, `workbench/vcs_operations.rs`, `vcs_ops/git_log.rs`, `vcs_ops/rebase_plan.rs`, `workbench/log_actions.rs`
- `vcs/log_filters.rs`, `workbench/log_window.rs`
- `agents/task_actions.rs`, `agents/task_actions/tests.rs`, `agents/task_list_view.rs`, `agents/task_list_view/tests/proofs11_task_list.rs`
- `agents/composer.rs`, `agents/config_bar.rs`, `agents/session_view.rs`, `agents/session/tests.rs`
- `platform_more/remote_settings.rs`, `settings_view/pages/build_deployment.rs`

Only these files were formatted, through `$S/fmt.sh`.

## 5. Stale or false in the notes

- The report says the native chooser "has no favorites or collapsed groups". That was true; the branch filter now has both.
- The report treats the Services default as Spring Boot through `DEFAULT_TYPES`. That constant is gone now.
- Before this pass, the deployment test's title ("normalizes … to SFTP") contradicted what it asserted (refusal).
