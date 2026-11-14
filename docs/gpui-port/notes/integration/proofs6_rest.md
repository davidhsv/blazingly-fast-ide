# proofs6_rest — IPC commands the native app issues (all but debugger, database, agents)

Scope: the 54 `ipc_map.json` commands with verdict `needs_assertion`/`needs_test` whose first native call site is not in `debugger/driver.rs`, `workbench/database.rs` or `workbench/agents.rs`. Nothing was compiled or run (no cargo). Every statement about runtime behaviour below is a hypothesis until the integrator runs the tests; statements about code are from reading it in this session.

## Files written (all new)

| file                                                                          | tests                                                | lines |
| ----------------------------------------------------------------------------- | ---------------------------------------------------- | ----- |
| `app-gpui/src/workbench/plugins_window/tests/proofs6_plugins.rs`              | 5                                                    | 154   |
| `app-gpui/src/workbench/plugins_window/tests/proofs6_plugins_market.rs`       | 2                                                    | 117   |
| `app-gpui/src/workbench/plugin_runtime/tests/proofs6_plugin_runtime.rs`       | 1                                                    | 26    |
| `app-gpui/src/workbench/tests/proofs6_workbench.rs`                           | 1                                                    | 25    |
| `app-gpui/src/workbench/tests/proofs6_attach.rs`                              | 1 (expected to FAIL, see "Differs from the webview") | 31    |
| `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs`             | 1                                                    | 29    |
| `app-gpui/src/settings_view/custom_controls/tests/proofs6_custom_controls.rs` | 1                                                    | 16    |

The market tests are in their own file so that an error in the `IndexEntry` literal cannot take the other plugin tests down; each file depends only on its parent `mod tests`.

## mod lines to add

1. `app-gpui/src/workbench/plugins_window.rs`, inside `mod tests {`, directly before the anchor `    const AI: &str = "openidea.ai-completion";` (line ~1058):
   ```rust
       mod proofs6_plugins;
       mod proofs6_plugins_market;
   ```
2. `app-gpui/src/workbench/plugin_runtime.rs`, inside `mod tests {`, directly before the anchor `    const LABEL: &str = "w1";` (line ~1572): `    mod proofs6_plugin_runtime;`
3. `app-gpui/src/workbench/mod.rs`, inside `mod tests {`, after the anchor `    mod proofs5_run;` (line ~4077):
   ```rust
       mod proofs6_attach;
       mod proofs6_workbench;
   ```
4. `app-gpui/src/workbench/app_actions.rs`, inside `mod tests {`, after the anchor `    use std::cell::Cell;` (line ~1154): `    mod proofs6_app_actions;`
5. `app-gpui/src/settings_view/custom_controls.rs`, inside `mod tests {`, after the anchor `    use crate::workbench::Workbench;` (line ~407): `    mod proofs6_custom_controls;`

No accessor hunk is needed: every accessor used exists (read in this session): `plugins_repository_prompt`, `plugins_consent_dialog`, `welcome_plugins_page`, `PluginsPage::{rows, details, gear_items}`, `PromptDialog::field`, `ConsentDialog::request`, `PluginHost::{package_files_of, set_market_url, market_url, link, install_from_dir, new}`, `ide_extension::package::{pack, package_digest, PublisherKey, Index, IndexEntry, Publisher, KeyEntry, INDEX_FORMAT}`, `Welcome::lines`, `RecentProjects::{load, touch, set_groups}`, `ShellPathView::shells`, `new_project_dialog`, `trust_dialog`, `app_shell::open_project_with_policy` (pub(super), reachable from `workbench::tests::*`), `ide_workspace::workspace::Workspace::load`, the plugin_runtime tests' `project`/`run_in_window`/`wait`/`LABEL`.

## Per test → rows

### proofs6_plugins.rs (child of `workbench::plugins_window::tests`)

