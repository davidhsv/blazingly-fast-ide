# Empty backend-call rows, batch 1 (110 commands), judged at HEAD 444f30969

Method: webview sites from `git grep -n -P "commands\.<camel>\b|\"<snake>\"" HEAD -- src` (bindings and tests skipped); the backend function read from the Tauri command's body in `src-tauri/src/commands/*.rs`; the native side from `git grep HEAD -- app-gpui/src` for that function (and for the feature's own calls when there was none); test names checked against `$S/rust_tests.txt`.

Two facts found on the way that change several rows:

- The Settings › Agents pages in the native app are drafts that are not compiled: `app-gpui/src/workbench/agent_list_backend.rs`, `workbench/settings_agents.rs`, `workbench/settings_agents_list.rs`, `agents/agent_list.rs`, `settings_view/pages/agents_list.rs`, `settings_view/pages/tools_agents.rs` exist at HEAD but no `mod` declares them (`agents/mod.rs` declares `builtin_agents` but not `agent_list`; `workbench/mod.rs` lines 4-100 declare no `settings_agents*` or `agent_list_backend`; `settings_view/pages/mod.rs` declares neither `agents_list` nor `tools_agents`). Their commits say so: 7ed286de6 "drafts (not compiled): the Coding Agents page of Settings" and bde11eb34 "drafts (not compiled): the Built-in tab of the Agents page of Settings". Their tests (`workbench/tests/proofs36_settings_agents.rs`, `proofs38_settings_agents_list.rs`, `settings_view/pages/agents_list/proofs38_agents_list_view.rs`) are not in `rust_tests.txt`. Calls found only in these files are counted NOT BUILT below.
- The Services window carries out only some Docker requests: `app-gpui/src/workbench/services_window.rs:689-741` (`carry_out_docker`) runs LoadSettings, SaveSettings, Snapshot and RefreshLater; WatchEvents is answered as ended at once (line 712-716), and ContainerOp, Remove, Stream (pull, prune, compose), RegistryLogin and Inspect fall into `other => tracing::info!(... "Services: not carried out in this window")` (line 737-739). The doc comment at line 688 says so: "the events stream, container operations, streamed tasks and dialogs are not built".

## Agents

### agent_accept_changes

- Webview `src/features/agents/store.ts:756` (`acceptChanges`): in an agent task's Changes review the user accepts all or some changed files; the task's change list empties for them.
- NOT BUILT: `git grep "accept_changes" HEAD -- app-gpui/src` finds only a test name in `workbench/vcs_text_actions.rs:535` (merge conflicts, unrelated). The native agent calls are the `AgentCall` variants of `app-gpui/src/workbench/agents.rs:251-268` (Prompt, Cancel, Rollback, Fork, Unqueue, SendQueuedNow, SetConfig, StopBackground, Respond, Archive, Unarchive, Rename, Delete): no changes review.
- Verdict: not built.

### agent_base_text

- Webview `src/features/agents/SessionEditor.tsx:3489` (`openChangeDiff`): clicking a changed file of a task opens a diff whose left side is the file as it was when the task started.
- NOT BUILT: `base_text` in `app-gpui/src` is only `github/create.rs:136` (the PR base label, unrelated). No task change diff in the native agents.
- Verdict: not built.

### agent_create_ephemeral

- Webview `src/features/agents/store.ts:1232` (`startSideConversation`): the user opens a side conversation of a task; a temporary pane opens with the task's conversation as context.
- NOT BUILT: no `create_ephemeral` in `app-gpui/src`; no `AgentCall` for it.
- Verdict: not built.

### agent_custom_import

- Webview `src/features/agents/registry.ts:67` (`importAcp`): on Settings › Agents the user pastes ACP agent JSON and imports; the custom agents appear in the list.
- NOT BUILT (draft): the only call is `app-gpui/src/workbench/agent_list_backend.rs:86` (`ide_host::ops::agent::import_acp_agents`), a file no `mod` declares (see the note above).
- Verdict: not built.

### agent_custom_remove

- Webview `src/features/agents/registry.ts:65` (`removeCustom`): the user removes a custom agent on Settings › Agents; it leaves the list.
- NOT BUILT (draft): only `workbench/agent_list_backend.rs:76` (`remove_custom_agent`), not compiled.
- Verdict: not built.

### agent_custom_save

- Webview `src/features/agents/registry.ts:62` (`saveCustom`): the user adds or edits a custom agent in its dialog and saves; it shows in the list.
- NOT BUILT (draft): only `workbench/agent_list_backend.rs:68` (`save_custom_agent`), not compiled.
- Verdict: not built.

### agent_external_sessions

- Webview `src/features/agents/store.ts:1012` (`refreshExternalSessions`): the agents' list shows sessions of Claude Code / Codex run outside the IDE (a terminal) that no task holds yet.
- NOT BUILT: no `external_sessions` in `app-gpui/src`.
- Rules: `crates/ide-agent/src/lib.rs:677-690` reads the Claude config dir and `home/.codex`, with `paths.home_dir` overridable, so a test could point it at a temporary home.
- Verdict: not built.

### agent_grouped_changes

- Webview `src/features/agents/store.ts:717` (`refreshChanges`): for a worktree task with "group by commit" on, the Changes review lists the changes grouped by commit.
- NOT BUILT: no `grouped_changes` in `app-gpui/src`.
- Verdict: not built.

### agent_import_session

- Webview `src/features/agents/store.ts:1037` (`importOne`): the user imports an external session; it becomes a task and opens.
- NOT BUILT: no `import_session` in `app-gpui/src`.
- Verdict: not built.

### agent_mcp_discover_sources

- Webview `src/features/agents/mcpModel.ts:44` (`discoverSources`): the MCP servers page's Import dialog lists other tools' MCP configurations found on the machine.
- NOT BUILT: `git grep -i mcp HEAD -- app-gpui/src` (non-test) finds only the built-in MCP host reporter (`features_b/mcp.rs`, `code_insight/problems_report.rs`) and `agents/store.rs:1105` (a comment: "The New Task page has no MCP servers menu"); no MCP servers settings page.
- Rules: hypothesis, not read: it reads other tools' config files under the user's home (`crates/ide-host/src/ops/agent.rs:617`); check with `git show HEAD:crates/ide-host/src/ops/agent.rs | sed -n 617,650p`.
- Verdict: not built.

### agent_mcp_import

- Webview `src/features/agents/mcpModel.ts:41` (`importWorkspace`): on the MCP servers page the user imports the project's MCP configuration; the servers appear in the list.
- NOT BUILT: as for agent_mcp_discover_sources (no MCP servers page).
- Verdict: not built.

### agent_mcp_import_selected

- Webview `src/features/agents/mcpModel.ts:46` (`importSelected`): in the Import dialog the user picks servers from the discovered sources and imports them.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_list

- Webview `src/features/agents/mcpModel.ts:35` (`list`): opening the MCP servers page lists the configured servers.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_remove

- Webview `src/features/agents/mcpModel.ts:40` (`remove`): the user deletes a server; it leaves the list.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_save

- Webview `src/features/agents/mcpModel.ts:36` (`save`): the user adds or edits a server in the form and saves.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_save_json

