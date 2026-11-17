# Integrator report: native backend calls that drifted from the webview's (`int_follow_upstream3`)

Part 0 and item 8 are done. Items 2, 4, 5 and 6 are done. Item 1 is done except the archive test. Item 9 is done in part. Items 3 and 7 were not done. The final gates pass: two whole-suite runs, every added or changed test 20 times, `web_view`, clippy and the build with 0 warnings. Nothing is committed.

Logs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol3/` (written as `dfol3/` below).

## Part 0: Export Settings, Import Settings and MCP server values

**Tests** (in `app-gpui/src/workbench/app_actions.rs`, `mod tests`). Both use the real backend in a temp home. Each asserts `ide_ssh::secrets::secrets_in_files()` first. The marker is `mcp-marker-value-not-a-key-7c41`, and every check compares and then fails with a fixed message.

- `workbench::app_actions::tests::export_settings_moves_mcp_server_values_to_the_secret_store_and_writes_only_their_names`
  - The marker is written into the user settings through `host.global_settings.set`, which does not move it.
  - After Export Settings:
    - the exported file holds no marker but names `PROBE_TOKEN`;
    - the user settings file no longer holds it;
    - `agent_secrets.get("mcp/user/probe/env/PROBE_TOKEN")` holds it.
- `workbench::app_actions::tests::import_settings_moves_mcp_server_values_out_of_the_user_settings`
  - A bundle whose settings hold the marker is imported.
  - Afterwards `settings.toml` holds no marker but keeps the name, and the secret store holds the value.

**Failed before the change: yes, both** (measured, `dfol3/p0_before.log`):

- "the exported file holds no MCP server value" at `app_actions.rs:1902`;
- "the user settings file holds no MCP server value" at `:1930`.

**Change** (in the adapters' order, `src-tauri/src/commands/app.rs:289` and `:307`): `ide_host::ops::agent::move_user_mcp_values(&host)` is now called before `export_bundle` and right after `import_bundle`, inside the existing off-thread `call`.

**What else copies the user settings whole.** Measured by grep over `app-gpui/src` outside tests, for `settings.toml`, `export_bundle`, `import_bundle`, `fs::copy`, `dirs.config`, `config_dir`, and log or diagnostics collection: nothing. Only Export and Import read or write the whole global layer. Other users of the config folder:

- `UiStorage::load` and `LanguageRegistry::load`;
- the About dialog, which shows and copies the folder's path, not its contents.

The native Import still skips the adapter's `crate::commands::mcp::apply(&state)` and update wake (`app.rs` after `:307`). That is outside this slice.

## Item 8: a path on another server's share (security fix)

- **New functions:** `on_other_share` and `opens_other_share` in `app-gpui/src/platform/paths.rs`. They follow `src/platform/paths.ts:31-55` and use the `regex` crate in a `LazyLock`.
- **Spec tests:** the six cases of `src/platform/paths.test.ts`, as `platform::paths::tests::{is_true_for_a_share_the_base_is_not_on, reads_the_server_and_share_of_a_device_path, is_false_on_the_bases_own_share_and_off_any_share, is_true_for_a_share_the_text_names_itself_in_any_mix_of_slashes, is_true_for_a_share_that_only_appears_once_the_path_is_resolved, is_false_on_a_local_drive_and_on_the_share_the_text_is_on}`.

Callers. A grep of `src/` finds only the four the brief names:

| Counterpart                                                                   | Change                                                                                                                                                                    | Test                                                                                                                                                                                                                                                              | Reachable from the app?                                                                                                                                                |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Document links (`src/editor/lsp/links.ts:67`)                                 | `editor_lsp::links::link_reference` takes the document path and refuses another share                                                                                     | `editor_lsp::links::tests::follows_no_server_link_to_a_share_the_file_is_not_on` (spec case, plus a local path)                                                                                                                                                   | No: `link_reference` has no production caller                                                                                                                          |
| Hover and documentation links (`src/editor/lsp/markdown.ts:210`)              | `source_link` and `doc_link` take the project root. `HoverView` and `QuickDocView` gained `set_link_base`; `EditorIntelligence::show_hover` sets it from the project root | `editor_lsp::markdown::tests::names_no_share_the_project_is_not_on` (spec), `editor_lsp::hover_view::tests::a_documentation_link_to_another_share_opens_nothing` (a click on the view emits nothing for another share and a `Source` link on the project's share) | Partly: `HoverEvent::Link` is dropped at `editor/intelligence.rs:1205`, and `QuickDocView::new` has no production caller, so its base stays empty (the strictest case) |
| Agent conversation file references (`src/features/agents/conversation.ts:40`) | `agents::conversation::file_href`                                                                                                                                         | `agents::conversation::tests::{names_no_network_share_however_the_link_spells_it, follows_links_in_a_task_that_is_on_a_share}` (spec)                                                                                                                             | Yes                                                                                                                                                                    |
| Markdown preview links (`src/features/viewers/markdown/render.ts:220`)        | `features_b::viewers::render::resolve_link`                                                                                                                               | `features_b::viewers::render::tests::resolves_no_link_to_a_network_share_however_the_file_spells_it` (spec)                                                                                                                                                       | Yes, also through the notebook                                                                                                                                         |

**Failed first:** the caller tests could not compile against the old signatures. So the failure was measured with both checks short-circuited to `false` (`dfol3/item8_without_check.log`): all 9 share-dependent tests failed (`test result: FAILED. 61 passed; 9 failed`). Then the short-circuit was removed.

**Notes:**

- The preview has no same-share test. `join_path` (like the webview's `joinPath`) collapses a leading `//`, so a relative link in a file on a share resolves to `/nas/…` on both sides.
- No undeclared draft (`proofs36`–`38`, `*_pending`) calls the changed functions (grep).