- `the_marketplace_tab_lists_the_plugins_of_the_local_catalog` → `ipc:plugin_catalog` — done. Proof: the Welcome Plugins page's Marketplace tab lists a `Catalog` item "String Tools", which comes only from `PluginHost::catalog(&catalog_dirs())` (`read_plugin_catalog`, the adapter's `plugins.catalog`); `catalog_dirs()` is the checked-in `plugins/official` in debug builds, so no folder needs configuring; nothing is installed by listing.
- `installing_a_package_file_from_disk_unpacks_it_and_installs_its_plugin` → `ipc:plugin_unpack_package` — done. Proof: `plugins.installFromDisk` given an unsigned `.oiplugin` (packed from fixture `hello` with `ide_extension::package::pack`) takes `InstallStep::Unpack` → `host.unpack_package` (`package_source` sends `.oiplugin`/`.zip` there); the consent page, when it shows, names `hello`; the host then lists `hello` installed inside its own plugins folder.
- `roll_back_returns_an_updated_plugin_to_its_previous_version` → `ipc:plugin_roll_back` — done. Proof: string-tools installed as 9.0.0 then 9.1.0 (previous 9.0.0); Roll Back on its details makes the host list 9.0.0 (`host.roll_back_update` via `PluginsPageEvent::RollBack`), the window notifies "Rolled back …" and the details show 9.0.0.
- `a_permissions_row_revokes_and_allows_the_permission_on_the_host` → `ipc:plugin_set_capability` — done. Proof: ai-completion installed with all optional permissions; the Permissions row's `plugins-capability-net:localhost` makes the host's `CapabilityInfo.granted` false, a second click true (`host.set_capability`). The todo's "a plugin call needing it fails" is not asserted; the host state can only come from `set_capability`.
- `review_permissions_grants_a_plugin_waiting_for_consent` → `ipc:plugin_grant` — partial: done for the Plugins page site (`plugins_window.rs:446`). Proof: a linked `hello` is `NeedsConsent`; Review Permissions (consent page accepted if shown) and the host no longer lists it `NeedsConsent` (`host.grant`). Missing: the second site, `grant_asked` (`plugins_window.rs:740`, a running plugin's `ConsentRequested`).

### proofs6_plugins_market.rs (child of `workbench::plugins_window::tests`)

- `the_marketplace_lists_the_indexs_plugins_and_install_downloads_and_installs_one` → `ipc:plugin_market`, `ipc:plugin_market_download` — done. Proof: a local signed index (fixture `hello` as `acme.hello` 1.0.0, packed and signed by an `acme` key exactly as `crates/ide-host/tests/distribution.rs` does; `read_url` accepts local paths) set as the host's market URL; Marketplace lists a `Market` item `acme.hello` (`host.market_listing(false)` reads the index when none is cached); Install on its details (`DownloadMarket` → `host.download_market_plugin`, which checks digest and signature), consent accepted if asked, and the host lists `acme.hello` 1.0.0 with publisher `acme`, which only a verified download gives.
- `plugin_repository_shows_the_stored_url_and_saves_a_new_one_the_host_keeps` → `ipc:plugin_market_url`, `ipc:plugin_set_market_url` — done. Proof: the gear menu's Plugin Repository… prompt shows the URL the host stored (`host.market_url()` from `read_plugin_market`, handed back as `EditRepository { current }`); a second URL typed (select all + `simulate_input`, because `PromptDialog` answers with the value it cached on `Changed`) and Enter becomes `host.market_url()`, the page lists the new index's plugin, and a new `PluginHost` on the same folder reads the same URL (persisted by `set_market_url`'s `save_state`).

### proofs6_plugin_runtime.rs (child of `workbench::plugin_runtime::tests`)

- `a_plugin_command_runs_an_ide_action_in_its_window_and_hears_the_outcome` → `ipc:plugin_answer_command` — done. Proof: fixture `api14`'s `execute` command calls `host::execute_command(arg, ..)`; the window runs the action through the registry (`RuntimeEffect::ExecuteAction`, `plugin_runtime.rs:570-579`) and answers with `answer_plugin_command` → `host.answer_command` (the adapter's `plugins.answer_command`; the webview calls `pluginAnswerCommand` in `src/features/plugins/backend.ts:258`). With `view.toggleTheme` the theme turns dark and the command returns Ok; with `no.such.action` the command returns Err, which only the `Failed` answer produces. Correction to an earlier draft of these notes: the existing `a_plugins_text_question_is_answered_and_the_command_gets_the_text` does NOT prove this row; plugin prompts are answered through `host.answer_prompt` (`plugin_runtime.rs:1046-1051`).

### proofs6_workbench.rs (child of `workbench::tests`)

- `the_welcome_screen_shows_the_stored_project_groups_with_their_projects` → `ipc:recent_project_groups` — done. Proof: a recent project and a group "Work" holding it are stored in the test home's config (`RecentProjects::touch`, `set_groups`); a new window's Welcome lines start with the group header `[Work…` and list the project under it; `sync_welcome` reads them with `RecentProjects::load(..).groups()`, as the adapter's `recent.groups()`.

### proofs6_attach.rs (child of `workbench::tests`) — expected to fail

- `opening_a_folder_with_the_attach_policy_attaches_it_to_the_windows_workspace` → `ipc:workspace_attach` — stays open until the fix lands. See below.

### proofs6_app_actions.rs (child of `workbench::app_actions::tests`)

- `new_project_with_git_creates_a_repository_in_the_new_folder` → `ipc:vcs_init` — done. Proof: New Project with its Git checkbox left on (the default the existing test turns off) creates `<location>/repo/.git` (`CreateStep::GitInit` → `ops::vcs::vcs_init`, as the adapter) before the trust question; the project then opens. Not asserted: `vcs_roots` listing it. Note: backend git runs with the machine's git configuration in every `git_project` test too (only `init_repo`/`git_in` isolate it); the existing new-project test avoided Git for that reason, so the integrator may prefer to drop this one.

### proofs6_custom_controls.rs (child of `settings_view::custom_controls::tests`)

- `the_shell_path_list_offers_the_installed_shells` → `ipc:term_available_shells` — done. Proof: Tools › Terminal's Shell path control ends up offering exactly `ide_terminal::available_shells()` (read off the window's thread at `custom_controls.rs:106`), asserted non-empty so the comparison is not vacuous.

