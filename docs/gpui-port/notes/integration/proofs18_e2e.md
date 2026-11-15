# proofs18_e2e — end-to-end flows of the e2e specs, one native test per `test(...)`

Module names are snake_case (`proofs18_e2e_git_ignore`, not `gitIgnore`): a camelCase module name trips `non_snake_case` under `-D warnings`.

## `mod` lines to add

In `app-gpui/src/workbench/mod.rs`, inside `mod tests`, in alphabetical place after the anchor `    mod proofs17_interrupt;` (keep the `#[cfg(unix)]` above that line where it is):

```rust
    mod proofs18_e2e_commit;
    mod proofs18_e2e_diff_navigation;
    mod proofs18_e2e_editor;
    mod proofs18_e2e_fixtures;
    mod proofs18_e2e_git_ignore;
    mod proofs18_e2e_keymap;
    mod proofs18_e2e_merge_editor;
    mod proofs18_e2e_new_branch;
    mod proofs18_e2e_open_project;
    mod proofs18_e2e_run;
    mod proofs18_e2e_search;
    mod proofs18_e2e_toolwindows;
```

In `app-gpui/src/workbench/plugin_runtime.rs`, inside `mod tests`, after the anchor `    mod proofs16_runtime;`:

```rust
    mod proofs18_e2e_plugin_notifications;
```

In `app-gpui/src/workbench/plugin_contributions.rs`, inside `mod tests`, after the anchor `    mod proofs11_plugin_debug;`:

```rust
    mod proofs18_e2e_plugin_settings;
```

In `app-gpui/src/editor/intelligence.rs`, inside `mod tests`, after the anchor `    mod proofs9_syntax_usages;`:

```rust
    mod proofs18_e2e_code_insight;
```

In `app-gpui/src/workbench/diff_tools.rs`, inside its inline `mod tests`, after the anchor `    use std::rc::Rc;` (the first `mod` of that module; it creates `diff_tools/tests/`):

```rust

    mod proofs18_e2e_compare_with;
```

In `app-gpui/src/workbench/diff_tab.rs`, inside its inline `mod tests`, after the anchor `    use serde_json::json;` (it creates `diff_tab/tests/`):

```rust

    mod proofs18_e2e_diff_navigation;
```

Not declared (pending, written to the spec): `workbench/tests/proofs18_e2e_{toolwindows,merge_editor,run,search,editor,diff_navigation}_pending.rs` and `editor/intelligence/tests/proofs18_e2e_code_insight_pending.rs`.

`proofs18_e2e_fixtures.rs` holds the helpers the workbench files share (`pub(super)`): clicks on run-time selectors, right/double clicks, the Project view's context menu walked by labels (`click_menu_path`, `click_menu_row`, `menu_level`), double Shift through `simulate_modifiers_change`, `primary(key)` (⌘ on macOS, Ctrl elsewhere), `open_from_tree`.

## Accessors the tests depend on (exact hunks)

1. `app-gpui/src/settings_view/dialog.rs`, in `impl SettingsDialogView` (next to `custom_control`), used by `proofs18_e2e_plugin_settings`:

```rust
    /// The view drawn for page `page` when it is not a built-in page (a plugin's page).
    pub fn page_view(&self, page: &str) -> Option<&AnyView> {
        self.page_views.get(page)
    }
```

The declared tests depend on that one hunk only. The pending files assume accessors that do not exist, named in each file's module comment and in the spec sections below: `EditorIntelligence::inplace_rename_hint`, `MergeView::focus_control`, `Workbench::merge_views`, `FindPopup::preview_editor`, `Workbench::code_breadcrumbs` / `status_breadcrumbs` / `navigation_bar`, `Workbench::diff_tab` / `diff_tab_path`, `RunSession::base_config`, `ide_run::BeforeLaunch::Build`, and debug selectors `tool-window-options:<id>`, `tool-window-title:<id>`, `terminal-tab:<id>`, `terminal-close:<id>`, `tab-modified:<id>`, `run-rerun-failed`, `diff-maximize` / `diff-restore`.

## Tests most likely to fail on first run

