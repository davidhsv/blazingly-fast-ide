Only item 1 (the composer's pickers) is done. Items 2–4 were not reached, nothing was compiled, and 3 of the 6 waiting tests are still unmet.

## 1. Files written

- `app-gpui/src/agents/composer_view/pickers.rs` (about 940 lines). It is a child module of `composer_view` rather than its own view entity, because `picker_entries(&self)` takes no `cx` and the waiting tests call it with that signature. It adds `impl ComposerView` blocks plus a `Pickers` state struct.
  - Constructor: `Pickers::new(cx)`, called in `ComposerView::new`. It owns the search field and three key claims: list, search field, settings popup.
  - Events: unchanged. Choices still go out as `ComposerEvent::Effects` carrying `AgentCall::SetConfig`.
  - New accessors: `picker_details()`, `picker_query(cx)`, `settings_title(cx)`, `settings_rows(cx)`, `settings_button_focused(window)`.
  - Moved unchanged: `picker_entries()`.
  - Selectors kept: `composer-config-N`, `composer-picker-N`, `composer-effort-N`, `composer-allow-all`. New: `composer-picker-search`, `composer-effort-value`, `composer-settings`, `composer-settings-popup`, `composer-setting-N`, `composer-setting-N-M`.
  - What it does:
    - The Model & effort popup stays open after an effort step and after a model pick. A mode pick and a plain list still close on a choice; the Allow All switch keeps the mode popup open.
    - The model list has a search field that takes the keyboard when the popup opens; ↑/↓/Enter/Escape are claimed ahead of the IDE keymap.
    - Each entry shows its description, a check on the current value and its icon.
    - An open list is rebuilt when the agent reports changed settings.
    - The compact Agent settings popup keeps open after a choice; Escape gives the keyboard back to its button.
  - 8 view tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/agents_views_integration.md`.
  - Hunks in `composer_view.rs`: imports, `mod pickers;`, the field, its initial value, the store observer, releasing the claims, Escape in the message field, deleting the moved functions, and render.
  - A small `hovered()` getter in `cards.rs`.
  - An `on_hover` hunk in `session_view.rs` `question_element`, for the question card's hover preview.
  - The `mod` lines for the waiting test files.

No `workbench/agents_views.rs` was written: item 1 needs no wiring outside the composer.

## 2. Actions

None registered; item 1 adds no action. Choices go through the existing path to `agent::set_config`, which is the IPC command `agent_set_config` (I read this in `workbench/agents.rs` and `src-tauri/src/commands/agent.rs`). The New Task, Settings and status-bar items, which may need actions, were not reached.

## 3. Spec files (counts assume the tests pass)

- **`ConfigPickers.test.tsx`: 13 of 13.**
  - Cases 1, 2, 4 and 9 are covered by new view tests; 11 and 12 by the two waiting workbench tests plus view tests of the same names.
  - The effort control is step buttons, not a slider.
- **`sessionState.test.tsx`: case 19 is now complete through the hover hunk, but the count stays 29 of 31.** Cases 10 and 27 are left.
- **Unchanged:** `composerCommands.test.tsx` 1 of 12, `HomeEditor.test.tsx` 0 of 4, `agentRegistry.test.tsx` 0 of 26.

## 4. Not built, assumptions, likely failures

**Waiting tests:**

| Test                                                                         | Result                                                                                                                                           |
| ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `proofs13_agents_config_pending` (both)                                      | Hypothesis: pass with the `composer_view.rs` hunks. I checked that "sol" leaves only "GPT-6 Sol" under `fuzzy_match`.                            |
| `proofs13_agents_session_pending::picks_options_with_number_keys_previews_…` | Hypothesis: passes with the hover hunks. The row selector is 1-based (`choice.key`), so `session-question-0-1` is "Table" and the test is right. |
| `proofs13_agents_session_pending::defers_prepending_during_a_pointer_drag_…` | Not reached, still failing.                                                                                                                      |
| `proofs13_agents_pending::opens_an_image_of_a_sent_message_in_a_dialog`      | Not reached: no image dialog.                                                                                                                    |
| `proofs13_agents_pending::keeps_docked_changes_opt_in_…`                     | Not reached: no docked changes panel.                                                                                                            |

Declare `proofs13_agents_session_pending` only if one known failure is acceptable; keep `proofs13_agents_pending` undeclared.

**Not reached:**

- Item 2: New Task page chips, slash-command popup, tags, image paste, project selector.
- Item 3: Settings → Tools → Agents (Registry and Built-in tabs).
- Item 4: unread status item, tool-window badge, Markdown images.

Reusing the pickers on the New Task page means making `Pickers` take its config from either the task or the draft (`draft_config` / `set_draft_option`).

**Differences from the webview:**

- Hovering a list entry does not move the highlight, and there is no type-ahead.
- Closing a picker gives focus to the message field, not the chip.
- The Agent settings popup lists a setting's values under its row instead of opening a nested dropdown.

**Existing tests checked against the new close-on-choice rules** (hypothesis: they survive):

- `proofs6_agents::picking_a_setting_in_the_composer_…`
- `proofs13_agents_config.rs`
- `composer_view::tests::opens_a_setting_chip_and_sends_the_chosen_value`

**Hypotheses the integrator should check first:**

- `Pickers::new` works when called inside the `Self { … }` literal of `ComposerView::new`.
- `refresh_pickers` uses a by-move binding in a guarded match arm; if the borrow checker refuses it, bind by reference and clone.

**Most likely to fail on first run (hypotheses):**

- **The two `filters_the_model_list_…` tests:** `simulate_input` must reach the search field drawn in the deferred popup in the same frame its focus was set.
- **`edits_secondary_settings_…`:** Escape relies on the popup's focus handle still having focus after clicks on its rows.
- **The view tests in general:** the popups open upwards, so the tests put the composer at the bottom of the window with an `AtBottom` wrapper.