- Webview `src/features/agents/mcpModel.ts:38` (`saveJson`): the user pastes a server's JSON and saves it for chosen agents.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_set_enabled

- Webview `src/features/agents/mcpModel.ts:39` (`setEnabled`): the user toggles a server on or off.
- NOT BUILT: no MCP servers page.
- Verdict: not built.

### agent_mcp_test

- Webview `src/features/agents/mcpModel.ts:47` (`testServer`): the user tests a server and sees whether it answers and its tools.
- NOT BUILT: no MCP servers page.
- Rules: `crates/ide-agent/src/mcp.rs:860` `probe_stdio_server` starts the server's program; a stdio test needs a program of the machine, an HTTP server could be a loopback stand-in (`ops/agent.rs:676` `probe_http_mcp_server`).
- Verdict: not built.

### agent_probe

- Webview `src/features/agents/HomeEditor.tsx:330` (`useProbe`): when the New Task page is shown or the window regains focus, the selected agent is checked again (installed, signed in).
- NOT BUILT: no `ops::agent::probe` in `app-gpui/src` (`probe(` hits are deployment and file-kind helpers).
- Rules: `crates/ide-agent/src/lib.rs:1283` `probe` starts the agent's program (`probe_once`), so only a spawn seam would let a test drive it.
- Verdict: not built.

### agent_refresh_models

- Webview `src/features/agents/store.ts:415`: the New Task page reads the selected agent's model catalog (forced on demand).
- NOT BUILT: no `refresh_models` in `app-gpui/src`; the native New Task looks up commands through `ops::agent::commands` (`workbench/agents_views.rs:63`), not models.
- Verdict: not built.

### agent_registry_install

- Webview `src/features/agents/registry.ts:41`: on Settings › Agents › Registry the user installs an agent and sees download progress, then it shows as installed.
- NOT BUILT (draft): only `workbench/agent_list_backend.rs:144`, not compiled.
- Rules: `crates/ide-agent/src/registry.rs:26` fetches `REGISTRY_URL` (a public CDN) and downloads the agent; a test needs a stand-in (the draft has `RegistryStandIn`).
- Verdict: not built.

### agent_registry_list

- Webview `src/features/agents/registry.ts:37`: opening the Registry tab lists the registry's agents.
- NOT BUILT (draft): only `workbench/agent_list_backend.rs:122`, not compiled.
- Rules: network (`registry.rs:26`, `REGISTRY_URL`), as above.
- Verdict: not built.

### agent_registry_remove

- Webview `src/features/agents/registry.ts:44`: the user removes an installed registry agent.
- NOT BUILT (draft): only `workbench/agent_list_backend.rs:163`, not compiled.
- Verdict: not built.

### agent_rollback_changes

- Webview `src/features/agents/store.ts:747`: in the Changes review the user rolls back all or some of a task's file changes; the files return to the task's start.
- NOT BUILT: no `rollback_changes` in `app-gpui/src`. The native `AgentCall::Rollback` (`workbench/agents.rs:254`) is `ops::agent::rollback` of conversation turns, a different feature.
- Verdict: not built.

### agent_send_queued_now

- Webview `src/features/agents/store.ts:1416` (`sendQueuedNow`): on a message queued while the agent works the user clicks Send now; it is sent at once.
- NOT BUILT (dispatch only): `workbench/agents.rs:260` dispatches `AgentCall::SendQueuedNow` to `ops::agent::send_queued_now`, but nothing constructs that variant (`git grep SendQueuedNow HEAD -- app-gpui/src` finds only the enum `agents/store.rs:141` and the dispatch/labels in `workbench/agents.rs`); no queue UI in `session_view.rs`/`composer_view.rs`/`message_editor.rs`.
- Verdict: not built.

### agent_set_command

- Webview `src/features/agents/registry.ts:56` (`set`): on Settings › Agents the user sets an agent's executable and applies.
- NOT BUILT (draft): only `workbench/settings_agents.rs:29` and `workbench/settings_agents_list.rs:30`, neither compiled. The test `agents::builtin_agents::tests::stages_a_binary_path_and_writes_it_only_on_button` drives a model with a fake backend (`agents/builtin_agents/tests.rs`, `FakeCommands`), not the backend call.
- Verdict: not built.

### agent_set_env

- Webview `src/features/agents/registry.ts:59` (`setEnv`): the user sets an agent's environment variables and applies.
- NOT BUILT (draft): only `workbench/settings_agents.rs:30` and `settings_agents_list.rs:31`, not compiled. `agents::builtin_agents::tests::writes_the_agents_environment_variables_on_apply_and_shows_them_again` asserts on `FakeCommands.saved_env` (`agents/builtin_agents/tests.rs:211`), a fake, so it does not count.
- Verdict: not built.

### agent_set_mcp_overrides

- Webview `src/features/agents/store.ts:1431`: in a task's MCP servers menu the user turns servers on or off for that task.
- NOT BUILT: no `set_mcp_overrides` in `app-gpui/src`; `agents/store.rs:1105-1106` sends empty overrides with the comment that the page has no MCP servers menu.
- Verdict: not built.

### agent_stop_background

- Webview `src/features/agents/store.ts:1488` (`stopBackgroundTask`): the user stops one of the agent's background tasks (a background shell) shown in the session.
- NOT BUILT (dispatch only): `workbench/agents.rs:262-263` dispatches `AgentCall::StopBackground`, but nothing constructs it (only `agents/store.rs:143` and the label at `workbench/agents.rs:136`).
- Verdict: not built.

### agent_unqueue

- Webview `src/features/agents/store.ts:1401` (`editQueued`): the user takes a queued message back into the composer to edit it.
- Webview `src/features/agents/store.ts:1415` (`removeQueued`): the user removes a queued message.
- Site 1401: NOT BUILT (model only): `agents/store.rs:1159` `edit_queued` builds `AgentCall::Unqueue` and its answer refills the composer (line 1184), and `workbench/agents.rs:259` dispatches it, but `git grep edit_queued HEAD -- app-gpui/src` finds no caller: no view offers it.
- Site 1415: NOT BUILT: no remove-queued path at all.
- Verdict: not built.

## Analysis, Android, app

### analysis_locate_duplicates

- Webview `src/features/analysis/duplicates.ts:100`: Code › Locate Duplicates over a scope; a report of duplicated fragments opens.
- NOT BUILT: `git grep -i duplicat HEAD -- app-gpui/src` finds only unrelated database and test strings; `ops::analysis` is used only for `InspectEvent` types (`features_b/analysis/inspect.rs:16`).
- Verdict: not built.

### android_devices

- Webview `src/features/logcat/LogcatView.tsx:54` (`refreshDevices`): the Logcat tool window lists connected Android devices.
- NOT BUILT: `ide_android` appears only as types in `app-gpui/src/features_a/logcat.rs:21`; no call of `ide_android::devices`.
- Rules: hypothesis, not read: `ide_android::devices` runs `adb`, a program of the machine; check with `git grep -n "Command::new" HEAD -- crates/ide-android`.
- Verdict: not built.

### android_logcat

- Webview `src/features/logcat/LogcatView.tsx:105`: choosing a device streams its log lines into the Logcat view.
- NOT BUILT: as above, no `ide_android::logcat` call.
- Verdict: not built.

