# Triage of the 98 rows added by D178

Read at HEAD 5135a131a (native files through `git show HEAD:` / `git grep … HEAD`, webview files from the working tree). Nothing was built or run. Every test cited below is in `rust_tests.txt` and its body was read; claims about native code carry `file:line` at HEAD, and the few that are not read are marked "hypothesis" with the command that settles them. Unit and view tests mentioned in the prose only by module (for example "tested on the store") were seen by name in the test list, not read, and no proof cites them.

Counts: 42 proposed done, 33 proposed partial, 23 left empty (nothing native to cite). 14 of the partial rows close with tests only. Every row was checked; the "not checked" list is empty.

## Summary table

### Context keys (49)

| Row                                                            | Proposed    | What is missing                                                             | Slice that closes it                           |
| -------------------------------------------------------------- | ----------- | --------------------------------------------------------------------------- | ---------------------------------------------- |
| context:(dynamic) src/features/plugins/points.tsx:210          | done        |                                                                             |                                                |
| context:(dynamic) src/features/plugins/runtimeUi.tsx:222       | done        |                                                                             |                                                |
| context:(dynamic) src/features/plugins/uiPoints.tsx:190        | done        |                                                                             |                                                |
| context:(dynamic) src/features/plugins/workspaceContains.ts:64 | done        |                                                                             |                                                |
| context:(dynamic) src/features/settings/window/bridge.ts:48    | partial     | no window test of a Keymap conflict decided by a registered action's `when` | tests only                                     |
| context:(dynamic) src/workbench/toolwindows/store.ts:455       | partial     | the keys are not read while a side is maximized                             | tests only                                     |
| context:build.running                                          | done        |                                                                             |                                                |
| context:databaseViewFocus                                      | done        |                                                                             |                                                |
| context:distractionFree                                        | done        |                                                                             |                                                |
| context:github.isReviewModeOn                                  | leave empty | never set natively; review mode is not wired                                | feature (review mode)                          |
| context:github.showCurrentBranchPullRequest                    | leave empty | never set; Show Pull Request in the Tool Window has no handler              | feature (review mode)                          |
| context:github.submitReview                                    | leave empty | never set; Submit Review… has no handler                                    | feature (review mode / details' Submit Review) |
| context:github.toggleReviewMode                                | leave empty | never set; Review Mode has no handler                                       | feature (review mode)                          |
| context:github.toggleViewed                                    | partial     | not read off; set under another condition                                   | tests only                                     |
| context:github.updateReviewBranch                              | leave empty | never set; Update to Enable Review Mode… has no handler                     | feature (review mode)                          |
| context:inlineCompletionVisible                                | leave empty | inline completion is a model nothing starts; the key is never set           | feature                                        |
| context:merge.<29 commands>                                    | done        |                                                                             |                                                |
| context:merge.annotate                                         | partial     | always off natively; no blame in the side panes                             | feature                                        |
| context:projectViewFocus                                       | done        |                                                                             |                                                |
| context:servicesViewFocus                                      | partial     | no test reads it or presses Add Service's key                               | tests only                                     |
| context:testTreeFocus                                          | leave empty | the Run window's test tree is not reached                                   | feature                                        |

### Extension point registrations (36)

| Row                                                                                 | Proposed    | What is missing                                                                | Slice                                                  |
| ----------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------ | ------------------------------------------------------ |
| point.registerCommitCheck:vcs.analysis                                              | partial     | no test of a file no server reported on, nor of the warnings wording           | tests only                                             |
| point.registerCommitCheck:vcs.detachedHead                                          | done        |                                                                                |                                                        |
| point.registerCommitCheck:vcs.todo                                                  | partial     | the hits are plain text, not links to the line                                 | a view                                                 |
| point.registerCommitCheck:vcs.crlf                                                  | partial     | reads normalised text, so CRLF on disk is never told                           | a call                                                 |
| point.registerCommitCheck:vcs.rebase                                                | partial     | no window test                                                                 | tests only                                             |
| point.registerCommitCheck:vcs.fileNames                                             | partial     | no window test                                                                 | tests only                                             |
| point.registerCommitCheck:vcs.largeFiles                                            | partial     | no window test                                                                 | tests only                                             |
| point.registerCellGrid:(at) src/features/database/Grid.tsx:432                      | partial     | the five db.* cell actions and dbCellGridFocus do not exist                    | a call                                                 |
| point.registerClientCommand:java.action.applyRefactoringCommand                     | leave empty | no native handler for the JDT client command                                   | feature (hypothesis: a call if the move dialogs exist) |
| point.registerContentTabSender:(at) src/features/plugins/views/WebView.tsx:311      | leave empty | no plugin content tabs                                                         | feature                                                |
| point.registerDiffViewerProvider:github.review                                      | partial     | the review diff model is reached by nothing                                    | feature                                                |
| point.registerDiffViewerProvider:vcs.annotateRevision                               | leave empty | no Annotate Previous Revision / at Revision, no annotated left pane            | feature                                                |
| point.registerDropTarget:(at) src/editor/core/view/EditorArea.tsx:292               | leave empty | no drag of tabs or files into a pane                                           | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/editor/core/view/EditorArea.tsx:97                | leave empty | same                                                                           | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/agents/AgentsView.tsx:112                | leave empty | no task drag in the Agents list                                                | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/agents/AgentsView.tsx:241                | partial     | view handles a dropped tab; nothing can drag one                               | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/agents/Composer.tsx:856                  | leave empty | no file drop on the composer                                                   | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/project-tree/ProjectView.tsx:755         | leave empty | no remote-file drop into a folder                                              | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/terminal/drop.ts:32                      | leave empty | Project view files cannot be dragged                                           | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/terminal/TerminalView.tsx:198            | partial     | store moves exist; no terminal tab can be dragged                              | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/features/vcs/CommitView.tsx:900                   | done        |                                                                                |                                                        |
| point.registerDropTarget:(at) src/ui/Tabs.tsx:238                                   | leave empty | no tab-strip reordering by drag                                                | feature (drag and drop)                                |
| point.registerDropTarget:(at) src/ui/treeReorder.ts:79                              | leave empty | no tree reordering by drag (Services)                                          | feature (drag and drop)                                |
| point.registerExternalIcons:(at) src/features/plugins/iconThemes.ts:172             | partial     | icon themes are a model nothing loads                                          | feature                                                |
| point.registerExternalIcons:(at) src/features/plugins/pluginIcon.ts:44              | partial     | only the Plugins page uses it, in a page test; commands do not fall back to it | a call + tests                                         |
| point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:339              | leave empty | plugin file icons are not read                                                 | a call + a view                                        |
| point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:424              | done        |                                                                                |                                                        |
| point.registerLanguageSemantics:go                                                  | partial     | semantic tokens are never requested                                            | feature (semantic tokens)                              |
| point.registerLanguageSemantics:kotlin                                              | partial     | same, plus annotation names                                                    | feature (semantic tokens)                              |
| point.registerLanguageSemantics:php                                                 | partial     | construct docs never reached                                                   | a call                                                 |
| point.registerLanguageSemantics:python                                              | partial     | same as go, plus symbol kinds, syntax ranges, intentions                       | feature (semantic tokens)                              |
| point.registerRelatedProvider:(at) src/editor/navigation/index.ts:246               | leave empty | no Related Symbol…                                                             | feature                                                |
| point.registerRelatedProvider:(at) src/features/rails/index.ts:53                   | leave empty | same                                                                           | feature                                                |
| point.registerTableEditor:(at) src/features/database/ResultsView.tsx:487            | partial     | the nine db.* table commands are not registered                                | a call                                                 |
| point.registerTableEditor:(at) src/features/database/TableEditor.tsx:254            | partial     | same, plus Changes while a cell is edited                                      | a call                                                 |
| point.registerValueEditorActions:(at) src/features/database/JsonValueEditor.tsx:150 | leave empty | no JSON value editor                                                           | feature                                                |

### Settings pages (3)

| Row                                           | Proposed    | What is missing                                                  | Slice                            |
| --------------------------------------------- | ----------- | ---------------------------------------------------------------- | -------------------------------- |
| settings:editor.colorScheme.console-colors    | leave empty | the page is not in the native tree                               | a view (after the scheme editor) |
| settings:editor.colorScheme.language-defaults | partial     | scheme editor unavailable; Semantic highlighting read by nothing | feature                          |
| settings:vcs                                  | partial     | its links are not read against the webview's                     | tests only                       |

### Design tokens, icons and locales (8)

| Row                     | Proposed | What is missing                                                   | Slice                  |
| ----------------------- | -------- | ----------------------------------------------------------------- | ---------------------- |
| design:global.css       | partial  | focus-ring modality, selection, scrollbars, cursor locks; no test | feature (design sweep) |
| design:messages.en.json | done     |                                                                   |                        |
| design:messages.zh.json | done     |                                                                   |                        |
| design:messages.es.json | partial  | no window test in Spanish                                         | tests only             |
| design:messages.fr.json | partial  | no window test in French                                          | tests only             |
| design:messages.ko.json | partial  | no window test in Korean                                          | tests only             |
| design:messages.pl.json | partial  | no window test in Polish                                          | tests only             |
| design:messages.pt.json | partial  | no window test in Portuguese                                      | tests only             |

### Modules (2)

| Row                          | Proposed | What is missing                                                      | Slice          |
| ---------------------------- | -------- | -------------------------------------------------------------------- | -------------- |
| module:src/main.tsx          | partial  | no test of the start; the start-up installs are absent or other rows | a call + tests |
| module:src/features/index.ts | partial  | no per-feature failure isolation; no test                            | a call + tests |

## Context keys

How native keys work, read at HEAD: `when` clauses are evaluated against the window's context in app-gpui/src/platform/actions.rs:366-375 (an action whose `when` fails is refused, so a test's `run_action` succeeding proves the clause held, and `try_action` returning false proves it failed); the workbench publishes focus keys in app-gpui/src/workbench/mod.rs:2387-2417 from which panel's focus handle contains the focus. The action metadata, `when` and `keyWhen` included, comes from the generated app-gpui/assets/gen/actions.json, so a native key with the webview's name gates the same actions.

