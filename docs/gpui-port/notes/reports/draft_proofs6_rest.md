I drafted 12 tests in 7 new files, plus the notes file. Nothing has been compiled or run. One test is written to fail until a fix lands. I edited no existing file and spawned no sub-agents.

**Files, all under `app-gpui/src/`:**

| File                                                             | Tests                | Lines |
| ---------------------------------------------------------------- | -------------------- | ----- |
| `workbench/plugins_window/tests/proofs6_plugins.rs`              | 5                    | 154   |
| `workbench/plugins_window/tests/proofs6_plugins_market.rs`       | 2                    | 117   |
| `workbench/plugin_runtime/tests/proofs6_plugin_runtime.rs`       | 1                    | 26    |
| `workbench/tests/proofs6_workbench.rs`                           | 1                    | 25    |
| `workbench/tests/proofs6_attach.rs`                              | 1 (expected to fail) | 31    |
| `workbench/app_actions/tests/proofs6_app_actions.rs`             | 1                    | 29    |
| `settings_view/custom_controls/tests/proofs6_custom_controls.rs` | 1                    | 16    |

The notes give the 8 `mod` lines with their anchor lines, the proof text for each test and the full 54-command ledger: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs6_rest.md`. No test needs a new accessor.

**The 54 commands:**

- **12 become done** with a new test: `plugin_catalog`, `plugin_unpack_package`, `plugin_roll_back`, `plugin_set_capability`, `plugin_market`, `plugin_market_download`, `plugin_market_url`, `plugin_set_market_url`, `plugin_answer_command`, `recent_project_groups`, `vcs_init`, `term_available_shells`.
- **1 partial:** `plugin_grant` is proven only from the Plugins page; the second call site, a running plugin asking for consent, is not driven.
- **1 failing by design:** `workspace_attach`.
- **1 already proven** by an existing test: `plugin_list`. `vcs_line_hunks` and `vcs_stashes` stay partial, as the map already has them.
- **10 not issued natively:**
  - The five `gh_pr_*` read and review commands: both the native window and the webview go through `gh_api` instead.
  - `fs_rename`: its request is never constructed.
  - `docker_save_settings`: it comes only from dialogs the native window doesn't build.
  - The three `syntax_*` commands: they are only sent when a running language server fails Find Usages. With no server, the backend answers itself with text matches. I wrote two Find Usages tests, found this, and deleted them; the notes have a recipe for a test with a failing fake server.
- **27 left out:**
  - 5 have a hard blocker:
    - Docker snapshot and settings: loading them runs the machine's `docker`.
    - Terminal busy check: the test shell has no job control, so it never reads as busy.
    - Proxy password: it is read from the keychain.
    - MCP problems report: there is no way to read back what was reported.
  - 1 differs from the webview without a test: `plugin_answer_commit` (below).
  - 1 needs a fixture I didn't draft: `gh_pr_prepare_diff`.
  - 20 were not drafted for lack of time; their todos in `ipc_map.json` are the recipes.

**Differences from the webview:**

- **`workspace_attach`:** the webview attaches a folder in place when "Open project in" is set to Attach. Natively, that setting is ignored, the Attach button is removed from the question, and `project.attach` has no handler. `proofs6_attach.rs` follows the webview and will time out until the fix lands. The notes give the fix hunk for `app_models/windows.rs` and `app_shell.rs`. The reason the button was removed still stands: the Project view can't show a second root yet.
- **`plugin_answer_commit`:** natively every plugin commit request is answered "no repository", where the webview sends the user's selection. There is no small fix, so I wrote no test.

**Most likely to fail first, after the attach test:**

1. The two marketplace tests: they need the signed entry to count as compatible and as from a trusted publisher in a debug build, and they type the new URL with `cmd-a` then `simulate_input`.
2. `review_permissions_grants_a_plugin_waiting_for_consent`: it assumes a linked plugin starts out waiting for consent.
3. The plugin-runtime test: it assumes a failed action reaches the plugin as an error.
4. `new_project_with_git_creates_a_repository_in_the_new_folder`: the backend's `git init` reads the machine's git configuration. The existing New Project test avoided Git for that reason, so you may prefer to drop this one.

One correction: an early draft of the notes said an existing test already proved `plugin_answer_command`. It doesn't, because plugin questions are answered through a different host call, so I wrote the new plugin-runtime test for it.