## Item 1: Reload with Encoding through the backend (archive test not done)

- **Sides:** adapter `src-tauri/src/commands/doc.rs:118`; backend `crates/ide-host/src/ops/doc/commands.rs:45-66`; webview `src/editor/core/view/StatusItems.tsx:368-372` and `:419`.
- **Change:**
  - `Editor::set_encoding` now calls `ide_host::ops::doc::doc_set_encoding` asynchronously and returns a `Task`.
  - It keeps the answer's encoding and `has_bom`, and `EditorStatus` has `has_bom`.
  - The encoding item shows `"<name> BOM"`.
  - The three callers (`run_status_command`, which gains `notify_refusal`, and `reload_from_disk`) await it and report refusals.
- **New test:** `workbench::tests::reload_with_encoding_shows_the_byte_order_mark_state_the_backend_answers`. A file with a UTF-8 BOM shows `UTF-8 BOM`; Reload in ISO-8859-1 re-reads the bytes and shows no BOM; Reload in UTF-8 shows `UTF-8 BOM` again. It was not measured failing first, because it needs the new field to compile.
- **Changed test:** `workbench::tests::the_status_bars_items_open_go_to_line_and_their_menus` now waits for the asynchronous answer. Its expectation after Convert to UTF-16LE changes to `"UTF-16LE BOM"`, because the backend answers `has_bom` true and the webview shows it (`StatusItems.tsx:369`, `:419`).
- **Not done: the archive test.** The native editor opens tabs only through `ops::doc::open` (disk files). No production code opens a virtual document as a tab (grep: `doc_open_virtual` is used only by `structure_view.rs` and `refactor_actions.rs`), and there is no zip crate in `app-gpui`.
  - Hypothesis: a library source cannot be opened in a tab natively at all. Check with `grep -rn "doc_open_virtual" app-gpui/src`.
- **Not done (item 9 parts):** the UTF-8 BOM menu entry and disabling Reload with Encoding when the document cannot be re-read.

## Item 2: completion back-off and the type column

- **Webview:** `src/editor/lsp/completion.ts:514`, `:527`; `src/editor/lsp/completionItems.ts:177` (`labelDescription` wins over `detail`, trimmed, blank falls back).
- **Change:**
  - `CompletionSource::ask` takes `explicit`, and an explicit request skips the back-off.
  - Any `Ok` answer clears the document's back-off (new `FeatureBackoff::forget`).
  - The type column uses the trimmed `label_description` before `detail`.
- **Tests, both failed first** (`dfol3/batch2_before.log`):
  - `editor_lsp::completion::tests::an_explicit_completion_bypasses_the_back_off_and_an_answer_clears_it` (left `Fallback`);
  - `editor_lsp::completion_view::tests::the_type_column_shows_the_label_description_before_the_detail`.
- **Changed test:** `asks_on_words_and_triggers_and_drops_stale_answers` only gains the `false` argument.
- **Not checked:** the hypothesis that the webview also clears back-offs when plugins change.

## Item 3: Stop while a run is starting (not done)

The native store already has `set_resolve_request` and `StopAction::CancelRequest` for the Java resolution step. Still to do:

- pass a request id from `run_process` (`app-gpui/src/run/driver.rs:1096`) into `start_request`;
- clear it in `process_started` and stop the run if the session was stopped meanwhile (`src/features/run/store.ts:1023`, `:1336-1340`).

I found no stand-in runner whose start waits, so I timeboxed it out. Nothing was edited.

## Item 4: `general.closeWithoutWelcome`