### app_check_update

- Webview `src/features/app/updates.tsx:90`: Help › Check for Updates (and the automatic check); a balloon says whether an update is available.
- NOT BUILT: `app-gpui/src/app_models/updates.rs` is a model (`Updater`), but `git grep "Updater\b\|UpdateChecks" HEAD -- app-gpui/src` finds no use outside that file, and no call of an update check. Its tests (`app_models::updates::tests::*`) are model tests.
- Rules: the backend (`src-tauri/src/commands/window.rs:493`, `crate::updates::check`) is the Tauri updater over the network and lives in `src-tauri`, not `ide_host`; a native port needs its own check.
- Verdict: not built.

### app_cli_install

- Webview `src/features/app/cliCommand.ts:15`: Tools › Create Command-line Launcher; a message names the installed path.
- NOT BUILT: no `cli::install`, `ide_cli` or launcher action in `app-gpui/src`.
- Rules: `src-tauri` `crate::cli::install` writes a link into a system bin folder (outside the test's sandbox unless redirected).
- Verdict: not built.

### app_cli_uninstall

- Webview `src/features/app/cliCommand.ts:38`: removing the launcher; a message lists the removed links.
- NOT BUILT: as above.
- Verdict: not built.

### app_install_update

- Webview `src/features/app/updates.tsx:245`: Install and Restart from the update balloon; a background task shows download progress, then the app restarts.
- NOT BUILT: the `Updater` model is not wired (see app_check_update); the backend is the Tauri updater (`window.rs:597`).
- Verdict: not built.

### app_log_location

- Webview `src/features/app/appActions.ts:23`: Help › Show Log in Finder/Explorer reveals the log file.
- NOT BUILT: `app_views/about_dialog.rs:24` names the action `help.showLog`, and its test asserts it is absent: `about_dialog.rs:310` `assert!(cx.debug_bounds("about-link-log").is_none(), "Show Log is not registered")`. No `log_dir` call in `app-gpui/src`.
- Verdict: not built.

### app_metrics

- Webview `src/features/perf/store.ts:23`: the memory indicator of the status bar and the memory popup read the app's memory and process counts.
- NOT BUILT: `features_b/perf.rs:344` defines an `AppMetrics` model and `metrics_loaded`, and `workbench/status_items.rs:202` `memory_item`, but `git grep memory_item HEAD -- app-gpui/src` finds only its own test; nothing loads metrics.
- Verdict: not built.

### app_release_notes

- Webview `src/features/whats-new/WhatsNew.tsx:42`: after an update the What's New page fetches and shows the release notes.
- NOT BUILT: no release notes fetch in `app-gpui/src` (the "What's New" hits are the plugins page's change notes, `plugin_models/plugins_page.rs:2387`).
- Rules: network (`window.rs:653`, update endpoints).
- Verdict: not built.

### app_update_announcement

- Webview `src/features/app/updates.tsx:165`: when an update is found the window claims the announcement so one balloon shows across windows.
- NOT BUILT: no use of the updates model or an announcement call.
- Verdict: not built.

### app_update_status

- Webview `src/features/settings/pages/updates.tsx:26`: Settings › Updates shows the last check's status.
- NOT BUILT: no call in `app-gpui/src`.
- Verdict: not built.

### app_update_window_ready

- Webview `src/features/app/updates.tsx:143` and `:150`: a window says it is ready (or not) for the automatic update notifications.
- NOT BUILT: `UpdateChecks` in `app_models/updates.rs` is not used elsewhere.
- Verdict: not built.

## Build, caches, clipboard

### build_compile_file

- Webview `src/features/build/store.ts:64` (`compileFile`, reached by the `build.compileFile` action, `src/features/build/index.ts:90/151`): Build › Compile File on a Java source; the Build window streams `javac`'s output and the file's diagnostics.
- NOT BUILT: `git grep -i "compile_file\|compileFile\|recompile" HEAD -- app-gpui/src` finds nothing. The native Build window calls only `ops::build::build_run` (`workbench/build_window.rs:157`), a different command (`build_run`) the webview also has.
- Rules: `crates/ide-host/src/ops/build.rs:855-880` compiles with `javac` (`compile::run`), a real toolchain.
- Verdict: not built (and cannot be tested under the rules).

### caches_local_data_info

- Webview `src/features/settings/pages/behavior.tsx:606`: Settings › Behavior shows where local data (caches, indexes) lives, its default and any override.
- NOT BUILT: no `data_dirs` call in `app-gpui/src` beyond the start-up migration (`backend.rs:218`, `ide_host::data_dirs::migrate`); no such settings field.
- Verdict: not built.

### caches_set_local_data_dir

- Webview `src/features/settings/pages/behavior.tsx:632` (`apply`): the user picks another local data folder (optionally moving the data) and applies; it takes effect after restart.
- NOT BUILT: as above (`set_override` not called).
- Rules: `data_dirs::set_override` writes the override for the app identifier, a file outside the project; a test needs the data root redirected (the native `AppDirs` are injectable in tests, `backend.rs:119`).
- Verdict: not built.

### clear_caches

- Webview `src/features/perf/store.ts:35`: the Memory popup's Clear Caches drops syntax trees, editorconfig caches and compacts the index, then reports memory before and after.
- NOT BUILT: `features_b/perf.rs:486-537` has the action ids and a `clear_caches_notification` model, but `git grep "perf.clearCaches" HEAD -- app-gpui/src` finds only `features_b/perf.rs`; no handler registers it. (The Invalidate Caches dialog, `app_views/invalidate_caches_dialog.rs`, is a different feature: schedule and restart.)
- Verdict: not built.

### clipboard_file_list

- Webview `src/editor/core/cm/markdownImages.ts:197`: pasting an image copied in the file manager into a Markdown editor inserts a link to the copied file.
- Webview `src/features/project-tree/clipboard.ts:26` (`files`): Paste in the project tree pastes the files the file manager copied.
- Webview `src/platform/clipboard.ts:35` (`readClipboardText`): a paste of files copied in the file manager pastes their paths as text.
- All three sites: NOT BUILT. `app-gpui/src/project_view/clipboard.rs:7-14` declares `OsClipboardCall::FileList/HasFiles/SetFileList`, but `git grep OsClipboardCall HEAD -- app-gpui/src` finds no use outside that file, and no `ide_clipboard` call exists in `app-gpui/src`.
- Rules: the backend reads the OS pasteboard (`ide_clipboard::files`), shared with the user's session; a test would need the clipboard behind a seam.
- Verdict: not built.

### clipboard_has_files

- Webview `src/features/project-tree/clipboard.ts:32` (`hasFiles`): the project tree's Paste item is enabled when the file manager copied files.
- NOT BUILT: `project_view/clipboard.rs:54` `has_files` only maps an answer; nothing asks the OS.
- Verdict: not built.

### clipboard_has_image

- Webview `src/features/project-tree/clipboard.ts:51` (`hasImage`): the project tree offers pasting an image from the clipboard as a file.
- NOT BUILT: no `has_image` or image paste in the project view (`git grep has_image HEAD -- app-gpui/src` empty).
- Verdict: not built.

### clipboard_set_file_list

- Webview `src/features/project-tree/clipboard.ts:41` (`setFiles`): Copy / Cut in the project tree puts the files on the OS clipboard, so the file manager can paste them.
- NOT BUILT: `workbench/project_files.rs:656-664` handles `Effect::HandOff`: `OsClipboardWrite::Text` goes to `cx.write_to_clipboard`, and `OsClipboardWrite::Files(_) => false` with the comment "a file list is kept on the internal clipboard only".
- Verdict: not built.

### clipboard_text

- Webview `src/platform/clipboard.ts:17` (`readClipboardPlainText`): every Paste from a menu (no paste event) reads the clipboard's text natively.
- DIFFERENT FUNCTION: the native app reads the clipboard through GPUI, `cx.read_from_clipboard()` (e.g. `editor/view.rs:1396` `paste`), with no backend call. Nothing is lost: the backend command exists only because WebKit's `navigator.clipboard` cannot read there.
- COVERED (through GPUI's clipboard): `editor::view::editing_tests::clipboard_keys_copy_cut_and_paste`, `app-gpui/src/editor/view.rs:3467`, `assert_eq!(text(&o, cx), "alpha betaalpha \n");` after `cmd-c` then `end cmd-v`.
- Verdict: every site covered (through a different function).

### clipboard_write_text

- Webview `src/platform/clipboard.ts:6` (`writeClipboardText`): every Copy action of the UI (copy path, copy reference, copy text of a view) writes the text natively.
- DIFFERENT FUNCTION: GPUI's `cx.write_to_clipboard(ClipboardItem::new_string(..))` (e.g. `app_views/about_dialog.rs:115`, `workbench/project_files.rs:660`, `debugger/frames_view.rs:282`); no backend call.
- COVERED (through GPUI's clipboard): `app_views::about_dialog::tests::copy_info_fills_the_clipboard_and_close_and_escape_close`, `app-gpui/src/app_views/about_dialog.rs:301`, `assert!(copied.starts_with("About OpenIDEA\n"), "{copied}");`; also `editor::view::editing_tests::clipboard_keys_copy_cut_and_paste`, `editor/view.rs:3465`, `assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("alpha "));`.
- Verdict: every site covered (through a different function).

## Coverage, Cucumber

### coverage_collect

- Webview `src/features/coverage/executor.ts:94`: after a "Run with Coverage" run ends, the report is read and the editor gutter and the Coverage view show line coverage.
- NOT BUILT: `app-gpui/src/features_b/coverage.rs` is a model (types, `coverage_supports`, tree nodes) with no use outside the file (`git grep "features_b::coverage\|coverage::" HEAD -- app-gpui/src` finds nothing else); no `ops::coverage` call.
- Verdict: not built.

### coverage_import

- Webview `src/features/coverage/reports.tsx:55`: Run › Show Coverage Data › import a report file; its coverage is shown.
- NOT BUILT: as above.
- Verdict: not built.

### coverage_prepare

- Webview `src/features/coverage/executor.ts:72`: Run with Coverage asks the backend how to instrument the configuration before starting it.
- NOT BUILT: as above.
- Rules: hypothesis, not read: a coverage run starts the project's real toolchain (JaCoCo agent, coverage.py); check `crates/ide-host/src/ops/coverage.rs`.
- Verdict: not built.

### cucumber_sources

- Webview `src/features/cucumber/stepIndex.ts:47` (`stepIndex`): in a `.feature` file, step completion, navigation to a step's definition and the undefined-step inspection read the project's step definition sources.
- NOT BUILT: no `ops::bdd` call in `app-gpui/src`; only the inspection's settings entry exists (`settings_model/inspections_model.rs:152`).
- Verdict: not built.

## Database

### db_connect

- Webview `src/features/database/store.ts:544` (`connect`): called when the user expands a data source or a database in the Database tree (`DatabaseView.tsx:318`, `:330`), by the plugin data-source connect action (`src/features/database/index.ts:191`), and before consoles and catalog reads (`store.ts:765`, `:775`); the tree marks the source connected.
- Tree expansion: DIFFERENT FUNCTION. The native store never issues `DbOp::Connect` from a view: `DbStore::connect` and `plugin_connect` (`database/store.rs:1817`, `:1825`) have no caller outside its tests (`git grep "\.connect(\|plugin_connect" HEAD -- app-gpui/src`). Expanding a source runs `tree_load` (`workbench/database.rs:1603`) → `DbOp::Schemas`/`DbOp::Databases` → `ops::db::schemas`/`ops::db::databases` (`workbench/database.rs:188-189`), which connect on demand; the user sees the same result.
- COVERED (expansion, through `ops::db::schemas`): `workbench::tests::proofs20_database::the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects`, `app-gpui/src/workbench/tests/proofs20_database.rs:142`, `wait_long(cx, "the database the server lists", |cx| listed(cx) && connected(cx));` (a loopback ClickHouse stand-in).
- Plugin connect action (`index.ts:191`): NOT BUILT (`plugin_connect` has no caller).
- Verdict: some covered.

### db_consoles

- Webview `src/features/database/store.ts:967` (`consoleFiles`): the source's Query Consoles submenu lists `console.sql`, `console_1.sql`, ….
- Webview `src/features/database/store.ts:980` (`newConsole`): New › Query Console creates another console file and opens it.
- Both sites: NOT BUILT. The native app opens one console per target through `DbOp::ConsolePath` → `ops::db::console_path` (`workbench/database.rs:220`, `db.openConsole` at `:741`), the webview's other command `db_console_path`; there is no list of consoles and no "another console".
- Verdict: not built.

### db_database_catalog

- Webview `src/features/database/introspection.ts:131`: for a Snowflake source, introspecting a database reads its schemas with their tables and views and saves the catalog.
- NOT BUILT: `git grep -i catalog HEAD -- app-gpui/src` finds nothing for databases.
- Verdict: not built.

### db_databases

- Webview `src/features/database/store.ts:675` (`loadDatabases`): expanding a source whose driver lists databases (Postgres without "single database") lists the server's databases.
- BUILT, NO TEST: `database/store.rs:1943-1946` (`load_databases`, `DbOp::Databases`) → `workbench/database.rs:188` `ops::db::databases`; also the source dialog's database picker, `workbench/database.rs:1524-1526`. Only Postgres lists databases (`database/tree_view.rs:263-264` `lists_databases`), and the window tests use SQLite or the ClickHouse stand-in; `database::store::tests` at `store.rs:3701` is a store test with a fake backend. A window test needs a loopback Postgres wire stand-in (allowed: loopback, in-process).
- Verdict: built, no test.

### db_retain_catalog

- Webview `src/features/database/introspection.ts:166`: after introspection of a Snowflake source, the saved catalog keeps only the databases still shown.
- NOT BUILT: as for db_database_catalog.
- Verdict: not built.

### db_saved_catalog

- Webview `src/features/database/introspection.ts:52` (`loadSavedCatalog`): opening a Snowflake source's tree first shows what earlier introspection saved, without connecting.
- NOT BUILT: as above.
- Verdict: not built.

### db_schema_objects

- Webview `src/features/database/store.ts:703` (`loadTables`): expanding a schema lists its tables and, as extras, routines and sequences.
- BUILT, NO TEST: `workbench/database.rs:189-192` runs `ops::db::schema_objects` after `ops::db::tables` for `DbOp::Tables`. No window test shows a routine or sequence: the trees' routine tests (`database::tree_view::tests::keeps_an_existing_expanded_routines_folder_and_both_overloads_by_default`) give the view fixture objects. Hypothesis: SQLite (the window tests' database) has no routines or sequences, so a test needs a server dialect that has them (a loopback stand-in); check with `git show HEAD:crates/ide-db/src/...` for the SQLite `schema_objects`.
- Verdict: built, no test.

### db_session_context

- Webview `src/features/database/console.tsx:661`: a query console's toolbar shows the session's current database, schema and role.
- NOT BUILT: no `session_context` in `app-gpui/src`.
- Verdict: not built.

### db_set_ssh_secret

- Webview `src/features/database/Dialogs.tsx:80` (`submit`): connecting through an SSH tunnel that needs a password or passphrase asks for it; the answer is stored and the connection retried.
- Webview `src/features/database/store.ts:396`: saving a data source whose SSH tab has a password stores it.
- Site Dialogs.tsx:80: BUILT, NO TEST: `database/store.rs:1378-1379` (`ConnectRemedy::SshSecret` → `DbOp::SetSshSecret`) → `workbench/database.rs:175-177` `ops::db::set_ssh_secret`. No window test answers an SSH secret question (the proofs20 test answers the database password, `ConnectRemedy::Password`). A test would drive a connect through an in-process SSH stand-in, allowed by the rules.
- Site store.ts:396: COVERED: `workbench::database_extras::tests::saving_a_data_source_with_an_ssh_password_keeps_it_in_the_saved_passwords`, `app-gpui/src/workbench/database_extras.rs:1819`, `wait_long(cx, "the saved password", |_| !holding(home.path()).is_empty());` and `:1822` `assert!(files.iter().all(vault), "only the saved passwords' vault holds it: {files:?}");` (the save path is `database/store.rs:1748-1750`).
- Verdict: some covered.

### db_test_tunnel

- Webview `src/features/database/SourceDialog.tsx:500` (`runTest(true)`): Test Tunnel on the data source dialog's SSH tab says whether the tunnel works and its route.
- NOT BUILT: `git grep -i tunnel HEAD -- app-gpui/src` finds only model helpers (`database/model.rs:577` `ssh_tunnel_ready`); the dialog's test is Test Connection, `ops::db::test` (`workbench/database.rs:1487`), the webview's `db_test` command.
- Verdict: not built.

### db_trust_ssh_host

- Webview `src/features/database/SourceDialog.tsx:517` (`trustAndRetry`): Test Connection meeting an unknown SSH host key offers Trust Host and Retry.
- Webview `src/features/database/store.ts:497`: connecting meets an unknown host key; the user confirms trusting it and the connection is retried.
- Site SourceDialog.tsx:517: COVERED: `workbench::database_extras::tests::trust_host_in_the_data_source_dialog_asks_the_backend_for_that_source`, `app-gpui/src/workbench/database_extras.rs:1846`, `assert_eq!((test.ok, test.text.as_str(), test.trust), (false, "no host key to trust", false));` (native call `workbench/database.rs:1497`). The unknown-key question is injected (`dialog.test_finished(Err(question))`), the Trust click reaches the backend.
- Site store.ts:497: BUILT, NO TEST: `database/store.rs:1381` (`ConnectRemedy::TrustHostKey` → `DbOp::TrustSshHost`) → `workbench/database.rs:179`. Only `database/store.rs:3363` (a store test with a fake backend) reaches it. A window test needs a connect that meets an unknown key: an in-process SSH stand-in, allowed.
- Verdict: some covered.

## Debugger

The native debugger's modules are `app-gpui/src/debugger/*` (`actions`, `attach`, `breakpoints*`, `driver`, `effects`, `evaluate_dialog`, `frames*`, `hotswap`, `java`, `launch`, `state`, `variables_view`, `window_view`, …): no heap snapshot, JVM memory, JavaScript network, event-breakpoint or profiler module. The backend calls are the `BackendCall` variants carried out in `debugger/driver.rs` (about lines 600-830).

### debug_attachable_processes

- Webview `src/features/debugger/attachPopup.ts:21` (`openAttachPopup("all")`): Run › Attach to Process opens a popup listing the machine's processes a debugger can attach to.
- BUILT, NO TEST: `debugger/actions.rs:436-442` (`open_attach_popup`, action `debug.attachToProcess` at `:692`, menu `workbench/menu_spec.rs:569`) → `BackendCall::DebugAttachableProcesses` → `debugger/driver.rs:808-810` `ops::debug::debug_attachable_processes`. `debugger::actions::tests::attach_to_process_opens_the_popup_and_loads_the_process_list` (`actions.rs:2027`) runs on a `Bench` with a fake backend, so it does not reach the backend.
- Rules: `crates/ide-host/src/ops/debug.rs:482` → `crates/ide-dap/src/attach.rs:400` `attachable_processes`, which runs `ps` (`attach.rs:206`). A window test can only stop at the popup with a stand-in list; the backend call itself cannot be driven under the rules.
- Verdict: cannot be tested under the rules.

### debug_completions

- Webview `src/features/debugger/ui/ExpressionCompletion.tsx:106`: typing in the Evaluate field or the Debug console while suspended completes names from the debugger.
- NOT BUILT: no `debug_completions` in `app-gpui/src`; `debugger/evaluate_dialog.rs` has no completion (`git grep -i complet` empty there).
- Verdict: not built.

### debug_free_port

- Webview `src/features/run/javaDebug.ts:197`: debugging a Java configuration whose debug agent's "Listening" line may never reach the console (a Surefire fork) pins the agent to a free loopback port first.
- NOT BUILT: `app-gpui/src/run/java_debug.rs:181` `needs_pinned_port` and `AgentDebug::start(config, pinned, …)` (`:215`) take the port, but `git grep needs_pinned_port HEAD -- app-gpui/src` finds only that file's tests, and no `debug_free_port` call exists.
- Verdict: not built.

### debug_heap_capture

- Webview `src/features/debugger/heap/heapState.ts:112`: Capture Memory Snapshot on a JVM debug session dumps the heap with progress and opens it.
- NOT BUILT: no `ops::heap` or heap view in `app-gpui/src` (`git grep -i heap` finds only `features_b/perf.rs:348` and a JVM option string).
- Verdict: not built.

### debug_heap_classes

- Webview `src/features/debugger/heap/HeapSnapshotEditor.tsx:53` and `heapState.ts:80`: an opened heap snapshot lists its classes with counts and sizes.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_close

- Webview `src/features/debugger/heap/heapState.ts:198` (`closeHeapSnapshot`): closing a snapshot's tab frees it in the backend.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_compare

- Webview `src/features/debugger/heap/HeapSnapshotEditor.tsx:56`: comparing a snapshot with a base shows per-class differences.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_dominators

- Webview `src/features/debugger/heap/HeapDominators.tsx:45`: the dominator tree of a snapshot, page by page.
- Webview `src/features/debugger/heap/editor/BiggestObjects.tsx:26`: a class's biggest objects.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_open

- Webview `src/features/debugger/heap/heapState.ts:152`: opening an `.hprof` file parses it with progress and shows the snapshot.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_path

- Webview `src/features/debugger/heap/HeapPath.tsx:27`: an object's path to a GC root.
- NOT BUILT: as above.
- Verdict: not built.

### debug_heap_save

- Webview `src/features/debugger/heap/heapDumpFiles.tsx:95`: Save As of a captured snapshot writes it to a chosen file.
- NOT BUILT: as above.
- Verdict: not built.

### debug_js_custom_breakpoints

- Webview `src/features/debugger/js/eventBreakpoints.ts:128`: the JavaScript event and XHR breakpoints chosen in the Breakpoints dialog are sent to each live browser page.
- NOT BUILT: no `debug_js_*` in `app-gpui/src`; `debugger/state.rs:1765` drops `DebugEvent::JsNetwork`/`JsNetworkUnavailable` (`=> {}`).
- Verdict: not built.

### debug_js_response_body

- Webview `src/features/debugger/js/NetworkView.tsx:107` and `:209`: the Network tab of a browser debug session shows (and copies) a response's body.
- NOT BUILT: as above (no Network tab).
- Verdict: not built.

### debug_js_response_overrides

- Webview `src/features/debugger/js/responseOverrides.ts:126`: response override rules are sent to each live page.
- NOT BUILT: as above.
- Verdict: not built.

### debug_js_start_profile

- Webview `src/features/debugger/js/profile.ts:38`: Start CPU Profile on the selected page.
- NOT BUILT: no profiler call in `app-gpui/src/debugger`.
- Verdict: not built.

### debug_js_stop_profile

- Webview `src/features/debugger/js/profile.ts:55`: Stop CPU Profile writes the profile and opens it.
- NOT BUILT: as above.
- Verdict: not built.

### debug_jvm_memory_classes

- Webview `src/features/debugger/memory/memoryState.ts:56`: the Memory tab of a suspended JVM session lists loaded classes with instance counts.
- NOT BUILT: no `jvm_memory` in `app-gpui/src`.
- Verdict: not built.

### debug_jvm_memory_fields

- Webview `src/features/debugger/memory/instancesState.ts:127`: expanding an instance in the instances view shows its fields.
- NOT BUILT: as above.
- Verdict: not built.

### debug_jvm_memory_instances

- Webview `src/features/debugger/memory/instancesState.ts:103`: double-clicking a class lists its instances.
- NOT BUILT: as above.
- Verdict: not built.

### debug_jvm_memory_referrers

- Webview `src/features/debugger/memory/instancesState.ts:134`: an instance's referrers.
- NOT BUILT: as above.
- Verdict: not built.

### debug_send_input

- Webview `src/features/debugger/state.ts:1257` (`sendProgramInput`): while a debugged program runs in a terminal the IDE gave it, typing in the Debug console sends the text to the program.
- NOT BUILT (dispatch only): `debugger/driver.rs:753-755` carries out `BackendCall::DebugSendInput` and `debugger/state.rs:1633` `send_program_input` builds it, but its only caller is the test at `state.rs:3172`. The workbench's `RunViewEvent::SendInput` (`workbench/mod.rs:2801-2802`) goes to the run driver's `send_input` (`run/driver.rs:1470`), which writes a run's stdin through `ops::run::stdin`, not a debug session's program.
- Verdict: not built.

### debug_unity_processes

- Webview `src/features/debugger/attachPopup.ts:21` (`openAttachPopup("unity")`): Attach to Unity Editor lists the running Unity editors of a Unity project.
- NOT BUILT: no `debug_unity_processes` call and no Unity mode of the popup in `debugger/actions.rs`; `debugger/attach.rs:127-130` only builds the attach request for a Unity entry of the general list.
- Rules: `crates/ide-host/src/ops/debug.rs:496-505` runs the project's `unity-debug` tool (`unity::editors`), a program of the machine.
- Verdict: not built.

## Dependencies, diff, documents

### dependency_versions

- Webview `src/features/dependencies/index.ts:106`: an open build file (pom, package.json…) marks dependencies with newer versions.
- Webview `src/features/dependencies/index.ts:196`: completing a version at the caret offers the available versions.
- Both sites: NOT BUILT: `app-gpui/src/features_b/dependencies.rs` is a model (`completion_item`, `version_completions`) with no use outside the file (`git grep "features_b::dependencies" HEAD -- app-gpui/src` empty), and no `ops::dependencies` call.
- Rules: hypothesis, not read: the versions come from package registries over the network; check `crates/ide-host/src/ops/dependencies.rs`.
- Verdict: not built.

### diff_external_tool

- Webview `src/features/vcs/DiffEditor.tsx:1086`: the diff viewer's "Show in external tool" opens both sides in the configured external diff program.
- NOT BUILT: only the settings key `externalDiff` (`platform_more/client_settings.rs:95`); no launch.
- Rules: `src-tauri/src/commands/vcs.rs:526-532` (`launch_external_diff`) starts a program of the machine; not in `ide_host`.
- Verdict: not built (and cannot be tested under the rules).

### doc_open_untitled

- Webview `src/editor/core/backend.ts:86` (`openUntitled`): File › New › Untitled file (scratch) opens an unsaved document named `Untitled-N`.
- NOT BUILT: no `open_untitled` in `app-gpui/src`; `untitled` appears only in the new-project dialog's default name and a URI test (`platform/virtual_docs.rs:187`).
- Verdict: not built.

### doc_open_virtual

- Webview `src/editor/core/backend.ts:85` (`openVirtual`): navigating to a library class (`jdt://`, decompiled `csharp:`) opens its read-only document in an editor tab.
- Webview `src/editor/lsp/files.ts:48`: an LSP feature reading another file reads a library class's virtual document.
- Webview `src/editor/navigation/index.ts:34`: navigation reading a target file reads a virtual one.
- Webview `src/features/search/structure/store.ts:47`: File Structure of a library class lists its members.
- Site structure/store.ts:47: BUILT, NO TEST: `everywhere/structure_view.rs:195-196` opens a virtual path with `ops::doc::doc_open_virtual`. The `everywhere::structure_view::tests::*` tests use files on disk.
- Site files.ts:48 / navigation/index.ts:34: BUILT, NO TEST for Super Method only: `workbench/refactor_actions.rs:640-644` (`document_of`) opens a `jdt://` class the server returned. Other LSP readers and navigation were not traced.
- Site backend.ts:85: NOT BUILT: the native library-class tab is modelled in `editor_model/virtual_input.rs` (`virtual_input`, fed by `ide_lsp::types::VirtualDocument`), but `git grep "VirtualDocument\|virtual_input(" HEAD -- app-gpui/src` finds no use outside that file, and `doc_open_virtual` is not called from `editor/`.
- Rules: virtual documents come from a language server (jdtls, a C# server), a real toolchain, unless a test serves one through an in-process LSP stand-in; whether `doc_open_virtual` needs the server for every scheme was not read.
- Verdict: built, no test.

### doc_python_facts

- Webview `src/features/analysis/inheritanceInspections.ts:447` and `:457`: the Python inheritance inspections (unimplemented abstract methods, overrides) read the facts of a file and of its bases.
- NOT BUILT: no `python_facts` in `app-gpui/src`.
- Verdict: not built.

### doc_save_as

- Webview `src/editor/core/backend.ts:87` (`saveAs`): File › Save As (and saving an untitled file) writes the document to a chosen path and re-keys the editor.
- NOT BUILT: `git grep -i "save_as\|Save As" HEAD -- app-gpui/src` finds only `save_as` of window layouts in a test (`workbench/app_actions/tests/proofs20_layouts.rs`).
- Verdict: not built.

### doc_set_bom

- Webview `src/editor/core/backend.ts:95` (`setBom`): the status bar's encoding popup Add BOM / Remove BOM rewrites the file's byte-order mark.
- NOT BUILT: the native editor reads `has_bom` (`editor/view.rs:190`, `:1334`) and changes encoding through `ops::doc::doc_set_encoding` (`editor/view.rs:1325`), but no `doc_set_bom` call.
- Verdict: not built.

### doc_snapshot

- Webview `src/editor/core/backend.ts:92` (`snapshot`): the editor service reads a document's text as the backend holds it.
- Webview `src/features/cucumber/index.ts:73`: Create step definition appends to the step file's current text.
- Webview `src/features/vcs/commitChecks.tsx:142`, `:146`, `:153`: the before-commit checks (Optimize Imports, Cleanup, Reformat) read each file's text to apply code actions to it.
- Webview `src/features/vcs/fileRevisions.ts:67`: Show History for Selection / blame of a file not in an editor reads its text.
- All sites: NOT BUILT through this function: no `ops::doc::snapshot` in `app-gpui/src`. The native editor keeps the text itself (`editor.text()`); the features of the cucumber, commit-check and file-revision sites were not found in the native app (`git grep "source.organizeImports" HEAD -- app-gpui/src` finds only editor intentions and paste-imports).
- Verdict: not built.

### doc_sync_from_disk

- Webview `src/editor/core/backend.ts:91` (`syncFromDisk`, installed by `src/editor/core/controller.ts:466` `installDiskSync`): when the window regains focus (Synchronize files on frame activation), open documents changed on disk are reloaded.
- NOT BUILT: `app-gpui/src/editor_model/disk_sync.rs` models the debounced check, but `git grep "disk_sync::\|DiskSync\|SyncWindow" HEAD -- app-gpui/src` finds no use outside the file. (File › Reload All from Disk is built differently, `workbench/editor_core.rs:87-97`, reloading clean editors, and covered by `workbench::editor_core::tests::reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes`; that is the action, not the focus sync of this site.)
- Verdict: not built.

### doc_ui_mode

- Webview `src/editor/core/backend.ts:101` (`uiModeOf`), used by `src/editor/core/cm/preview.ts:27`: a code preview of a closed file (in a popup) highlights it in its project's mode (an Angular project's templates).
- NOT BUILT for this site: the native code preview (`ui/code_preview.rs:416`) opens the file with `ops::doc::open` and never reads a UI mode (`git grep -i "ui_mode\|framework" HEAD -- app-gpui/src/ui/code_preview.rs` is empty), so a preview of an Angular template is not shown in the framework's mode. The native app does call `ops::doc::doc_ui_mode`, elsewhere: `editor/structure/commands.rs:310-314` picks the framework mode for the structural editing support of an open editor, a use the webview site does not have. No test reaches that call either (no Angular or framework-mode test in `rust_tests.txt`).
- Verdict: not built (for the webview's site; the function is used for another purpose, untested).

## Docker (Services window)

Rules for all ten: `crates/ide-docker/src/lib.rs:82-90` runs the `docker` executable found on PATH (`Command::new(binary()?)`), so the backend functions cannot be driven by a test under the rules (no Docker, no program of the machine) unless `ide_docker` gains a seam. A window test could stop at the request the store emits.

### docker_compose_op

- Webview `src/features/services/store.ts:433`: on a Compose project node the user runs Up / Down / Stop / Restart …; its output streams into a task console and the tree refreshes.
- NOT BUILT (store only): `services/docker.rs:736` models `StreamOp::Compose`, but `workbench/services_window.rs:737-739` logs every `DockerRequest::Stream` as "not carried out in this window".
- Verdict: not built (and cannot be tested under the rules).

### docker_container_op

- Webview `src/features/services/store.ts:343`: Start / Stop / Restart / Pause / Kill on selected containers; the tree shows the new state, a failure notifies.
- NOT BUILT (store only): `services/docker.rs:1614-1638` (`container_op`) emits `DockerRequest::ContainerOp`, which `services_window.rs:737-739` does not carry out.
- Verdict: not built (and cannot be tested under the rules).

### docker_container_stats

- Webview `src/features/services/dashboard.tsx:687` and `:698` (`reloadStats`): a container's dashboard shows CPU, memory, network and block I/O, refreshed.
- NOT BUILT: no stats in `services/docker.rs`/`services/dashboard.rs` (`git grep -i stats` empty there) and no `DockerRequest` for it.
- Verdict: not built (and cannot be tested under the rules).

### docker_container_top

- Webview `src/features/services/dashboard.tsx:1057` and `:1073` (`reload`): a running container's Processes tab lists its processes.
- NOT BUILT: no `container_top` and no `DockerRequest` for it.
- Verdict: not built (and cannot be tested under the rules).

### docker_events

- Webview `src/features/services/store.ts:276`: while a connection is open, Docker's event stream keeps the tree current.
- NOT BUILT: `services_window.rs:710-716` answers `DockerRequest::WatchEvents` as ended at once ("the Docker events stream is not followed; the poll reads it"): the tree is refreshed by polling `ide_docker::snapshot` instead, a different function, so the user sees changes later rather than at once.
- Verdict: not built (and cannot be tested under the rules).

### docker_image_history

- Webview `src/features/services/dashboard.tsx:1278`: an image's dashboard lists its layers.
- NOT BUILT: no image history in `services/*` and no `DockerRequest` for it.
- Verdict: not built (and cannot be tested under the rules).

### docker_inspect

- Webview `src/features/services/docker.tsx:342`: the Inspection tab of a container, image, network or volume shows `docker inspect`'s JSON.
- NOT BUILT (store only): `services/docker.rs:808` `DockerRequest::Inspect` is not carried out (`services_window.rs:737-739`).
- Verdict: not built (and cannot be tested under the rules).

### docker_logs

- Webview `src/features/services/docker.tsx:319`: a container's Log tab streams its log (with timestamps on demand).
- Webview `src/features/services/logEditor.tsx:406`: Open Log in Editor follows the log in an editor tab.
- NOT BUILT: `services/docker.rs:1719`/`:1805` select a Log detail tab, but no `DockerRequest` streams logs, and `DockerRequest::OpenLogInEditor` is in the not-carried-out arm.
- Verdict: not built (and cannot be tested under the rules).

### docker_prune

- Webview `src/features/services/store.ts:415`: Prune (containers, images, volumes, networks or system) after a confirmation; the output streams to a task.
- NOT BUILT (store only): `StreamOp::Prune` (`services/docker.rs:734`) and `DockerRequest::ConfirmPrune` are not carried out.
- Verdict: not built (and cannot be tested under the rules).

### docker_pull

- Webview `src/features/services/store.ts:402` (`pullImage`): Pull Image asks for a reference and pulls it, logging in with a saved registry first.
- NOT BUILT (store only): `DockerRequest::PromptPull` and `StreamOp::Pull` (`services/docker.rs:731`) are not carried out.
- Verdict: not built (and cannot be tested under the rules).

## Summary

All 110 commands read. Verdicts: every site covered 2; some covered 3; built, no test 3; cannot be tested under the rules 1; not built 101 (of which build_compile_file, diff_external_tool and the ten docker commands could not be tested under the rules even once built, as their backends run `javac`, an external diff program or the `docker` CLI).

### Every site covered (can be claimed now)

- clipboard_text: through GPUI's clipboard, not the backend command; `editor::view::editing_tests::clipboard_keys_copy_cut_and_paste`.
- clipboard_write_text: through GPUI's clipboard; `app_views::about_dialog::tests::copy_info_fills_the_clipboard_and_close_and_escape_close` and `editor::view::editing_tests::clipboard_keys_copy_cut_and_paste`.

Both are claims of a different function: the row should say the native app needs no backend call for the text clipboard.

### Some sites covered (the covered site can be claimed, the rest listed)

- db_connect: tree expansion covered (through `ops::db::schemas`) by `workbench::tests::proofs20_database::the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects`; the plugin data source's connect action is not built.
- db_set_ssh_secret: saving a source with an SSH password covered by `workbench::database_extras::tests::saving_a_data_source_with_an_ssh_password_keeps_it_in_the_saved_passwords`; the SSH secret question on connect is built with no test.
- db_trust_ssh_host: the source dialog's Trust Host covered by `workbench::database_extras::tests::trust_host_in_the_data_source_dialog_asks_the_backend_for_that_source`; the connect-time trust question is built with no test.

### Built with no test, where one window test would do (a tests slice)

Harness `workbench/database_extras.rs` tests (`start_isolated`, `trusted_project`, `new_postgres_source`) and `workbench/tests/proofs20_database.rs` (a loopback server stand-in, `clickhouse_server`):

- db_set_ssh_secret, connect-time question (`database/store.rs:1378-1379`): needs an in-process SSH stand-in that asks for a password.
- db_trust_ssh_host, connect-time question (`database/store.rs:1381`): needs an in-process SSH stand-in with an unknown host key.
- db_databases (`database/store.rs:1943-1946`): needs a loopback Postgres wire stand-in, since only Postgres lists databases.
- db_schema_objects (`workbench/database.rs:189-192`): needs a dialect with routines or sequences behind a loopback stand-in (hypothesis: SQLite answers none).

Harness `everywhere::structure_view` tests: doc_open_virtual's File Structure site (`everywhere/structure_view.rs:195-196`) and `workbench::refactor_actions` tests for Super Method (`refactor_actions.rs:640-644`); both need a language server that serves `jdt://` documents, so more than one test's work unless an in-process LSP stand-in exists.

### Cannot be tested under the rules (built)

- debug_attachable_processes: the backend runs `ps` (`crates/ide-dap/src/attach.rs:206`); only the popup with a stand-in list is testable.

### Not built, by feature

- Agents › changes review: agent_accept_changes, agent_base_text, agent_grouped_changes, agent_rollback_changes.
- Agents › side conversations and external sessions: agent_create_ephemeral, agent_external_sessions, agent_import_session.
- Agents › queue and background tasks (dispatch exists, no UI): agent_send_queued_now, agent_stop_background, agent_unqueue.
- Agents › New Task page checks: agent_probe, agent_refresh_models.
- Agents › MCP servers (page and per-task menu): agent_mcp_discover_sources, agent_mcp_import, agent_mcp_import_selected, agent_mcp_list, agent_mcp_remove, agent_mcp_save, agent_mcp_save_json, agent_mcp_set_enabled, agent_mcp_test, agent_set_mcp_overrides.
- Settings › Agents (drafts at HEAD, not compiled): agent_custom_import, agent_custom_remove, agent_custom_save, agent_registry_install, agent_registry_list, agent_registry_remove, agent_set_command, agent_set_env.
- Code analysis: analysis_locate_duplicates, doc_python_facts.
- Android Logcat: android_devices, android_logcat.
- Updates (model at `app_models/updates.rs`, not wired): app_check_update, app_install_update, app_release_notes, app_update_announcement, app_update_status, app_update_window_ready.
- App and Help actions: app_cli_install, app_cli_uninstall, app_log_location.
- Memory / caches: app_metrics, clear_caches, caches_local_data_info, caches_set_local_data_dir.
- Build: build_compile_file.
- OS clipboard file list (project tree, Markdown paste): clipboard_file_list, clipboard_has_files, clipboard_has_image, clipboard_set_file_list.
- Coverage: coverage_collect, coverage_import, coverage_prepare.
- Cucumber: cucumber_sources.
- Database: db_consoles, db_database_catalog, db_retain_catalog, db_saved_catalog (Snowflake catalog), db_session_context, db_test_tunnel; plus db_connect's plugin connect site.
- Debugger: debug_completions, debug_free_port, debug_send_input, debug_unity_processes.
- Debugger › heap snapshots: debug_heap_capture, debug_heap_classes, debug_heap_close, debug_heap_compare, debug_heap_dominators, debug_heap_open, debug_heap_path, debug_heap_save.
- Debugger › JavaScript (network, overrides, event breakpoints, profiler): debug_js_custom_breakpoints, debug_js_response_body, debug_js_response_overrides, debug_js_start_profile, debug_js_stop_profile.
- Debugger › JVM memory view: debug_jvm_memory_classes, debug_jvm_memory_fields, debug_jvm_memory_instances, debug_jvm_memory_referrers.
- Dependencies (model at `features_b/dependencies.rs`, not wired): dependency_versions.
- Diff: diff_external_tool.
- Documents: doc_open_untitled, doc_save_as, doc_set_bom, doc_snapshot, doc_sync_from_disk, doc_ui_mode (the preview site; the function is used for structural editing instead); plus doc_open_virtual's editor-tab site.
- Services › Docker (store models exist; the window carries out only settings, snapshots and refreshes): docker_compose_op, docker_container_op, docker_container_stats, docker_container_top, docker_events, docker_image_history, docker_inspect, docker_logs, docker_prune, docker_pull.

### Uncertainties left

- db_connect's covered site rests on `ops::db::schemas` connecting on demand; the test proves the tree marks the source connected after listing, but I did not read `ide_db`'s connect path.
- The hypotheses marked in the blocks (Android's `adb`, coverage toolchains, dependency registries over the network, MCP discovery reading the home folder, SQLite having no schema objects) were not checked in `crates/`.
- `rust_tests.txt` names some tests differently from HEAD (e.g. `agents::builtin_agents::tests::stages_a_binary_path_and_writes_it_only_on_button` while `workbench/tests/proofs38_settings_agents_list.rs:177` at HEAD has `..._only_on_apply_or_ok`); that HEAD file is not compiled, which fits the list lacking it.
