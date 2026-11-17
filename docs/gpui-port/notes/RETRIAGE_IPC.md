# Re-verification of the `ipc:` rows whose backend changed in the merge of v0.1.46

Read at HEAD `27294748a` (the brief names `f8d8ccb57`; the two commits after it change only `docs/gpui-port/notes/`, measured with `git diff --stat f8d8ccb57 HEAD`). Every `app-gpui/` file read with `git show HEAD:`. Upstream range `37c2a604..v0.1.46`. The fork's own backend commits after v0.1.46 touch `ops/lsp.rs`, `ops/build.rs` and `ops/fs.rs` among the files below; the functions of these rows compare equal between v0.1.46 and HEAD (measured for the six `ops/lsp.rs` functions, `build_run`, `build_sync`, `fs::copy`); the `ops/fs.rs` hunks are in `set_writable` and `delete` (`git diff -U0 v0.1.46 HEAD`), which Part 2 reads.

## Totals

Part 1 (rows marked BODIES CHANGED in `RETRIAGE.md`): 37 rows found in the table (the note's count, 37, is right). Read 37, kept 27, lowered 10.

Part 1b (rows the note marked "bodies unchanged" whose `ops::` function it looked for in the wrong file): a script swept all 263 other table rows: every adapter body compares equal between `37c2a604` and `v0.1.46`, and 64 `ops::` functions were misresolved (not in the named file at either revision). Located in their real file (`ops/vcs/commands.rs`, `ops/doc/commands.rs`, `ops/run/commands.rs`, `ops/search/commands.rs`), 52 compare equal and 12 changed: `vcs_blame`, `vcs_block_history`, `vcs_commit`, `vcs_commit_files`, `vcs_diff_revisions`, `vcs_discard`, `vcs_file_at`, `vcs_line_hunks`, `vcs_remotes`, `vcs_remove_worktree`, `vcs_stage`, `vcs_unstage`. Read 12, kept 12, lowered 0.

Part 2 (adapters the note says call no `ops::` function): 110 by the note (99 in the table and 11 after it); 4 of them are Part 1 rows, so 106 read. Kept 95, lowered 11.

All parts: read 155, kept 134, lowered 21.

Every citation in `lowerings.json` was checked by script against `rust_tests.txt`: all present (39 citations over 21 rows), and every key is a `done` row of `parity.json`. No citation of the 311 `ipc:` rows of the note is among the tests the merge report names as failing after the merge, except `building_a_project_without_a_build_system_ends_in_a_failed_build`, which the report says it fixed (script over the report's names).

Argument and result types: for the 134 kept rows, a script followed each command's signature in `src/bindings.ts` through every referenced type, transitively, and compared each declaration between `37c2a604` and `v0.1.46` (`tools/typedeep.py`, output `typedeep_kept.txt`). Changed types reached by kept rows: `CodeAction` (round trip), `BuildSystem` (new `dotnet`, `javac`; neither side matches on it, measured with grep), `EventKind` (gap recorded under `plugin_post_events`), `PluginInfo` and its new dependency types (`plugin_grant`, `plugin_install_dir`, `plugin_reload`, `plugin_roll_back`, `plugin_set_capability`: the webview discards these answers and lists again, `src/features/plugins/store.ts:125-140`, `:271`, so the gap is recorded once, under `plugin_list`), `Remote` (`vcs_remotes`, used natively), and `Settings` and dozens of nested settings types (`open_project`, `light_edit_open`: the effective settings the window then reads; each new setting belongs to the settings and feature rows that use it, for example `closeWithoutWelcome` under `window_close_context`).

## Native differences found, by native file (work for an integrator)

- `app-gpui/src/editor_lsp/completion.rs:131` (`CompletionSource::ask`) and `:143` (`answer`): an explicit completion (Ctrl+Space) does not bypass the back-off and a successful answer does not clear it; the webview does both (`src/editor/lsp/completion.ts:514`, `:527`, ba0b3d9c7). Size: a few lines (pass `explicit` to `ask`, clear on `Answered::List`). Hypothesis: the webview also clears every back-off when plugins change (`src/features/plugins/bridge.ts`, same commit); check whether `FeatureBackoffs` clearing at `app-gpui/src/editor_lsp/availability.rs:85` is reached from plugin changes.
- `app-gpui/src/editor_lsp/completion_view.rs:89`: the type column takes `item.detail` and ignores `label_description`, which the webview shows first (`src/editor/lsp/completionItems.ts:177`, 0b70ede66). Size: one line, plus the rest of `typeColumnText` if the native type column is meant to follow it.
- `app-gpui/src/workbench/build_tools.rs:647` and the build tool windows: no sbt tool window; the sync's answer carries `BuildModel.sbt`, which the webview shows (`src/features/build-tools/SbtView.tsx:68`, `index.ts:170`, 5150e8e81). Size: a feature.
- `app-gpui/src/plugin_models/plugins_page.rs:1559` (details), `:1720` (permissions): no Requires, Required by, Extension points or allowed net hosts sections (`src/features/plugins/PluginsPage.tsx:353`, `:369`, `:381`, `:569`, `:595`). Size: a feature (four sections; removing a net host needs the new revoke command too).
- `app-gpui/src/plugin_models/plugins_page.rs:922`: disabling a plugin that enabled plugins need does not ask first (`PluginsPage.tsx:404-418`). Size: a few lines once the page has `dependents`.
- `app-gpui/src/plugin_models/plugins_page.rs:1442`, `:1975`: the marketplace shows `publisher`, not `publisherName || publisher` (`PluginsPage.tsx:1044`). Size: one line.
- `app-gpui/src/workbench/plugin_runtime.rs:1406-1556`: no `DocumentChanged`, `WebsocketReady` or `ProblemsChanged` workbench events posted to plugins (`src/features/plugins/runtimeUi.tsx:348`, `:552`, `:369`). Size: a feature (document versions and the problems snapshot behind them).
- `app-gpui/src/agents/store.rs:1106`: New Task sends `mcp_overrides` empty; the webview sends the per-session MCP picker's choices (`src/features/agents/store.ts:989`, `HomeEditor.tsx:207`). Size: a feature (the MCP picker; the merge report lists it).
- `app-gpui/src/agents/cards.rs:65`: a permission entry's `command` and `form` are ignored; the webview titles command requests from `command` (`src/features/agents/permission.ts:15`) and answers forms in a card through `agent_respond` (`SessionEditor.tsx:2365`, `FormCard.tsx:47`). Size: a few lines for `command`; a feature for forms.
- `app-gpui/src/agents/store.rs:798`: `AgentEvent::ModelCatalog` dropped (webview `store.ts:605`); config values' `costCategory`, `usageMultiplier`, `pricing` and `defaultValue` unread, whether they come from the task events, the providers or a fork's answer (webview `ConfigPickers.tsx:111`, `:354`, `store.ts:259`). Size: a feature (the model picker's catalog, cost badges and Reset); one fix closes `agent_subscribe`, `agent_providers` and `agent_fork`.
- `app-gpui/src/workbench/plugin_runtime.rs:1037`: a plugin's net:ask question is answered Deny without asking; the webview asks Allow once / Always allow / Deny (`src/features/plugins/netAsk.tsx:46-48`, `runtimeUi.tsx:437`). Size: a few lines for the dialog with the existing prompt views, plus the revocable host list under `plugin_list`.
- `app-gpui/src/workbench/app_actions.rs:896`: Export Settings calls `export_bundle` without `ide_host::ops::agent::move_user_mcp_values(&host)` first (adapter `src-tauri/src/commands/app.rs:289`). Size: one line. Same gap, not a row of this list: native Import Settings at `app_actions.rs:949` does not call it after `import_bundle` (adapter `app.rs:307`).
- `app-gpui/src/app_models/close_guard.rs:415`: the last project window always goes to Welcome; the webview closes it when `general.closeWithoutWelcome` is on (`src/features/app/closeGuard.ts:52`, `:227`) and drops the saved tabs when `general.closeTabsWithWindow` is on (`closeGuard.ts:204`). No native code reads either setting (`git grep` for `close_without_welcome`/`close_tabs_with_window` under `app-gpui/src`: no hit). Size: a few lines for `closeWithoutWelcome` (plus quitting the app when the platform does); a feature for `closeTabsWithWindow` with its restore on a cancelled close.
- `app-gpui/src/run/driver.rs:1096`: `ops::run::start` (no request id); the webview passes one (`src/features/run/store.ts:1023`) and Stop during "starting" cancels it (`store.ts:1336-1340`). Size: a few lines.
- `app-gpui/src/workbench/vcs_operations.rs:475`: `vcs_stash_push(…, None)` always; the webview has Git › Stash Selected Files… (`src/features/vcs/actions.tsx:705-721`) that passes the selected paths (`changeOps.tsx:335`). Size: a feature (the action, its Commit-view selection and the dialog without a repository choice).
- `app-gpui/src/workbench/mod.rs:3186`, `:3246`: the Welcome list is `recent.list()`, branches as stored; the adapter returns `ide_workspace::recent::with_live_branches(items)` (`src-tauri/src/commands/app.rs:51`). Size: one line, a few to keep the `.git/HEAD` reads off the UI thread.
- `app-gpui/src/editor/view.rs:1289`: Reload with Encoding / Convert to Encoding call `Documents::set_encoding`, not `ide_host::ops::doc::doc_set_encoding`, which now re-reads a virtual document's origin (`crates/ide-host/src/ops/doc/commands.rs:53-63`). Size: a few lines (an async call through the backend instead of the synchronous one). Hypothesis (not run): on a virtual document the native reload fails or reads nothing new; check with Reload with Encoding on a library source opened from a jar.
- `app-gpui/src/settings_view/pages/build_deployment.rs:85`: Test Connection passes `password: None`; the webview passes the typed FTP password (`src/features/settings/pages/deployment.tsx:423`, `:432`). Size: a feature (the page has no FTP type, host, port, user, passive or password fields; the merge report lists them as not ported).
- `app-gpui/src/backend.rs:118` (`AppDirs::resolve`): the native data, config and local-data folders are the platform's, and ignore `OPENIDEA_DATA_DIR` and the other variables of `crates/ide-host/src/data_dirs.rs` (f91b87d79), and the custom local-data root of Settings › System Settings (93642b374), which the adapters now follow (`src-tauri/src/commands/caches.rs:16`). Size: a few lines in `AppDirs::resolve`; it changes every folder the native app uses, so it is the owner's call.

## Lowered rows

### `ipc:lsp_completion` → partial

1. Adapter `src-tauri/src/commands/lsp.rs:10` unchanged. Backend `crates/ide-host/src/ops/lsp.rs:63` `lsp_completion`: when the plugin providers return nothing, it now answers the language server's result (an Unavailable error when there is none) unless a completion or injection plugin covers the document, instead of an empty incomplete list (ba0b3d9c7 "fix(completion): clear feature backoff on plugin updates and explicit completion"). `CompletionItem` gained `label_description` (0b70ede66).
2. Webview: an explicit completion skips the back-off, a successful one clears it (`src/editor/lsp/completion.ts:514`, `:527`); the type column shows `labelDescription` first (`src/editor/lsp/completionItems.ts:177`).
3. Native: `app-gpui/src/editor/intelligence.rs:513` calls it with the adapter's arguments; `CompletionSource::ask` (`app-gpui/src/editor_lsp/completion.rs:131`) applies the back-off whatever `explicit` is, `answer` (`:143`) marks it on Unavailable and never clears it; the type column (`app-gpui/src/editor_lsp/completion_view.rs:89`) takes `detail`.
4. Lower: the cited test still proves typing completes; explicit completion after an Unavailable answer and the label description are missing.

### `ipc:build_sync` → partial

1. Adapter `src-tauri/src/commands/build.rs:47` and its arguments unchanged; `ops/build.rs:225` always sends `ProjectRootsChanged` and refreshes the Java server's library archives (backend only). The answer's `BuildModel` gained `sbt` (5150e8e81 "feat(build): sbt tool window and sbt Task run configurations").
2. Webview: `src/features/build-tools/store.ts:587` keeps the answer as its model (`:592`); `SbtView.tsx:68` and the tool window registration (`index.ts:170`) read `model.sbt`.
3. Native: `app-gpui/src/workbench/build_tools.rs:647`, `:1680` pass the adapter's arguments; `sbt` appears only in test fixtures.
4. Lower: the cited Maven and Gradle tests hold; the sbt builds the answer carries are not shown.

### `ipc:settings_export` → partial

1. Adapter `src-tauri/src/commands/app.rs:284` now calls `ide_host::ops::agent::move_user_mcp_values(&state.host)` (`crates/ide-host/src/ops/agent.rs:421`) before `export_bundle` (22685815c "fix(agents): keep the user's MCP server values in the credential store"). The host also moves them at start (`crates/ide-host/src/lib.rs:194`) and whenever the MCP maps are read (`agent.rs:433`).
2. Webview: `src/features/app/projectActions.ts:67`, arguments unchanged.
3. Native: `app-gpui/src/workbench/app_actions.rs:896` calls `export_bundle` alone. Hypothesis: values written into the user settings after the host started (by hand, by an import) and before an MCP read reach the exported file natively and not from the webview.
4. Lower (one line to fix).

### `ipc:window_close_context` → partial

1. Adapter `src-tauri/src/commands/window.rs:243`: only its comment changed ("the frontend may return it to the welcome screen"), from c1a6079ee "feat(app): close the last window instead of returning to Welcome", which added `general.closeWithoutWelcome` and `general.closeTabsWithWindow` (`crates/ide-core/src/settings.rs`).
2. Webview: `src/features/app/closeGuard.ts:220` reads the context; `returnsToWelcome` (`:52`) is false when `closeWithoutWelcome` is on, then the window closes (`:227`); `forgetTabs` (`:204`) drops saved tabs when `closeTabsWithWindow` is on.
3. Native: `app-gpui/src/app_models/close_guard.rs:415` returns `ToWelcome` for the last window with a project or LightEdit file, with no setting; no native reference to either setting.
4. Lower: the three cited tests hold for the default settings; both settings are missing.

### `ipc:run_start` → partial

1. Adapter `src-tauri/src/commands/run.rs:98` gained `request: Option<RequestId>` and calls `ide_host::ops::run::start_request` (`crates/ide-host/src/ops/run.rs:228`), which begins a cancellable request and refuses a cancelled start; `start` (`:224`) is `start_request(…, None)` (0b6bf83b9 "feat(plugins): validate run target launches"). The backend also wraps runs an MCP agent launched (`host.mcp_agent_runs`), the same for every caller.
2. Webview: `src/features/run/store.ts:1023` passes `request.id`; `stopSession` (`:1336-1340`) cancels a session still starting with `cancelRequest`. `src/platform/dotnet.ts:74` passes `null`.
3. Native: `app-gpui/src/run/driver.rs:1096` calls `ops::run::start`.
4. Lower: the cited test still holds; Stop while starting (plugin run target validation, and the resolution steps the request covers) is missing.

### `ipc:vcs_stash_push` → partial

1. Adapter `src-tauri/src/commands/vcs.rs:433` gained `paths: Option<Vec<PathBuf>>`; `crates/ide-host/src/ops/vcs/commands.rs:803` stashes only those paths, per repository (f154bf8c8 "feat(vcs): stash only the selected files").
2. Webview: `src/features/vcs/changeOps.tsx:330-336` passes `paths` or `null`; the new action `vcs.stashSelected` (`src/features/vcs/actions.tsx:705-721`) passes the Commit view's selection.
3. Native: `app-gpui/src/workbench/vcs_operations.rs:475` passes `None` (the merge report says so too).
4. Lower: Stash Changes without a selection is as the webview's; the selected-files stash is missing. No `vcs.stashSelected` row exists in `parity.json` (measured with grep).

### `ipc:recent_projects` → partial

1. Adapter `src-tauri/src/commands/app.rs:46` now returns `ide_workspace::recent::with_live_branches(items)` (`crates/ide-workspace/src/recent.rs:138`: each project's branch read from its `.git/HEAD`) off the async runtime (c54253fba "fix(welcome): show the live git branch on recent projects").
2. Webview: `src/features/welcome/RecentList.tsx:201` shows `project.branch`.
3. Native: `app-gpui/src/workbench/mod.rs:3186` and `:3246` take `RecentProjects::list()`; `app-gpui/src/workbench/welcome.rs:954` shows the stored branch.
4. Lower: listing and opening hold; the live branch is missing.

### `ipc:doc_set_encoding` → partial

1. Adapter `src-tauri/src/commands/doc.rs:118` returns `DocumentEncoding { encoding, has_bom }` (833ae4c3c). Backend `crates/ide-host/src/ops/doc/commands.rs:45` (not `ops/doc.rs`, which `RETRIAGE.md` names): Reload with Encoding of a virtual document re-reads its origin (`revision_virtual_document` for `git-revision:`, `lsp.reload_virtual` otherwise) and replaces the document (`:53-63`, 5063f6756 "fix(editor): correct library source encoding and Reload with Encoding"). The cited proof's "which ide_host::ops::doc::doc_set_encoding only forwards to" is no longer true.
2. Webview: `src/editor/core/view/StatusItems.tsx:368-372` sets encoding and BOM from the answer and invalidates the virtual document's cache; `src/editor/core/controller.ts:1723` the same on reload from disk.
3. Native: `app-gpui/src/editor/view.rs:1289` calls `Documents::set_encoding` synchronously and keeps `.encoding`.
4. Lower: a disk file reloads as the cited test proves; virtual documents and the BOM state are missing.

### `ipc:deploy_test_connection` → partial

1. Adapter `src-tauri/src/commands/deploy.rs:157` gained `password: Option<String>`; `crates/ide-host/src/ops/deploy.rs:192` calls `test_connection_with` (38d4dd8c1 "feat(deployment): enter an FTP server's password in its connection settings").
2. Webview: `src/features/settings/pages/deployment.tsx:423` (`typedPassword`, FTP only), `:432`.
3. Native: `app-gpui/src/settings_view/pages/build_deployment.rs:85` passes `None`; the page has no FTP fields.
4. Lower: local and SSH tests are unchanged in meaning; the FTP password is missing.

### `ipc:caches_schedule_invalidation` → partial

1. Adapter `src-tauri/src/commands/caches.rs:15`: the data folder is now `ide_host::data_dirs::data_dir(identifier)` (`crates/ide-host/src/data_dirs.rs:25`, `OPENIDEA_DATA_DIR` first) instead of Tauri's `app_data_dir` (f91b87d79 "feat: move configuration, user data and logs with environment variables").
2. Webview: arguments unchanged.
3. Native: `app-gpui/src/workbench/app_actions.rs:842` schedules in `backend.dirs.data`, which is `dirs::data_dir()/<identifier>` (`app-gpui/src/backend.rs:118`), without the variable.
4. Lower: identical without the variable; with it the native app uses another folder. Hypothesis: the native start reads the pending invalidation from the same `backend.dirs.data`, so native alone stays consistent; the difference is with the webview's folder and the user's setting.

## Kept rows

Argument and result types of the kept rows were compared in `src/bindings.ts` between `37c2a604` and `v0.1.46` (`BuildTool`, `BuildKind`, `BuildEvent`, `BuildResult`, `LaunchConfig`, `RunConfig`, `RunInfo`, `OutputChunk`, `SymbolFilter`, `SymbolMatch`, `FileMatch`, `ShelvedChangeList`, `ChangelistState`, `LineHunk`, `WorkspaceEdit`, `CompletionList`, `Command`, `GotoKind`, `Location`, `InvalidateCachesOptions`, `IpcPath`): unchanged. `CodeAction` gained `diagnostics`/`originalDiagnostics` (round trip, see below); `RecentProject` changed only its comment.

- `ipc:apply_workspace_edit`: adapter `lsp.rs:566` unchanged; `ops/lsp.rs:1749` moved the Local History point into `record_before_multi_file_edit` (`:1771`, same logic). Backend only. Native `app-gpui/src/workbench/editor_language.rs:832` passes the edit as the adapter.
- `ipc:lsp_apply_code_action`: adapter `lsp.rs:267` unchanged; `ops/lsp.rs:1092` first runs the action's command through a plugin that claims it (`run_plugin_server_command`, `:1844`; ad236cf1a "feat(plugins): route a server's client commands to a plugin (serverCommands, API 1.9)"). Backend only; native calls at `editor/intelligence.rs:1436`, `editor_language.rs:501`, `:529` with the adapter's arguments.
- `ipc:lsp_code_actions`: adapter `lsp.rs:231` unchanged; plugin quick fixes now edit within the project's content roots (`plugin_edit_roots`, 757412cb7 "fix(security): reach content roots in plugin quick fixes and keep diagrams local"). `CodeAction` gained `diagnostics`/`original_diagnostics`, which round-trip through the backend; the webview shows neither (grep). Backend only.
- `ipc:lsp_execute_command`: adapter `lsp.rs:599` unchanged; `ops/lsp.rs:1804` routes a command a plugin claims to it before the server (ad236cf1a). Backend only; native `intelligence.rs:696`, `plugin_contributions.rs:761`.
- `ipc:lsp_goto`: adapter `lsp.rs:73` unchanged; `ops/lsp.rs:316` no longer follows a plugin's link when the caret is on a declaration's name, so the server's answer comes back and the editor shows usages (4fcf4555a "fix(navigation): show usages on a declaration a plugin's index links elsewhere"); a notebook cell skips plugin navigation (fd5f4b863). The webview made the same check on its client-side plugin step (`src/editor/lsp/navigation.ts:455`); the native app has no client-side plugin step (no `navigation_at` under `app-gpui/src`), takes the backend's answer, and shows usages on a self target (`app-gpui/src/editor_lsp/navigation.rs:469`). Backend only.
- `ipc:build_run`: adapter `build.rs:168` unchanged; `ops/build.rs:745` gained the "Up to date: no changes, build skipped." fast path for an unchanged single Maven project and builds a Java project without a build tool from its Project Structure. Backend only; native `workbench/build_window.rs:157` passes the adapter's arguments. The webview's new Compile File uses a new command (`buildCompileFile`), not this one.
- `ipc:debug_start`: adapter `debug.rs:14` unchanged; `ops/debug.rs:12` dropped the plugin run-target path (2b80c7531), added Unity attach checks (aa8822d9a), the Kotlin debugger choice (c73d85889), the JVM inspector for the memory view (a3d1e57f3), the Go compiler (5e7304a75). Backend only for one config; which configs the native app builds belongs to the run and debugger rows (the merge report lists Unity and .NET attach as adapted).
- `ipc:find_files`, `ipc:find_symbols`: adapter and `ops/nav.rs:57`, `:72` became async on the blocking pool (c8112f901); same arguments and answer; every production native call awaits them (`everywhere/view.rs:323`, `:341`, `run/driver.rs:659`, `debugger/driver.rs:954`, `editor_language.rs:979`; the `block_on` calls are in test modules).
- `ipc:sdk_python_effective`: adapter `sdk.rs:40` unchanged; `ops/sdk.rs:121` falls back to the primary project's interpreter for a workspace member with none (cc29842e6). Backend only; native `run_context.rs:222` passes `near` as `src/features/run/runtimes.ts:86`. The proof names `ops::sdk::python_effective`; the function is `sdk_python_effective` (wording only). The webview's Python status widget now passes the open file's member (`src/features/settings/languages/PythonStatus.tsx:119`); the native app has no such widget (no match for `PythonStatus`/`python_status`), which is not this row.
- `ipc:db_console_path`: adapter `db.rs:362` unchanged; `ops/db.rs:476` now shares `console_dir` with the new `db_consoles` (ad8df8fe0), same path and meaning. Native `workbench/database.rs:220`. The new New › Query Console (another console each time, `db_consoles(id, true)`, `src/features/database/store.ts:980`) has no `ipc:db_consoles` row in `parity.json` (grep).
- `ipc:run_apply_settings`, `ipc:run_without_derived`: adapters unchanged; `ops/run.rs:203`, `:186` pass the primary project as a fallback for a member (cc29842e6). Backend only; native `run/driver.rs:298`, `:238`.
- `ipc:run_save_configs`, `ipc:run_templates`: adapters now run the same function on the blocking pool; `save_configs` (`ops/run.rs:122`) and `run_templates` (`ops/run/commands.rs:98`, not `ops/run.rs`) compare equal. Native `run/driver.rs:605`, `:562`.
- `ipc:run_stdin`, `ipc:run_stop`: adapters unchanged; `ops/run.rs:481`, `:464` tell `host.mcp_agent_runs` of the user's input and the stop. Backend only; native `run/driver.rs:1459`, `:1293`.
- `ipc:vcs_changelists_load`, `ipc:vcs_shelf_delete`, `ipc:vcs_shelf_patch`, `ipc:vcs_shelf_rename`, `ipc:vcs_shelves`, `ipc:history_content`: adapters now call the same function on the blocking pool; the functions (`ops/vcs/commands.rs:1415`, `:1372`, `:1382`, `:1377`, `:1340`; `ops/doc/commands.rs:205`) compare equal.
- `ipc:vcs_diff_hunks`: adapter runs off the main thread now; `ops/vcs/commands.rs:931` equal; the native code calls `ide_vcs::diff_hunks` as before.
- `ipc:vcs_rename_branch`: adapter and `ops/vcs/commands.rs:778` gained `unset_upstream` (38b56e724). Native `app-gpui/src/workbench/branches.rs:869-872` passes the dialog's checkbox when the branch has an upstream, as `src/features/vcs/branchOps.ts:295-298`. The merge report (passes `false`) is out of date: 587b5a7ec adapted it.
- `ipc:fs_copy`: adapter and `ops/fs.rs:177` gained `window` and tell the checkout backend of the added file. Native `workbench/project_files.rs:764` passes its label.
- `ipc:fs_read_image`: adapter unchanged; `ops/bytes.rs:22` reads a network path only from a share an open project is on (a497178b3 "fix(security): close the findings of GHSA-442p-gxfg-qxv9"). Backend only; native `workbench/tab_views.rs:857` calls it.

## Part 1b: misresolved `ops::` functions that changed (12 rows, all kept)

Each adapter body compares equal; each native call goes through the same `ops::` function (`git grep` under `app-gpui/src`); argument and result types compared in `src/bindings.ts`.

- `ipc:vcs_blame`, `ipc:vcs_block_history`, `ipc:vcs_line_hunks`, `ipc:vcs_file_at`, `ipc:vcs_commit_files`, `ipc:vcs_diff_revisions`, `ipc:vcs_commit`, `ipc:vcs_discard`, `ipc:vcs_stage`, `ipc:vcs_unstage` (`crates/ide-host/src/ops/vcs/commands.rs`): `with_repo_*` / `per_repo` became `with_backend_*` / `per_backend` (b0d20d799 "feat(vcs): Backend trait for the Changes-view surface"); Git behaves as before, other checkout backends are reached the same way for every caller. Backend only. Native calls: `workbench/annotate.rs:100`, `vcs_text_actions.rs:321`, `editor_gutter.rs:501`, `github/review_editor.rs`, `log_actions.rs:714`, `branches.rs:293`, `commit.rs:316`, `vcs_operations.rs:523`, `conflicts.rs:169`, `shelf.rs:1764`.
- `ipc:vcs_remotes` (`ops/vcs/commands.rs:1112`): fills `Remote.resolved_url` from `~/.ssh/config`. The webview uses it to parse GitHub remotes (`src/features/github/data/repository.ts:114`, `reviewMode/model.ts:50`); native `github/repository.rs:52` and `github/review_mode.rs:59` do the same (merge report). Keep.
- `ipc:vcs_remove_worktree` (`ops/vcs/commands.rs:1160`): the check that the worktree does not hold the open project was renamed and takes its message as an argument; same message. Backend only; native `branches.rs:904`.

## Part 2: adapters with no direct `ops::` call (106 rows)

Method: each adapter body at HEAD; its callees one level down located with `git grep` at v0.1.46 and diffed against `37c2a604` (`tools/callee.py`); argument and result types compared in `src/bindings.ts`; where a type gained a field, the webview's use of it (`git grep` under `src/`) against the native code's (`git grep` under `app-gpui/src`). Callees further down not followed.

### Lowered (11)

#### `ipc:plugin_list` → partial

1. Adapter `src-tauri/src/commands/plugin.rs:176` and its callees (`wait_initial_load`, `sync_project`, `list`) compare equal except `sync_links` (`crates/ide-host/src/plugins/mod.rs:748`, now reconciles dependencies). `PluginInfo` gained `netHosts`, `depends`, `dependents`, `extensionPoints`; `PluginStatus` gained `blocked` (835a9c883 "feat(plugins): let plugins depend on each other and declare extension points (API 1.5)", 11594437b "… (net:ask)").
2. Webview: `src/features/plugins/PluginsPage.tsx:353` (Requires), `:369` (Required by), `:381` (Extension points), `:569`, `:595` (allowed hosts, each with Remove).
3. Native: `app-gpui/src/plugins.rs:335` lists as the adapter; `plugin_models/plugins_page.rs:1559` (details) and `:1720` (permissions) read none of the new fields; "Blocked" is shown (merge report).
4. Lower. The same `PluginInfo` comes back from `plugin_grant`, `plugin_install_dir`, `plugin_reload`, `plugin_roll_back`, `plugin_set_capability`; the webview discards those answers and lists again (`src/features/plugins/store.ts:125-140`; `rollBackPlugin` reads only `name` and `version`, `:271`), so they are kept and the gap is recorded once, here.

#### `ipc:plugin_market` → partial

1. Adapter `plugin.rs:658` and `market_listing` (`plugins/market.rs:246`) equal; `MarketPlugin` gained `publisherName` (908740b3f "feat(plugins): a marketplace that helps pick plugins").
2. Webview: `PluginsPage.tsx:1044` shows `publisherName || publisher`.
3. Native: `plugins_page.rs:1442`, `:1975` use `publisher`.
4. Lower (one line).

#### `ipc:plugin_post_events` → partial

1. Adapter `plugin.rs:367` and `post_window_events` (`plugins/mod.rs:1879`) equal; `WorkbenchEvent` gained `documentChanged`, `websocketReady`, `extensionsChanged`, `problemsChanged`, `scheduled`, and `EventKind` the matching kinds.
2. Webview posts `documentChanged` (`src/features/plugins/runtimeUi.tsx:348`), `problemsChanged` (`:369`), `websocketReady` (`:552`) when a plugin wants them (39d798e8c, c8bdfa8ed). `extensionsChanged` and `scheduled` are not posted by the webview (no match under `src/` outside `bindings.ts`): Hypothesis: the host raises them itself.
3. Native: `app-gpui/src/workbench/plugin_runtime.rs:1406-1556` posts file, settings, VCS and action events only.
4. Lower.

#### `ipc:plugin_set_enabled` → partial

1. Adapter `plugin.rs:193`; `set_enabled` (`plugins/mod.rs:850`) now reconciles dependents and restarts optional ones. Backend only for the call.
2. Webview asks before disabling a plugin that enabled plugins need (`PluginsPage.tsx:404-418`).
3. Native: `plugins_page.rs:922` sends at once; no `dependents` in native code.
4. Lower.

#### `ipc:agent_create` → partial

1. Adapter `src-tauri/src/commands/agent.rs:262` and `ops::agent::create` (`crates/ide-host/src/ops/agent.rs:225`) equal; `NewAgentTask` gained `mcpOverrides` (32abc77a7 "feat(agents): multi-level MCP hub, CLI config discovery, and per-session MCP picker").
2. Webview: `src/features/agents/store.ts:989` sends `draft.mcpOverrides ?? {}`, set by the MCP picker (`HomeEditor.tsx:207`).
3. Native: `app-gpui/src/agents/store.rs:1106` sends an empty map (the merge report lists it).
4. Lower.

#### `ipc:agent_subscribe` → partial

1. Adapter `agent.rs:21` and `ops::agent::subscribe` (`ops/agent.rs:21`) equal; `AgentEvent` gained `append`, `attention`, `modelCatalog`; `AgentConfigValue` gained `costCategory`, `usageMultiplier`, `pricing`; `AgentConfigOption` gained `defaultValue`.
2. Webview: `store.ts:570` (attention), `:605` (modelCatalog), `ConfigPickers.tsx:111`, `:354` (cost badges), `store.ts:259` (Reset to `defaultValue`).
3. Native: `app-gpui/src/agents/store.rs:771` handles `Append` and `:789` `Attention` (587b5a7ec and the merge); `:798` drops `ModelCatalog`; no native reference to `cost_category`, `usage_multiplier`, `pricing` of a model, or `default_value` of a config option.
4. Lower.

#### `ipc:agent_entries` → partial

1. Adapter `agent.rs:251` and `ops::agent::entries` (`ops/agent.rs:207`) equal; `AgentEntry::Permission` gained `command` (b43db63ff "fix(agents): judge permission requests by their kind and command, not their title") and `form` (c67b336a8 "feat(agent): answer ACP form elicitation").
2. Webview: `src/features/agents/permission.ts:15` (command), `SessionEditor.tsx:2365` (`FormCard`).
3. Native: `app-gpui/src/agents/cards.rs:65` destructures `title`, `options`, `questions`, `chosen` and ignores the rest.
4. Lower.

#### `ipc:agent_respond` → partial

1. Adapter `agent.rs:412` and `ops::agent::respond` (`ops/agent.rs:300`) equal; the `option` now also carries a form's JSON answer or `decline` (c67b336a8).
2. Webview: `src/features/agents/FormCard.tsx:47` answers through `respond` (`store.ts:1490`).
3. Native: no form card (`cards.rs:65`).
4. Lower.

#### `ipc:plugin_answer_prompt` → partial

1. Adapter `plugin.rs:415` and `answer_prompt` (`plugins/mod.rs:1780`) equal; `Prompt` gained `NetPermission { host, sessionOnly }` (11594437b "feat(plugins): ask at first connect to a host the manifest does not list (net:ask)").
2. Webview: `src/features/plugins/runtimeUi.tsx:437` asks with `askNetPermission` (`netAsk.tsx:46-48`: Allow once, Always allow, Deny) and answers with the choice.
3. Native: `app-gpui/src/workbench/plugin_runtime.rs:1037` answers `None` at once (logged); the merge report lists it as not adapted.
4. Lower.

#### `ipc:agent_providers` → partial

1. Adapter `agent.rs:37` and `ops::agent::providers` (`ops/agent.rs:25`) equal; `AgentProvider.config` / `thoughtLevels` hold `AgentConfigOption`s, whose values gained `costCategory`, `usageMultiplier`, `pricing` and the options `defaultValue` (7b6962001 "feat(agents): two-page model picker with pinned models, segmented effort, Reset and ACP cost badges").
2. Webview: `ConfigPickers.tsx:111`, `:354`, `store.ts:259`; providers read at `store.ts:455`, `:779`.
3. Native: no reference to those fields under `app-gpui/src`.
4. Lower.

#### `ipc:agent_fork` → partial

1. Adapter `agent.rs:338` and `ops::agent::fork` (`ops/agent.rs:260`) equal; the returned `AgentTask` carries the same new config fields.
2. Webview: `store.ts:1152-1156` keeps the fork's task and opens it; its picker shows the fields as above.
3. Native: as above.
4. Lower.

### Kept (95), one line each

- `src-tauri/src/commands/window.rs` (7): `app_build_info`, `app_cancel_quit`, `menu_refresh`, `window_cycle`, `window_new`, `window_open_files`, `window_toggle_fullscreen`: callees `windows::cycle`, `create_window`, `open_files_window`, `window_groups::toggle_fullscreen`, `persist`, `menu::install`, `updates_configured` compare equal; `BuildInfo` unchanged. Shell commands the native app does itself.
- `src-tauri/src/commands/app.rs` (13): `cancel_task`, `languages`, `light_edit_open`, `open_project`, `project_trust_list`, `project_trust_status`, `project_trust_set`, `recent_project_groups`, `remove_recent_project`, `set_recent_project_groups`: callees equal. `project_trust_add_location`, `project_trust_remove_location`: `Host::refresh_project_trust` (`crates/ide-host/src/trust.rs:245`) now also prepares a container Python interpreter; native calls the same function (`app_actions.rs:441`, `settings_feature_pages.rs:68`). `close_project`: `AppState::close_project` (`src-tauri/src/state.rs:181`) now clears the window's system notifications (83805734b "feat(notifications): system notifications for balloons while OpenIDEA is in the background"); the native app posts no system notifications (no match under `app-gpui/src`), so there is nothing to clear; the missing system notifications are not this row.
- `src-tauri/src/commands/fs.rs` (7): `fs_create_dir`, `fs_create_file`, `fs_delete`, `fs_entry_kind`, `fs_read_dir`, `fs_read_text`, `fs_set_writable`: six of them call `ide_host::ops::fs::*` through `use ide_host::ops::fs` (the note says "no ops:: call"); `create_file` and `delete` now tell a checkout backend of the added or deleted file (b0d20d799), the rest equal; native calls the same functions (`project_files.rs:756`, `backend.rs:83`, …). The fork's own hunks in `delete` and `set_writable` (v0.1.46..HEAD) reach every caller.
- `src-tauri/src/commands/plugin.rs` (27 kept of 32): `plugin_answer_command`, `plugin_assets`, `plugin_cancel_task`, `plugin_catalog`, `plugin_context_keys`, `plugin_event_kinds`, `plugin_finish_operation`, `plugin_log_tab_spawn`, `plugin_market_url`, `plugin_preview_install`, `plugin_secrets`, `plugin_set_active_file`, `plugin_set_market_url`, `plugin_set_secret`, `plugin_set_theme`, `plugin_status_items`, `plugin_subscribe`, `plugin_unpack_package`, `plugin_view_event`, `plugin_view_render`: callees equal. `plugin_grant`, `plugin_install_dir`, `plugin_reload`, `plugin_set_capability`, `plugin_uninstall`: the host now settles dependents after them (backend only); the webview discards their `PluginInfo` and lists again (`src/features/plugins/store.ts:125-140`), so the display gap is under `plugin_list`. `plugin_roll_back`: equal. `plugin_market_download`: refuses a download not reachable from the marketplace in use (backend only).
- `src-tauri/src/commands/notebook.rs` (8): all call `ide_host::ops::notebook::*` through `use ide_host::ops::notebook`; every function equal; `NotebookSnapshot`, `NotebookEdit` unchanged; native calls them through `use ide_host::ops::notebook as ops` (`workbench/notebook.rs:36`). The new `notebook_cell_docs` is not one of these rows.
- `src-tauri/src/commands/http.rs` (6): `http_environments`, `http_history`, `http_history_get`, `http_history_clear`, `http_requests` callees equal; `http_execute`: `ide_http::run` (`crates/ide-http/src/lib.rs:35`) split into `prepare` and `send` with the same steps (d2b0bd6e7, 7cc100367); types unchanged; native `workbench/http_client.rs:308` calls `ide_http::run`.
- `src-tauri/src/commands/agent.rs` (8 kept of 14): `agent_archive`, `agent_cancel`, `agent_delete`, `agent_prompt`, `agent_rename`, `agent_rollback`, `agent_set_config`, `agent_unarchive`: all call `ide_host::ops::agent::*` through `use ide_host::ops::agent`, every function equal, argument and result types unchanged (transitive check).
- `src-tauri/src/commands/ssh.rs` (5): `git_credentials`, `git_credential_forget`, `ssh_needs_secret`, `ssh_set_secret`, `ssh_test`: callees equal (`ide_ssh::git_creds`, `SshManager`); `SshConfiguration` unchanged.
- `src-tauri/src/commands/mcp.rs` (2): `mcp_report_problems`, `mcp_set_open_editors`: `McpReports` methods equal.
- `src-tauri/src/commands/db.rs` (1): `db_export_file`: writes the file, nothing else.
- Adapter file unchanged (11): `docker_settings`: `ide_docker::load_settings` now drops the connection an older version injected (d5cb379ca), for every caller; Hypothesis: `AppState.data_dir` now follows `OPENIDEA_DATA_DIR` while native `backend.dirs.data` does not; that app-wide difference is recorded once, under `caches_schedule_invalidation`, and not counted against this row or `db_console_path`. `gh_api`, `gh_availability`, `gh_pr_checkout`: callees equal. `gh_pr_prepare_diff`: `ide_github::prepare_diff` now requires a trusted repository (backend only; native `workbench/github.rs:1420`). `term_write`, `term_resize`: `Terminals::write`/`resize` (`crates/ide-terminal/src/lib.rs:310`, `:324`) now also drive SSH channel sessions (76faa39bc), same for PTYs; `term_is_busy`, `term_kill`, `term_available_shells`, `term_default_shell`: equal.

## Corrections to `RETRIAGE.md` and the merge report

- The note's resolver looks for an `ops::` function only in `ops/<module>.rs`, and when it is not there it reports "unchanged": 64 functions of 263 "bodies unchanged" rows live in `ops/vcs/commands.rs` (53), `ops/doc/commands.rs` (5), `ops/run/commands.rs` (3), `ops/search/commands.rs` (3), measured by script. 12 of them changed (Part 1b, all kept). The same bug hides three Part 1 changes it marked "unchanged": `vcs_rename_branch` and `vcs_stash_push` (new parameters) and `doc_set_encoding` (new return type and virtual-document branch). So the 37 "BODIES CHANGED" rows are a lower bound: 12 more rows have a changed `ops::` body, and `fs_create_file` and `fs_delete` (behind the alias below) two more.
- The note reports "no ops:: backend call" for adapters that call `ops::` through an imported module alias: `fs.rs` (`use ide_host::ops::fs`), `notebook.rs` (`use ide_host::ops::notebook`), `agent.rs` (`use ide_host::ops::agent`). Their functions are compared in Part 2.
- The note names `ops/run.rs::start_request` as the backend of `run_start`; at `37c2a604` the adapter called `ops::run::start`, and `start_request` is new.
- Merge report: native `vcs_rename_branch` no longer passes `false` (587b5a7ec adapted it); native handles `AgentEvent::Attention` now (`agents/store.rs:789`), not ignores it.
- The brief names HEAD `f8d8ccb57`; HEAD is `27294748a`, two notes-only commits later.