- `proofs18_e2e_toolwindows::cmd_shift_f12_hides_all_tool_windows_and_restores_them`: the second ⌘⇧F12 may not restore. The parity note says only F12 is proven to bring windows back.
- `proofs18_e2e_editor::each_file_keeps_its_own_find_bar_query_and_caret`: a new bar may start empty instead of from the last search.
- `proofs18_e2e_run::cmd_f2_stops_a_running_process`: the tooltip's exit code for a SIGTERM-ed process may not be 143.
- `proofs18_e2e_merge_editor::accepting_a_deleted_version_removes_the_file_before_staging`: the Conflicts dialog may not open the merge editor for a modify/delete conflict, or Accept Theirs may not delete.
- `proofs18_e2e_keymap::*`: they assume `Alt+Button1` conflicts with `debug.quickEvaluate` regardless of its `when`; if it does not, OK adds the click without the Warning, and the `asks_to_remove_conflicts` assertion fails. They also assume the Keymap view's row `keymap-row-navigate.findAction` is drawn after the search.
- `proofs18_e2e_open_project::…`: it assumes double Shift on the root view opens Search Everywhere on All with Close Project as the selected row. If the dispatcher's double-Shift window uses the executor clock, back-to-back modifier events still count.
- `proofs18_e2e_plugin_settings::…`: it depends on the `page_view` hunk and on the tree keys `settings-page-plugins` / `settings-page-plugins.settings-pages`. Reopening must keep the LLM page selected; `SettingsDialog::open(None, …)` keeps `focus` only if the workbench reuses the model.
- `proofs18_e2e_plugin_notifications::…`: `wait` advances the clock 20 ms per poll; the info balloon must not expire before the click (it is clicked as soon as it shows).
- `proofs18_e2e_code_insight::completion_lists_the_servers_items_and_inserts_the_chosen_one`: Enter's auto-indent and the `.`-triggered lookup before ⌃Space may change the row count the wait expects (2).
- `proofs18_e2e_search::search_everywhere_finds_and_runs_an_action`: "Language Servers" may first match a Settings item, not an action. Either opens the page; the assertion checks only the page.
- `proofs18_e2e_new_branch::overwrites_a_local_branch_only_after_explicitly_opting_in`: it overwrites the checked-out `main`. If the backend uses `git branch -f` instead of `checkout -B`, git refuses.
- `proofs18_e2e_new_branch::a_typed_space_becomes_a_separator_and_create_stays_enabled`: two typed spaces must collapse to one `-`, as the spec says `fix-login-redirect`.
- `workbench::diff_tab::tests::proofs18_e2e_diff_navigation::…`: `diff-view` must be the only drawn diff viewer, and its width must grow by more than 100 px when the Project view hides.
- `proofs18_e2e_toolwindows::the_terminal_sends_typed_commands_to_the_shell_and_shows_its_output`: `TerminalPanel::panes()`'s first element is taken as the group id for `terminal-group:<group>`.

## Group 1

### `e2e:e2e/commit.spec.ts` — 1 test

- "⌘K commits the checked changes with the typed message" → `workbench::tests::proofs18_e2e_commit::cmd_k_commits_the_checked_changes_with_the_typed_message`. It opens `build.gradle.kts` from the Project view, presses ⌘K (Ctrl+K off macOS), and checks four things. First, the Commit window lists `Changes (1) / build.gradle.kts / Unversioned Files (1) / notes.txt`, with only the tracked file included and both checkboxes drawn. Second, the message has the keyboard. Third, clicking Commit with no message sets `empty_error` ("Specify commit message") and HEAD keeps its count. Fourth, after typing the message and clicking Commit, the notice "1 file committed: Return order DTOs from create" shows, `git show --name-only HEAD` is `build.gradle.kts`, `git status --porcelain` is `?? notes.txt`, and the tree lists only the unversioned group, with no `commit-include-build.gradle.kts` drawn.
- Fixture differences: the spec stubs `vcs_*` IPC calls. Here it is a real temp repository. "vcs_stage was not called" becomes "the unversioned file is neither committed nor staged" (`??`, not `A `). The native path cannot observe whether the index was staged before the commit; only the result shows.
- Row: done once this test passes.

### `e2e:e2e/gitIgnore.spec.ts` — 2 tests (one per destination)

- "ignore gitignore: menu dispatch and immediate tree/status refresh" → `workbench::tests::proofs18_e2e_git_ignore::ignore_gitignore_menu_dispatch_and_immediate_tree_status_refresh`
- "ignore localExclude: menu dispatch and immediate tree/status refresh" → `workbench::tests::proofs18_e2e_git_ignore::ignore_local_exclude_menu_dispatch_and_immediate_tree_status_refresh`
- Both run one shared flow:
  - Right-click `scratch.log` in the Project view, click Git, then Add to .gitignore. Its submenu lists exactly `.gitignore` and `.git/info/exclude`. Click the destination.
  - The rule lands in that file and not the other.
  - The window's changes no longer list the file, the Project view draws it as ignored, and `git status` agrees. For `.gitignore`, only `?? .gitignore` is left.
- Fixture differences:
  - The spec uses a memory file system with a mocked `vcsAddToIgnore`, and checks that `vcsStatus` ran after the write. Here a real repository shows the same thing: the status the window shows comes from after the write.
  - The spec's last step fills the rules file in an editor. That is screenshot staging and is not asserted; the rule on disk is asserted instead.
- Row: done once both pass.

### `e2e:e2e/keymap.spec.ts` — 2 tests

- "Keymap double-click offers mouse and keyboard shortcuts" → `workbench::tests::proofs18_e2e_keymap::keymap_double_click_offers_mouse_and_keyboard_shortcuts`
- "Keymap Enter offers mouse and keyboard shortcuts" → `workbench::tests::proofs18_e2e_keymap::keymap_enter_offers_mouse_and_keyboard_shortcuts`
- The shared flow, in a workbench window:
  - ⌘, then a click on `settings-page-keymap`. Typing "Find Action" into the Keymap search lists `navigate.findAction`.
  - Activating the row (a double click, or a click then Enter) opens its menu with `> Add Keyboard Shortcut` first. Down selects `> Add Mouse Shortcut`, and Enter opens the mouse dialog for `navigate.findAction`; the action's title starts with "Find Action".
  - An Option-click on the pad records `Alt+Button1`. OK asks to remove conflicts (the Warning), and Leave closes it. The row then binds `Alt+Button1`, shown as "⌥Left Click".
  - Activating the row again and clicking Add Keyboard Shortcut opens the keyboard dialog. A pressed shortcut is recorded there, which is the native form of "First stroke is focused". Cancel adds nothing.
  - A right click on the row offers both Add rows.
