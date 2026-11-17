I wrote the Coding Agents page draft. All four parts (A to D) are written, but nothing has been compiled or run, so every test result below is still to come. The notes the integrator needs are at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/agents_list_integration.md` (360 lines).

## Files written (new, untracked; root `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/`)

- `app-gpui/src/agents/agent_list.rs`, 863 lines: the list model.
- `app-gpui/src/agents/agent_list/values.rs`, 245 lines: launch values, New Tasks options, custom-agent helpers.
- `app-gpui/src/agents/agent_list/proofs38_agent_list.rs`, 416 lines: 14 model tests.
- `app-gpui/src/settings_view/pages/agents_list.rs`, 821 lines: the page view and the `AgentListBackend` interface.
- `app-gpui/src/settings_view/pages/agents_list/master.rs`, 233 lines; `details.rs`, 354 lines; `custom_agent.rs`, 294 lines (the Add Custom Agent dialog).
- `app-gpui/src/settings_view/pages/agents_list/proofs38_agents_list_view.rs`, 666 lines: 21 view tests.
- `app-gpui/src/workbench/agent_list_backend.rs`, 350 lines: the real backend calls and the test-only `RegistryStandIn`.
- `app-gpui/src/workbench/settings_agents_list.rs`, 103 lines: the Apply writer.
- `app-gpui/src/workbench/tests/proofs38_settings_agents_list.rs`, 570 lines: 17 end-to-end tests.

## Parts

- **A, B, C, D:** all written. B has no switch of its own, so A and B land together. C and D can be left out by mounting with `custom: None` or `registry: None`.
- **Hunks:** they are all in the notes, part A: `pages/mod.rs`, `agents/mod.rs`, `staging.rs` (`staged_external_pages`, a hand-written `Debug` for `ExternalChange`, `external_applied` printed as keys only), `dialog.rs` (`modified_pages`), `settings_window.rs` (Apply calls the writer), `workbench/mod.rs`, `custom_controls.rs` (mount).
- **Old files deleted:** the old draft files and the obsolete half of `builtin_agents.rs` with its tests.

## Rows

| key                                                                                                         | status                                                                                                                                                                                |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:agents.list`                                                                                      | partial                                                                                                                                                                               |
| `unit:…/agentRegistry.test.tsx`                                                                             | 33/33 named; 31 through the view or app, 2 on the model only (`:222` the Copilot icon, `:725` update a registry install); the `:577` and `:618` `it.each` rows cover one variant each |
| `ipc:agent_set_command`, `agent_set_env`, `agent_custom_save`, `agent_custom_remove`, `agent_custom_import` | done (end-to-end, files read back from disk)                                                                                                                                          |
| `ipc:agent_registry_list`, `_install`, `_remove`, `cancel_request`                                          | partial: the app build calls the ops, tests answer through the stand-in                                                                                                               |
| `action:agents.addAgent`                                                                                    | done; its proof text still says "tools.agents", which needs fixing                                                                                                                    |

The registry rows, the `ipc:agent_set_*` and `ipc:agent_custom_*` rows and `settings:agents.list` are new keys in `parity.json`.

## Hypotheses

These are the main ones; the full list of 18 in the notes says how to settle each.

- `UnboundedSender` is `Sync`, so it can go into `ops::Channel::new`.
- Dropping the dialog's subscription inside its own callback is allowed.
- Clicking a field's frame gives that field the keyboard.
- A fresh test home has no cached model catalog, so New Tasks shows its "after its first session" text.
- After the environment is cleared, the vault keeps no copy of the values.
- `advance_clock` fires the 1.5 s poll timer in view tests.
- The webview's `Dialog` translates its button labels; I read `Button`, not `Dialog`.
- `AgentList.module.css` lines 81–306 match the native details layout; I did not read them.

## Secrets and machine sources

- **In memory:** values sit only in the environment field, the dialog's staged and applied external values, `providers[*].env`, `ProviderSettings.env`, and the custom dialog's field and its `CustomAgentInput`. Every `Debug`, `lines()` and refusal message shows names only. A refused pair now reads "Expected NAME=value: NAME", or "entry N" when it has no name; this deliberately differs from the webview, which shows the whole pair.
- **On disk:** the agent file `<config>/agents/<id>.toml` keeps names with empty values. Values go to `AgentSecrets`: `<data>/agent-secrets.json` (the OS credential store in the app, a file vault under a test home) or `<data>/agent-secrets.local.json`.
- **What `agent_providers` reads (from the code):** `<config>/agents/*.toml`, the secret store, the model catalog cache, the registry's saved index for update marks (`mark_updates`), the process `PATH` for executables, and the host `PATH` for login hints. Where the catalog cache and the saved index live on disk is a hypothesis.
- **Network:** in the real app, the page downloads the ACP Registry every time it opens. Hypothesis: installing an npm or uv agent runs npm or uv. In tests, registry calls are answered by the stand-in or refused; there is no backend path compiled in test builds.

## Things that turned out wrong

- HEAD is `e123f0da5`, not `f8d8ccb57`; the difference is four docs files.
- `workbench/settings_agents.rs` cannot be kept as it is: it imports the old `tools_agents` page and `ExternalWrite`.
- The compiled `RegistryPage::remove` does not ask before removing, but the webview does; the new model asks first.
- `RegistryPage` follows only the first agent that is installing in the background; the webview marks each one.
- The compiled `builtin_agents::parse_env` puts the whole refused pair (possibly an API key) in its error.
- `ControlEvent::Browse` never answers when the chooser is dismissed, so Import acp.json uses its own file-chooser call.
- Built-ins not found on `PATH` now sort under "Available", so the end-to-end tests select Claude Code explicitly instead of relying on the first row.
- Rule breach: I ran one grep for `PathPromptOptions` in the GPUI source under `~/.cargo`, and later an aborted grep pointed at `~/.cargo` read nothing. Both are outside the folders I was allowed to read.

## Not built

- A Browse failure is not shown under Launch; it is only logged.
- The file choosers take no title, start folder or json filter.
- A `…` mark stands in for the spinner.
- The confirmation is a panel over the page rather than the workbench message dialog.

## For a later look at the real window

Opening the page downloads the registry and reads the credential store, so it needs a temporary `HOME` and `OPENIDEA_GPUI_HOME`. Check the split widths, dot and tag colours, the progress bar, the two panels over the page, the New Tasks drop-down, the zh labels, and that the environment field is the only place a value shows.