**context:(dynamic) src/features/plugins/points.tsx:210 (done).** Webview: a plugin command's `when` from its manifest becomes the `when` of its action (points.tsx:210); the user sees the command greyed out in Tools and menus, and not runnable from Find Action, while the clause fails. Native: plugin commands are registered with their `when` (app-gpui/src/plugins.rs:420-426). `workbench::plugin_contributions::tests::the_tools_menu_lists_plugin_entries_with_their_state_and_runs_the_chosen_one` installs a plugin whose Sync command has `when = workspaceContains:MODULE.bazel`, reads the Tools menu state: Sync greyed out while the key is false, enabled once MODULE.bazel is written; Insert Date chosen from the menu bar inserts a date in the editor.

**context:(dynamic) src/features/plugins/runtimeUi.tsx:222 (done).** Webview: plugin code sets `plugin.<id>.<key>` (JSON value; null clears) in the window's context (runtimeUi.tsx:212-224), used by the plugin's own `when` clauses. Native: app-gpui/src/plugin_models/runtime_ui.rs:161-168 builds the same names; the workbench fetches and applies them (app-gpui/src/workbench/plugin_runtime.rs:562, :609). `workbench::plugin_runtime::tests::proofs10_host_ui::a_context_key_the_plugin_sets_is_on_and_removing_it_takes_it_away` runs the plugin's command with `flag=true`, waits for `plugin.jobs.flag` to read Bool(true) in the window, runs it with `flag=` and waits for the key to be gone.

**context:(dynamic) src/features/plugins/uiPoints.tsx:190 (done).** Webview: entries a plugin adds to the commit message toolbar, the editor's tab strip and the main toolbar carry the menu entry's own `when` (uiPoints.tsx:185-197); a button shows only while it holds. Native: app-gpui/src/workbench/context_menus.rs:1136 `slot_buttons` draws all three slots (called from app-gpui/src/vcs/commit_box.rs:722, app-gpui/src/workbench/mod.rs:1736, app-gpui/src/workbench/title_bar.rs:749). `workbench::context_menus::tests::renders_toolbar_slots_with_command_icons_and_honors_when_clauses` uses a manifest whose `editor.title` entry for Sync has `when = workspaceContains:MODULE.bazel` (context_menus.rs:1474-1476): no button while the key is false, the Insert Date button runs its command, and Sync's button appears once the file is written.

**context:(dynamic) src/features/plugins/workspaceContains.ts:64 (done).** Webview: every `workspaceContains:<path>` key named in a `when` is set true when a project root holds the path, re-evaluated as files change. Native: app-gpui/src/plugin_models/workspace_contains.rs, driven from app-gpui/src/workbench/plugin_runtime.rs:122, :1361. Both tests above read the key false without the file and true after writing it, and show the Tools entry and the tab-strip button following it.

**context:(dynamic) src/features/settings/window/bridge.ts:48 (partial).** Webview: the Settings window is a separate webview, so the project window sends it every action with its `when` (bridge.ts:41-55, sent at :77); the Keymap page uses the `when` to decide whether two bindings conflict (src/features/settings/keymap/model.ts:148-162). Native: no bridge is needed, the dialog runs in the project window and the Keymap page reads the registry's actions with their `when` (app-gpui/src/settings_view/keymap.rs:52-63, `when` copied at app-gpui/src/settings_model/keymap_model.rs:68-73). Missing: no test opens the Keymap page from a window and shows a conflict that exists or not according to the `when` of registered actions. `settings_view::keymap::tests::records_a_shortcut_shows_conflicts_and_applies_it_to_the_keymap_on_ok` shows the conflict list on the page view alone, with two actions the test builds (both `editorFocus`, keymap.rs:1788-1793); `workbench::tests::settings_keymap_assigns_a_recorded_shortcut_that_the_keymap_then_resolves` opens the page from a window and records a shortcut but leaves any conflict as it is and does not read it. Tests only.

