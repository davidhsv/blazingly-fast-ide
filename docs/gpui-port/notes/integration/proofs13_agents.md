# proofs13_agents: Agents spec cases through the views

Nothing here was compiled or run. Every claim about how the native code behaves is read from the code (file:line given) and is a hypothesis until the integrator's run.

## Files

- `app-gpui/src/workbench/tests/proofs13_agents_fixtures.rs` (helpers only, every one used by a file needing no hunk: `IMAGE_AGENT` script, `image_project`, `start_with`, log readers)
- `app-gpui/src/workbench/tests/proofs13_agents_images.rs` (2 tests, pass on existing API)
- `app-gpui/src/workbench/tests/proofs13_agents_failed_send.rs` (4 tests, depend on hunk A)
- `app-gpui/src/workbench/tests/proofs13_agents_stale_editor.rs` (1 test, depends on hunk A and on `proofs13_agents_failed_send`; expected to fail)
- `app-gpui/src/workbench/tests/proofs13_agents_delete_failure.rs` (1 test, depends on hunk B)
- `app-gpui/src/workbench/tests/proofs13_agents_config.rs` (11 tests: 9 expected to pass, 2 expected to fail)
- `app-gpui/src/workbench/tests/proofs13_agents_new_task.rs` (1 test)
- `app-gpui/src/workbench/tests/proofs13_agents_pending.rs` (2 tests, expected to fail: missing views)
- `app-gpui/src/agents/session_view/tests/proofs13_agents_session.rs` (10 tests)
- `app-gpui/src/agents/session_view/tests/proofs13_agents_session_pending.rs` (2 tests, expected to fail)

## `mod` lines to add

In `app-gpui/src/workbench/mod.rs`, inside `mod tests {` (anchor: the line `    mod proofs11_vcs_windows;`), in sorted position after the proofs11/proofs12 lines:

```rust
    mod proofs13_agents_config;
    mod proofs13_agents_delete_failure;
    mod proofs13_agents_failed_send;
    mod proofs13_agents_fixtures;
    mod proofs13_agents_images;
    mod proofs13_agents_new_task;
    mod proofs13_agents_pending;
    mod proofs13_agents_stale_editor;
```

Dropping order if a hunk is refused: without hunk A drop `proofs13_agents_failed_send` and `proofs13_agents_stale_editor` together; without hunk B drop `proofs13_agents_delete_failure`. The fixtures stay warning-free either way.

In `app-gpui/src/agents/session_view.rs`, inside `mod tests {` (anchor: `    mod proofs11_session;`):

```rust
    mod proofs13_agents_session;
    mod proofs13_agents_session_pending;
```

The session tests reach private items of `SessionView` (`snapshot`, `geometry`, `scroll`, `question`, `open_subagent`, `RowKind`, `RowModel`, `QuestionState::{form, focus}`) and of the parent `mod tests` (`open`, `lines`, `click`, `bounds_of`, `effect_calls`, `long_conversation`, `scroll_up`, `clock`, `Events`); both are visible because the files are descendants of `session_view`. No accessor hunk is needed for them.

## Hunks the tests depend on

### A. A test seam that fails a send after a successful rollback (`app-gpui/src/workbench/agents.rs`)

Why: `ide_agent::Agents::prompt` (`crates/ide-agent/src/lib.rs:622-665`) returns `Ok` as soon as it has spawned the turn, so an agent answering an error to `session/prompt` only fails the turn, never the send. The only `Err`s of the send are "empty message" (prevented by `can_send`), "the task is archived" (Edit/Regenerate are not offered on an archived task: `session.rs:315-333`, `model.rs:255`) and a missing task. The existing `AgentSpawnSeam` refuses Rollback too (`starts_agent`, agents.rs:153-161), so it cannot make the rollback pass and the send fail. The seam below fails only `AgentCall::Prompt`, at the same place the webview spec mocks `agentPrompt`.

Inside `pub(super) mod spawn_guard {`, after `pub(in crate::workbench) const REFUSED: &str = "refused to start agent";`:

```rust
    /// Set by a test: while it holds an error, every message sent to an agent fails with it before reaching the
    /// backend, as a send the backend refused; `None` lets messages through again.
    #[derive(Clone, Default)]
    pub(in crate::workbench) struct FailingPrompts(pub(in crate::workbench) Option<String>);

    impl gpui_kit::Global for FailingPrompts {}
```

In `fn run_agent_call`, right after the `#[cfg(test)] let guarded = { … };` statement:

```rust
        #[cfg(test)]
        let failing = matches!(call, AgentCall::Prompt { .. })
            .then(|| cx.try_global::<spawn_guard::FailingPrompts>().and_then(|failing| failing.0.clone()))
            .flatten();
```

and inside `move |host, label| async move {`, before `#[cfg(test)] if let (Some(provider), seam) = guarded {`:

```rust
                #[cfg(test)]
                if let Some(error) = failing {
                    return Err(error);
                }
```