## Differs from the webview (test written as the webview demands)

`ipc:workspace_attach`. The webview attaches in place when "Open project in" is `attach` (`src/features/app/windows.ts:164-188`, spec `windows.test.ts` › Attach › "attaches in place without reopening or asking to leave"), offers Attach in the question, and has a `project.attach` action. Natively `OpenFlow::with_policy` honours only `new`/`same` (`app_models/windows.rs:290`), `carry_out_open` strips the Attach button (`app_shell.rs:769`, `const ATTACH` with the reason "the Project view shows the project's own folder only"), and no handler for `project.attach` is registered (grep finds only `menu_spec.rs:190` and the `when` table). So `OpenStep::Attach` (`app_shell.rs:828`) is unreachable. Expected failure: `proofs6_attach` times out in `wait_long("the folder is attached to the workspace")` with the window asking "Open project in" instead. Fix (behaviour part):

```diff
--- a/app-gpui/src/app_models/windows.rs
+++ b/app-gpui/src/app_models/windows.rs
@@ fn with_policy
         let step = match window.open_project_in.as_str() {
             mode @ ("new" | "same") => flow.act(mode),
+            "attach" if window.project.is_some() && !window.remote => flow.act("attach"),
             _ => {
--- a/app-gpui/src/workbench/app_shell.rs
+++ b/app-gpui/src/workbench/app_shell.rs
@@ async fn carry_out_open
             OpenStep::Ask(mut prompt) => {
-                prompt.buttons.retain(|button| button.id != ATTACH);
                 let answer = ask(this, cx, prompt).await?;
```