- Differences:
  - The webview's `toBeFocused` on menu items is the native menu's selected row (`> `).
  - The dialog's title is not exposed, so the test asserts the recording's action and that action's title.
- Row: done once both pass.

### `e2e:e2e/openProject.spec.ts` — 1 test

- "Close Project shows the Welcome screen; a recent project opens from it once trusted" → `workbench::tests::proofs18_e2e_open_project::close_project_shows_the_welcome_screen_a_recent_project_opens_from_it_once_trusted`.
- The test opens with `acme-api` open; `billing` is recent and undecided. It then runs through:
  - Double Shift opens Search Everywhere. Typing "Close Project" until the selected row is the action's title, then Enter, closes the project: the Project view is gone, the backend has no project, and the Welcome screen lists billing and acme-api.
  - A click on billing's row shows the trust question about billing, and nothing opens yet.
  - Trust Project makes billing (decided, trusted) and leaves the parent folder out of the trusted locations ("folder: false"). billing opens, and its tree's root is billing with `package.json` listed.
- Fixture differences: the spec stubs `recent_projects`, `project_trust_*`, `open_project` and `fs_read_dir`. Here they are real folders, with `RecentProjects::touch` in the temp config and the real trust store.
- Row: done once it passes. It also proves double Shift opening Search Everywhere in the application, a step the search row lacked.

### `e2e:e2e/plugin-settings.spec.ts` — 1 test

- "plugin settings pages stage edits, apply before actions and discard on Cancel" → `workbench::plugin_contributions::tests::proofs18_e2e_plugin_settings::plugin_settings_pages_stage_edits_apply_before_actions_and_discard_on_cancel`. It depends on hunk 1 (`SettingsDialogView::page_view`).
- The test uses the `settings-pages` fixture plugin, linked in the temp home, and walks the spec's whole path:
  - ⌘, then the search "LLM Settings": the tree row is clicked and the search cleared. Plugins, Right, Model Profiles, Right, LLM Settings.
  - Add, then "Cloud" typed into Name 1, then Test. Test applies the row before the command runs: the plugin's notice reads "Read 1 saved profile(s)", Apply is disabled, and `plugins.settings-pages.profiles[0].name` is Cloud in the global settings.
  - "Discard me" typed, then Cancel, then ⌘, again: the LLM page is still selected and Name 1 reads Cloud.
  - Commit Template lists Allowed commit types, and its first cell is typed "docs".
  - On LLM Settings, "Reset only this page" is typed, then Reset: Name 1 reads Cloud again. Commit Template still reads "docs". Apply leaves nothing to apply, and `types[0]` is docs on disk.
- Fixture differences: the spec stubs `runCommand` to throw "Example provider unavailable" and asserts that alert. The real fixture's `test` command succeeds once a row exists, so the test asserts its notice. The plugin's inline error (`plugin-settings-action-error:0`) is already proven by `workbench::plugin_contributions::tests::applies_pending_values_before_a_command_reads_them_and_shows_command_errors_inline`. To assert the spec's error text natively, you would need a fixture command that fails after reading the row, which no fixture has.
- Row: partial. The spec's failing action and its alert ("Example provider unavailable") are not driven in this flow. The inline error is proven separately, and separate proofs do not count.

### `e2e:e2e/pluginNotifications.spec.ts` — 1 test

- "plugin notification actions run their command and dismiss the balloon" → `workbench::plugin_runtime::tests::proofs18_e2e_plugin_notifications::plugin_notification_actions_run_their_command_and_dismiss_the_balloon`. Its steps:
  - The `jobs` fixture's `notify-actions` shows a balloon with Action 1, Action 2 and Action 3, and every button is drawn.
  - A click on `balloon-<id>-action-<Action 1>` dismisses it: it is no longer in `shown()` and not drawn.
  - The plugin's `context` command ran with `notification-action=1\nvalue`. The context key `plugin.jobs.notification-action` is `"1\nvalue"`, which is the spec's multi-line argument.
- Fixture differences: the spec uses a memory plugins backend that records `runCommand(plugin, command, argument, …)`. Here the real plugin host runs the fixture, and the context key it sets proves both the command and the argument.
- Row: done once it passes.

## Group 2

### `e2e:e2e/newBranch.spec.ts` — 4 tests, file `workbench/tests/proofs18_e2e_new_branch.rs` (add `mod proofs18_e2e_new_branch;` to `workbench/mod.rs`'s `mod tests`)

- "a typed space becomes a separator and Create stays enabled" → `workbench::tests::proofs18_e2e_new_branch::a_typed_space_becomes_a_separator_and_create_stays_enabled`. It types `fix login  redirect` one character at a time into New Branch…'s dialog. The field reads `fix-login-redirect`, there are no errors and `can_confirm` holds. A click on `prompt-button-ok` creates and checks out that branch, as the branch widget and `git rev-parse` show.
- "overwrites a local branch only after explicitly opting in" → `…::overwrites_a_local_branch_only_after_explicitly_opting_in`. It starts from `vcs.log.newBranch` at HEAD~1; the title is "Create New Branch from …". Typing `main` gives a disabled Create and an unticked `prompt-overwrite`. Ticking it enables Create. `new-branch` takes the offer away, and `main` again gives it back unticked. Ticking it again and pressing Create makes `git rev-parse main` the chosen commit, with HEAD on main.
- "a space typed in the middle keeps the caret after it" → `…::a_space_typed_in_the_middle_keeps_the_caret_after_it`. It types `ab`, presses Left, then types a space: the field reads `a-b`. Typing `x` then gives `a-xb`.
- "pasted text has its spaces replaced" → `…::pasted_text_has_its_spaces_replaced`. With `feature/add new thing` on the clipboard, ⌘V into the emptied field gives `feature/add-new-thing`, and Create is enabled.
- Fixture differences:
  - The spec stubs `vcs_*` IPC calls. Here it is a real repository, and "vcs_create_branch received {name, start, overwrite}" is read back from git: the branch exists, at the start commit.
  - The spec's paste uses `insertText`; here it is a real clipboard paste.
  - "Overwrite is (not) checked" is read through `can_confirm`, because `PromptDialog` exposes no overwrite state. Unticked with a taken name, the name is refused; ticked, it is accepted.