### B. The message of a dialog (`app-gpui/src/vcs/prompt_dialogs.rs`)

After `pub fn title(&self) -> &str { &self.title }`:

```rust
    /// The message under the title, when the dialog has one.
    #[cfg(test)]
    pub fn message_text(&self) -> Option<&str> {
        self.message.as_deref()
    }
```

(`message` is taken by the constructor `PromptDialog::message`, hence `message_text`.)

## Tests → rows

All rows are `unit:` keys. "done" means the spec file's last missing cases are covered; otherwise the row stays partial.

### `proofs13_agents_images.rs`

- `shows_the_images_and_lists_only_the_files_as_chips` → `unit:src/features/agents/messageEditing.test.tsx`: a message sent with `@README.md` and a PNG shows `[files: README.md] [images: 1]`, an image tile `session-user-0-image-0` and no second tile; the agent got the image data and the file. Partial for this case: the native tile shows the image's name and an icon, not the picture (`session_view.rs:1559-1579`); the webview draws `<img src=data:…>` and no name text.
- `keeps_the_images_when_editing_minus_the_removed_ones` → same row: Edit shows one Remove Image button per image; removing the first and sending reruns from a fresh session with only `BBB`. Covers the editing half of "opens an image in a dialog and keeps the images when editing, minus the removed ones".

### `proofs13_agents_failed_send.rs` (needs hunk A)

- `recovers_images_after_failed_regeneration_without_a_new_draft`, `…_with_a_new_draft` → messageEditing: Regenerate Response on reply 1, fresh session confirmed, the send fails, the composer gets `Original message` (+`\nAnother draft @README.md`), the image first then the draft's image, the draft's file kept, and the keyboard; Send then delivers text, both images and the file; the agent saw exactly two `session/new` and no `_openidea/rollback`. The draft is put in the composer before Regenerate (the seam fails at once, so there is no "while pending" window); `restore_composer` (store.rs:831-837) merges the same way either way.
- `recovers_the_edit_after_rollback_if_sending_fails_without_a_draft`, `…_with_another_draft` → messageEditing: as above for Edit + Send: after the failure the conversation is empty and no `session-edit-0` is drawn, the composer holds `Original message edited` (+`\nAnother draft`) and the keyboard; Send then reaches the agent (two `session/new`, two `session/prompt`).

### `proofs13_agents_stale_editor.rs` (needs hunk A; expected to fail)

- `shows_a_message_sent_again_after_a_failed_edit_as_sent_not_in_an_editor` → messageEditing (no case of its own; the native defect behind case 13, see "expected to fail"). Not cited as proof of a case.

### `proofs13_agents_delete_failure.rs` (needs hunk B)

- `continues_after_an_error_and_reports_the_failed_task` → `unit:src/features/agents/AgentsView.test.tsx`: with the project's `.git` moved away, deleting [worktree task, local task] with the checkbox ticked fails the first (`worktree::remove` → `repo_at(project_root)?` fails at once, `crates/ide-agent/src/worktree.rs:56-57`, before the 10×300 ms retry loop at 68), still deletes the second, and shows one "Cannot delete agent tasks" box whose body is `Worktree greeting: …` alone; the worktree task stays listed with its folder; with `.git` back it deletes and its folder goes. Row → done (19 of 19) if it passes.

### `proofs13_agents_config.rs` (docked composer of a real task; the agent takes `configOptions` from `agent-config.json` and logs `set_config_option`)

→ `unit:src/features/agents/ConfigPickers.test.tsx`, each through the composer chips (`composer-config-N`), the picker (`composer-picker-N`, `picker_entries`), effort steps (`composer-effort-N`) and `composer-allow-all`, the choice asserted in the agent's log:

