# proofs11: spec cases driven through the views (Agents, Database results, GitHub list)

Written without a compiler: nothing here has been compiled or run. No existing file was edited.

## Files and the `mod` lines to add

| New file                                                               | Tests | `mod` line                    | Where (anchor line, inside the inline `mod tests`)                                                         |
| ---------------------------------------------------------------------- | ----- | ----------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/workbench/tests/proofs11_agents.rs` (629 lines)          | 13    | `    mod proofs11_agents;`    | `app-gpui/src/workbench/mod.rs`, next to `    mod project_threads;` (line ~4139; rustfmt sorts it)         |
| `app-gpui/src/agents/task_list_view/tests/proofs11_task_list.rs` (356) | 13    | `    mod proofs11_task_list;` | `app-gpui/src/agents/task_list_view.rs`, after `    use std::rc::Rc;` (line 654, inside `mod tests`)       |
| `app-gpui/src/agents/session_view/tests/proofs11_session.rs` (202)     | 5     | `    mod proofs11_session;`   | `app-gpui/src/agents/session_view.rs`, beside `    mod proofs8_agent_reply;` (line 2200)                   |
| `app-gpui/src/agents/markdown_view/tests/proofs11_markdown.rs` (86)    | 4     | `    mod proofs11_markdown;`  | `app-gpui/src/agents/markdown_view.rs`, after `    use std::cell::RefCell;` (line 530, inside `mod tests`) |
| `app-gpui/src/database/grid_view/tests/proofs11_grid.rs` (27)          | 1     | `    mod proofs11_grid;`      | `app-gpui/src/database/grid_view.rs`, after `    use std::rc::Rc;` (line 1009, inside `mod tests`)         |
| `app-gpui/src/database/results_view/tests/proofs11_results.rs` (234)   | 9     | `    mod proofs11_results;`   | `app-gpui/src/database/results_view.rs`, after `    use std::rc::Rc;` (line 1109, inside `mod tests`)      |
| `app-gpui/src/github/list_panel/tests/proofs11_list.rs` (230)          | 7     | `    mod proofs11_list;`      | `app-gpui/src/github/list_panel.rs`, after `    use std::rc::Rc;` (line 1616, inside `mod tests`)          |
| `app-gpui/src/workbench/github/tests/proofs11_github.rs` (84)          | 3     | `    mod proofs11_github;`    | `app-gpui/src/workbench/github.rs`, beside `    mod proofs7_github_urls;` (line 1553)                      |

55 tests. No accessor or fix is needed for them to compile, as far as reading tells: every helper, accessor and field they use exists (private fields `PrListPanel::field` and the parent `tests` helpers are reached from child modules).

## Per test → rows

Agents, through the Agents tool window with the fake agent (`workbench/tests/proofs11_agents.rs`; the editing tests use their own `EDITING_AGENT`: rollback and fork capable, logs every method it reads to `agent-calls.log` in the session folder, "…wait" holds until cancelled, "…stubborn" never stops, "refuse" makes later rollbacks fail):

- `deletes_the_right_clicked_selection_after_one_confirmation_excluding_group_headers` → `unit:src/features/agents/AgentsView.test.tsx`: a ⇧ range in the history across the Running and Ready groups selects three tasks, the right-clicked row's popup row "Delete (3)…" asks once ("Delete Agent Tasks", no checkbox), OK deletes all three, README untouched.
- `cancels_the_whole_batch` → same row: `agents.deleteTasks` of two, Cancel, both tasks and rows stay.
- `archives_only_eligible_tasks_in_a_mixed_selection` → same row: running + idle + archived selected with ⌘, "Archive (1)" archives the idle one only.
- `unarchives_only_archived_tasks_in_a_mixed_selection` → same row: "Unarchive (2)" brings back both archived, the idle one stays Ready.
- `applies_worktree_cleanup_only_to_worktree_tasks` → same row: worktree tasks in a repository; ticked checkbox removes the worktree folder from disk and keeps the project, unticked keeps the worktree folder.
- `rolls_back_before_sending_the_edited_text_when_send_is_clicked` → `unit:src/features/agents/messageEditing.test.tsx`: Edit + Send; the agent's log shows rollback before the second prompt; the conversation shows only the edited turn and "Answer 2".
- `waits_for_a_running_turn_to_stop_before_rolling_it_back` → same row: cancel < rollback < prompt in the agent's log.
- `stops_before_rollback_and_restores_text_without_sending_turn_0` / `_turn_1` → same row (the `it.each([0, 1])` case): cancel before rollback, composer "<message>\nDraft", no new prompt.
- `keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time` → same row: cancel sent, no rollback, the message back in the composer after the stop times out (needs ~15 s of real time; `wait_long` allows ~30 s).
- `recovers_the_original_prompt_when_rollback_fails` → same row.
- `confirms_before_retracting_a_turn_with_later_messages_and_guards_repeated_clicks` → same row: one dialog for a click plus `agents.retractMessage`, Cancel keeps both turns, OK rolls back and fills the composer.
- `keeps_the_transcript_and_running_session_when_a_fresh_session_rerun_is_cancelled` → same row: "Start a Fresh Agent Session" asked, Cancel leaves the running turn, no cancel/rollback/prompt reaches the agent.

Agents views (`agents/task_list_view/tests/proofs11_task_list.rs`, the tool window's view in a test window):

- `includes_the_current_session_in_history_and_keeps_the_conversation_mounted`, `gives_the_whole_tool_window_to_the_conversation_when_the_docked_task_is_the_only_one`, `opens_tabs_as_usual_without_a_docked_conversation`, `keeps_placement_out_of_session_menus` (docked and not), `offers_to_delete_the_right_clicked_selection_counting_its_tasks_and_not_its_group_headers`, `offers_to_archive_only_the_eligible_tasks_of_a_mixed_selection`, `offers_to_unarchive_only_the_archived_tasks_of_a_mixed_selection` → `unit:src/features/agents/AgentsView.test.tsx` (the last three are the menu half of the bulk cases whose batch half is in the workbench file).
- `does_not_mark_historical_tasks_unread_but_tracks_replies_even_without_loaded_entries`, `marks_background_status_transitions_unread`, `ignores_user_messages_thoughts_empty_replies_and_subagent_streaming`, `clears_only_the_viewed_task_and_keeps_reading_while_either_split_shows_it` (two `SessionView`s of one task), `cleans_up_archived_and_deleted_tasks_ignores_late_replies_and_resets_on_project_switch` → `unit:src/features/agents/unread.test.tsx`: the rows' ` | unread` and the drawn `agents-unread-<id>` dot.
- `leaves_a_task_unread_while_its_kept_alive_tab_is_hidden_and_reads_it_once_shown` → `unit:src/features/agents/sessionState.test.tsx` (`SessionView::set_shown`, read off the row).

Conversation view (`agents/session_view/tests/proofs11_session.rs`):

- `keeps_tool_reasoning_plan_and_approval_rows_in_one_author_run` (open and answered), `sends_the_original_option_id_then_collapses_the_resolved_card` (the four option kinds), `shows_the_providers_usage_limits_by_default_and_each_meter_as_settings_tools_agents_chat_allows` (header meters), `fetches_a_conversation_once_and_shows_it_again_from_the_store_when_its_tab_reopens` → `unit:src/features/agents/sessionState.test.tsx`.
- `keeps_the_latest_usage_limits_per_provider_without_marking_anything_unread` → `unit:src/features/agents/unread.test.tsx` (header meters follow the latest limits of the conversation's provider only).

Markdown (`agents/markdown_view/tests/proofs11_markdown.rs`): `keeps_raw_markup_from_reaching_the_page`, `ignores_data_attributes_written_in_a_reply_keeping_its_own`, `opens_a_markdown_file_link` (the five hrefs, clicked), `does_not_open_unsupported_links_as_files` (the four hrefs) → `unit:src/features/agents/Markdown.test.tsx`.

Database: `preserves_short_values_and_truncates_only_beyond_the_preview_limit` → `unit:src/features/database/Grid.test.tsx` (the cells the grid shows). `keeps_every_cell_of_a_pasted_row_in_the_submitted_edits`, `makes_a_single_table_select_with_its_primary_key_editable_and_submits_in_one_transaction`, `uses_the_execution_schema_even_after_the_console_selection_changes`, `explains_why_a_result_is_read_only` (join, aggregate, union, no key), `keeps_an_unresolved_sql_server_table_schema_read_only`, `matches_case_sensitive_key_columns_exactly` (Postgres, SQL Server), `keeps_clickhouse_read_only`, `keeps_a_read_only_data_source_read_only`, `keeps_a_table_without_a_primary_key_read_only` → `unit:src/features/database/results-view.test.tsx` (notice drawn, Submit not drawn, double-click opens no editor; Submit events and Output lines).

GitHub list (`github/list_panel/tests/proofs11_list.rs`): `offers_intellijs_quick_filters_checked_only_on_an_exact_match`, `writes_the_history_text_like_get_short_text`, `sets_or_clears_the_text`, `saves_searches_by_intellijs_rules_waiting_10_seconds_for_chip_only_changes`, `orders_reviewers_approved_changes_requested_commented_re_requested_needs_review`, `formats_the_info_line_and_the_comment_tooltip`, `requests_the_next_page_past_70_percent_of_the_scroll` (a wheel scroll over 200 rows) → `unit:src/features/github/list/list.test.tsx`.

GitHub window with the fake gh (`workbench/github/tests/proofs11_github.rs`): `view_pull_requests_shows_the_tool_window_and_gives_it_the_keyboard` (visible + `activeToolWindow`), `the_list_asks_gh_for_its_next_page_and_appends_it` (a second fake-gh case answers `"cursor":"c1"`; the test rewrites the fake gh script with one extra `case`), `the_chip_choosers_run_a_new_search_and_load_their_users_through_gh` (State → Merged runs `is:merged`; Author asks gh's assignees/collaborators/contributors endpoints) → `action:github.pullRequests`, and the list row too.

## Counts per spec (cases covered in full on the view, by the spec's sentence) and row status if all pass

- `unit:src/features/agents/AgentsView.test.tsx`: 18 of 19 (was 9). Stays **partial**: "continues after an error and reports the failed task" not written (below).
- `unit:src/features/agents/Markdown.test.tsx`: 7 of 18 (was 3). **partial**: 8 image cases and 3 diagram-drawing cases need behaviour the view lacks (below).
- `unit:src/features/agents/messageEditing.test.tsx`: 14 of 18 (was 7). **partial**: "recovers images after failed regeneration", "shows the images and lists only the files as chips", "opens an image in a dialog and keeps the images when editing…", "recovers the edit after rollback if sending fails" not written.
- `unit:src/features/agents/sessionState.test.tsx`: 19 of 31 (was 14). **partial**: not written: "builds only the requested tail", the three follow/scroll-geometry cases (pointer-drag prepend, offset moving back, scrollbar thumb), "shows file references and skills as distinct tags" (no accessor tells tag kinds apart in `lines`), "shows the whole question once inline…", "keeps separate answers for Codex question ids…", "picks options… previews the picked or hovered one" (preview half), "shows the requested command while another read tool is pending…", "keeps docked changes opt-in…", the three scroll-position cases (retains scrolling both directions, task/subagent positions, restore after load).
- `unit:src/features/agents/unread.test.tsx`: 6 of 18 in the views (was 0 in the app; the existing workbench test already shows the dot after a hidden reply). "The view is not built" is no longer true for the dot (task_list_view draws `agents-unread-<id>`), but these are still missing: the clickable status count (`AgentsStore::status_item` has no caller outside tests, by grep over `app-gpui/src`: a status bar item would go in `workbench/status_items.rs`/`chrome.rs`), the tool window badge (`AgentsStore::badge` has no caller outside tests, by grep; the workbench has `problems_badge` but no agents badge: the stripe button of `agents` in the tool window bar), the two restored-tab-title cases and the six dock/undock/drag-to-editor cases (conversations are never editor tabs: gap 17, `editor/tabs.rs` + `workbench/agents.rs`), and the two window-focus cases (not written: they need the window deactivated and reactivated in a test, through `Workbench::agents_window_active`). **partial**.
- `unit:src/features/database/Grid.test.tsx`: 12 of 12 → **done**.
- `unit:src/features/database/results-view.test.tsx`: 13 of 14 → **partial**: "records the selected schema or connection default" is the console's execution (store), not the result view; it needs a Database window test on a database with schemas (SQLite has none).
- `unit:src/features/github/list/list.test.tsx`: 15 of 15 → **done** (reviewers: the row draws the first two of the order, `AVATAR_STACK_MAX`; the full order is asserted as the drawn prefix).
- `action:github.pullRequests`: → **done** (visible, keyboard, next page, chooser popups in the app).

## Recipes not written, and why

- AgentsView "continues after an error and reports the failed task": no way found to make the backend's delete of one task fail deterministically without a seam (removing a worktree folder by hand first may or may not fail `ide_host::ops::agent` delete). A seam on `run_agent_call` (`workbench/agents.rs`) or a delete that fails for a known reason would make it writable; the expected native result is the "Cannot delete agent tasks" message box with "<title>: <error>" and the other task deleted.
- Markdown images (8 cases: loads a reply image from the project host, raw HTML images, inline/web image, reuses reads while streaming, read failure and retry, detached images, unsafe sources, network/decode failure): **behaviour missing in the view**. `markdown_view.rs` draws an image as a slot with its alternative text; `markdown::MessageImages` (the model) is never used by a view and nothing reads `fs_read_image`. The piece: `markdown_view::Builder::image` showing `MessageImages::view` states (loading / data / "Unable to load image: …"), reading host files through the backend, plus an accessor to read the shown state. "keeps an inline or web image" (`referrerpolicy`) and "does not update detached images" are DOM/async-specific in part.
- Markdown diagrams ("renders the fence as a placeholder…, then draws the diagram", "draws two equal diagrams with unique SVG ids", "keeps the source copyable once the diagram is drawn", "tells the conversation to follow when a diagram draws"): **behaviour missing**: the view keeps the fence as a code block and draws no diagram; the unique-SVG-id case is DOM/SVG-specific and has no native counterpart as written.
- messageEditing image cases: user messages' images are drawn (`session-user-<n>-image-<k>`), but writing them needs image entries with data and, for the regeneration/send-failure cases, an agent that fails a prompt; left out for time, not for a missing behaviour.
- sessionState follow/scroll geometry cases: need scroll-geometry driving (drags, layout-caused offsets) that the existing follow model tests do; not attempted in the window.

## Behaviours that may differ from the webview (expected failures if so)

- `confirms_before_retracting_a_turn_with_later_messages_and_guards_repeated_clicks`: if the native Retract of a turn with later messages does not ask first, this fails; the fix is a confirm in `reply_actions::retract` (models.replies) when later turns exist.
- `view_pull_requests_shows_the_tool_window_and_gives_it_the_keyboard`: `show_pull_requests` calls `layout.show(PULL_REQUESTS_TOOL_WINDOW, true)`; if that flag does not move focus, `activeToolWindow` stays unset and the test fails (webview: `toolWindowService().show(WINDOW, focus = true)`).
- `uses_the_execution_schema_even_after_the_console_selection_changes`: the completion model knows `users` only in `public`; if `Editing::details_request` needs the table known in the run schema `s2`, no LoadDetails for `s2` is asked and the test fails (webview asks `dbTableDetails("pg", "s2", "users")`).

## Most likely to fail on first run, and why

1. `applies_worktree_cleanup_only_to_worktree_tasks`: assumes the New Task page starts a worktree task from `draft.isolation = Worktree` + `isolation_chosen`, that `task.cwd` is the worktree folder, and that a ticked delete removes it from disk.
2. `keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time`: the stop deadline is checked against wall time (`now_ms`), so it takes ~15 s real; close to `wait_long`'s budget on a slow machine.
3. `the_list_asks_gh_for_its_next_page_and_appends_it`: assumes the request body is compact JSON (`"cursor":"c1"`) so the shell `case` matches.
4. The workbench bulk tests: assume history rows `agents-task-<id>` are clickable with ⇧/⌘ inside the deferred popup, and the menu labels "Delete (3)…", "Archive (1)", "Unarchive (2)" exactly as `I18n::tx_built` writes them.
5. `requests_the_next_page_past_70_percent_of_the_scroll`: assumes the test window shows well under 140 of 200 rows and that a −20 000 px wheel scroll over a row reaches the list.
6. `sends_the_original_option_id_then_collapses_the_resolved_card`: assumes the answered record still names "python3" and drops the `[answer]` buttons and the whole command.
7. `saves_searches_by_intellijs_rules_waiting_10_seconds_for_chip_only_changes`: assumes the State chooser rows read "Closed"/"Merged" and map to `gh-overlay-<row>`, and that Clear filters gives `SearchValue::default()`.
8. `explains_why_a_result_is_read_only` / `matches_case_sensitive_key_columns_exactly`: run several results through one window by replacing tab 1; if a stale details state carries over, a notice could differ.