- **Webview:** `src/features/app/closeGuard.ts:52`, `:227`.
- **Change:** `WindowCloseState.close_without_welcome`, read in `app_shell.rs` `close_state`. `CloseRequests::request` no longer routes to Welcome when it is on.
- **Test, failed first** (timed out: "the window to close did not happen"): `workbench::tests::proofs13_close_guard::closing_the_last_project_window_closes_it_when_close_without_welcome_is_on`. With the default the window shows Welcome; with the setting on it closes.
- **`general.closeTabsWithWindow` is listed, not built.** It drops saved tabs on close (`closeGuard.ts:204`) and puts them back on a cancelled close (`undoForget`). That needs `forgetSavedTabs` and `keepSavedTabs` counterparts in the native tab restore.

## Item 5: live branch on recent projects

- **Sides:** adapter `src-tauri/src/commands/app.rs:46-54`; webview `src/features/welcome/RecentList.tsx:201`.
- **Change:** `sync_welcome` and the `SetGroups` handler list `ide_workspace::recent::with_live_branches(recent.list())`.
- **Test, failed first** (left `Some("stored")`): `workbench::tests::recent_live_branch::the_welcome_screen_shows_the_live_branch_of_a_recent_project`. It is in a new file and writes `.git/HEAD` by hand; no git is run.
- Hypothesis: the `.git/HEAD` reads run on the UI thread, as `RecentProjects::load` already does there. The menu bar's recent list shows no branch (`menu.rs:403`).

## Item 6: marketplace publisher name

- **Webview:** `src/features/plugins/PluginsPage.tsx:1044`, `:1099`, `:1209`, `:1213`.
- **Change:** a local `publisher_of` helper in `plugin_models/plugins_page.rs`, used for the row vendor, the details' authors and "Signed by".
- **Test, failed first:** `plugin_models::plugins_page::tests::a_marketplace_plugin_shows_its_publishers_name`.

## Item 7: net:ask (not done)

No fixture or bundled plugin makes a `net:ask` request (grep over `plugins/`: only `README.md`), and none can be built without a toolchain. The native app keeps answering Deny (`plugin_runtime.rs:1037`), which is the safe state. Nothing was edited.

## Item 9 (later report)

**Done, each test failed first:**

- **Pending hint delay 250 → 50 ms** (`src/editor/lsp/hint.ts:86`). Test renamed after the spec: `editor_lsp::hint::tests::shows_a_slow_request_within_the_feedback_budget_and_removes_its_hint_on_completion`. Its `PENDING_DELAY_MS < 100` check is now a `const { assert!(…) }` because clippy rejects a constant assertion, so a regression now fails compilation.
- **npm hint text** (`src/features/run/types.ts:244`). Test: `run::types::tests::the_npm_node_interpreter_hint_names_the_javascript_runtime_page`.
- **Date formats** (`src/platform/dateFormat.ts:78-100`). New `format_date_time` (numeric system date, or the pattern), used for non-pretty dates and older pretty dates.
  - New spec test: `platform::date_format::tests::keeps_the_time_of_dates_before_yesterday_as_ideas_pretty_date_time_does`.
  - Changed expectations, each citing the webview line in a comment:
    - `writes_list_timestamps` now expects `"03.11.2026 12:00"` (`dateFormat.ts:99`);
    - `vcs::shelf_time::tests::a_shelf_row_shows_its_files_then_when_it_was_shelved` now expects `"3 files · 11/03/2026 03:30 PM"` (`dateFormat.ts:99` through `src/features/vcs/model.ts:137` and `shelf.tsx:539`). This one was found by the first whole-suite run.
- **Encoding item BOM label:** see item 1.

**Not done:**

- the Background Tasks popup closing on its last task, and with it the stale test `cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`;
- `configuration_navigation.rs:147` (a file change cancelling a ⌘-click already in flight);
- the language server status `label` and the 500 ms busy delay (`workbench/mod.rs` near 2086);
- the UTF-8 BOM menu entry and the disabled Reload with Encoding.

## Gates (all measured)