**context:(dynamic) src/workbench/toolwindows/store.ts:455 (partial).** Webview: `toolWindow.<id>.visible` for every tool window, off for windows of other sides while one side is maximized (store.ts:448-457); `when` clauses of tool-window actions read it. Native: app-gpui/src/workbench/tool_windows.rs:632-642 (maximized side honoured), published at app-gpui/src/workbench/mod.rs:2417. Tests read: `workbench::tests::tool_window_actions_show_focus_and_hide_the_project_view` (on at open, off after ⌘1), `workbench::tests::proofs20_shell::registers_toolwindow_id_and_generic_actions` (each declared window's key on after its `toolwindow.<id>` action), `workbench::tests::proofs_tool_windows::hide_active_tool_window_hides_the_focused_project_view` (off after Hide Active Tool Window). Missing: no test reads the keys with a side maximized. Tests only.

**context:build.running (done).** Webview: set on when a build starts and off when it ends or is stopped (src/features/build/store.ts:154, :183, :286, :339); the build actions need `!build.running` (src/features/build/index.ts:19) and Stop Build needs it (:135). Native: app-gpui/src/workbench/build_window.rs:229, the same `when` clauses from the manifest. `workbench::tests::building_a_project_without_a_build_system_ends_in_a_failed_build` reads (false, none) before, (true, none) right after Build Project, (false, failed) after; Build Module, Rebuild Project and Build Artifacts each end with it off. `workbench::tests::a_cargo_build_lists_its_error_succeeds_once_fixed_and_ends_cancelled_when_stopped` reads it on while a build script sleeps and runs Stop Build (its `when` is the key) to end it cancelled.

**context:databaseViewFocus (done).** Webview: set while the Database tree has the keyboard (src/features/database/DatabaseView.tsx:506); the tree's actions take their shortcuts only there (`keyWhen`, src/features/database/treeActions.ts:228). Native: app-gpui/src/workbench/mod.rs:2410-2412, `key_when` at app-gpui/src/workbench/database.rs:767. `workbench::tests::proofs5_database::refresh_in_the_tree_lists_a_table_created_since_it_was_read`: off with the editor's keyboard, where ⌘R opens Replace and the tree is not read again; on once the tree is focused, where ⌘R lists the new table. `workbench::tests::edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one` reads it on again when the tree takes the keyboard from the table editor.

**context:distractionFree (done).** Webview: published by the mode (src/workbench/distractionFree.ts:89, :130, :143). Native: app-gpui/src/workbench/app_actions.rs:312, :651. `workbench::app_actions::tests::hides_tool_windows_and_restores_the_previous_layout_on_exit`: after Enter Distraction Free Mode the key is on, the title reads Exit…, status bar and tool window bars are hidden, the title bar is compact, the hint shows and the mode is stored; the action again turns the key off and brings the Project view back. Seen on the way, hypothesis for the menu row and not this one: the native main menu formats its labels with `distractionFree` always "off" (app-gpui/src/workbench/menu.rs:419), where the webview passes the mode (src/features/app/menuBridge.ts:103); settle with `git show HEAD:app-gpui/src/workbench/menu.rs | sed -n 410,440p` and a test that reads View › Appearance's label in the mode.

**context:github.isReviewModeOn, github.showCurrentBranchPullRequest, github.toggleReviewMode, github.updateReviewBranch (leave empty).** Webview: review mode provides its handlers at activation (src/features/github/reviewMode/index.ts:128-134), which sets the four keys on for the whole session; they enable Show Pull Request in the Tool Window, Update to Enable Review Mode… and the Review Mode toggle (src/features/github/index.ts:228, :237, :245), offered in the branch popup's pull request row. Native: nothing sets any of them (`git grep` of the names in app-gpui/src at HEAD finds only github.toggleViewed), and no handler is registered for those three actions (the only workbench mention is in an undeclared pending test); the branch widget model that names them (app-gpui/src/github/review_mode.rs:675-692) is not reached from the workbench. The user would see the three actions disabled. Slice: review mode wiring, a feature (D160 already lists review mode as unreached).

**context:github.submitReview (leave empty).** Webview: on while a pull request's details panel is mounted (src/features/github/details/DetailsPanel.tsx:44); enables Submit Review… (index.ts:211) and the review toolbar's Submit (src/features/github/diff/ReviewToolbar.tsx:20). Native: the key is never set and no handler is registered for github.pullRequest.submitReview; the details view decides whether to show the Submit Review button (app-gpui/src/github/details_view.rs:243) but that is not this key. Slice: feature.

**context:github.toggleViewed (partial).** Webview: on while a pull request's changes tree is mounted (src/features/github/details/ChangesTree.tsx:100); with `githubReviewDiff` it enables Toggle viewed/not viewed state (index.ts:220). Native: on while the Pull Requests window is visible with a #N tab active (app-gpui/src/workbench/github_part2.rs:497-506). `workbench::github::part2::tests::toggle_viewed_marks_the_selected_file_viewed_then_not_viewed` runs the action through the registry twice (so the key was on) and reads the gh calls markFileAsViewed then unmarkFileAsViewed and the row's read state. Missing: no test reads the key off (list tab, New Pull Request tab, window hidden). Tests only.

**context:inlineCompletionVisible (leave empty).** Webview: on while an inline suggestion is shown in the focused editor (src/editor/lsp/inlineCompletion.ts:188, :232); Tab, ⌥→ word accept, Escape, ⌥] / ⌥[ cycle are bound only then (:287-301; src/platform/keymaps/intellij-macos.ts:80-83). Native: the actions come from the manifest with that `when`, the key is never set, and the inline completion model (app-gpui/src/editor_lsp/inline_completion.rs) is reached by nothing (`git grep -n "InlineSession\|inline_completion::" HEAD -- app-gpui/src`: no hit outside the file). With the key never on, Tab and Escape never resolve to these actions natively. Slice: feature.

**context:merge.<command>, the family (29 done, 1 partial).** Webview: the focused merge editor sets `merge.<command>` to its `enabled(command)` for each of 30 commands (src/features/vcs/mergeActions.ts:48-52, rules at src/features/vcs/MergeEditor.tsx:405-439), and each action `merge.<command>` has `when: mergeEditorFocus && merge.<command>` (mergeActions.ts:69). The user sees toolbar buttons, menu rows and Find Action entries enabled or not; there are no default key bindings. Native: same 30 commands and the same `when` (app-gpui/src/vcs_more/merge_actions.rs:46-148), published from app-gpui/src/vcs/merge_view.rs:806-838 by app-gpui/src/workbench/conflicts.rs:714-733, which keeps them while a menu the editor opened is in front. Common to every key: `workbench::tests::proofs_merge_focus::keeps_merge_commands_registered_and_disabled_outside_a_merge_editor` counts 30 registered merge actions, reads every key off and every action refused before a merge editor opens and while the file's editor has the keyboard, reads twelve of them on with the merge editor's keyboard, and runs Apply All once it is back. Each key's own on and off reads and effect:

- merge.applyLeft, merge.applyRight (done): `workbench::tests::proofs_merge_apply::applies_those_of_the_chosen_side_only`: both on with changes on each side; Left takes Yours' change, reads off and is refused; Right takes Theirs', reads off.
- merge.applyAll (done): `workbench::tests::the_merge_actions_move_between_the_changes_and_apply_the_non_conflicting_ones` (on, applied, off once only the conflict is left), plus the focus test.
- merge.resolveSimple (done): `workbench::tests::proofs_merge_apply::resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more` (on, applied, off and refused for a same-word conflict), `workbench::tests::proofs_merge_apply::resolve_simple_conflicts_then_apply_writes_both_sides_words_and_stages_the_file`.
- merge.revert (done): the same resolveSimple test (off and refused with nothing settled, on after, back to base and off again).
- merge.acceptYours (done): on in the focus test; `workbench::tests::the_merge_editor_opened_from_the_banner_runs_its_actions_and_the_rebase_is_continued_from_the_notice` runs it, the editor closes and "All conflicts resolved" offers Continue Rebase.
- merge.acceptTheirs (done): `workbench::tests::proofs_merge_apply::accept_theirs_asks_once_the_result_was_changed_and_writes_their_version` (on, asks, No keeps, Discard writes Theirs and stages; refused once closed).
- merge.apply (done): `workbench::tests::proofs_merge_apply::apply_merge_result_asks_about_unresolved_changes_and_writes_the_result` (on, asks about unresolved changes, writes and stages; off and refused after the editor closes).
- merge.cancel, merge.compare, merge.settings (done): on in the focus test; the banner test runs Compare (the menu lists the comparisons, a row shows the diff window), Merge Settings (its menu opens and Escape gives the keys back) and Cancel Merge (asks; No keeps the editor, OK closes it with the file still unmerged).
- merge.reload (done): `workbench::tests::proofs_merge_compare::reload_from_disk_reads_the_versions_again`.
- merge.autoApply (done): `workbench::tests::proofs_merge_compare::automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge` (checked in the menu, stored, applied, starts the next merge).
- merge.compareYoursResult, merge.compareResultTheirs, merge.compareYoursTheirs, merge.compareBaseYours, merge.compareBaseResult, merge.compareBaseTheirs (done): `workbench::tests::proofs_merge_compare::each_comparison_shows_its_two_versions_in_the_diff_window` reads each on and shows its two exact sides; off outside by the focus test.
- merge.accept (done): `workbench::tests::proofs_merge_change::settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it` (off in the result and on an unchanged line, on and applied on a changed line, off after), `workbench::tests::proofs_merge_change::takes_the_text_and_leaves_a_conflict_open_for_the_other_side`.
- merge.resolveUsingYours, merge.resolveUsingTheirs (done): `workbench::tests::proofs_merge_change::settles_the_change_at_once_when_resolving_using_a_side`.
- merge.ignore (done): the settles_one_side test above and `workbench::tests::proofs_merge_change::merges_a_conflict_whose_sides_changed_different_words`.
- merge.resolveAutomatically (done): the merges_a_conflict test (on, merged, off and refused, off on a settled conflict).
- merge.cut, merge.copy, merge.paste, merge.selectAll (done): `workbench::tests::proofs_merge_edit::select_all_copy_cut_and_paste_edit_the_result` (copy and cut off with no selection, on after Select All, effects on the clipboard and the result), `workbench::tests::proofs_merge_edit::a_side_pane_copies_its_version_but_is_not_cut_or_pasted_into` (paste and cut off in a read-only side).
- merge.change (done): `workbench::tests::proofs_merge_change::resolve_merge_change_does_what_its_argument_says_to_the_change` (on; run with accept, ignore and auto arguments it does each, and nothing on a settled change).
- merge.annotate (partial): native `enabled` returns false for Annotate (app-gpui/src/vcs/merge_view.rs:832) and running it does nothing (:902), where the webview enables it with the caret in a side pane and toggles Git blame annotations there (MergeEditor.tsx:429-430, :490-500). `workbench::tests::the_merge_actions_move_between_the_changes_and_apply_the_non_conflicting_ones` reads it off. The test that reads it on in the side panes, app-gpui/src/workbench/tests/proofs_merge_annotate_pending.rs, is not declared. Slice: feature (blame in the merge editor's side panes).

**context:projectViewFocus (done).** Webview: on while the Project tree has the keyboard (src/features/project-tree/ProjectView.tsx:868); the Project view's keys are bound under it (src/platform/keymaps/intellij-macos.ts:205) and actions read it to take the selection (src/features/project-tree/actions.ts:87). Native: app-gpui/src/workbench/mod.rs:2398; actions read it at app-gpui/src/workbench/project_view_actions.rs:149, :189, app-gpui/src/editor/actions.rs:26. `workbench::project_view_actions::tests::proofs20_tree::compare_with_hands_two_files_to_compare_files_and_one_to_compare_with` reads it on with the tree selected and Compare With… runs, then off once the editor has the keyboard, where Compare With… is refused. `workbench::project_view_actions::tests::proofs20_tree::undo_in_the_project_view_takes_back_its_last_move_and_rename` reads it on and Edit › Undo there takes back the view's move on disk.

**context:servicesViewFocus (partial).** Webview: on while the Services tree has the keyboard (src/features/services/ServicesTree.tsx:227); Add Service takes its key (⌘N, Alt+Insert: src/platform/keymaps/intellij-macos.ts:402, intellij-windows.ts:400) only there (`keyWhen`, src/features/services/index.ts:131). Native: on while the Services tool window's panel has the keyboard (app-gpui/src/workbench/mod.rs:2409), `keyWhen` from the manifest. Missing: no test reads it or presses Add Service's shortcut in and out of the tree. Read at HEAD: the Services view has one focus handle for the whole view (app-gpui/src/services/view.rs:107, :704), so the native key also holds in the view's details part, where the webview's holds in the tree only. Tests only, plus a one-line narrowing if the details part should not take Add Service's key.

**context:testTreeFocus (leave empty).** Webview: on while the Run window's test tree has the keyboard (src/features/run/TestTreeView.tsx:325); Expand All / Collapse All of the tests take their keys only there (src/features/run/index.ts:688, :697). Native: nothing sets it; the test tree view in app-gpui/src/run/test_tree_pending is not even compiled: no `mod test_tree_pending` is declared (`git grep -n "mod test_tree_pending" HEAD -- app-gpui/src`: no hit), and its tests are not in the test list. Slice: feature.

## Commit checks

Webview: seven built-in checks registered in order (src/features/vcs/commitChecks.tsx:261-448, installed at :470); each one that finds something stops the commit and shows a problem in the commit box, with Commit Anyway going ahead. Native: no registry; app-gpui/src/workbench/shelf.rs:2024-2171 `run_commit_checks` runs the same checks in the same order, called from app-gpui/src/workbench/commit.rs:286, with the same texts.

**point.registerCommitCheck:vcs.analysis (partial).** Webview: files no server has analysed are opened so their servers analyse them, the check waits up to two seconds, then counts errors and warnings (commitChecks.tsx:324-364). Native: analyze_code (shelf.rs:1937-2019, called at :2126-2131) opens the unreported files, polls until they are analysed or the wait ends, closes them and counts. `workbench::shelf::tests::reported_errors_stop_the_commit_and_review_shows_the_problems`: an error published for the file stops the commit with "Code analysis: 1 error found in 1 file Review", Review shows the Problems tool window, nothing is committed. `workbench::shelf::tests::a_file_reported_clean_is_committed_without_waiting_for_analysis`: a file reported clean commits at once. Missing: no test commits a file no server has reported on (the open, wait and close path), nor reads the warnings and errors-and-warnings wording. Tests only.

**point.registerCommitCheck:vcs.detachedHead (done).** Native shelf.rs:2039-2059. `workbench::shelf::tests::a_detached_head_is_told_crlf_in_project_text_is_not_and_the_language_server_checks_are_disabled`: in a detached HEAD the commit stops with one problem, "Commit in detached HEAD: HEAD is detached…", nothing is committed and no "committed" notification appears.

**point.registerCommitCheck:vcs.todo (partial).** Native shelf.rs:2136-2153. `workbench::shelf::tests::a_todo_in_the_changed_lines_stops_the_commit_until_commit_anyway`: a TODO in a changed line stops the commit with "TODO: 1 TODO item found in the changed lines:" and "a.txt:2 // TODO finish"; the button becomes Commit Anyway, which commits. Missing: each hit is a link that opens the file at its line in the webview (commitChecks.tsx:387-399); natively the hits are text with no action (shelf.rs:2146-2152, `details: None`). Slice: a view.

**point.registerCommitCheck:vcs.crlf (partial).** Native shelf.rs:1918-1929 and :2154-2170. Missing, a wrong behaviour: the native check looks for CRLF in the project's text of the file, whose line breaks are normalised, so a file with CRLF on disk and core.autocrlf unset is never told; the webview reads the bytes on disk (commitChecks.tsx:420-447, containsCrlf at :457-470) and stops the commit. `workbench::shelf::tests::a_detached_head_is_told_crlf_in_project_text_is_not_and_the_language_server_checks_are_disabled` asserts the native behaviour (no Line separators problem for a CRLF file). Slice: a call (read the file's bytes) and that test's assertion turned around.

**point.registerCommitCheck:vcs.rebase (partial).** Native shelf.rs:2039-2059 (the rebase branch of the detached-HEAD block, same condition as commitChecks.tsx:263-272). Missing: no test commits during a rebase. Tests only.

**point.registerCommitCheck:vcs.fileNames (partial).** Native shelf.rs:2062-2088 with bad_file_name (unit tests in vcs_ops::commit_checks). Missing: no window test commits an added file with a name Windows rejects. Tests only.

**point.registerCommitCheck:vcs.largeFiles (partial).** Native shelf.rs:2093-2125 (sizes from the folder listing, read_for_checks at :1887-1907). Missing: no window test commits a file over the limit. Tests only.

## Other registries

**point.registerCellGrid:(at) src/features/database/Grid.tsx:432 (partial).** Webview: each result grid registers its cell actions (Grid.tsx:432, src/features/database/cellActions.ts:41); while one has the keyboard, dbCellGridFocus is on and Toggle Boolean Cell, Set NULL, Set DEFAULT, Local Filter and Edit Cell are actions with keys and Find Action entries (cellActions.ts:58-100). Native: the grid has its own menu with Set NULL and the local filter (app-gpui/src/database/grid_view.rs:608, :635), but none of the five ids is registered (`git grep` of the ids in app-gpui/src at HEAD: no hit) and dbCellGridFocus is never set. Missing: the five actions and the key; no window test. Slice: a call, plus two grid commands: the grid has no Toggle Boolean or Set DEFAULT (`git grep -n -i "set_default\|toggle_bool\|SetDefault\|ToggleBoolean" HEAD -- app-gpui/src/database/grid.rs app-gpui/src/database/grid_view.rs`: no hit).

**point.registerClientCommand:java.action.applyRefactoringCommand (leave empty).** Webview: when a JDT code action carries this client command, the editor runs Change Signature or the Move File / Instance Method / Static Member / Type dialogs (src/editor/refactor/javaMove.ts:18, :235-256, registered at src/editor/lsp/index.ts:127). Native: no handler (`git grep applyRefactoringCommand HEAD -- app-gpui/src`: no hit; only crates/ide-lsp mentions it). Hypothesis: native has refactor.changeSignature and refactor.move entries (app-gpui/src/editor_refactor/catalog.rs:66, :73), so the slice may be a call; settle with `git grep -n 'handle("refactor.move' HEAD -- app-gpui/src` (a handler with a Java move dialog makes it a call). Otherwise a feature.

**point.registerContentTabSender:(at) src/features/plugins/views/WebView.tsx:311 (leave empty).** Webview: a plugin web view can open a content tab in the editor and receive its replies (WebView.tsx:291-311, src/features/plugins/views/contentTab.ts:138). Native: no plugin content tabs (no hit for open-content-tab or a plugin content editor kind). Slice: feature.

**point.registerDiffViewerProvider:github.review (partial).** Webview: a pull request file's diff becomes a review diff, with review threads, the "+" on commentable lines, Add Review Comment and Mark File as Viewed (src/features/github/diff/index.ts:34). Native: the model is built (app-gpui/src/github/review_diff.rs, entry review_diff_context at :43) and the diff of a pull request's file carries its kind (app-gpui/src/workbench/github.rs:497-503), but nothing outside the module calls review_diff_context (`git grep review_diff:: HEAD -- app-gpui/src`: only submit_review_text and the kind constant). Missing: the model only; no review diff is ever shown. Slice: feature.

**point.registerDiffViewerProvider:vcs.annotateRevision (leave empty).** Webview: Annotate Previous Revision and Annotate at Revision… in the annotations' menu open a diff whose left pane is annotated (src/features/vcs/annotateRevisionProvider.ts:21-26, src/features/vcs/blame.ts:290-300). Native: the annotation menu offers Show Commit in Git Log, Copy Revision Number and Close Annotations only (app-gpui/src/workbench/annotate.rs:303-328). Slice: feature.

**Drop targets, the family.** Webview: src/ui/pointerDrag.ts:32 lets a component accept a pointer drag of a type. Native: GPUI drag and drop is used in one place, the changes tree (app-gpui/src/vcs/commit_view.rs:708-711); terminals take files dragged from the operating system (app-gpui/src/terminal/view.rs:580, view/clipboard.rs:163-175), which is not one of these rows; the drag model app-gpui/src/platform_more/pointer_drag.rs is used by nothing outside its file. Measured by `git grep` of on_drag, drag_over, on_drop, PointerDrag at HEAD.

- EditorArea.tsx:292 (leave empty): a pane takes a dragged editor tab (into a split zone), files dragged from the Project view, and plugin drop types. Native: editor tabs cannot be dragged; the tab model can move tabs (app-gpui/src/editor/tabs.rs:720, :852) for other actions.
- EditorArea.tsx:97 (leave empty): the empty editor area takes the same drops.
- AgentsView.tsx:112 (leave empty): a task dragged onto a row of the Agents list.
- AgentsView.tsx:241 (partial): an agent session's editor tab dropped on the Agents tool window docks the task. Native: app-gpui/src/agents/task_list_view.rs:184 tab_dropped handles a drop and highlights, tested on the view (agents::task_list_view::tests), but nothing outside tests calls it and no editor tab can be dragged. Missing: the drag itself and a window test. Slice: feature.
- Composer.tsx:856 (leave empty): files dropped on the composer become mentions.
- ProjectView.tsx:755 (leave empty): remote files dragged from the deployment browser onto a folder are downloaded into it.
- terminal/drop.ts:32 (leave empty): Project view files dropped on a terminal type their quoted paths. The native drop of operating system files on a terminal exists, but the Project view has no drag source.
- TerminalView.tsx:198 (partial): a terminal tab dropped on a group's terminal moves there or splits off. Native: app-gpui/src/terminal/store.rs:686 and :711 do the moves (tested on the store), called by nothing outside tests; the panel drags only the line between groups. Missing: the tab drag. Slice: feature.
- CommitView.tsx:900 (done): `workbench::shelf::tests::a_change_dragged_onto_another_list_moves_there` drags a.txt's row onto the Other list with the mouse, and the file belongs to Other and the tree shows it there. The hover highlight (commit_view.rs:711) is drawn but not asserted.
- Tabs.tsx:238 (leave empty): reordering tabs of a strip (editor, terminal) by drag.
- treeReorder.ts:79 (leave empty): reordering the Services tree by drag (src/features/services/ServicesTree.tsx:228).

**point.registerExternalIcons:(at) src/features/plugins/iconThemes.ts:172 (partial).** Webview: the selected plugin icon theme's SVGs become icons for files and folders. Native: app-gpui/src/plugin_models/icon_themes.rs and icon_theme_resolver.rs, with unit tests; the Plugins page adopts a theme as the setting (app-gpui/src/plugin_models/plugins_page.rs:690), but IconThemes::loaded and resolver are called by nothing, so no file shows a theme's icon. Missing: the model only. Slice: feature.

**point.registerExternalIcons:(at) src/features/plugins/pluginIcon.ts:44 (partial).** Webview: a plugin's own icon is registered once and used in the Plugins page and as the fallback icon of its commands and toolbar buttons (src/features/plugins/points.tsx:216-220, uiPoints.tsx:196). Native: app-gpui/src/plugin_models/plugin_icon.rs, registered by the Plugins page (plugins_page.rs:1338-1346). `plugin_models::plugins_page::tests::shows_a_plugins_own_icon_in_its_row_and_details` opens the page view alone and reads that the row and the details use an ext:pluginIcon/<id>/… name and that a plugin without one gets "plugin". Missing: the test does not start from a window; the workbench never uses PluginIcons (no hit in app-gpui/src/workbench), so commands and slot buttons without an icon of their own do not show the plugin's. Slice: a call plus a window test.

**point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:339 (leave empty).** Webview: a plugin's fileIcons contribution gives files it matches its light and dark SVGs. Native: nothing reads fileIcon assets (no hit for fileIcon/ in app-gpui/src). Slice: a call plus a view (file icon lookup in the tree and tabs).

**point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:424 (done).** Webview: a plugin view whose icon is an .svg of the plugin is drawn with it. Native: app-gpui/src/workbench/plugin_tool_windows.rs:247-252, :384-409. `workbench::plugin_tool_windows::tests::proofs9_view_icon::a_views_svg_icon_is_read_from_the_plugin_and_draws_its_tool_window`: the tool window's icon becomes ext:hello/view-greetings, its stripe button is drawn, and reading the icon starts no plugin code.

**Language semantics (four partial rows).** Webview: per-language hooks over the language server's semantic tokens (src/editor/lsp/semantic/languages.ts:38-69): Go marks readonly variables as constants and static ones as globals, and decides which tokens are locals for rainbow colours (go/index.ts); Kotlin colours annotation usages as annotations, operator declarations as functions, `modification` as mutable, labels as constants (kotlin/index.ts); PHP gives hover and completion docs for language constructs (php/index.ts); Python adds PyCharm's category colours, syntax ranges and fallbacks, member intentions and Implement Methods (python/index.ts). Native: the token-class hooks for Python, Kotlin and Go are written (app-gpui/src/editor_lsp/semantic.rs:80-140), but semantic_ranges has no caller outside tests and nothing in app-gpui/src requests semantic tokens (`git grep -i semantic_tokens HEAD -- app-gpui/src`: only the test module's declaration). PHP's constructs (app-gpui/src/editor_model/semantic_php.rs) and Python's members (app-gpui/src/editor_model/python_members.rs) are reached by nothing (`git grep -n "php_construct_at\|semantic_php::\|implement_abstract\|python_members::" HEAD -- app-gpui/src`: no hit outside the two files).

- point.registerLanguageSemantics:go (partial): Missing: no semantic tokens are requested or painted, so none of the hooks is seen. Slice: feature.
- point.registerLanguageSemantics:kotlin (partial): same, and annotation names (kotlin/index.ts:10-34, :38-46) are not built. Slice: feature.
- point.registerLanguageSemantics:php (partial): Missing: hover asks only the server, the construct docs are never used. Slice: a call.
- point.registerLanguageSemantics:python (partial): same as Go, and symbol kinds, syntax ranges, fallbacks and the member intentions are not reached. Slice: feature.

**point.registerRelatedProvider:(at) src/editor/navigation/index.ts:246 and :(at) src/features/rails/index.ts:53 (leave empty).** Webview: Navigate › Related Symbol… (index.ts:252-254) lists the header for a source and back, and Rails' controller, view, model and helper targets. Native: no navigate.relatedSymbol handler and no provider (`git grep relatedSymbol HEAD -- app-gpui/src`: no hit). Slice: feature.

**point.registerTableEditor:(at) src/features/database/TableEditor.tsx:254 (partial).** Webview: the table editor registers its handle (submit, revert, columns, and nine commands: reload, first/previous/next/last page, add row, export, delete and restore rows) for dbTableEditorFocus, dbTableEditorChanges and the db.* actions with `keyWhen: dbTableEditorFocus` (src/features/database/tableEditorFocus.ts:50-77, src/features/database/index.ts:66-76, :314-348). Native: submit and revert (app-gpui/src/workbench/database.rs:773, :783), Go to Column (app-gpui/src/workbench/database_extras.rs:154) and the two keys. `workbench::tests::edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one`: with the editor's keyboard Focus is on and Changes off, an edit turns Changes on, Submit writes the cell to the table and Changes goes off, Revert discards an edit, and the keys go off when the tree takes the keyboard. Missing: the nine db.* table commands are not registered (no hit for their ids), so their keys and Find Action entries do nothing; Changes is not on while a cell is being edited with nothing staged (tableEditorFocus.ts:52, :63). Slice: a call.

**point.registerTableEditor:(at) src/features/database/ResultsView.tsx:487 (partial).** Webview: an editable query result registers the same handle. Native: `workbench::tests::proofs43_database_results::submit_revert_and_go_to_column_reach_a_focused_console_result`: with the result grid's keyboard Focus is on, an edit turns Changes on, Submit writes, Revert drops, Go to Column selects the column picked by name, a cell being edited counts as the grid (Focus stays on) and Submit confirms it, and the keys go off with the editor's keyboard. Missing: the nine table commands, as above. Slice: a call.

**point.registerValueEditorActions:(at) src/features/database/JsonValueEditor.tsx:150 (leave empty).** Webview: the JSON value editor registers Reformat, Find and Open in Window, with dbJsonValueFocus, dbJsonValueEditable and dbJsonValueCanOpenWindow (src/features/database/valueEditorActions.ts:15-56). Native: no JSON value editor in app-gpui/src/database and no hit for the three action ids outside a keymap preset test. Slice: feature.

## Settings pages

**settings:editor.colorScheme.console-colors (leave empty).** Webview: Console Colors, the scheme row and the attribute editor for the console's attributes (src/features/settings/pages/editor/colorScheme.tsx:1775-1781). Native: not in the tree; the colour scheme pages are the landing page, General, Language Defaults, Debugger and the languages (app-gpui/src/settings_model/builtin_pages/editor.rs:540-574). Slice: a view, once the scheme editor exists.

**settings:editor.colorScheme.language-defaults (partial).** Webview: the scheme row and attribute editor for Language Defaults, and the Semantic highlighting and Rainbow brackets checkboxes with their hints (colorScheme.tsx:1739-1765). Native: the page with both checkboxes and hints (editor.rs:548-564). `workbench::tests::smoke_settings_pages::every_settings_page_opens_and_applies_a_change_made_with_the_mouse` opens it through File › Settings…, clicks its first checkbox and reads the value in effect after Apply. Missing: the scheme editor part draws the "not available" line, as on the other scheme pages; editor.semanticHighlighting is stored and read by nothing that draws (its only readers are crates/ide-core/src/settings.rs:925 and the list at app-gpui/src/platform_more/client_settings.rs:36; there are no semantic tokens natively). Slice: feature.

**settings:vcs (partial).** Webview: the Version Control folder page, with no controls, linking to its sub-pages (src/features/settings/pages/vcs.tsx:356-358). Native: a folder page (app-gpui/src/settings_model/builtin_pages/vcs.rs:153). The smoke test above opens it through File › Settings… on its id and accepts it as a page with nothing to change (a folder, or a page drawn otherwise); it does not read its links. Missing: no test reads which pages it links to against the webview's, or follows one. Tests only.

## Design rows

**design:global.css (partial).** Webview: rules the tokens do not hold (src/styles/global.css): ligatures off unless editor.ligatures (:8-15), the frame's background, text colour, font and no text selection (:35-46), the drag cursor kept during a resize (:49-54), selection locked to a scope (:56-67), button, select and radio resets (:75-121), focus rings on controls only and suppressed after a pointer click (:130-187), the selection colour (:189), syntax colours for read-only previews (:198-243), overlay scrollbars (:246-270) and IDE and editor antialiasing (:272-286). Native: no stylesheet; ligatures follow the setting in the code font (app-gpui/src/editor/view.rs:69-83), the antialiasing values reach the window model (app-gpui/src/settings_model/appearance_runtime.rs:33-54), colours come from the theme per view. Missing: no test, and the focus-ring modality (module:src/platform/pointerFocus.ts is an empty row) and the selection lock have no counterpart read at HEAD. Slice: feature (a sweep of views).

**design:messages.en.json, messages.zh.json (done).** Webview: the keyed catalogues read by t("…") (src/platform/locales/en.json, zh.json, 527 keys each), English as fallback. Native: embedded and parsed (app-gpui/src/platform/i18n.rs:43-75). `platform::i18n::tests::catalogs_are_complete_and_well_formed`: every message well formed and an English key, the same parameters as English, zh with every key. `workbench::tests::ui_language::the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action`: Chinese applied in Settings turns the menu bar's titles to Chinese (文件 for File) and English turns them back; the menu bar's labels are the keyed menu.* messages (app-gpui/src/workbench/menu.rs:418-421; menu.file at src/platform/locales/zh.json:192). The test also reads the Project window's title, Find Action and the dialog in Chinese; which catalogue those come from was not checked string by string, so they are not counted for these rows. `workbench::tests::ui_language::a_home_whose_settings_choose_chinese_opens_its_first_window_in_chinese`: a home whose stored settings choose Chinese opens its first window with 文件 in the menu bar and 项目 as the Project window's title.

**design:messages.es.json, fr, ko, pl, pt (partial).** Native: embedded, parsed and checked by the catalogue test above (well formed, English keys, English parameters). Missing: no test applies the language in a window and reads one of its keyed messages, or reads the English fallback of a key the catalogue lacks. Tests only.

## Modules

**module:src/main.tsx (partial).** Webview: start-up installs a repeated double click, a WebKit reordered-tap fix, a clipboard guard on Linux, title bar double click zoom, pointer focus modality, an IME key guard, error logging and page swipe tracking (main.tsx:38-53); then, by the window's kind, the Settings, comparison, diff or deployment window, or the workbench with its features and the keyboard dispatcher after them (:55-88), and the splash hides once the UI is up (:91-95). Native: app-gpui/src/main.rs opens one window for a folder, a file or nothing from the first argument after app-gpui/src/lib.rs:58-90 init; the auxiliary windows are dialogs of the native window. Missing: no test runs the start (argument to window); the installs are other rows or have no counterpart (pointer focus modality, page swipe, uncaught-error logging). Slice: a call (the argument's mapping as a function) plus tests.

**module:src/features/index.ts (partial).** Webview: activates the features in order, each in a try/catch, so a feature whose activation fails is logged and the others still start (index.ts:65-127), plus the feature sets of the secondary windows (:130-200). Native: app-gpui/src/lib.rs:58-90 registers everything in one sequence, and a failing plugin host start ends the start (`plugins::Plugins::init(cx)?` at lib.rs:86; hypothesis about when it fails, settle with `git show HEAD:app-gpui/src/plugins.rs | sed -n 158,230p`). Missing: per-feature failure isolation and any test of the start. Slice: a call plus tests.

## Tests-only candidates, by host module

- `workbench::tests` (the Settings keymap tests in app-gpui/src/workbench/mod.rs): context:(dynamic) src/features/settings/window/bridge.ts:48 (a binding that conflicts only where two registered actions' `when` overlap).
- `workbench::tests::proofs_tool_windows`: context:(dynamic) src/workbench/toolwindows/store.ts:455 (keys of the other sides off while one side is maximized).
- `workbench::github::part2::tests`: context:github.toggleViewed (off with the list tab, the New Pull Request tab and the window hidden).
- `workbench::tests::proofs3_run` (where services.addService is driven): context:servicesViewFocus (on in the tree, off in the editor; Add Service's key only in the tree).
- `workbench::shelf::tests`: point.registerCommitCheck:vcs.rebase, vcs.fileNames, vcs.largeFiles, vcs.analysis (a file no server reported on; warnings).
- `workbench::tests::ui_language`: design:messages.es.json, fr, ko, pl, pt (one test per language, or one looping over them, reading a menu title and a fallback key).
- `workbench::tests::smoke_settings_pages` or `workbench::tests::proofs19_settings_misc`: settings:vcs (the folder's links and following one).

14 rows in all.

## Not checked

None: all 98 rows were triaged.

## What the snippet sets

The 75 proposed rows (42 done, 33 partial). The 23 "leave empty" rows are listed in EMPTY and not written. Run from the repository root with the tests list as the argument: `python3 set_rows.py <path to rust_tests.txt>`; the list of the 98 keys is read from `gen/added_rows.txt` next to the tests list, or from a second argument. Checked here with the final write replaced by a no-op: every assertion holds and it would set 75 rows (42 done, 33 partial); parity.json was not written.

```python
import json
import re
import sys
from pathlib import Path

TESTS = set(Path(sys.argv[1]).read_text(encoding="utf-8").split())
ADDED_FILE = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(sys.argv[1]).parent / "gen" / "added_rows.txt"
ADDED = [line.split("\t")[0] for line in ADDED_FILE.read_text(encoding="utf-8").splitlines() if line.strip()]
PARITY = Path("docs/gpui-port/parity.json")


def t(name):
    return f"`{name}`"


W = "workbench::tests::"
FOCUS = t(W + "proofs_merge_focus::keeps_merge_commands_registered_and_disabled_outside_a_merge_editor")
BANNER = t(W + "the_merge_editor_opened_from_the_banner_runs_its_actions_and_the_rebase_is_continued_from_the_notice")
MOVE = t(W + "the_merge_actions_move_between_the_changes_and_apply_the_non_conflicting_ones")
SIDES = t(W + "proofs_merge_apply::applies_those_of_the_chosen_side_only")
SIMPLE = t(W + "proofs_merge_apply::resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more")
SIMPLE_APPLY = t(W + "proofs_merge_apply::resolve_simple_conflicts_then_apply_writes_both_sides_words_and_stages_the_file")
APPLY = t(W + "proofs_merge_apply::apply_merge_result_asks_about_unresolved_changes_and_writes_the_result")
THEIRS = t(W + "proofs_merge_apply::accept_theirs_asks_once_the_result_was_changed_and_writes_their_version")
COMPARE = t(W + "proofs_merge_compare::each_comparison_shows_its_two_versions_in_the_diff_window")
AUTO = t(W + "proofs_merge_compare::automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge")
RELOAD = t(W + "proofs_merge_compare::reload_from_disk_reads_the_versions_again")
EDIT = t(W + "proofs_merge_edit::select_all_copy_cut_and_paste_edit_the_result")
SIDE_EDIT = t(W + "proofs_merge_edit::a_side_pane_copies_its_version_but_is_not_cut_or_pasted_into")
ONE_SIDE = t(W + "proofs_merge_change::settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it")
OPEN_SIDE = t(W + "proofs_merge_change::takes_the_text_and_leaves_a_conflict_open_for_the_other_side")
USING = t(W + "proofs_merge_change::settles_the_change_at_once_when_resolving_using_a_side")
WORDS = t(W + "proofs_merge_change::merges_a_conflict_whose_sides_changed_different_words")
CHANGE = t(W + "proofs_merge_change::resolve_merge_change_does_what_its_argument_says_to_the_change")
OFF_OUTSIDE = "; off, and refused, before a merge editor opens and while another editor has the keyboard"

TOOLS_MENU = t("workbench::plugin_contributions::tests::the_tools_menu_lists_plugin_entries_with_their_state_and_runs_the_chosen_one")
SLOTS = t("workbench::context_menus::tests::renders_toolbar_slots_with_command_icons_and_honors_when_clauses")
DETACHED = t("workbench::shelf::tests::a_detached_head_is_told_crlf_in_project_text_is_not_and_the_language_server_checks_are_disabled")
SMOKE = t(W + "smoke_settings_pages::every_settings_page_opens_and_applies_a_change_made_with_the_mouse")
CATALOGS = t("platform::i18n::tests::catalogs_are_complete_and_well_formed")
CHINESE = t(W + "ui_language::the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action")
CHINESE_HOME = t(W + "ui_language::a_home_whose_settings_choose_chinese_opens_its_first_window_in_chinese")
EDIT_DATA = t(W + "edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one")
TABLE_COMMANDS = (
    "the nine db.* table commands (reload, first, previous, next and last page, add row, export, delete and "
    "restore rows: src/features/database/index.ts:66-76, :332-348) are not registered natively, so their keys "
    "(keyWhen dbTableEditorFocus) and Find Action entries do nothing"
)
SEMANTIC = (
    "the language's token-class hooks are written (app-gpui/src/editor_lsp/semantic.rs:80-140) but semantic_ranges "
    "has no caller outside tests and nothing in the app requests semantic tokens"
)


def done(*parts):
    return {"status": "done", "proof": "".join(parts)}


def partial(*parts):
    return {"status": "partial", "proof": "".join(parts)}


ROWS = {
    "context:(dynamic) src/features/plugins/points.tsx:210": done(
        TOOLS_MENU,
        "; a plugin command's own when is its action's: Tools lists Sync (when workspaceContains:MODULE.bazel) "
        "greyed out without the file and enabled once it is written, and an entry chosen in the menu bar runs "
        "the plugin's command",
    ),
    "context:(dynamic) src/features/plugins/runtimeUi.tsx:222": done(
        t("workbench::plugin_runtime::tests::proofs10_host_ui::a_context_key_the_plugin_sets_is_on_and_removing_it_takes_it_away"),
        "; a key the plugin's code sets is plugin.<id>.<key> in the window's context (read as true) and is gone "
        "once the code clears it",
    ),
    "context:(dynamic) src/features/plugins/uiPoints.tsx:190": done(
        SLOTS,
        "; a plugin's editor.title entry whose own when fails has no button until the key holds, then shows, and a "
        "slot button runs its command; the commit.message and mainToolbar slots are drawn by the same slot_buttons "
        "(app-gpui/src/workbench/context_menus.rs:1136)",
    ),
    "context:(dynamic) src/features/plugins/workspaceContains.ts:64": done(
        TOOLS_MENU, ", ", SLOTS,
        "; workspaceContains:MODULE.bazel is answered false in a project without the file and true once it is "
        "written, and the Tools entry and the tab strip button that need it follow",
    ),
    "context:(dynamic) src/features/settings/window/bridge.ts:48": partial(
        "the native Settings dialog runs in the project window, so the Keymap page reads the registered actions with "
        "their when directly (app-gpui/src/settings_view/keymap.rs:52-63, app-gpui/src/settings_model/keymap_model.rs:68-73) "
        "where the webview sends them to the settings window (src/features/settings/window/bridge.ts:41-55, used for "
        "conflicts at src/features/settings/keymap/model.ts:148-162). Missing: no test opens the Keymap page from a "
        "window and shows a conflict that exists or not according to the when of registered actions",
    ),
    "context:(dynamic) src/workbench/toolwindows/store.ts:455": partial(
        t(W + "tool_window_actions_show_focus_and_hide_the_project_view"), ", ",
        t(W + "proofs20_shell::registers_toolwindow_id_and_generic_actions"), ", ",
        t(W + "proofs_tool_windows::hide_active_tool_window_hides_the_focused_project_view"),
        "; toolWindow.project.visible reads on when the project opens and off after ⌘1 and after Hide Active Tool "
        "Window, and each declared tool window's key reads on once its toolwindow.<id> action shows it. Missing: with "
        "a side maximized the other sides' windows read off (src/workbench/toolwindows/store.ts:448-457; native "
        "app-gpui/src/workbench/tool_windows.rs:632-642): no test reads the keys while a side is maximized",
    ),
    "context:build.running": done(
        t(W + "building_a_project_without_a_build_system_ends_in_a_failed_build"), ", ",
        t(W + "a_cargo_build_lists_its_error_succeeds_once_fixed_and_ends_cancelled_when_stopped"),
        "; off before any build, on as soon as Build Project runs (before the backend answers), off once the build "
        "fails, succeeds or is cancelled, also after Build Module, Rebuild Project and Build Artifacts; Stop Build "
        "(when build.running) runs while a Cargo build script sleeps and ends the build cancelled",
    ),
    "context:databaseViewFocus": done(
        t(W + "proofs5_database::refresh_in_the_tree_lists_a_table_created_since_it_was_read"), ", ", EDIT_DATA,
        "; off with the editor's keyboard, where ⌘R is Replace in File and the tree is not read again; on once the "
        "Database tree has the keyboard, where ⌘R (keyWhen databaseViewFocus) lists a table created since; on again "
        "when the tree takes the keyboard from the table editor",
    ),
    "context:distractionFree": done(
        t("workbench::app_actions::tests::hides_tool_windows_and_restores_the_previous_layout_on_exit"),
        "; on after Enter Distraction Free Mode (status bar and tool window bars hidden, compact title bar, the hint "
        "shown, the mode stored, the action titled Exit), off after the action again with the Project view back",
    ),
    "context:github.toggleViewed": partial(
        t("workbench::github::part2::tests::toggle_viewed_marks_the_selected_file_viewed_then_not_viewed"),
        "; with a pull request's tab shown, Toggle viewed/not viewed state runs through the registry (its when needs "
        "the key; set at app-gpui/src/workbench/github_part2.rs:497-506) and marks the selected file viewed on GitHub, "
        "then not viewed. Missing: no test reads the key off (the list tab or the New Pull Request tab active, the "
        "window hidden); the webview sets it while a pull request's changes tree is mounted "
        "(src/features/github/details/ChangesTree.tsx:100)",
    ),
    "context:merge.applyLeft": done(SIDES, ", ", FOCUS, "; on with a non-conflicting change of Yours left; run, it takes that change alone, then reads off and is refused", OFF_OUTSIDE),
    "context:merge.applyRight": done(SIDES, ", ", FOCUS, "; on with a non-conflicting change of Theirs left; run, it takes that change alone and reads off", OFF_OUTSIDE),
    "context:merge.applyAll": done(MOVE, ", ", FOCUS, ", ", SIDES, "; on with non-conflicting changes left, applied as an action, off once only the conflict is left", OFF_OUTSIDE),
    "context:merge.resolveSimple": done(SIMPLE, ", ", SIMPLE_APPLY, ", ", FOCUS, "; on with simple conflicts, run it settles them with the non-conflicting changes, then off and refused for a conflict of the same word", OFF_OUTSIDE),
    "context:merge.revert": done(SIMPLE, ", ", USING, ", ", FOCUS, "; off and refused with nothing settled, on once changes are settled, run it sends them back to the base and reads off again", OFF_OUTSIDE),
    "context:merge.acceptYours": done(FOCUS, ", ", BANNER, "; on while the merge editor has the keyboard; run from a merge editor the conflict banner opened, it resolves the file with Yours, the editor closes and All conflicts resolved offers Continue Rebase", OFF_OUTSIDE),
    "context:merge.acceptTheirs": done(THEIRS, ", ", FOCUS, "; on; run after the result changed it asks, No keeps the editor, Discard writes Theirs and stages it; refused once the editor is closed", OFF_OUTSIDE),
    "context:merge.apply": done(APPLY, ", ", SIMPLE_APPLY, "; on; with an unresolved change it asks, No keeps the file unmerged, Mark Resolved writes and stages it; with everything settled it writes without asking; off and refused once the editor is closed"),
    "context:merge.cancel": done(FOCUS, ", ", BANNER, "; on while the merge editor has the keyboard; run after a change it asks Cancel Merge, No keeps the editor with the keyboard, OK closes it and the file stays unmerged", OFF_OUTSIDE),
    "context:merge.reload": done(RELOAD, ", ", FOCUS, "; on; run it reads the versions again", OFF_OUTSIDE),
    "context:merge.compare": done(FOCUS, ", ", BANNER, "; on; run it opens the menu of comparisons, and a row chosen there shows the diff window over the merge editor, which has the keyboard again after Escape", OFF_OUTSIDE),
    "context:merge.settings": done(FOCUS, ", ", AUTO, ", ", BANNER, "; on; run it opens the merge settings menu with Automatically Apply Non-Conflicting Changes, and Escape gives the keyboard back to the merge editor", OFF_OUTSIDE),
    "context:merge.autoApply": done(AUTO, ", ", FOCUS, "; on; run it is checked in the menu, stored, applies the non-conflicting changes at once and starts the next merge with them; run again it is stored off and unchecked", OFF_OUTSIDE),
    "context:merge.compareYoursResult": done(COMPARE, ", ", FOCUS, "; on, and run it shows Yours and the result in the diff window", OFF_OUTSIDE),
    "context:merge.compareResultTheirs": done(COMPARE, ", ", FOCUS, "; on, and run it shows the result and Theirs in the diff window", OFF_OUTSIDE),
    "context:merge.compareYoursTheirs": done(COMPARE, ", ", FOCUS, "; on, and run it shows Yours and Theirs in the diff window", OFF_OUTSIDE),
    "context:merge.compareBaseYours": done(COMPARE, ", ", FOCUS, "; on, and run it shows the base and Yours in the diff window", OFF_OUTSIDE),
    "context:merge.compareBaseResult": done(COMPARE, ", ", FOCUS, "; on, and run it shows the base and the result in the diff window", OFF_OUTSIDE),
    "context:merge.compareBaseTheirs": done(COMPARE, ", ", FOCUS, "; on, and run it shows the base and Theirs in the diff window", OFF_OUTSIDE),
    "context:merge.accept": done(ONE_SIDE, ", ", OPEN_SIDE, "; off and refused with the caret in the result or on an unchanged line of a side, on on a changed line, where it takes that side's text, then off once the change is settled"),
    "context:merge.resolveUsingYours": done(USING, ", ", THEIRS, "; on from Yours' side of the conflict, run it takes Yours and settles the conflict at once, then both resolutions read off and are refused there"),
    "context:merge.resolveUsingTheirs": done(USING, "; off and refused on a settled conflict, on from Theirs' side after Revert, where it settles the conflict with Theirs"),
    "context:merge.ignore": done(ONE_SIDE, ", ", WORDS, "; on with the caret on a side of a conflict, run it settles that side and keeps the result's text, then off and refused; in the result it settles the conflict with the base"),
    "context:merge.resolveAutomatically": done(WORDS, "; on with the caret on a conflict whose sides changed different words, run it merges both words, then off and refused, and off on a conflict already settled"),
    "context:merge.cut": done(EDIT, ", ", SIDE_EDIT, "; off and refused with nothing selected and in a read-only side, on after Select All in the result, where it cuts to the clipboard and reads off again"),
    "context:merge.copy": done(EDIT, ", ", SIDE_EDIT, "; off and refused with nothing selected, on after Select All in the result or a side, where it puts that version on the clipboard and leaves the result"),
    "context:merge.paste": done(EDIT, ", ", SIDE_EDIT, "; on in the result, where it replaces the selection with the clipboard and moves the caret after it; off and refused in a read-only side"),
    "context:merge.selectAll": done(EDIT, ", ", SIDE_EDIT, ", ", FOCUS, "; on, and run it selects the whole result or the whole side", OFF_OUTSIDE),
    "context:merge.change": done(CHANGE, ", ", FOCUS, "; on; run with a change and what to do (accept with Yours, ignore, automatic) it does that to the change, and nothing on a settled change", OFF_OUTSIDE),
    "context:merge.annotate": partial(
        MOVE,
        "; read off with the caret in the result. Missing: natively Annotate is always off (app-gpui/src/vcs/merge_view.rs:832) "
        "and does nothing (:902), where the webview turns it on with the caret in a side pane and toggles Git blame in "
        "that pane (src/features/vcs/MergeEditor.tsx:429-430, :490-500); the test that reads it on in the side panes "
        "(proofs_merge_annotate_pending.rs) is not declared",
    ),
    "context:projectViewFocus": done(
        t("workbench::project_view_actions::tests::proofs20_tree::compare_with_hands_two_files_to_compare_files_and_one_to_compare_with"), ", ",
        t("workbench::project_view_actions::tests::proofs20_tree::undo_in_the_project_view_takes_back_its_last_move_and_rename"),
        "; on with a row of the Project view selected, where Compare With… (when projectViewFocus) hands the files to "
        "the diff and Edit › Undo takes back the view's last move on disk; off once a file's editor has the keyboard, "
        "where Compare With… is refused",
    ),
    "context:servicesViewFocus": partial(
        "set while the Services tool window has the keyboard (app-gpui/src/workbench/mod.rs:2409), with Add Service's "
        "keyWhen from the manifest. Missing: no test reads it or presses Add Service's shortcut (⌘N, Alt+Insert: "
        "src/platform/keymaps/intellij-macos.ts:402, intellij-windows.ts:400) in and out of the Services tree; the "
        "webview sets it for the tree only (src/features/services/ServicesTree.tsx:227)",
    ),
    "point.registerCommitCheck:vcs.analysis": partial(
        t("workbench::shelf::tests::reported_errors_stop_the_commit_and_review_shows_the_problems"), ", ",
        t("workbench::shelf::tests::a_file_reported_clean_is_committed_without_waiting_for_analysis"),
        "; errors a language server reported for a committed file stop the commit as Code analysis: 1 error found "
        "in 1 file, whose Review shows the Problems tool window, and nothing is committed; a file reported clean is "
        "committed without waiting (app-gpui/src/workbench/shelf.rs:1937-2019, :2126-2131). Missing: no test commits a "
        "file no server has reported on, which the check opens, waits for and closes (src/features/vcs/commitChecks.tsx:329-344), "
        "nor reads the warnings and errors-and-warnings wording",
    ),
    "point.registerCommitCheck:vcs.detachedHead": done(
        DETACHED,
        "; a commit in a detached HEAD stops with Commit in detached HEAD and its text, nothing is committed and no "
        "committed notification shows (app-gpui/src/workbench/shelf.rs:2039-2059)",
    ),
    "point.registerCommitCheck:vcs.todo": partial(
        t("workbench::shelf::tests::a_todo_in_the_changed_lines_stops_the_commit_until_commit_anyway"),
        "; a TODO in a changed line stops the commit with the count and a.txt:2 // TODO finish, the button becomes "
        "Commit Anyway, which commits. Missing: each hit is a link that opens the file at its line in the webview "
        "(src/features/vcs/commitChecks.tsx:387-399); natively the hits are text with no action "
        "(app-gpui/src/workbench/shelf.rs:2146-2152)",
    ),
    "point.registerCommitCheck:vcs.crlf": partial(
        DETACHED,
        "; the test asserts that a file with CRLF line separators and core.autocrlf false is not told. Missing: the "
        "native check reads the project's text of the file, whose line breaks are normalised "
        "(app-gpui/src/workbench/shelf.rs:1918-1929), so CRLF on disk is never told; the webview reads the bytes on "
        "disk and stops the commit with Line separators (src/features/vcs/commitChecks.tsx:420-447, :457-470)",
    ),
    "point.registerCommitCheck:vcs.rebase": partial(
        "built in the commit's checks with the webview's condition and text (app-gpui/src/workbench/shelf.rs:2039-2059; "
        "src/features/vcs/commitChecks.tsx:263-272). Missing: no test commits during a rebase and reads Commit during rebase",
    ),
    "point.registerCommitCheck:vcs.fileNames": partial(
        "built with the webview's texts for reserved names and characters Windows rejects, on added, untracked and "
        "renamed paths only (app-gpui/src/workbench/shelf.rs:2062-2088; src/features/vcs/commitChecks.tsx:286-305). "
        "Missing: no window test commits such a file and reads File names",
    ),
    "point.registerCommitCheck:vcs.largeFiles": partial(
        "built with the webview's limit and texts (app-gpui/src/workbench/shelf.rs:2093-2125, sizes at :1887-1907; "
        "src/features/vcs/commitChecks.tsx:306-323). Missing: no window test commits a file over the limit and reads Large files",
    ),
    "point.registerCellGrid:(at) src/features/database/Grid.tsx:432": partial(
        "the native grid has its own menu with Set NULL and the local filter (app-gpui/src/database/grid_view.rs:608, "
        ":635). Missing: Toggle Boolean Cell, Set NULL, Set DEFAULT, Local Filter and Edit Cell are not registered "
        "actions and dbCellGridFocus is never set (src/features/database/cellActions.ts:41-100), so no key or Find "
        "Action reaches them; no window test",
    ),
    "point.registerDiffViewerProvider:github.review": partial(
        "the review diff is built as a model (app-gpui/src/github/review_diff.rs, review_diff_context at :43) and a "
        "pull request file's diff carries its kind (app-gpui/src/workbench/github.rs:497-503). Missing: nothing calls "
        "review_diff_context, so no diff shows review threads, the comment gutter, Add Review Comment or Mark File as "
        "Viewed (src/features/github/diff/index.ts:34)",
    ),
    "point.registerDropTarget:(at) src/features/agents/AgentsView.tsx:241": partial(
        "the Agents tool window's view handles a dropped session tab and docks it (app-gpui/src/agents/task_list_view.rs:184), "
        "tested on the view alone. Missing: nothing outside tests calls it and no editor tab can be dragged natively, "
        "so a session tab cannot be docked by dragging",
    ),
    "point.registerDropTarget:(at) src/features/terminal/TerminalView.tsx:198": partial(
        "the terminal store moves a tab within a group and splits one off a group's edge (app-gpui/src/terminal/store.rs:686, "
        ":711), tested on the store. Missing: nothing outside tests calls them and the panel drags only the line "
        "between groups, so a terminal tab cannot be dragged into a group or a split",
    ),
    "point.registerDropTarget:(at) src/features/vcs/CommitView.tsx:900": done(
        t("workbench::shelf::tests::a_change_dragged_onto_another_list_moves_there"),
        "; a change dragged with the mouse onto another change list's row moves to that list and the tree shows it "
        "under it (app-gpui/src/vcs/commit_view.rs:708-711)",
    ),
    "point.registerExternalIcons:(at) src/features/plugins/iconThemes.ts:172": partial(
        "plugin icon themes are a model with unit tests (app-gpui/src/plugin_models/icon_themes.rs, icon_theme_resolver.rs) "
        "and the Plugins page adopts one as the setting (app-gpui/src/plugin_models/plugins_page.rs:690). Missing: "
        "nothing loads a theme's SVGs or asks its resolver, so no file or folder shows a theme's icon",
    ),
    "point.registerExternalIcons:(at) src/features/plugins/pluginIcon.ts:44": partial(
        t("plugin_models::plugins_page::tests::shows_a_plugins_own_icon_in_its_row_and_details"),
        "; the Plugins page draws a plugin's own icon in its row and details, and the generic one without. Missing: "
        "the test opens the page view alone, not from a window; the workbench never uses the plugin icons, so commands "
        "and toolbar buttons without an icon of their own do not show the plugin's (src/features/plugins/points.tsx:216-220, uiPoints.tsx:196)",
    ),
    "point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:424": done(
        t("workbench::plugin_tool_windows::tests::proofs9_view_icon::a_views_svg_icon_is_read_from_the_plugin_and_draws_its_tool_window"),
        "; a view whose icon is an SVG of the plugin gets its tool window drawn with it (ext:hello/view-greetings), "
        "its stripe button is drawn, and reading the icon runs no plugin code",
    ),
    "point.registerLanguageSemantics:go": partial(
        SEMANTIC, " (Go's readonly, static and locals rules). Missing: no semantic tokens are requested or painted, so none of it is seen",
    ),
    "point.registerLanguageSemantics:kotlin": partial(
        SEMANTIC, " (Kotlin's operator, modification and label rules). Missing: no semantic tokens are requested or painted; annotation usages coloured as annotations (src/editor/lsp/semantic/kotlin/index.ts:10-46) are not built",
    ),
    "point.registerLanguageSemantics:php": partial(
        "the constructs' docs are a model with unit tests (editor_model semantic_php). Missing: hover and completion ask "
        "only the server, so empty, isset and the other constructs get no documentation (src/editor/lsp/semantic/php/index.ts:10-13)",
    ),
    "point.registerLanguageSemantics:python": partial(
        SEMANTIC, " (Python's static, method call and builtin classes). Missing: no semantic tokens are requested or painted; symbol kinds, syntax ranges, fallbacks, member intentions and Implement Methods (src/editor/lsp/semantic/python/index.ts:45-58) are not reached",
    ),
    "point.registerTableEditor:(at) src/features/database/ResultsView.tsx:487": partial(
        t(W + "proofs43_database_results::submit_revert_and_go_to_column_reach_a_focused_console_result"),
        "; with an editable console result's grid focused dbTableEditorFocus is on, an edit turns dbTableEditorChanges "
        "on, Submit writes it, Revert drops one, Go to Column selects the column picked by name, a cell being edited "
        "counts as the grid and Submit confirms it, and the keys go off with the editor's keyboard. Missing: ",
        TABLE_COMMANDS,
    ),
    "point.registerTableEditor:(at) src/features/database/TableEditor.tsx:254": partial(
        EDIT_DATA,
        "; with the table editor focused dbTableEditorFocus is on and dbTableEditorChanges off, an edit turns Changes "
        "on, Submit writes the cell and turns it off, Revert discards an edit, and the keys go off when the Database "
        "tree takes the keyboard. Missing: ", TABLE_COMMANDS,
        "; Changes is not on while a cell is edited with nothing staged (src/features/database/tableEditorFocus.ts:52, :63)",
    ),
    "settings:editor.colorScheme.language-defaults": partial(
        SMOKE,
        "; opened through File › Settings…, its first checkbox is clicked and in effect after Apply; both checkboxes "
        "and hints are drawn (app-gpui/src/settings_model/builtin_pages/editor.rs:548-564). Missing: the scheme row and "
        "attribute editor for Language Defaults (src/features/settings/pages/editor/colorScheme.tsx:1745) draw the not "
        "available line; editor.semanticHighlighting is stored and read by nothing that draws, as there are no semantic tokens natively",
    ),
    "settings:vcs": partial(
        SMOKE,
        "; the Version Control folder page (app-gpui/src/settings_model/builtin_pages/vcs.rs:153) opens through File › "
        "Settings… on its id and passes as a page with nothing to change. Missing: no test reads which pages it links to "
        "against the webview's (src/features/settings/pages/vcs.tsx:356-358) or follows a link",
    ),
    "design:global.css": partial(
        "no stylesheet natively: ligatures follow editor.ligatures in the code font (app-gpui/src/editor/view.rs:69-83), "
        "antialiasing reaches the window model (app-gpui/src/settings_model/appearance_runtime.rs:33-54), colours come "
        "from the theme per view. Missing: focus rings on controls only and dropped after a pointer click "
        "(src/styles/global.css:130-187), the selection colour (:189), the scrollbars (:246-270), the resize cursor and "
        "selection locks (:49-67) have no counterpart read at HEAD; no test",
    ),
    "design:messages.en.json": done(
        CATALOGS, ", ", CHINESE,
        "; the English keyed catalogue is embedded and well formed, and the menu bar's titles, which are its menu.* "
        "messages (app-gpui/src/workbench/menu.rs:418-421), read File, also after switching back from Chinese",
    ),
    "design:messages.zh.json": done(
        CATALOGS, ", ", CHINESE, ", ", CHINESE_HOME,
        "; the Chinese keyed catalogue has every English key with the English parameters; applied in Settings it turns "
        "the menu bar's titles, its menu.* messages (app-gpui/src/workbench/menu.rs:418-421), to Chinese (文件) without "
        "a restart, and a home whose settings choose Chinese opens its first window with 文件 in the menu bar",
    ),
    "module:src/main.tsx": partial(
        "app-gpui/src/main.rs opens one window for a folder, a file or nothing from its first argument after "
        "app-gpui/src/lib.rs:58-90 init; the webview's auxiliary windows (src/main.tsx:55-71) are dialogs of the native "
        "window. Missing: no test runs the start from its argument to its window; pointer focus modality, page swipe "
        "tracking and uncaught-error logging installed at start (src/main.tsx:38-53) have no counterpart read at HEAD",
    ),
    "module:src/features/index.ts": partial(
        "app-gpui/src/lib.rs:58-90 registers the features in one sequence. Missing: the webview starts each feature in "
        "a try/catch, so one whose activation fails is logged and the others start (src/features/index.ts:117-127); "
        "natively a failing plugin host start ends the start (lib.rs:86); no test of the start",
    ),
}

for lang, name in [("es", "Spanish"), ("fr", "French"), ("ko", "Korean"), ("pl", "Polish"), ("pt", "Portuguese")]:
    ROWS[f"design:messages.{lang}.json"] = partial(
        CATALOGS,
        f"; the {name} keyed catalogue is embedded and parsed (app-gpui/src/platform/i18n.rs:43-75), every message "
        "well formed, an English key and with the English parameters. Missing: no test applies the language in a "
        f"window and reads one of its keyed messages, or the English text of a key the {name} catalogue lacks",
    )

EMPTY = {
    "context:github.isReviewModeOn",
    "context:github.showCurrentBranchPullRequest",
    "context:github.submitReview",
    "context:github.toggleReviewMode",
    "context:github.updateReviewBranch",
    "context:inlineCompletionVisible",
    "context:testTreeFocus",
    "point.registerClientCommand:java.action.applyRefactoringCommand",
    "point.registerContentTabSender:(at) src/features/plugins/views/WebView.tsx:311",
    "point.registerDiffViewerProvider:vcs.annotateRevision",
    "point.registerDropTarget:(at) src/editor/core/view/EditorArea.tsx:292",
    "point.registerDropTarget:(at) src/editor/core/view/EditorArea.tsx:97",
    "point.registerDropTarget:(at) src/features/agents/AgentsView.tsx:112",
    "point.registerDropTarget:(at) src/features/agents/Composer.tsx:856",
    "point.registerDropTarget:(at) src/features/project-tree/ProjectView.tsx:755",
    "point.registerDropTarget:(at) src/features/terminal/drop.ts:32",
    "point.registerDropTarget:(at) src/ui/Tabs.tsx:238",
    "point.registerDropTarget:(at) src/ui/treeReorder.ts:79",
    "point.registerExternalIcons:(at) src/features/plugins/uiPoints.tsx:339",
    "point.registerRelatedProvider:(at) src/editor/navigation/index.ts:246",
    "point.registerRelatedProvider:(at) src/features/rails/index.ts:53",
    "point.registerValueEditorActions:(at) src/features/database/JsonValueEditor.tsx:150",
    "settings:editor.colorScheme.console-colors",
}

CITED = re.compile(r"`([a-z_]\w*(?:::\w+)+)`")  # docs/gpui-port/inventory.mjs:261

assert len(ADDED) == 98 and len(set(ADDED)) == 98, len(ADDED)
assert set(ROWS) <= set(ADDED), sorted(set(ROWS) - set(ADDED))
assert not set(ROWS) & EMPTY, sorted(set(ROWS) & EMPTY)
assert set(ROWS) | EMPTY == set(ADDED), sorted(set(ADDED) - set(ROWS) - EMPTY)
parity = json.loads(PARITY.read_text(encoding="utf-8"))
already = sorted(k for k in ROWS if k in parity)
assert not already, already
for key, row in ROWS.items():
    cited = CITED.findall(row["proof"])
    unknown = [name for name in cited if name not in TESTS]
    assert not unknown, (key, unknown)
    if row["status"] == "done":
        assert cited, key
        assert "Missing:" not in row["proof"], key
    else:
        assert row["status"] == "partial" and "Missing:" in row["proof"], key
parity.update(ROWS)
PARITY.write_text(json.dumps(parity, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
counts = {s: sum(1 for r in ROWS.values() if r["status"] == s) for s in ("done", "partial")}
print(f"set {len(ROWS)} rows: {counts}; left empty: {len(EMPTY)}")
```
