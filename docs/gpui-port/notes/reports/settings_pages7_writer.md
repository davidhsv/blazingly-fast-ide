The Built-in tab of Tools › Agents is written in full, but nothing has been compiled or run. The ACP Registry tab is not started and not drawn. Because of that, the page has no network, install or remove path, and the `agent_registry_*` rows are untouched.

**Decision for you first.** The brief says no secret is ever drawn. The webview draws each agent's environment variables in a plain text field (`BuiltinAgents.tsx:186-193`), and those can hold API keys. I matched the webview. The strict alternative is a masked field, which breaks editing the pairs.

- The values are stored as plain TOML in `<config>/agents/<id>.toml`, not in the keychain (`provider.rs:228-230, 323-331`).
- A refused environment also repeats the bad pair in the dialog footer ("Expected NAME=value: …").
- They are kept out of my text accessor, events and logs, and a view test checks that a typed key never appears in the page's text.
- So nobody should screenshot this page over your real home: it opens with Claude Code selected and would show its real `[env]` values.

**Files written** (new, untracked):

- `app-gpui/src/settings_view/pages/tools_agents.rs` (727 lines): the page view.
- `app-gpui/src/settings_view/pages/tools_agents/proofs36_agents_view.rs` (342): 12 view tests.
- `app-gpui/src/workbench/settings_agents.rs` (102): the Apply writer, run on the backend's runtime.
- `app-gpui/src/workbench/tests/proofs36_settings_agents.rs` (381): 12 end-to-end tests.
- `$S/drafts/settings_pages7_integration.md` (about 280): hunks, rows, behaviours, hypotheses.

The notes list 7 hunks to existing files: `pages/mod.rs`, `staging.rs`, `dialog.rs`, `custom_controls.rs`, `workbench/mod.rs` (two lines) and `settings_window.rs`. Another agent changed several of those files while I worked; I re-checked every quoted anchor afterwards and all are still there.

**Rows:**

| key                                                | expected          | proof / missing                                                                                                        |
| -------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `settings:tools.agents` (new key)                  | partial           | the 12 `workbench::tests::proofs36_settings_agents::*` tests; missing: the whole ACP Registry tab                      |
| `unit:src/features/agents/agentRegistry.test.tsx`  | partial, 25 of 26 | every case is mapped to its test in the notes; missing: "shows the built-in Copilot name and icon in its registry row" |
| `ipc:agent_set_command`                            | done              | `stages_a_binary_path_and_writes_it_only_on_apply_or_ok` reads `<config>/agents/claude.toml` from disk                 |
| `ipc:agent_set_env`                                | done              | `writes_the_agents_environment_variables_on_apply_and_shows_them_again`                                                |
| `ipc:agent_registry_list` / `_install` / `_remove` | not started       | —                                                                                                                      |

The `src/` webview code calls `agentSetCommand` and `agentSetEnv` only from `registry.ts:48-53`, so those two rows belong to this page alone. Two caveats:

- Four of the existing model test names write the spec's "%s" as a word, and one drops "(no trusted project)" (a new view test and an end-to-end test carry the full name). The notes leave you to decide whether that counts.
- The 8 registry cases are proved only on the model, because the registry view does not exist.

**Hypotheses** (read, not compiled or run):

- Hypothesis: `use super::*` in the view-test file brings in the parent file's private imports.
- Hypothesis: `update_default_global` borrows cleanly next to `self` in the view.
- Hypothesis: the agent files are written as `command = "<path>"` and `KEY = "value"`, which is what the end-to-end tests compare against.
- Hypothesis: an unreadable `claude.toml` is skipped when listing and makes the write fail, which is how the "retains a failed command" test gets a real failure.
- Hypothesis: `cmd-a` then `backspace` empties a text field in tests.
- Hypothesis: Up and End reach the agent list after a row click.
- Hypothesis: opening a project writes no `<config>/agents` folder.

**What the page touches in the real app:** showing it calls `agent_providers`. That reads `<config>/agents/*.toml` and the saved registry index `<local data>/agents/registry.json`. It also checks whether each built-in command is an executable in a `PATH` directory. It starts no program, opens no network connection and writes nothing. Apply writes `<config>/agents/<id>.toml`; Browse… opens the system file chooser. Test builds use the real listing too, since it only reads files under the test's temporary home.

**What turned out wrong or missing in the existing code:**

- Apply refused every value kept outside the settings with "This window cannot do this" (`settings_window.rs:333-338`). The existing Built-in model could never be applied in a window; hunk 7 fixes it.
- The dialog's modified-pages list ignores those values (`dialog.rs:647-654`), so its Reset button never appeared on this page. The webview counts them (`SettingsDialog.tsx:239`); hunks 2 and 3 fix it.
- Several existing types print an agent's environment, so its keys, through `Debug`:
  - `ExternalWrite` and `Details` (`builtin_agents.rs:81, 126`);
  - `ExternalChange` (`staging.rs:99`);
  - the dialog's own debug output (`staging.rs:251-252`);
  - the Apply run's debug output (`staging.rs:604`).
- In app-gpui's tests, `cfg!(test)` inside the backend registry crate is false. A future Registry tab that called `registry_list` in a test would fetch the real registry over HTTPS, and an install would run `npm` or `uv`. Any test gate for it must live in app-gpui.
- The brief's GPUI source path (`gpui-pre-0.3.12`) is correct.

Smaller differences from the webview: the file chooser takes no title or start folder, the loading spinner is drawn as a muted "…", and a chooser failure is only logged.