- `sends_the_complete_nested_model_id_through_config_pickers` (entries `["9router", "Beam *"]`, sends `model=9router/cmc/Beam`)
- `shows_allow_all_as_a_switch_and_sends_on_off_values`
- `shows_allow_all_inside_the_mode_popup_when_both_options_exist`
- `shows_the_model_with_the_agents_effort_name_and_the_mode_with_allow_all`
- `keeps_a_plain_model_picker_when_the_agent_has_no_effort_option`
- `shows_approvals_separately_from_the_execution_mode_and_sends_the_choice` (an agent option `approvals`/category `approval`; the spec's id `openidea.cursor.approval` is never sent to the agent natively, `lib.rs:726`)
- `collects_several_providers_in_encounter_order_and_preserves_order_inside_each_group`
- `keeps_plain_ids_ungrouped_even_when_the_display_name_contains_a_slash`
- `leaves_non_model_option_labels_and_descriptions_intact`
- `steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup` (expected to fail)
- `filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard` (expected to fail)
  Row stays partial (see the case list).

### `proofs13_agents_new_task.rs`

- `disables_start_without_a_description_and_while_starting` → `unit:src/features/agents/composerCommands.test.tsx`: Start is off without text, on once typed; `enter enter` starts once (both keys are dispatched before the executor runs, `gpui-fast test_context.rs:623-634`), the seam's refusal shows `agents-home-error` and Start is on again. Row stays partial.

### `proofs13_agents_pending.rs` (expected to fail: views not built)

- `opens_an_image_of_a_sent_message_in_a_dialog` → messageEditing (the dialog half).
- `keeps_docked_changes_opt_in_preserves_the_draft_and_returns_to_the_same_conversation` → sessionState.

### `proofs13_agents_session.rs` → `unit:src/features/agents/sessionState.test.tsx`

- `shows_file_references_and_skills_as_distinct_tags`: the drawn user row's tokens are `@src/a.ts` File and `/triage` Command (they set the run colours, `session_view.rs:1501-1536`).
- `shows_the_whole_question_once_inline_with_every_answer_and_sends_the_chosen_one`: one permission row, heading "Allow tool call?", the whole question as its detail, three buttons with the agent's names; `session-permission-7-1` sends `b`.
- `keeps_separate_answers_for_codex_question_ids_with_identical_wording`: answer JSON `{"first":["One"],"second":["Two"]}` (compared parsed).
- `shows_the_requested_command_while_another_read_tool_is_pending_with_one_card_and_short_buttons`: one card, `codex wants to run a command`, the whole command, labels Allow Once / Always Allow / Decline / Decline and Stop, the long name as the second button's title, the running tool on its own row, no error row.
- `leaves_the_end_for_a_scroll_that_no_layout_caused_as_dragging_the_scrollbar_thumb`
- `keeps_following_the_end_when_the_offset_moves_back_without_the_user_scrolling`
- `builds_only_the_requested_tail_with_absolute_indexes`: with the pointer held (so no further batch is painted), 45 entries paint rows 5..44 only.
- `retains_scrolling_and_follow_output_when_moving_in_both_directions` (both `stick` values): editor host → docked host in a 600×300 window → editor host, new output in between.
- `keeps_task_and_subagent_scroll_positions_separate`: A 120, A/c1 70, B 340, A 120 (subagent opened through `open_subagent`, the card's click handler, because at offset 120 the card is scrolled out of the hit area).
- `restores_after_entries_load_and_clears_positions_on_project_disconnect` (likely to fail, see below).

### `proofs13_agents_session_pending.rs` (expected to fail)

- `defers_prepending_during_a_pointer_drag_and_preserves_the_first_rows_viewport_position` → sessionState.
- `picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape` → sessionState (the preview half; the number-key and Escape half passes today in `session_view::tests::picks_options_with_number_keys_and_skips_with_escape`).

## Per spec file: every case

### messageEditing.test.tsx (18 cases, parity counting: an `it.each` is one)

1. keeps the transcript and running session when a fresh-session rerun is cancelled → `proofs11_agents::keeps_the_transcript_and_running_session_when_a_fresh_session_rerun_is_cancelled`
2. stops before rollback and restores text/images without sending (turn %s) → `proofs11_agents::stops_before_rollback_and_restores_text_without_sending_turn_0/_1` (text only; images not asserted)
3. keeps the draft and avoids rollback when the agent does not stop in time → `proofs11_agents::keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time`
4. recovers the original prompt when rollback fails → `proofs11_agents::recovers_the_original_prompt_when_rollback_fails`
5. confirms before retracting a turn with later messages and guards repeated clicks → `proofs11_agents::confirms_before_retracting_…`
6. recovers images after failed regeneration (new draft: %s) → `proofs13_agents_failed_send::recovers_images_after_failed_regeneration_{without_a_new_draft,with_a_new_draft}` (hunk A)
7. shows the images and lists only the files as chips → `proofs13_agents_images::shows_the_images_and_lists_only_the_files_as_chips` (native draws a name tile, not the picture)
8. opens an image in a dialog and keeps the images when editing, minus the removed ones → editing half `proofs13_agents_images::keeps_the_images_when_editing_minus_the_removed_ones`; dialog half `proofs13_agents_pending::opens_an_image_of_a_sent_message_in_a_dialog` (expected to fail)
9. names the icon-only edit-and-resend action and disables empty edits → `session_view::tests::names_the_icon_only_edit_and_resend_action_and_disables_empty_edits`
10. keeps Enter, Shift+Enter and IME confirmation for editing → `session_view::tests::keeps_enter_shift_enter_and_ime_confirmation_for_editing`
11. keeps the draft and shows a rollback failure beside it → `session_view::tests::keeps_the_draft_and_shows_a_rollback_failure_beside_it`
12. rolls back before sending the edited text when Send is clicked → `proofs11_agents::rolls_back_before_sending_the_edited_text_when_send_is_clicked`
13. recovers the edit after rollback if sending fails (draft: %j) → `proofs13_agents_failed_send::recovers_the_edit_after_rollback_if_sending_fails_{without_a_draft,with_another_draft}` (hunk A)
14. still sends with modified Enter (%j) → `session_view::tests::still_sends_with_modified_enter`
15. waits for a running turn to stop before rolling it back → `proofs11_agents::waits_for_a_running_turn_to_stop_before_rolling_it_back`
16. %s restores the message and discards edits without touching the conversation → `session_view::tests::escape_and_cancel_restore_…`
17. keeps an accessible icon-only Send action with %s submission → `composer_view::tests::keeps_an_accessible_icon_only_send_action_with_{click,enter}_submission`
18. keeps Queue named and shows icon-only Stop actions while the agent works → `composer_view::tests::keeps_queue_named_…`

17 of 18 through the views once hunk A lands and the new tests pass (case 8's dialog half pending; case 7 shows a name tile, not the picture). Row stays partial until the image dialog exists. Without hunk A: 15 of 18.

### sessionState.test.tsx (31 cases)

1. keeps authors above messages and their folded work with matching icon-only actions → `session_view::tests::keeps_authors_above_…`
2. keeps tool, reasoning, plan and approval rows in one author run (%s) → `proofs11_session::keeps_tool_reasoning_plan_and_approval_rows_in_one_author_run`
3. starts another author run after subagent/message entries → `session_view::tests::starts_another_author_run_after_subagent_entries`
4. anchors every agent footer with its time before the hover actions → `session_view::tests::anchors_every_agent_footer_…`
5. builds only the requested tail, with absolute indexes → `proofs13_agents_session::builds_only_the_requested_tail_with_absolute_indexes`
6. folds reasoning and tool calls into one block → `session_view::tests::folds_reasoning_and_tool_calls_into_one_block`
7. is rendered for Find in Conversation, so its text is found → `session_view::tests::is_rendered_for_find_in_conversation_so_its_text_is_found`
8. shows file references and skills as distinct tags → `proofs13_agents_session::shows_file_references_and_skills_as_distinct_tags`
9. hides Jump to Latest and follows new replies after retracting the conversation → `session_view::tests::hides_jump_to_latest_…`
10. defers prepending during a pointer drag and preserves the first row's viewport position → `proofs13_agents_session_pending::defers_prepending_…` (expected to fail)
11. keeps following the end when the offset moves back without the user scrolling → `proofs13_agents_session::keeps_following_the_end_…`
12. leaves the end for a scroll that no layout caused, as dragging the scrollbar thumb → `proofs13_agents_session::leaves_the_end_…`
13. offers one click back to the latest message after scrolling up → `session_view::tests::offers_one_click_back_…`
14. keeps %s tools outside folds with their output expanded → `session_view::tests::keeps_status_tools_outside_folds_with_their_output_expanded`
15. folds consecutive finished tool calls into one collapsed row → `session_view::tests::folds_consecutive_finished_tool_calls_into_one_collapsed_row`
16. shows the whole question once inline, with every answer, and sends the chosen one → `proofs13_agents_session::shows_the_whole_question_once_inline_…`
17. keeps separate answers for Codex question ids with identical wording → `proofs13_agents_session::keeps_separate_answers_…`
18. shows every question with its descriptions in a card and sends all answers at once → `session_view::tests::shows_every_question_…`
19. picks options with number keys, previews the picked or hovered one, and skips with Escape → `session_view::tests::picks_options_with_number_keys_and_skips_with_escape` + `proofs13_agents_session_pending::picks_options_with_number_keys_previews_…` (hover half expected to fail)
20. shows the requested command while another read tool is pending, with one card and short buttons → `proofs13_agents_session::shows_the_requested_command_…`
21. sends the original $kind option id, then collapses the resolved card → `proofs11_session::sends_the_original_option_id_then_collapses_the_resolved_card`
22. is answered from its line too, and one without options can be cancelled → `session_view::tests::is_answered_from_its_line_too_…`
23. leaves a task unread while its kept-alive tab is hidden, and reads it once shown → `task_list_view::tests::proofs11_task_list::leaves_a_task_unread_…`
24. fetches a conversation once and shows it again from the store when its tab reopens → `proofs11_session::fetches_a_conversation_once_…`
25. opens a long conversation at its end and renders the earlier entries after it → `session_view::tests::opens_a_long_conversation_…`
26. returns to the task conversation when the docked task changes after opening a subagent → `session_view::tests::returns_to_the_task_conversation_…`
27. keeps docked changes opt-in, preserves the draft, and returns to the same conversation → `proofs13_agents_pending::keeps_docked_changes_opt_in_…` (expected to fail: no view)
28. retains scrolling and follow-output (%s) when moving in both directions → `proofs13_agents_session::retains_scrolling_and_follow_output_…`
29. keeps task and subagent scroll positions separate → `proofs13_agents_session::keeps_task_and_subagent_scroll_positions_separate`
30. restores after entries load and clears positions on project disconnect → `proofs13_agents_session::restores_after_entries_load_…` (likely to fail)
31. shows the provider's usage limits by default and each meter as Settings › Tools › Agents › Chat allows → `proofs11_session::shows_the_providers_usage_limits_…`

19 already through the views + 10 new = 29 of 31 if the new tests in `proofs13_agents_session.rs` pass (28 if case 30 fails as suspected). Left: 10 and 27 (pending tests), and the hover half of 19. Row stays partial.

### AgentsView.test.tsx (19 cases)

1. extends and shrinks a range without opening its lead or losing selection to an editor echo → `task_list_view::tests::extends_and_shrinks_a_range_…`
2. keeps the initial active task as the anchor for repeated keyboard extension → `task_list_view::tests::keeps_the_initial_active_task_as_the_anchor_…`
3. toggles individual tasks with %s → `task_list_view::tests::toggles_individual_tasks_with_the_command_key`
4. opens session history on demand and closes it after switching tasks → `task_list_view::tests::opens_session_history_on_demand_…`
5. lets keyboard users browse history before confirming a session → `task_list_view::tests::lets_keyboard_users_browse_history_…`
6. includes the current session in history and keeps the conversation mounted → `proofs11_task_list::includes_the_current_session_in_history_…`
7. gives the whole tool window to the conversation when the docked task is the only one → `proofs11_task_list::gives_the_whole_tool_window_…`
8. switches placement from the global Agents header → `task_list_view::tests::switches_placement_from_the_agents_header`
9. opens tabs as usual without a docked conversation → `proofs11_task_list::opens_tabs_as_usual_without_a_docked_conversation`
10. deletes the right-clicked selection after one confirmation, excluding group headers → `proofs11_agents::deletes_the_right_clicked_selection_…`
11. right-clicking outside the selection targets only that task → `task_list_view::tests::right_clicking_outside_the_selection_targets_only_that_task`
12. archives only eligible tasks in a mixed selection → `proofs11_agents::archives_only_eligible_tasks_in_a_mixed_selection`
13. unarchives only archived tasks in a mixed selection → `proofs11_agents::unarchives_only_archived_tasks_in_a_mixed_selection`
14. cancels the whole batch → `proofs11_agents::cancels_the_whole_batch`
15. applies worktree cleanup (%s) only to worktree tasks → `proofs11_agents::applies_worktree_cleanup_only_to_worktree_tasks`
16. continues after an error and reports the failed task → `proofs13_agents_delete_failure::continues_after_an_error_and_reports_the_failed_task` (hunk B)
17. keeps placement out of session menus (docked: %s) → `proofs11_task_list::keeps_placement_out_of_session_menus`
18. docks a session editor tab dropped on the tool window and highlights while it hovers → `task_list_view::tests::docks_a_session_editor_tab_dropped_…`
19. ignores editor tab drags that are not agent sessions → `task_list_view::tests::ignores_editor_tab_drags_that_are_not_agent_sessions`

19 of 19 → done if case 16 passes.

### ConfigPickers.test.tsx (13 cases): the view exists (the session composer)

Evidence: chips `composer_view.rs:86-126` and 609-627, picker `354-417` / `491-589`, effort steps `532-553`, Allow All `554-568`.

1. groups nested paths by their first provider and preserves the full value and description → partial: section and label through `sends_the_complete_nested_model_id_…` (`["9router", "Beam *"]`, full value sent); the detail `cmc/Beam · Fast` is not drawn (picker rows draw the label only, `composer_view.rs:516-529`). Not written separately.
2. keeps plain ids ungrouped even when the display name contains a slash → `keeps_plain_ids_ungrouped_…` (detail `Offline` not drawn natively)
3. collects several providers in encounter order and preserves order inside each group → `collects_several_providers_…`
4. leaves non-model option labels and descriptions intact → `leaves_non_model_option_labels_…` (description not drawn natively)
5. shows Cursor approvals separately from its execution mode and sends the choice → `shows_approvals_separately_…`
6. shows Allow All as a switch and sends on/off values → `shows_allow_all_as_a_switch_…`
7. shows Allow All inside the mode popup when both options exist → `shows_allow_all_inside_the_mode_popup_…`
8. sends the complete nested model id through ConfigPickers → `sends_the_complete_nested_model_id_…`
9. edits secondary settings from the compact popup and restores focus when dismissed → not written: **missing control** (below)
10. shows the model with the agent's effort name and the mode with Allow All → `shows_the_model_with_the_agents_effort_name_…`
11. steps through the advertised effort levels and picks a model from the popup → `steps_through_the_advertised_effort_levels_…` (expected to fail)
12. filters the model list and picks the highlighted model with the keyboard → `filters_the_model_list_…` (expected to fail)
13. keeps a plain model picker when the agent has no effort option → `keeps_a_plain_model_picker_…`

7 of 13 fully through the views if they pass (3, 5, 6, 7, 8, 10, 13); 2 and 4 pass but leave the description undrawn; 1 partial; 9, 11, 12 missing behaviour. Row stays partial.

### composerCommands.test.tsx (12 cases): New Task page exists (`workbench/agents.rs:1368-1530`: description field, agent chooser, Start, error), its composer features do not

1. reaches commands after the first twelve with the keyboard → missing: command popup
2. groups commands before skills, skips headers, wraps and hides empty groups → missing: command popup
3. names the icon-only Start action and disables it while starting → `proofs13_agents_new_task::disables_start_without_a_description_and_while_starting` (Start is a text button natively; no accessor for its "Starting…" label)
4. completes the project's skills merged with the agent's commands → missing: command popup
5. tags file references and skills where they stand → missing: the New Task field is a plain `TextField` (agents.rs:1385-1389) with no tag runs (the conversation's user rows have them: `session_view.rs:1501`)
6. offers nothing the project does not have → missing: command popup
7. completes at the caret without replacing the rest of the message → missing: command popup
8. completes a command typed after other text in place → missing: command popup
9. attaches the clipboard's image when WebKitGTK pastes an empty clipboardData → DOM/WebKitGTK only; the native counterpart (paste an image into the New Task page) is missing: no paste handling in `NewTaskPage` or `ComposerView`; model `composer::image_from_data_url` (composer.rs:504)
10. leaves a text paste to the text box → DOM-only distinction; no native counterpart beyond the text field's own paste
11. defaults to the active member, last used member, then first member → model only: `store::default_task_project` (store.rs:1351) via `AgentsStore::open_home(workspace)`; no view shows the project
12. sends the selected project with %s isolation → missing: project selector on the New Task page

1 of 12 through the views.

### HomeEditor.test.tsx (4 cases): view missing

All four need the model + effort chip on the New Task page, which draws none (agents.rs:1462-1529):

1. switches model-specific levels and restores each draft effort before starting → model only (`store::tests`)
2. hides effort for %s models and sends no stale effort → model only
3. sends the displayed remembered model and effort even when neither is changed → model only
4. ignores old provider-wide effort choices and refreshes unsupported per-model choices → model only
   0 of 4.

### agentRegistry.test.tsx (26 cases): view missing

Settings declares the page `tools.agents` (`settings_model/builtin_pages/features.rs:34`) but `settings_view/custom_controls.rs:59-140` builds no control for it, and `agents.addAgent` only opens that page (`workbench/agents.rs:328-335`). The New Task agent menu (`workbench/agents.rs:1300-1319`) lists provider names, disabled when not installed. Every case, with what it lacks:

1. matches every word of the query against name, id, description and authors → model only: `registry::matches_query` (registry.rs:31); its view is the Registry tab's search (missing)
2. offers install, update and remove by state → model only: `registry::row_actions` (registry.rs:50); view missing
3. lists installed agents, then well-known ones, then the rest by name → model only: `registry::sort_agents` (registry.rs:105); view missing
4. drops a removed agent this system cannot install again → model only: `registry::after_remove` (registry.rs:68); view missing
5. shows registry versions and updates, then Add Agent… → model only: `registry::provider_options` (registry.rs:131); missing in the New Task agent menu
6. shows the built-in Copilot name and icon in its registry row → missing: Registry tab
7. lists registry agents, says they are third-party, and searches them → missing: Registry tab
8. installs with progress, then reports the change → missing: Registry tab (`RegistryPage::install`/`install_event`/`installed`, registry.rs:338-410)
9. asks before installing an agent whose download has no checksum → missing: Registry tab (`RegistryPage::confirmed`, registry.rs:363)
10. installs an agent with a checksum without asking → missing: Registry tab
11. shows why an install failed, cancels one on Cancel, and leaves it running when the page closes → missing: Registry tab (`cancel_install` 411, `close` 474)
12. follows an install started before the page opened until it ends → missing: Registry tab (`poll`/`polled` 445-473, `BACKGROUND_POLL_MS`)
13. removes an installed agent → missing: Registry tab (`remove`/`removed` 415-444)
14. falls back to the saved list with a warning, and reports a registry it cannot load → missing: Registry tab (`warning` 283, `load_error` 288)
15. follows locale changes in tabs and renders login commands as inline code → missing: Built-in tab (`page_tabs` builtin_agents.rs:30, `login_hint` 62)
16. reports when providers cannot be read (no trusted project) → missing: Built-in tab (`BuiltinAgents::error` 195)
17. stages a binary path and writes it only on %s → missing: Built-in tab (`set_command` 274, `external_write` 91)
18. writes the agent's environment variables on Apply and shows them again → missing: Built-in tab (`set_env` 282, `parse_env`/`format_env` 35-60)
19. Cancel discards command edits without writing → missing: Built-in tab
20. %s stages automatic detection until Apply → missing: Built-in tab
21. page Reset discards edits and returning to the saved value clears the modified state → missing: Built-in tab (`modified` 305)
22. keeps edits across page navigation and applies with Agents mounted=%s → missing: Built-in tab
23. retains a failed command for retry and shows only the applied badge → missing: Built-in tab (`stored_value` 104)
24. lists built-ins and writes two independent edits on %s → missing: Built-in tab (`rows` 213)
25. keeps %s built-ins visible and read-only → missing: Built-in tab (`details` 229, `AgentCommandReadOnlyReason`)
26. stages a browsed executable → missing: Built-in tab (`browse`/`chosen` 290-304)
    0 of 26.

## Missing views and controls (specification for a later slice)

1. **Image dialog of a sent message** (webview `ImagePreview.tsx`, used by `SessionEditor.tsx`): clicking an image tile of a user row opens a modal showing the image full size (`image_src` already builds the `data:` URL, `message_editor.rs:117`) with a Close button; Escape closes. Native today: the tile has no click handler (`session_view.rs:1559-1579`) and draws the name, not the picture. Selectors the pending test expects: `session-image-dialog`, `session-image-dialog-close`.
2. **Docked changes panel** (webview `AgentsView.tsx` Show changes / Back to conversation, `Session.module.css` changes list): in the docked header a "Show changes"/"Hide changes" toggle (model: `session::ChangesOpen` `session.rs:415-445` (`new` starts closed when docked, `set` saves under `CHANGES_OPEN_KEY`), `session::changes_toggle` `session.rs:459`, `reads_conversation(shown, docked, changes_open)` at 445; `AgentsStore::changes` store.rs:510 and `changes_due`/`changes_loaded` 746-752). While open it replaces the conversation and composer by the changed files (path, +added/−removed), the task stays unread (`reads_conversation` false), and "Back to conversation" returns to the same `SessionView` and `ComposerView` entities with the draft. Docked, it starts closed whatever is saved. Selectors in the pending test: `agents-show-changes`, `agents-changes`, `agents-back-to-conversation`.
3. **Question card hover preview**: `QuestionForm::hover` exists (`cards.rs:328`) but `question_element` registers no hover handler (`session_view.rs:1957-1993`); each choice row needs `on_hover` → `form.hover(position, Some(choice))` / `None` on leave.
4. **Model picker search** (webview `ConfigPickers.tsx` "Search…" field, focused on open): a text field atop the model list feeding `OptionList::set_query` (`config_bar.rs:211`, used today only by `config_bar/tests.rs:150`); ↓/Enter already work (`composer_view.rs:398-417`).
5. **Picker stays open on choice** for the Model & effort popup: `entry_clicked` and the effort/Allow All buttons set `picker = None` (`composer_view.rs:389-395`, 542, 561); the webview keeps the popup open after an effort step and a model pick. Also no slider (max / value text), only steps.
6. **Picker entry detail**: draw `PickerEntry::detail` (`config_pickers.rs:10`) beside the label (`composer_view.rs:516-529`).
7. **Compact "Agent settings" popup** (webview `ConfigPickers.tsx` settings button + dialog): a button with `config_bar::settings_button` (icon, tooltip summary; `config_bar.rs:106-121`) opening the `ConfigBar::secondary` pickers (`config_bar.rs:58-79`); a choice keeps the popup open; Escape closes it and returns focus to the button. Not referenced by any view today.
8. **New Task page composer** (webview `HomeEditor.tsx`): the settings chips of the draft (`AgentsStore::draft_config` store.rs:786, `set_draft_option` 791, per-model thought levels in `Draft::thought_levels` store.rs:55) reusing `ComposerView`'s chip and picker code against the draft instead of a task; the slash-command popup (`composer::completion_at` composer.rs:93, `match_commands` 134, `apply_completion` 112, the agent's `AgentProvider::commands`), `@file` completion (`merge_matches` 191), tag runs in the field (`message_tokens` 53), image paste (`image_from_data_url` 504), and a project selector for workspaces (`default_task_project` store.rs:1351, `TaskProjects`).
9. **Agents settings page** (webview `AgentRegistryPage.tsx`, 342 lines, and `BuiltinAgents.tsx`, 244 lines): a custom page for `tools.agents` in `settings_view/custom_controls.rs`, with the two tabs of `builtin_agents::page_tabs` (builtin_agents.rs:30):
   - **ACP Registry** tab, driving `registry::RegistryPage` (registry.rs:227; it does no I/O, its `RegistryEffect`s go to the backend's registry list/install/remove calls with a `RequestId`): a "Search the ACP Registry" field filtering with `matches_query`; a list "Registry agents", one row per agent in `sort_agents` order with icon, name, version, distribution (`distribution_label`: Binary / npm / uv), description, authors, badges "No checksum provided" and "Installed", Repository and Website links, and the row's Install / Update / Remove (`row_actions`) with "Removing…" while removing; an install shows a progress bar ("Installing <name>", from `install_event`) and Cancel; an install without a checksum asks first (`ConfirmRequest`, OK "Install"); a load failure or the saved-list fallback shows `load_error` / `warning`; installs started elsewhere are followed by `poll` every `BACKGROUND_POLL_MS`; closing the page leaves installs running (`close`).
   - **Built-in** tab, driving `builtin_agents::BuiltinAgents` (rows of the built-in agents; selecting one shows its details): "Found on PATH" / "Not found on PATH" / "Custom executable", the launch command in code style, an executable field with placeholder "Auto-detect (<default program>)", Reset and Browse… (`browse`/`chosen`), an environment field "NAME=value; OTHER=value" (`parse_env`/`format_env`), the login hint with commands as inline code (`login_hint`), read-only agents per `AgentCommandReadOnlyReason`; edits are staged with `SettingsDialog::stage_external` and written on Apply/OK through `external_write` / `stored_value`, with the modified mark of `modified`.
   - In the New Task agent menu: the versions and update marks of `registry::provider_options` (registry.rs:131) and a last row "Add Agent…" running `agents.addAgent`.

## Expected to fail (behaviour the native app lacks)

- `proofs13_agents_stale_editor::shows_a_message_sent_again_after_a_failed_edit_as_sent_not_in_an_editor`: after the rollback removed the edited message, `edit_sent(Err)` keeps the editor (`session_view.rs:769-778`, called from `workbench/agents.rs:847-857`) with `index == 0`; when the resent message lands at entry 0 it is drawn as that stale editor. Fix: close the editor when its entry no longer exists (in `snapshot`/`render`, `editing.index >= entries.len()` → `close_editor`), or let `edit_sent` close it when the failure came after the rollback.
- `proofs13_agents_config::steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup`: the picker closes on an effort step (composer_view.rs:542). Fix: item 5.
- `proofs13_agents_config::filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard`: no search field. Fix: item 4.
- `proofs13_agents_pending::opens_an_image_of_a_sent_message_in_a_dialog`: item 1.
- `proofs13_agents_pending::keeps_docked_changes_opt_in_…`: item 2.
- `proofs13_agents_session_pending::picks_options_with_number_keys_previews_the_picked_or_hovered_one_…`: item 3.
- `proofs13_agents_session_pending::defers_prepending_during_a_pointer_drag_…` (hypothesis): after the drag, `due` stores `anchor_row` and `Follow::anchor`, but the next `sync_scroll` measures `row_top(anchor)` at the start of the render that first paints the prepended rows, from the previous layout, so the shift `now_top - from` is 0 and the text moves down by the height of the prepended rows. Fix: measure the anchor row after the frame that drew the prepended rows (keep `anchor_row` until `self.drawn` reflects the new `painted`), or anchor on `bounds_for_item` of the new position.

## Likely to fail on first run, and why

- `restores_after_entries_load_and_clears_positions_on_project_disconnect` (hypothesis): the first layout comes with no entries; `Follow::laid_out` then sees `len == 0 && g.at_end()` and resets the position to `{ top: 0, stick: true }`, saving it (`follow.rs:202-206`), which overwrites the kept 230 before the `!restored` branch (210-214) is reached; once entries load the view follows the end. If so it is a real bug. Fix: while nothing has been restored yet and there are no entries, leave the position and `restored` alone, e.g. at the top of the `visible` branch `if len == 0 && !self.restored { self.previous_len = 0; return effects; }`.
- `retains_scrolling_and_follow_output_when_moving_in_both_directions`: relies on `window.remove_window()` releasing the `SessionView` (the test drops its handle first) so `closed()` saves the position, on `gpui_kit::Size` being exported, and on `simulate_resize`. For `stick = true` it scrolls up then down 10 000 px with the wheel to put the user at the end.
- The scroll tests set the offset through the view's private `ScrollHandle`; GPUI may clamp it differently from what `geometry()` computed (tolerances are 1 px, `AT_END_PX` is 40).
- `continues_after_an_error_and_reports_the_failed_task`: assumes `Repo::discover` on the project root with `.git` renamed finds no repository above the temp dir; if a parent of `$TMPDIR` is a repository, `remove_worktree` runs against it and the error path changes.
- The config tests assume a fresh test home starts the first task with no remembered settings (so no `set_config_option` at session start); if the backend applies remembered choices, `agent_settings` would hold extra lines and the `assert_eq!(agent_settings(…), [...])` lines fail (the `wait_for_setting` lines still hold).
- `disables_start_without_a_description_and_while_starting`: needs the page's field to take `simulate_input` after `window.focus(page)`; and the second Enter to be ignored because both keys are dispatched before the executor runs.
- `shows_the_whole_question_once_inline_…`: the native detail keeps the line break; the test compares with line breaks as spaces.

## Behaviours found that differ from the webview

- A failed send after a rollback leaves a stale inline editor (above).
- The composer's pickers close after each choice, have no search, show no detail text, and there is no compact Agent settings popup.
- Images of a sent message are tiles with their name, not thumbnails, and do not open.
- The question card does not preview on hover.
- Probably: a kept scroll offset is lost when the view opens before the entries load; prepending after a drag does not keep the first row in place.