| Gate                                                                                                             | Result                                                                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Suite run 0 (`dfol3/suite1.log`)                                                                                 | `test result: FAILED. 8203 passed; 1 failed` — `vcs::shelf_time::tests::a_shelf_row_shows_its_files_then_when_it_was_shelved`, left `"3 files · 11/03/2026 03:30 PM"`, right `"3 files · Nov 3, 2026"`. My date-format change, fixed as above. Load 2.92 before, 113.21 after |
| Suite run 1, alone (`dfol3/suite1b.log`)                                                                         | `test result: ok. 8204 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 212.94s`. Load 16.02 before, 20.83 after                                                                                                                                          |
| Suite run 2 (`dfol3/suite2.log`)                                                                                 | `test result: ok. 8204 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 219.55s`. Load 17.93 before, 45.18 after                                                                                                                                          |
| 20× during run 2 (`dfol3/twenty_summary.txt`, `dfol3/twenty-*.log`; 29 tests listed in `dfol3/tests_sorted.txt`) | 20/20 runs `test result: ok. 29 passed; 0 failed`; no test failed in any run                                                                                                                                                                                                  |
| `cargo test --test web_view` (`dfol3/web_view.log`)                                                              | `test result: ok. 1 passed; 0 failed`                                                                                                                                                                                                                                         |
| `cargo clippy --all-targets -- -D warnings` (`dfol3/clippy2.log`)                                                | exit 0                                                                                                                                                                                                                                                                        |
| `cargo build` (`dfol3/build.log`)                                                                                | exit 0, 0 warnings                                                                                                                                                                                                                                                            |

The 20× runs finished within suite run 2's span, judging by the timing in the logs. No cargo, rustc or test process of mine is left (checked with `ps` after the build). Only the files I edited were formatted, with rustfmt in stdin mode through `dfol3/fmt.sh` (a copy of `$S/fmt.sh` that keeps its temp files in `dfol3/` instead of `/tmp`).

## Parity rows (not written to `parity.json`)

| Row                        | Missing clause now proven   | Test                                                                                                                                                                                                                   |
| -------------------------- | --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:settings_export`      | the MCP values move first   | `export_settings_moves_mcp_server_values_to_the_secret_store_and_writes_only_their_names`. The Import counterpart, which is not a row, is proven by `import_settings_moves_mcp_server_values_out_of_the_user_settings` |
| `ipc:doc_set_encoding`     | the BOM state of the answer | `reload_with_encoding_shows_the_byte_order_mark_state_the_backend_answers`. The editor now calls the backend's function, but the virtual-document re-read has no test. Stays partial                                   |
| `ipc:lsp_completion`       | both clauses                | `an_explicit_completion_bypasses_the_back_off_and_an_answer_clears_it`, `the_type_column_shows_the_label_description_before_the_detail`. Both are model or view level, not end to end                                  |
| `ipc:run_start`            | none                        | —                                                                                                                                                                                                                      |
| `ipc:window_close_context` | `closeWithoutWelcome`       | `closing_the_last_project_window_closes_it_when_close_without_welcome_is_on`. `closeTabsWithWindow` is still missing                                                                                                   |
| `ipc:recent_projects`      | the live branch             | `the_welcome_screen_shows_the_live_branch_of_a_recent_project`                                                                                                                                                         |
| `ipc:plugin_market`        | `publisherName`             | `a_marketplace_plugin_shows_its_publishers_name` (page model level)                                                                                                                                                    |
| `ipc:plugin_answer_prompt` | none                        | —                                                                                                                                                                                                                      |

## Files

**Edited** (from `git diff --name-only`), all under `app-gpui/src/`:

- `agents/conversation.rs`
- `app_models/close_guard.rs`
- `editor/intelligence.rs`, `editor/view.rs`
- `editor_lsp/availability.rs`, `editor_lsp/completion.rs`, `editor_lsp/completion_view.rs`, `editor_lsp/hint.rs`, `editor_lsp/hover_view.rs`, `editor_lsp/links.rs`, `editor_lsp/markdown.rs`
- `features_b/viewers/render.rs`
- `platform/date_format.rs`, `platform/paths.rs`
- `plugin_models/plugins_page.rs`
- `run/types.rs`
- `vcs/shelf_time.rs`
- `workbench/app_actions.rs`, `workbench/app_shell.rs`, `workbench/editor_core.rs`, `workbench/status_items.rs`
- `workbench/mod.rs` (also the `mod recent_live_branch;` declaration)
- `workbench/tests/proofs13_close_guard.rs`

**New:** `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/recent_live_branch.rs`.

Nothing changed under `crates/`, `docs/`, `src/`, `src-tauri/` or `e2e/` (`git status --short` on them is empty). The `docs/…/int_follow_upstream3_brief.md` modification seen at my first command was not mine, and it no longer appears in the diff.

## Stale or wrong in the notes

- The brief's line numbers are close, as it says.
- `RETRIAGE_IPC.md` implies native library sources reload through `Documents::set_encoding`. In fact no native code opens a virtual document as an editor tab (grep), so the archive branch of item 1 is unreachable natively.
- `RETRIAGE_IPC.md` presents hover links as a native caller. Native hover and quick-doc link events are emitted but never handled (`intelligence.rs:1205`), and `link_reference` is not wired.