- Row: done once all four pass.

### `e2e:e2e/codeInsight.spec.ts` — 7 tests: 4 declared, 3 pending

File `editor/intelligence/tests/proofs18_e2e_code_insight.rs` (add `mod proofs18_e2e_code_insight;` to `editor/intelligence.rs`'s `mod tests`, after the anchor `    mod proofs9_syntax_usages;`). The undeclared `proofs18_e2e_code_insight_pending.rs` holds three more. All of them run in the `editor::intelligence` fixture: a trusted project and the fake language server for `*.mock` files.

- "completion lists the server's items and inserts the chosen one" → `editor::intelligence::tests::proofs18_e2e_code_insight::completion_lists_the_servers_items_and_inserts_the_chosen_one`. With the caret after `return order.toDto()`, it presses Enter, types `order.`, then ⌃Space. The lookup lists toDto and total. Typing `toD` leaves `["toDto"]`, and Enter makes the caret's line read `order.toDto()`. The server got `textDocument/completion`.
- "⌘B goes to the declaration in another file" → `…::cmd_b_goes_to_the_declaration_in_another_file`. ⌘B on `service` opens `settings.mock` with the caret on line 3, which holds `val order: Order = service.create(req)`. The definition request is at 5:23.
- "Find Usages lists every usage by file and opens one" → `…::find_usages_lists_every_usage_by_file_and_opens_one`. ⌥F7 shows the rows "Found usages 3 results", `build.mock … 2 usages`, `2 service.find(status)`, `6 val order: Order = service.create…` and `settings.mock … 1 usage`. A double click on the other file's usage row (`usages-row-<i>`) opens `settings.mock`.
- "diagnostics underline the range and ⌥⏎ applies the preferred quick fix" → `…::diagnostics_underline_the_range_and_alt_enter_applies_the_preferred_quick_fix`. The Diagnostics mark is exactly `Order`, with the ErrorProblem kind. ⌥⏎ lists both fixes. A click on `list-row-<Import…>` applies the import edit, which becomes line 5 of the text, and the list closes.
- "⇧F6 renames in place and sends the server's edit to be applied" → pending, `shift_f6_renames_in_place_and_sends_the_servers_edit_to_be_applied`. The missing step is in-place rename on ⇧F6. Native ⇧F6 opens the Rename dialog (`workbench/editor_language.rs`), and `editor_lsp/inplace_rename.rs` is only reached from `editor_refactor/server_introduce.rs`. The test also needs the accessor `EditorIntelligence::inplace_rename_hint(&self) -> Option<&str>`.
- "indexed single-file rename previews all anchors and applies only after confirmation" → pending, `indexed_single_file_rename_previews_all_anchors_and_applies_only_after_confirmation`. It needs the same in-place step. The preview dialog itself exists after the Rename dialog (`workbench/editor_language.rs:795`, "Do Refactor").
- "lazy plugin actions resolve only when invoked, never for their intention preview" → pending, `lazy_plugin_actions_resolve_only_when_invoked_never_for_their_intention_preview`. The native side exists (`editor_lsp/intentions.rs::is_lazy_plugin_action`, `NO_PREVIEW`). The missing piece is a fixture plugin that contributes a lazy code action: data, no edit, no command. None of the fixtures does.
- Fixture differences:
  - The spec uses `build.gradle.kts` with a stubbed kotlin server. Here it is `build.mock` / `settings.mock` with the fake server answering LSP.
  - "lsp_execute_code_action got the action" is read as the action's edit applied to the text.
  - The spec's `lsp_goto` args become the `textDocument/definition` position.
- Row: stays partial (4 of 7).

### `e2e:e2e/toolwindows.spec.ts` — 12 tests: 7 declared, 5 pending

File `workbench/tests/proofs18_e2e_toolwindows.rs` (add `mod proofs18_e2e_toolwindows;`). The undeclared `proofs18_e2e_toolwindows_pending.rs` holds the other five. The project is trusted and the terminals start the isolated plain shell from `start()`.

- "maximized left/right/bottom Terminal fills the workbench and restores the layout" (3) → pending, `maximized_{left,right,bottom}_terminal_fills_the_workbench_and_restores_the_layout`.
  - These exist: the Options menu (`more:<id>`, with Maximize and Restore Size, as `context_menus::tests::move_to_in_a_tool_windows_options_moves_it` drives), Hide (`hide:<id>`), ⌘⇧' and `move_window`/`set_size` through `with_layout`.
  - Missing step: a double click on the header's title toggling the maximized state. No double-click handler was found on the tool-window header in `workbench/mod.rs` (`grep -n "click_count" app-gpui/src/workbench/mod.rs` finds none there). The test assumes the selector `tool-window-title:<id>`.
  - Not reproduced: window resizing (`setViewportSize`).
  - Once the title double click exists, the rest of the test should hold as written.
- "⌘1 focuses, hides and re-shows the Project tool window" → `workbench::tests::proofs18_e2e_toolwindows::cmd_1_focuses_hides_and_re_shows_the_project_tool_window`. With the editor focused, ⌘1 shows the Project view and focuses its tree; ⌘1 hides it; ⌘1 shows it.
- "⌘4 toggles Run; Shift+Escape hides the active tool window" → `…::cmd_4_toggles_run_shift_escape_hides_the_active_tool_window`. ⌘4 shows Run, hides it, then shows it again; Shift+Escape hides it.
- "⌥F12 opens the Terminal tool window and toggles it closed" → `…::alt_f12_opens_the_terminal_tool_window_and_toggles_it_closed`. The window is visible and listed in `layout.visible()`, then hidden.
- "the first ⌥F12 focuses the terminal input once the lazily loaded view mounts" → `…::the_first_alt_f12_focuses_the_terminal_input_once_the_view_mounts`. The terminal view is focused after the first ⌥F12, and again after hide and show.
- "a terminal view that loads late doesn't take focus moved elsewhere meanwhile" → `…::a_terminal_view_that_loads_late_does_not_take_focus_moved_elsewhere_meanwhile`. After ⌥F12, a click on README.md's row happens before the shell has started. Once the shell has started (`id()` and a prompt on screen), the tree still has the keyboard. Difference: no module loads late natively, so the asynchronous part is the shell's start.
- "Split Right gives the Terminal a tab group with its own header; Unsplit merges it back" → pending, `split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back`.
  - Missing: debug selectors on the tabs (`terminal-tab:<id>` is only an element id).
  - Hypothesis, from the sweep note and one `grep -n "header" terminal/panel.rs` hit: each group has no header of its own. Check it by reading `TerminalPanel::render`.
- "closing the last tab of a terminal group focuses its neighbour" → pending, `closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`. Missing: a `debug_selector` on the tab's close button (`terminal-close:<id>` is only an element id). With that, the test as written should hold.
- "⌘⇧F12 hides all tool windows and restores them" → `…::cmd_shift_f12_hides_all_tool_windows_and_restores_them`. ⌘6 shows Problems; ⌘⇧F12 leaves `layout.visible()` empty; ⌘⇧F12 again shows Project and Problems. Expected to fail if the second ⌘⇧F12 does not restore: the existing test restores with F12, and the parity note says the second press does not restore. In that case the fix belongs in the hide-all action, which must restore the windows it hid when nothing is shown.
- "the Terminal sends typed commands to the shell and shows its output" → `…::the_terminal_sends_typed_commands_to_the_shell_and_shows_its_output`. ⌥F12 shows a prompt. A click on `terminal-group:<group>` gives the terminal the keyboard. `echo hello from e2e` and Enter make exactly one screen line read `hello from e2e`. Difference: the spec stubs the shell and checks `term_write` received the exact keystrokes. Here a real plain shell answers, and the answer appearing once proves the input reached it.
- Row: stays partial (7 of 12).

### `e2e:e2e/mergeEditor.spec.ts` — 16 tests: 5 new declared, 9 already proven by existing tests, 2 pending

File `workbench/tests/proofs18_e2e_merge_editor.rs` (add `mod proofs18_e2e_merge_editor;`; it uses `proofs_merge_fixtures`). The undeclared `proofs18_e2e_merge_editor_pending.rs` holds two more. The spec mounts the merge editor alone over mocked `vcsConflictVersions`. The native tests open it from a real conflicted merge (`git_project` + `diverged`, then Merge in the Conflicts dialog). The new tests use the spec's own three versions (`BASE`/`OURS`/`THEIRS`).

- "CRLF resolutions use UTF-16 editor positions and preserve line endings on Apply" → new, `workbench::tests::proofs18_e2e_merge_editor::crlf_resolutions_use_utf16_editor_positions_and_preserve_line_endings_on_apply`. It uses the spec's CJK, emoji and CRLF versions. The platform-modified click on `merge-accept-left-0` leaves "All changes processed", and the result is Yours. A click on `merge-footer-apply` asks nothing, closes the editor, writes Yours byte for byte with CRLF, leaves `ls-files -u` empty and shows `M  a.txt`. Difference: the spec asserts the editor's UTF-16 `docEdit` range (0..13). Natively the bytes on disk are the visible proof, and the edit range has no window-level equivalent.
- "Apply writes and stages unresolved text only after confirmation" → existing `workbench::tests::proofs_merge_apply::apply_merge_result_asks_about_unresolved_changes_and_writes_the_result`. Apply asks; Escape (Cancel) writes nothing and the file stays unmerged; Apply then Mark Resolved writes the base and stages it. It runs Apply as an action, not through the footer click.
- "binary and unreadable versions show an error with no Apply controls" → existing `vcs::merge_view::tests::refuses_binary_versions_and_offers_to_retry`. That is a component test, which counts because the spec mounts the component alone.
- "per-change controls also work from the keyboard" → pending, `per_change_controls_also_work_from_the_keyboard`. Hypothesis: the divider controls cannot take the keyboard. `control_button` in `vcs/merge_view.rs` was not read; check with `grep -n "fn control_button" -A25 app-gpui/src/vcs/merge_view.rs` for `track_focus`/`on_key_down`. The test assumes an accessor `MergeView::focus_control(selector, window, cx)`.
- "accepting a deleted version removes the file before staging" → new, `…::accepting_a_deleted_version_removes_the_file_before_staging`. On a modify/delete conflict, Accept Theirs from the footer deletes `a.txt`, leaves `ls-files -u` empty and gives `D  a.txt`. Risk: the Conflicts dialog's Merge may not open the merge editor for a modify/delete conflict. If it refuses, the step is missing in `workbench/conflicts.rs`.
- "the result starts as the base and every change waits for a decision" → existing `vcs::merge_view::tests::starts_from_the_base_and_counts_the_changes_left` (component).
- "resolves the simple conflicts and applies the other changes in one step" → existing `workbench::tests::proofs_merge_apply::resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more`, run as actions.
- "accepts both sides of a conflict in turn, and undo brings the buttons back" → existing `vcs::merge_view::tests::accepts_both_sides_of_a_conflict_from_the_arrows_and_undoes_them` (component).
- "the toolbar applies the non-conflicting changes of one side" → new, `…::the_toolbar_applies_the_non_conflicting_changes_of_one_side`. With the spec's versions, ApplyLeft is disabled and a click on it changes nothing. A click on `merge-tool-applyRight` gives "2 changes left, 2 conflicts" and `\nC\n`, and ApplyAll is disabled.
- "has the dialog's buttons: Accept Yours, Accept Theirs, Cancel and Apply" → existing `vcs::merge_view::tests::takes_a_whole_side_and_asks_before_discarding_changes` (component).
- "compares the versions in pairs" → existing `vcs::merge_view::tests::compares_two_versions` (component). In the application: `workbench::tests::proofs_merge_compare::each_comparison_shows_its_two_versions_in_the_diff_window`.
- "keeps the option to apply the non-conflicting changes on opening in its settings" → existing `workbench::tests::proofs_merge_compare::automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge`.
- "the context menus decide the changes under the caret" → new, `…::the_context_menus_decide_the_changes_under_the_caret`. A right click on result line 7 (`same`) opens a menu where Resolve Automatically is disabled. Ignore there gives "2 changes left, 1 conflict". A right click on Theirs' line 4 (`C`) and Resolve Using Theirs gives "1 change left, 1 conflict" and `\nC\n`.
- "applies the non-conflicting changes on opening when asked to, and that is not a step to undo" → existing `vcs::merge_view::tests::starts_with_the_non_conflicting_changes_applied_when_asked_to` (component), and the workbench settings test above.
- "Find Action runs merge commands with the editor context and clears it on external focus" → new, `…::find_action_runs_merge_commands_with_the_editor_context_and_clears_it_on_external_focus`. A click in the result, then ⌘⇧A with "Resolve Simple Conflicts" and Enter, gives "1 change left, 1 conflict" and closes the popup. "Revert Conflict Resolution" gives "3 changes left, 2 conflicts" and the result equals BASE. With the Project tree focused, `merge.resolveSimple` does not run.
- "merge actions follow the focused instance and survive a context menu" → pending, `merge_actions_follow_the_focused_instance_and_survive_a_context_menu`. Hypothesis: there are not two merge editors at once. `Workbench::merge_view()` returns one; the test assumes `merge_views()`. Check whether a second conflicted file's Merge replaces the first.
- Row: stays partial (14 of 16 once the new tests pass; 2 pending). The 7 `vcs::merge_view::tests::*` and `proofs_merge_*` mappings were cited from `parity.json` and the files' doc comments. Their bodies were not re-read: `proofs_merge_apply.rs` was read, the `vcs::merge_view` tests were not.

## Group 3

### `e2e:e2e/run.spec.ts` — 4 tests: 2 declared, 2 pending

File `workbench/tests/proofs18_e2e_run.rs` (add `mod proofs18_e2e_run;`); `proofs18_e2e_run_pending.rs` is undeclared. Both declared tests use `run_project` with a saved `dev` configuration running `/bin/sh -c …` (a program configuration), selected in the store.

- "⌃R runs the selected configuration and shows its output and exit" → `workbench::tests::proofs18_e2e_run::ctrl_r_runs_the_selected_configuration_and_shows_its_output_and_exit`. ⌃R (Shift+F10 off macOS) is pressed. The session ends; Run is shown; the console has the stdout line and the stderr line; `can_stop` is off; the tab is `dev`; its tooltip ends with "exit code 0"; the session ran `/bin/sh` with exit 0.
- "⌘F2 stops a running process" → `…::cmd_f2_stops_a_running_process`. "Watching for changes…" shows, the session is Running and `can_stop` is on. ⌘F2 (Ctrl+F2) ends it with `stopped` set, `can_stop` goes off, and the tooltip ends with "exit code 143". Risk: the script `exec`s `sleep` so that SIGTERM ends it. If the native console reports a signalled process otherwise (no code, or "-1"), the assertion fails. 143 is what the spec shows.
- "Go reruns keep each package's exact failures and the original configuration" → pending, `go_reruns_keep_each_packages_exact_failures_and_the_original_configuration`.
  - These exist: `RunSession::base_config` and `run_testing::rerun::rerun_failed_configs`.
  - Hypothesis: nothing in the Run tool window calls `rerun_failed_configs`. Check with `grep -rn rerun_failed_config app-gpui/src --include='*.rs' | grep -v run_testing`.
  - The test assumes a `run-rerun-failed` button and uses a fake `go` script.
- "CMake builds the selected target in Build before starting only the application in Run" → pending, `cmake_builds_the_selected_target_in_build_before_starting_only_the_application_in_run`.
  - Correction: an earlier draft said `beforeLaunch` was missing. The run driver runs before-launch tasks (`run/driver.rs` uses `run::before_launch::BeforeLaunch`), and the test now sets `extras.before_launch = [BeforeLaunchTask { kind: "build", target }]`.
  - Not checked: that a CMake build id runs in the Build tool window, and the Build Project action id (`build.buildProject` assumed).
  - The test uses a fake `cmake` and may be promotable as written.
- Fixture differences: the spec stubs `run_start`/`run_stop`. Here real processes run, and "run_start got config {command, args}" is read from the session's config.
- Row: stays partial (2 of 4). The two declared tests replace the earlier action-driven proofs with key-driven ones.

### `e2e:e2e/search.spec.ts` — 4 tests: 3 declared, 1 pending

File `workbench/tests/proofs18_e2e_search.rs` (add `mod proofs18_e2e_search;`). The project has `src/main/kotlin/com/acme/orders/service/OrderService.kt` and waits for the file index.

- "double Shift opens Search Everywhere with its tabs; Escape closes it" → `workbench::tests::proofs18_e2e_search::double_shift_opens_search_everywhere_with_its_tabs_escape_closes_it`. Double Shift is sent through `simulate_modifiers_change`. The popup opens on All with the query focused. Tab ×6 goes through Classes, Files, Symbols, Actions, Text and back to All, which is the native proof that the six tabs exist, since `SearchEverywhere` exposes no tab list. Escape closes it.
- "Search Everywhere finds and runs an action" → `…::search_everywhere_finds_and_runs_an_action`. "Language Servers" is the selected row; Enter opens Settings with `languages.servers` selected. Difference: the spec also asserts the page's list of servers. Natively the selected page is asserted, not its content.
- "Search Everywhere finds a file and opens it" → `…::search_everywhere_finds_a_file_and_opens_it`. "OrderService" lists the file's relative path; Enter opens it, and the active tab is `OrderService.kt`.
- "Find in Files streams matches by file and opens the chosen one at its line" → pending, `find_in_files_streams_matches_by_file_and_opens_the_chosen_one_at_its_line`. Hypothesis, not checked: the preview is not an editor. `FindPopup` exposes `preview_lines`/`preview_marks`, and `preview_mode` is Replace's diff preview; no editor accessor was found. Check with `grep -n "Editor" app-gpui/src/find/popup_view.rs`. The spec edits, deletes and undoes in the preview. The test assumes `FindPopup::preview_editor()`. The listing and the double click that opens the match at its line exist (`proofs12_find_popup`).
- Fixture differences: the spec stubs `find_files` and `search_start`. Here they are the real index and search.
- Row: stays partial (3 of 4). The double-Shift step the row was missing is now proven.

### `e2e:e2e/editor.spec.ts` — 8 tests: 6 declared, 2 pending

File `workbench/tests/proofs18_e2e_editor.rs` (add `mod proofs18_e2e_editor;`); `proofs18_e2e_editor_pending.rs` is undeclared. The project holds `build.gradle.kts`, `settings.gradle.kts` and `README.md`, trusted, in an active window. Only `Workbench` accessors (`editor()`, `tab_titles()`, `find_bar()`, `status_items()`) are used, nothing of `Content::Editor`.

- "opens a file from the project tree and types into it" → `workbench::tests::proofs18_e2e_editor::opens_a_file_from_the_project_tree_and_types_into_it`. The file is opened from the tree with Enter. ⌘↓, Enter and the typed text put the comment in the text; ⌘Z takes it out.
- "Delete edits text after the focused Project tree is remounted" → `…::delete_edits_text_after_the_focused_project_tree_is_remounted`. A line is typed and the tree gets the keyboard. The `keymap` setting becomes `intellij-windows`, and `move_window(project, Slot::Right)` moves the Project view to the right, where it shows. Back in the editor, Left and Delete give `delete probe ab`, then Backspace gives `delete probe a`. Difference: there is no remount natively. The spec's "tree not focused after the remount" is a webview effect and is not asserted.
- "⌘F opens the find bar and highlights matches; Escape returns to the editor" → `…::cmd_f_opens_the_find_bar_and_highlights_matches_escape_returns_to_the_editor`. The find field has the keyboard; `MarkLayer::Find` is not empty after typing "Order"; Escape removes the bar and the editor has the keyboard.
- "each file keeps its own find bar, query and caret" → `…::each_file_keeps_its_own_find_bar_query_and_caret`. The spec's whole sequence is checked through the status bar caret item (`(5 chars)`, `(6 chars)`, unchanged carets) and each bar's find field. Expected to fail if a new bar does not start from the last search ("Order" in settings, "status" in README): the existing tests prove only that a tab keeps its own bar. In that case the fix is to seed a new bar's query from the latest search.
- "opening several files creates tabs and ⌘W closes the active one" → `…::opening_several_files_creates_tabs_and_cmd_w_closes_the_active_one`. Two tabs; ⌘W (Ctrl+F4) leaves `["build.gradle.kts"]`.
- "breadcrumbs follow the caret into the symbol under it" → pending, `breadcrumbs_follow_the_caret_into_the_symbol_under_it`. Missing: code breadcrumbs over the editor, and Navigation Bar (`navigate.navigationBar` is not registered). The test needs `Workbench::code_breadcrumbs`, `status_breadcrumbs` and `navigation_bar`.
- "an edit marks the tab modified and ⌘S saves it" → pending, `an_edit_marks_the_tab_modified_and_cmd_s_saves_it`. Missing: the modified mark on an edited tab (`pane_element` draws none), which the test expects as the `tab-modified:<id>` selector. ⌘S saving through the window is already proven by `workbench::tests::the_keymap_runs_editor_actions_on_the_windows_editor`.
- "⌘R replaces every match in the file" → `…::cmd_r_replaces_every_match_in_the_file`. ⌘R opens the replace bar. "OrderDto" goes in the find field and "OrderView" in the replace field. A click on `find-bar-replace-all` leaves no OrderDto line and 2 OrderView lines. One ⌘Z in the editor brings back the 2 OrderDto lines.
- Row: stays partial (6 of 8).

### `e2e:e2e/compareWith.spec.ts` — 4 tests: 3 already proven by existing window tests, 1 new

File `workbench/diff_tools/tests/proofs18_e2e_compare_with.rs` (add `mod proofs18_e2e_compare_with;` to `workbench/diff_tools.rs`'s inline `mod tests`, after its `use` lines, for example after the anchor `    use std::rc::Rc;`; it creates the folder `diff_tools/tests/`).

- "project tree menu offers Compare With… for a file" → existing `workbench::diff_tools::tests::compare_with_asks_for_the_other_file_and_shows_both`. Its `tree_menu` right-clicks the file's row and reads Compare With…, and not for a folder.
- "editor tab menu offers Compare With…" → existing `workbench::context_menus::tests::the_tab_menu_acts_on_the_tab_it_was_opened_on`. A right click on `tab:<id>` gives root rows including "Compare With…".
- "editor menu offers Compare With…" → existing `workbench::context_menus::tests::right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret`. A right click in the text gives root rows equal to `ROOT`, which ends with "Compare With…" and "Compare with Clipboard".
- "Compare With… compares against a file picked in the browser" → new, `workbench::diff_tools::tests::proofs18_e2e_compare_with::compare_with_compares_against_a_file_picked_in_the_dialog`. It right-clicks README.md and clicks the `Compare With…` row. The path prompt is asked and answered with `other.md`, a file outside the project. The diff window shows `readme text` / `other text`, and its title names other.md. Difference: the browser file picker becomes the system path prompt, and the compared files open in the diff window, not in an editor tab named other.md.
- Row: done once the new test passes. The three earlier flows need these existing tests cited in the row's proof.

### `e2e:e2e/diff-navigation.spec.ts` — 3 tests: 2 declared, 1 pending

The two declared tests are `workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs` (declared in `diff_tab.rs`) and `workbench/tests/proofs18_e2e_diff_navigation.rs` (declared in `workbench/mod.rs`). `proofs18_e2e_diff_navigation_pending.rs` is undeclared.

- "maximizes a diff preview tab and restores its previous layout" → `workbench::diff_tab::tests::proofs18_e2e_diff_navigation::maximizes_a_diff_preview_tab_and_restores_its_previous_layout`. It uses the non-modal commit interface, where Show Diff opens a diff tab (`non_modal_project`, `open_diff_tab`).
  - With the Project view shown, the counter reads "1 difference".
  - A click on `diff-maximize` hides the Project view. The button becomes Restore Size (checked), `diff-view` is drawn more than 100 px wider, and the counter still reads "1 difference".
  - A second click shows the Project view again, at the same width within 2 px, and the counter is unchanged.
  - Difference: the spec opens a two-file chain directly. Here it is one changed file.
  - Correction: an earlier draft of these notes said the native diff is only a window. That was wrong. The modal interface uses the diff window, which has no Maximize Editor (`proofs12_diff_window::does_not_offer_editor_maximization_in_an_embedded_diff`); the non-modal interface uses diff tabs, which have it (`workbench/diff_tab.rs`).
- "continues between changed files from toolbar buttons and the keymap" → `workbench::tests::proofs18_e2e_diff_navigation::continues_between_changed_files_from_toolbar_buttons_and_the_keymap`. It runs in the modal interface's diff window, from the Commit window's Show Diff on a.ts and b.ts:
  - The counter starts at "1 difference". A click on `diff-next` gives "1 of 1 differences"; a second click shows the hint "Press again to go to the next file" and stays on a.ts.
  - F7 with the viewer focused shows b.ts at "1 of 1 differences".
  - A click on `diff-previous` shows the previous-file hint; a second click shows a.ts.
- "skips binary and unchanged siblings and restores after changing preview files" → pending, `skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files`. Hypothesis, not checked: F7/⇧F7 walk between diff tabs past binary siblings. Settle it by reading how `workbench/diff_tab.rs` steps files: `grep -n "next_file\|NextFile" app-gpui/src/workbench/diff_tab.rs`. The test assumes `diff_tab()` / `diff_tab_path(cx)`, but the real accessor is `diff_tab_viewer` (`pub(super)`). An unchanged sibling never enters the native chain.
- Row: stays partial (2 of 3).
