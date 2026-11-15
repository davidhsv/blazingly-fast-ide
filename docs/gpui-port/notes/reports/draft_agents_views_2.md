Item 2 (the New Task page) is written in full, but item 3 (Settings → Tools → Agents) was not reached: none of the 26 `agentRegistry.test.tsx` cases has a view of mine. Nothing was compiled or run, so every runtime claim below is a hypothesis. To reuse the pickers on the New Task page I moved them into a shared module, so the `composer_view.rs` hunks from my first report are superseded; the notes file now holds the new ones.

## 1. Files written

- **`app-gpui/src/agents/config_pickers_view.rs`** (891 lines): the settings chips, their pickers and the Agent settings popup, usable by any view. A view implements the `PickerHost` trait: where the settings come from, where a choice goes, where the keyboard returns, and whether popups open downwards. The functions the host calls are `chip_elements`, `picker_element`, `settings_element`, `chip_clicked`, `refresh`, `close_popups`, `settings_rows`, `settings_title` and `chip_texts`. A picker now also closes when its chip changes kind, for example a model that has no effort levels.
- **`app-gpui/src/agents/composer_view/pickers.rs`** (307 lines): `impl PickerHost for ComposerView`. The accessors keep their names and output: `chips`, `picker_entries`, `picker_details`, `picker_query`, `settings_title`, `settings_rows`, `settings_button_focused`. 8 view tests.
- **`app-gpui/src/agents/new_task_view.rs`** (833 lines) and **`new_task_view/tests.rs`** (457 lines, 17 tests): `NewTaskView`, which replaces `NewTaskPage`.
  - **Constructor:** `new(store, cx)`.
  - **Events:** `NewTaskEvent::{Start { project_root }, ChooseAgent(Point), Effects(Vec<AgentEffect>)}`.
  - **Accessors:** `command_rows`, `tags`, `images`, `chips`, `picker_entries`, `project_label`, `project_rows`, `start_label`, `can_start`, `error`, `finished`, `set_commands`, `set_projects`.
  - **What it does:** chips for the draft's settings (via `set_draft_option`), the `/` command popup (Commands before Skills, wrapping, Enter/Tab complete at the caret, Escape closes), file and command tags drawn in the field, image paste with a 5 MB warning, and the workspace project selector.
- **`app-gpui/src/workbench/agents_views.rs`** (136 lines): declared as a child of `workbench::agents` through `#[path]`, so it can use that module's private store and backend runner.
  - When the page opens, it hands the page the workspace members, refreshes the agents and reads the agent's commands.
  - It handles the page's events and builds the agent menu from `registry::provider_options`.
- **`app-gpui/src/workbench/tests/proofs14_agents_views.rs`** (112 lines): 3 end-to-end tests with the fake agent.
- **Notes:** `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/agents_views_integration.md`. It has the `mod` lines and quoted hunks for:
  - `composer_view.rs`: the moved chip code is deleted.
  - `workbench/agents.rs`: `NewTaskPage` and its event handler are deleted, the page is created as `NewTaskView`, and unused imports are dropped.
  - `ui/text_field.rs`: a new `TextTag` type and `set_tags`, painted as text runs.
  - `cards.rs`: a `hovered()` getter.
  - `session_view.rs`: the hover handler for question answers.

## 2. Actions

None registered. The New Task page reaches the backend through these IPC commands:

- `agent_create`, for Start.
- `agent_providers`, when the page opens.
- `agent_commands(provider, None)`, when the page opens and when the agent changes. In test builds this call passes `spawn_guard::check` first, because for a Claude-driver agent it can start the agent's program.
- The "More agents…" menu row runs the existing action `agents.addAgent`.

## 3. Spec files (if the tests pass)

| Spec                        | Covered  | Notes                                               |
| --------------------------- | -------- | --------------------------------------------------- |
| `ConfigPickers.test.tsx`    | 13 of 13 | effort is step buttons, not a slider                |
| `HomeEditor.test.tsx`       | 4 of 4   | view tests                                          |
| `composerCommands.test.tsx` | 12 of 12 | case 11 by the existing store test of the same name |
| `sessionState.test.tsx`     | 29 of 31 | case 19 now complete; cases 10 and 27 left          |
| `agentRegistry.test.tsx`    | 0 of 26  | not reached                                         |

For item 3, the page `tools.agents` has no section to host a control. It first needs a `custom_owned("Agents", …)` section in `settings_model/builtin_pages/features.rs` and a `slots.put` in `custom_controls.rs`. The `RegistryPage` and `BuiltinAgents` models are ready for it.

Of the waiting tests:

- **`proofs13_agents_session_pending.rs`:** `picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape` should pass with the `cards.rs` and `session_view.rs` hunks. Move that one test into `proofs13_agents_session.rs`; `defers_prepending_during_a_pointer_drag_…` stays pending.
- **`proofs13_agents_config_pending.rs`:** both tests should pass with the composer hunks; declare it.
- **`proofs13_agents_pending.rs`:** stays undeclared, because the image dialog and the docked changes panel are not built.

## 4. Not built, assumptions, likely failures

- **Not built:**
  - On the New Task page: the isolation and base-branch selectors (a project without git is not forced to the local checkout), `@file` completion, history recall with ↑/↓, Shift+Tab mode cycling, file drop, and the agent probe line.
  - Elsewhere: the image dialog, the docked changes panel, prepending during a scrollbar drag, the status-bar unread item and tool-window badge, and Markdown images.
- **Check first when compiling:**
  - Hypothesis: `gpui_kit` re-exports GPUI's `ClipboardEntry`, `Image` and `ImageFormat`.
  - Hypothesis: `.top(relative(1.0))` accepts a relative length.
  - Hypothesis: the guarded `match` in `config_pickers_view::refresh` passes the borrow checker.
  - Hypothesis: `prelude::*` is still used in `agents.rs` after the deletion; if rustc calls it unused, drop it.
- **Most likely to fail on first run:**
  - Hypothesis: `proofs14::completes_the_commands_the_agent_offered_in_the_project` fails if the backend does not remember a session's `available_commands_update` as the project's commands.
  - Hypothesis: `proofs14::starts_the_next_task_with_the_model_picked_on_the_new_task_page` fails if the provider's `config` does not hold the first session's settings after the refresh.
  - The paste tests press `cmd-v`, so they assume a macOS runner.