(and `const ATTACH` with its comment goes). The reason the button was removed stays true: the Project view must show attached roots before this is user-visible parity; that is a product step beyond this hunk.

`ipc:plugin_answer_commit` also differs: the webview answers a plugin's commit request with the user's selection (`src/features/plugins/backend.ts:261`), natively every request is answered `CommitAnswer::NoRepository` (`plugin_runtime.rs:520-526`, "The Commit view does not hand its selection to plugins"). Not drafted: the fixture `commit` plugin and a commit chooser that does not exist natively would be needed; there is no small hunk.

## Already proven by existing tests (no new test)

- `ipc:plugin_list` — `workbench::plugins_window::tests::a_projects_own_plugins_load_only_while_the_project_is_trusted` asserts the Plugins page lists the project's `.openidea/plugins/hello` after Trust + Reload, which `list_plugins` gets only through `host.sync_project(root)` + `host.list(Some(root))` (the adapter's `sync_project` + `list`). Proposed done.
- `ipc:vcs_line_hunks` — stays partial (the map already cites `rollback_in_an_editor_restores_the_changed_lines_under_the_caret`; the gutter does not read it natively).
- `ipc:vcs_stashes` — stays partial (the map already cites `stash_then_unstash_restores_the_change`; the Commit window's stash list is not built).

## Dropped: not issued natively

- `ipc:gh_pr_list`, `ipc:gh_pr_view`, `ipc:gh_pr_review_comments`, `ipc:gh_pr_review`, `ipc:gh_pr_comment` — the native window sends `gh_api` for all of them (`github.rs:1027`, `1157`, `1161`, `1490`: `gh_call(.., "gh_api", ..)`), never `ide_github::{list, view, review_comments, submit_review, add_comment}`. The webview does the same: grep for `ghPrList|ghPrView|ghPrReviewComments|ghPrReview|ghPrComment` in the worktree's `src/` finds no caller outside `bindings.ts`. Unused by both front ends; nothing to fix.
- `ipc:fs_rename` — `BackendCall::FsRename` is matched at `project_files.rs:750`, but `FsRequest::Rename` is never constructed (only matched in `project_view/fs.rs:66,334`); Rename goes through the refactoring path.
- `ipc:docker_save_settings` — `DockerRequest::SaveSettings` (`services/docker.rs:1072`) comes only from connection/registry dialogs and removals, which `carry_out_docker` logs as not carried out in this window.
- `ipc:syntax_support`, `ipc:syntax_identifiers`, `ipc:syntax_identifiers_in_files` — reachable only in a narrow case: `UsageSearch::answered` starts `SyntaxSearch` only when `lsp_references` returns an error, and the backend never errors in the usual no-server case: for an explicit project-wide Find Usages with `Unavailable` it answers itself (a certain local from the syntax tree, else `text_references`, `ide-host/src/ops/lsp.rs:641-670`). So in Light mode (no server) the syntax commands are not issued; they are issued only when a running server fails with another error. A test needs a fake server that fails `textDocument/references` for a language that has a tree-sitter grammar; the fake server of `editor/intelligence.rs` serves `mock`, which has none (so `Support.tree` is false and the search stops). I drafted two Light-mode tests, found this, and deleted them before finishing. Recipe: register the fake server for `javascript` (a `languages/javascript.toml` override in the test config) in Full mode, answer `textDocument/references` with an error, Find Usages on an undeclared global → header `TEXT_NOTE`, identifier occurrences only.

## Left out, with reasons

- `ipc:docker_snapshot`, `ipc:docker_settings` — any Docker settings load is followed by `ide_docker::snapshot`, whose `availability` runs `docker version` from the machine's PATH (`ide-docker/src/lib.rs:116`); nothing injects the program, and changing PATH is process-wide (`unsafe` in Rust 2024). Needs an injectable docker executable first.
- `ipc:term_is_busy` — the busy branch needs the PTY's foreground group to differ from the shell's (`foreground_busy`, `ide-terminal/src/lib.rs:323`); the test shell is `/bin/sh` reading a script without job control, so a `sleep` stays in its group; a test build refuses an interactive shell.
- `ipc:settings_get_global` (proxy password) — `with_proxy_password` reads the credential store; tests must not touch the keychain.
- `ipc:mcp_report_problems` — `ide_host::mcp::Reports` has no reader for stored problems (only `set_*`, `report_problems`, `forget_window`, `*_remote`). Needs `pub fn problems(&self, window: &str) -> Vec<FileProblems>` (ide-host), or the MCP server running in the test.
- `ipc:gh_pr_prepare_diff` — same backend function natively (`github.rs:1378`), not drafted: `github_answers` uses one commit as base and head; the test needs a second commit pushed to the bare remote, `baseRefOid`/`headRefOid` set to the two, the `DetailsEvent::OpenDiff` click on `src/main.rs`, and the diff window's two sides asserted.
- Not drafted in this pass (time), each `todo` in `ipc_map.json` stands as the recipe: `ipc:settings_set`, `ipc:settings_import`, `ipc:project_current`, `ipc:ssh_set_secret`, `ipc:cancel_task`, `ipc:history_content`, `ipc:doc_open_virtual`, `ipc:doc_selection_ranges`, `ipc:lsp_resolve_completion`, `ipc:vcs_log`, `ipc:vcs_log_users`, `ipc:vcs_commits_changes`, `ipc:vcs_restore_head`, `ipc:vcs_config_get`, `ipc:vcs_diff_with_worktree` (its dialog is not built natively), `ipc:plugin_assets`, `ipc:plugin_activation_event`, `ipc:plugin_run_command`, `ipc:plugin_status_items`, `ipc:plugin_terminal_session_close`.

## Tests most likely to fail first

1. `proofs6_attach` — by design (see above).
2. Market tests: the details Install button needs the entry `compatible` (engine `^1.0` against this build's plugin API) and `verified_publisher` must accept the index's `acme` key in a debug build (it does in `distribution.rs`); the repository test assumes `cmd-a` selects all in the focused `TextField` and that the drawn gear menu indexes `gear_items` (index computed, not hard-coded).
3. `installing_a_package_file_…`: assumes the host canonicalises nothing differently (paths compared canonicalised on both sides).
4. `review_permissions_…`: assumes a linked plugin is `NeedsConsent` on `test_plugin_host` (the first assert says so plainly if not).
5. `a_plugin_command_runs_an_ide_action_…`: assumes `host::execute_command` returns an error to the plugin on a `Failed` answer and that `view.toggleTheme` accepts the string argument the fixture passes.
6. `new_project_with_git_…`: the machine's git configuration (template dir, hooks) applies to the backend's `git init`.
7. `roll_back_…`: assumes Roll Back is offered for an update not yet confirmed by a first start (the details show the button when `previous_version` is set).

## Counts (54 commands)

- New tests: 12 (one expected to fail). Rows that become done: 12 (`plugin_catalog`, `plugin_unpack_package`, `plugin_roll_back`, `plugin_set_capability`, `plugin_market`, `plugin_market_download`, `plugin_market_url`, `plugin_set_market_url`, `plugin_answer_command`, `recent_project_groups`, `vcs_init`, `term_available_shells`); 1 partial (`plugin_grant`); 1 failing by design (`workspace_attach`, done once the fix lands).
- Already proven by an existing test: 1 (`plugin_list`); 2 stay partial as the map has them (`vcs_line_hunks`, `vcs_stashes`).
- Not issued natively: 10 (5 `gh_pr_*`, `fs_rename`, `docker_save_settings`, 3 `syntax_*` in the no-server case).
- Differs, not drafted: 1 (`plugin_answer_commit`).
- Left out: 26 (5 with a concrete blocker: docker ×2, terminal busy, keychain, MCP reader; 1 fixture not drafted: `gh_pr_prepare_diff`; 20 not drafted for time).

12 + 1 + 1 + 1 + 2 + 10 + 1 + 26 = 54.
