# Plugin host in the native (GPUI) app: assessment for the Bazel plugin

Abbreviations: `W` = `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`, `B` = `/Users/davidvieira/code/openidea-bazel`, `AS` = `~/Library/Application Support`.

Evidence labels: **read** = I read that file and line in this session. **measured** = I ran a read-only command (ls, cat) in this session and quote its output. **reported** = someone else's measurement in a repo doc. **hypothesis** = native runtime behaviour inferred from reading code; each one names the test that would confirm it. Nothing was compiled or run (no cargo, git, pnpm or bazel), as instructed.

## 1. The Bazel plugin

### What it is made of (read)

- Manifest `B/plugin.toml`: id `davidhsv.bazel`, version 0.1.0 (lines 1-3), `engines.openidea = "^1.4"` (line 9), `wasm = "dist/plugin.wasm"` (line 10), activation `on-command:*`, `on-startup-finished`, `on-event:files-changed`, `on-event:editor-focused` (line 11).
- Capabilities: required `project.read`, `project.write`; optional `process:bazelisk`, `process:bazel`, `fs.read:/private/var/tmp`, `fs.read:~/.cache/bazel`, `events`, `storage`, `editor.read` (lines 13-19).
- It is a Rust crate (`B/Cargo.toml`, `crate-type = ["cdylib","rlib"]`). It depends on the SDK `openidea-plugin` from git `davidhsv/blazingly-fast-ide`, branch main (`B/Cargo.toml` dependencies; `B/Cargo.lock:106-108` pins commit `ff07083b…`). That SDK is the one in this repo at `W/sdk/rust/openidea-plugin`.
- WIT: package `openidea:plugin@1.4.0` (`W/wit/plugin.wit:11`; every `W/wit/*.wit` line 1). `register_plugin!(BazelPlugin, language_features)` (`B/src/plugin.rs:860`) composes the base `plugin` world with the export-only world `editor-language-features` (`W/sdk/rust/openidea-plugin/src/lib.rs:290,1342-1345`; `W/wit/plugin.wit:191-193`). The host's API version is `"1.4"` (`W/crates/ide-plugin/src/runtime.rs:57`).
- Build: `cargo build --release --target wasm32-wasip2 --lib`, then copy to `dist/plugin.wasm` (`B/README.md:41-45`). `B/.gitignore` ignores `/dist`, so the wasm is a local build, not checked in (read). On disk it is `B/dist/plugin.wasm`, 895,785 bytes, dated Nov 11 09:56 (measured).
- Linking: README says "Install Plugin from Disk…" or `openidea-plugin link .` (`B/README.md:47`). The CLI writes `links.json` in `$OPENIDEA_PLUGINS_DIR`, otherwise in `dirs::data_dir()/ai.l0ng.openidea/plugins`, which is the installed app's folder (`W/crates/ide-host/src/bin/openidea-plugin/main.rs:17-18,423-429,446-448`).
- Where the installed app looks: Tauri creates the host at `app_data_dir()/plugins` (`W/src-tauri/src/state.rs:69,106`; `W/src-tauri/src/commands/plugin.rs:106`). The host loads `<plugins_dir>/*` (installed) and then every path in `<plugins_dir>/links.json` (`W/crates/ide-host/src/plugins/mod.rs:552-563,2782-2793`).
- Measured: `AS/ai.l0ng.openidea/plugins/links.json` holds `{"paths": ["/Users/davidvieira/code/openidea-bazel"]}`. That folder also has `.cache/components`, `.data/davidhsv.bazel`, `grants.json` and `market-index.json`, and `.data/davidhsv.bazel/files` contains `jdtls`, `query.txt` and `targets.txt`.

### What it contributes (read; `B/plugin.toml`)

| Kind         | Id / name                                                                                                                                                  | Declared at                          |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| command      | `sync` "Bazel: Sync Project with BUILD Files"                                                                                                              | 21-25                                |
| command      | `sync-changed`                                                                                                                                             | 27-31                                |
| command      | `open-sync-log`                                                                                                                                            | 33-37                                |
| command      | `open-project-view`                                                                                                                                        | 39-43                                |
| command      | `gutter-test` (when `workspaceContains:.openidea/bazel/targets.tsv`)                                                                                       | 45-49                                |
| command      | `run-target`, `debug-target` (when `workspaceContains:.openidea/bazel/query.txt`)                                                                          | 55-65                                |
| command      | `test-file`, `debug-test-file`                                                                                                                             | 67-77                                |
| provider     | `gutter-actions` for `java`                                                                                                                                | 51-53                                |
| menus        | `editor.context` › submenu Bazel: test-file, debug-test-file; `tools`: sync, run-target, debug-target; `projectView.context`: sync (when MODULE/WORKSPACE) | 79-87, 101-116                       |
| settings     | `settings.schema.json`: `syncOnOpen` (bool, default true), `runEnvironment` (string list)                                                                  | 89-90; `B/settings.schema.json:3-15` |
| languages    | `starlark`, `bazelproject`, `bazelrc` (data only: extensions, filenames, comment, indent, `ui-mode`, `tree-sitter = "python"` for starlark)                | 92-99; `B/languages/*.toml`          |
| runDetectors | `bazel-test` (test, debugger `java-debug`, `--java_debug`), `bazel-build-all`, `bazel-test-all`                                                            | 120-143                              |

All commands except `gutter-test`, `run-target` and `debug-target` use `when = "workspaceContains:MODULE.bazel || …WORKSPACE || …WORKSPACE.bazel || …:.bazelproject"`.

The plugin contributes no tool windows, views, web views, custom editors, service providers or themes. Its only UI is commands, menus, a status bar item, notifications, a background task, prompts (input and pick), a log tab, Run tool window runs, and gutter icons.

### What it does at run time and which host functions it calls (read; `B/src/plugin.rs`, `B/src/ide.rs`)

- **Activation (`plugin.rs:53-86`).**
  - It picks `bazelisk` or `bazel`, whichever capability was granted.
  - With `events` granted, it subscribes to `files-changed` on the BUILD globs (`**/BUILD`, `**/BUILD.bazel`, `**/*.bzl`, `MODULE.bazel`, `WORKSPACE*`, `.bazelproject`, `.bazelrc`; lines 19-28), and to `EditorFocused`, `VcsChanged` and `SettingsChanged` (lines 62-71). WIT: `events.subscribe-paths` and `events.subscribe` (`W/wit/events.wit`).
  - For every project in `host::projects()` (WIT `workspace`) it calls `on_project_open` (lines 76-84). For a Bazel workspace without generated project data, that **syncs immediately** when `syncOnOpen` is true (lines 163-190, 177-181). Otherwise it notifies with a "Sync" action.
- **Events (`plugin.rs:118-156`).** Any event from a window counts as that project opening. BUILD-file edits newer than the last sync, or a `VcsChanged` event, mark the workspace out of sync and set the status item. A `SettingsChanged` event with `ide.mode` set to light (Blaze mode) removes the status item and runs `bazel shutdown` in each Bazel workspace (lines 270-289; `host::run_command`, WIT `process.run-command`).
- **`sync` (`plugin.rs:415-479`).** In order:
  - sets the status item `sync` (WIT `ui.set-status-item`, `W/wit/host.wit:54`);
  - starts a cancellable task (WIT `tasks.begin`, `W/wit/events.wit:63`);
  - runs Bazel as a kept-running child, polled every second and reporting progress (WIT `process.start` and `child.wait`, `W/wit/io.wit:136,156`; `B/src/ide.rs:64-135`);
  - reads Bazel's output base (WIT `fs.read-file` and `read-bytes`);
  - writes into the project (`.project`, `.classpath`, `.settings/…`, `.openidea/settings.toml`, `.openidea/bazel/*`);
  - writes its private dir (WIT `fs.plugin-dir`, `fs.write-file`);
  - notifies with a "Show Log" action (WIT `ui.notify`, `W/wit/host.wit:52`);
  - logs (WIT `log`).
- **`open-sync-log` and `open-project-view`.** They call `host::open_location` (WIT `views.open-location`, `W/wit/views.wit:177`; `plugin.rs:527-537`). `open-project-view` writes a default `.bazelproject` first (lines 108-113).
- **`run-target` and `debug-target` (`plugin.rs:340-413`).**
  - They read the stored last target (WIT `storage`), then call `host::input` and `host::pick` (WIT `ui.input` and `ui.pick`, `W/wit/host.wit:77,85`).
  - They read the `runEnvironment` setting (WIT `settings.get`, `W/wit/views.wit:182-185`).
  - They call `host::open_log_tab` with `ProcessLog(bazel, ["run", label, …])` (WIT `views.open-log-tab`, `W/wit/views.wit:171`).
  - `debug-target` also sends a notification.
- **`test-file`, `debug-test-file` and `gutter-test` (`plugin.rs:194-249`).** They read the editor selection (WIT `editor`, `invocation`) and the target index, then call `host::start_run("bazel-test", args, debug)` (WIT `run.start`, `W/wit/run.wit:4-9`).
- **Gutter (`plugin.rs:825-858`).** `LanguageFeatures::gutter_actions` puts Run and Debug icons on the `@Test` lines of synced workspaces. Each icon runs the command `gutter-test` with the argument `run|debug \t line \t path`. WIT export `language-features`.
- **WIT interfaces the plugin exercises:** `log`, `ui`, `storage`, `fs`, `workspace`, `editor`, `invocation`, `events`, `tasks`, `process`, `views`, `settings`, `run`, `types` (imports); `lifecycle`, `commands`, `event-handler`, `language-features` (exports). That is 18 of the 49 interface rows.

## 2. The backend plugin host

- **Crates (read).**
  - `W/crates/ide-plugin` is the wasmtime runtime only: actors, sandbox, `HostApi` (`W/crates/ide-plugin/src/lib.rs:1-32`).
  - `W/crates/ide-extension` handles manifests, the registry, `when` parsing and the grant ledger `grants.json` (`W/crates/ide-extension/src/ledger.rs:2,20,123-141`).
  - The host that assembles them is `ide_host::plugins::PluginHost` (`W/crates/ide-host/src/plugins/mod.rs:1-16,362-414`), about 20k lines across `W/crates/ide-host/src/plugins/*.rs` (measured with `wc`).
- **Public API (read, `mod.rs`).**
  - Lifecycle: `new(plugins_dir, sink, limits)` 428, `load_all` 509, `startup_finished` 572, `warm_component_cache` 601, `activation_event` 632, `wait_initial_load` 654.
  - Project plugins and policy: `set_project_policy` 664, `sync_project` 688.
  - Links: `sync_links`, `link` and `unlink` 719-768.
  - Listing and assets: `list` 771, `info_of` 787, `ui_items` 793, `assets` 798.
  - Management: `set_enabled` 815, `reload` 839, `preview_install` 851, `install_from_dir` 900, `grant` 1060, `set_capability` 1081, `choose_program` 1130, `uninstall` 1192.
  - Commands and views: `run_command` 1295, `render_view` 1308, `view_event` 1321, `service_detail` and `stream_service_log` 1336-1356, `cancel` 1513.
  - Run, syntax and active file: `run_detection` 1555, `syntax_provider` 1565, `set_active_file` 1570.
  - Secrets: `set_secret_store`, `set_secret` and `secrets` 1576-1592. Consent is `Consent` 307-326 together with `grant` and `set_capability`.
  - Window state: `status_items` and `status_snapshot` 1612-1617, `context_keys` 1621, `reopen_client` and `forget_client` 1626-1632.
  - Answers: `cancel_task` 1639, `answer_prompt` 1645, `answer_command` 1651, `answer_commit` and `commit_checks` 1655-1662.
  - Events: `event_kinds` 1716, `post_window_events` 1744, `post_events` 1768, `flush_events` 1846.
  - Themes and languages: `set_theme` 2069, `languages` 2126, `language_store` 2154.
  - Other: `catalog` 2160, `shutdown` 2201, `tick` 2219.
- **Events out.** The host emits `PluginEvent`s through the sink: `CommandsChanged`, `Notification`, `StatusItemChanged`, `WindowStatusItemChanged`, `ContextChanged`, `TaskStarted`, `TaskProgress`, `TaskEnded`, `SubscriptionsChanged`, `ThemeChanged`, `ConsentRequested`, `ViewChanged`, `OpenTerminal`, `OpenLocation`, `OpenLogTab`, `LanguagesChanged`, `PromptRequested`, `PromptClosed`, `WindowRequest{Run|Terminal|ContentTab|DatabaseConnect}` and more (`W/crates/ide-host/src/plugins/types.rs:168-285,447-464`).
- **Prompts block the call.** A prompt waits for `answer_prompt` until the call is cancelled (`W/crates/ide-host/src/plugins/api.rs:1828-1848`).
- **Runs are fire-and-forget.** `start_run` emits `WindowRequest::Run` with an empty request id and returns the operation id at once (`api.rs:1336-1374`).
- **Backend-side hooks already active in any app that calls `Host::set_plugin_host` (read).**
  - File batches reach `post_files_changed` (`W/crates/ide-host/src/project.rs:387,431-434`; `files_events_active` starts true).
  - Run-target withdrawal and active-file reset happen on window close (`W/crates/ide-host/src/lib.rs:258-269`).
  - Plugin LSP target hooks (`project.rs:151-175`).
  - Detected run configurations: `ops::run::configs` calls `refresh_run_configurations` and then `detected_and_saved` (`W/crates/ide-host/src/ops/run.rs:9-25`).
  - Gutter actions and plugin colours/injections come from `ops::lsp::plugin_language_visuals` (`W/crates/ide-host/src/ops/lsp.rs:1585-1603`); line markers from `ops::lsp::plugin_line_markers` (`lsp.rs:426-445`).
- **What Tauri adds (read, `W/src-tauri/src/commands/plugin.rs`, 1,067 lines).** Most commands are one-line forwards: `plugin_post_events` 360-367, `plugin_status_items` 386, `plugin_line_markers` 442-448, `plugin_prepare_debug_run` 1036-1045. The real additions are:
  - creating the host with remote-dev language sync (80-155);
  - the per-window event channel (`plugin_subscribe`, 169);
  - PTY output channels for log tabs, web PTYs and terminal sessions (`plugin_log_tab_spawn` 576-604, which calls `PluginHost::spawn_log_tab_pty`);
  - web-view file save and read (613-650);
  - the `openidea-plugin:` web scheme (`W/src-tauri/src/plugin_web.rs`);
  - the bundled catalog dir (305-308).
- DECISIONS D26 reports the same: "`src-tauri/src/commands/plugin.rs` (1,067 lines) only creates it and forwards commands" (`W/docs/gpui-port/DECISIONS.md:143`). Everything else is in `ide_host` and therefore already callable from `app-gpui`.

## 3. The webview's plugin feature (read)

Entry: `W/src/features/plugins/index.ts:77-172`. It registers:

- the Plugins settings page and the `plugins.consent` and `plugins.intellijThemes` overlays (79-82);
- the actions `plugins.settings`, `plugins.installFromDisk`, `plugins.updateAll`, `plugins.importIntellijTheme`, `plugins.importInstalledIntellijTheme` and `plugins.reloadAll` (84-154);
- the bridge, navigation, indexed markers and decorations (155-158);
- view UI for custom editors and context menus (160);
- `workspaceContains` keys (161);
- two editor contributions: `plugins.languageFeatures`, where the gutter actions live, and `plugins.lineMarkers` (163-164).

Extension points filled per contribution point:

- **`commands`.** `registerAction` with id `pluginActionId(pluginId, decl.id)`. The title, category (or the plugin name), icon, `when`, `verbatim`, keywords and description all come from the declaration. Running it calls `runPluginCommand` (`points.tsx:195-233`).
- **Other points in `points.tsx`.** `themes` (237), `iconThemes` (248), `secrets` (261), `endpoints` through `registerEndpointProvider` (161, 194), `mcpTools` (193).
- **`menus`.** `registerMenuSlotItem("pluginTools" | menu)` (`uiPoints.tsx:177,182,432`).
- **`keymaps`.** These are a plugin's shortcuts: `registerExtensionBindings(pluginActionId(...))` (`uiPoints.tsx:436-439`).
- **`statusBar`** (443) and **`settings`**. A plugin's settings become one `registerSettingsPage` per plugin, plus one per declared page, built from its JSON schema (`settings.tsx:323-346`; `uiPoints.tsx:452`).
- **`views`.** These become tool windows (`viewWindows.tsx:118` `registerToolWindow`) or window views (`uiPoints.tsx:389` `registerWindowView`); service providers use `registerServiceProvider` (`uiPoints.tsx:372`).
- **`customEditors`.** `registerTextEditorWrapper` or `registerFileEditorProvider` (`uiPoints.tsx:461-477`).
- **Remaining points.** `markerIcons` (489), `fileIcons` (493), `commitChecks` (497 and 212 `registerCommitCheckProvider`), `fileTemplates` (519).
- **Log and content tab actions** (`uiPoints.tsx:544-545`; `views/logActions.ts:28-42`, `views/contentActions.ts:29-37`) and **editor kinds** (546-561).
- **Runtime status items.** `registerStatusItem` (`runtimeUi.tsx:159`).
- **Runtime events.** `runtimeUi.tsx:478-610` handles `taskStarted`, `taskProgress`, `taskEnded`, `contextChanged`, `statusItemChanged`, `windowStatusItemChanged`, `windowRequest`, `openLogTab`, `openLocation`, `promptRequested`, `promptClosed`, `viewChanged`, `openTerminal` and the rest.
- **Prompts.** `quickPick`, `input`, `confirm` and `pickDirectory` (`runtimeUi.tsx:372-406`).
- **Workbench events to plugins.** Posted with `postEvents` for `settings-changed` and `vcs-changed` (`runtimeUi.tsx:314,327,350-354`). After a project change the window asks again for the event kinds, status items and context keys (`runtimeUi.tsx:619-633`).

How a plugin view is drawn:

- The plugin returns a declarative `View` whose body is `message | tree | list | form | table | html | other` (`W/wit/views.wit:68-80`).
- `views/PluginView.tsx` draws it with the workbench's own components and sends `ViewEvent`s back (header lines 1-8).
- An `html` body goes in a sandboxed frame (`views/WebView.tsx`).
- Log tabs are `views/logEditor.tsx` (509 lines) with `views/ptyBackend.ts` and `views/ptyBridge.ts`.

Consent and secrets:

- A `consentRequested` event refreshes the list and calls `grantPlugin` (`bridge.ts:327-328`).
- The dialog lists each capability by risk; optional ones can be refused (`consent.tsx:1-5,62,151-164`).
- Secrets are fields in the plugin's details that call `setSecret` (`secrets.tsx:39,88-92`).

## 4. The native state today

- **Start-up (read).** `lib.rs:60` calls `plugins::Plugins::init`.
  - It creates `PluginHost` at `backend.dirs.data/plugins` (`W/app-gpui/src/plugins.rs:143-160`).
  - It attaches the plugin language store, external syntax and run detection, the workspace, the macOS keychain secret store (service `<APP_IDENTIFIER>.plugin.<id>`), editor providers and the trust policy (161-178).
  - A thread runs `load_all`, waits 1.5 s, runs `startup_finished` and `warm_component_cache`, then flushes events every 250 ms and supervises (199-229).
  - Each listing calls `sync_links` and then `list(None)` (268-281). Commands are projected; themes are applied.
- **Data directories (read).** `APP_IDENTIFIER` is `ai.l0ng.openidea.gpui.dev` in debug builds and `ai.l0ng.openidea.gpui` in release (`W/app-gpui/src/backend.rs:20-23`), separate from Tauri's `ai.l0ng.openidea` (`W/src-tauri/tauri.conf.json:5`). `OPENIDEA_GPUI_HOME` overrides all three dirs, but only through `AppDirs::resolve()` (`backend.rs:25-26,40-43`).
- **Test isolation (read).** The tests do not use the env var: they call `AppDirs::under(tempdir)` (`plugins.rs:491,586,652`). Neither mechanism isolates the keychain or the process-wide globals that `init` sets (`set_external_syntax`, `set_external_detection`, `set_editor_providers`).
- **Measured.** Neither `AS/ai.l0ng.openidea.gpui` nor `AS/ai.l0ng.openidea.gpui.dev` exists, so the native app has never linked or installed a plugin outside a temporary home. The installed app's `links.json` holds the Bazel path.
- **Which plugins it discovers.** Installed plugins under its own `plugins` dir and its own `links.json` (none). Project plugins (`.openidea/plugins`) only when the Settings › Plugins page lists with a root (`W/app-gpui/src/workbench/settings_window.rs:465-471`).
- **Contributions surfaced (read).**
  - `commands`: every command becomes an action `plugin.<plugin>.<command>` with title, category, `when` and a named icon, registered before the code loads; running one passes the window label, project root, active editor and paths (`plugins.rs:316-409`).
  - `themes` (`plugins.rs:283-309`).
  - Plugin languages, through `attach_plugins` (164).
- **Events (read).** The pump acts on `CommandsChanged` and `ThemeChanged` only (`plugins.rs:235-240`) and then calls listeners. No built code registers a listener: the only `Plugins::listen` call is in `W/app-gpui/src/workbench/plugins_window.rs:221`, which is not in the build (`W/app-gpui/src/workbench/mod.rs:32` has `mod settings_window;` and no `mod plugins_window;`).
- **Dropped everywhere natively.** Status items, tasks, notifications, context keys, prompts, `OpenLocation`, `OpenLogTab` and `WindowRequest::Run`.
- **Never called from built code.** By grep over `W/app-gpui/src`, outside `plugin_models/` and `plugins.rs`, nothing calls `post_window_events`, `post_events`, `set_active_file`, `answer_prompt`, `event_kinds`, `activation_event`, `plugin_language_visuals` or `plugin_line_markers` (the hits are doc comments in `plugin_models`).
- **Menus (read).** The `menus` point is never projected. The Tools menu has the slot `pluginTools` (`W/app-gpui/src/workbench/menu_spec.rs:144-145,716`), but `menu.rs:485-486` fills only the tool-window and status-widget slots.
- **`workspaceContains` (read).** Nothing built sets the `workspaceContains:*` keys; the model `plugin_models/workspace_contains.rs` (348 lines) is unused. The tokenizer accepts `:`, `.` and `/` in keys (`W/app-gpui/src/platform/context.rs:154`). An unset key is false (`context.rs:101-103`).
- **Hypothesis: all nine Bazel commands register but are disabled.** Their `when` clauses can never hold. To confirm, extend `a_plugin_folder_named_by_the_environment_loads` to open a workbench on a temp folder holding `MODULE.bazel` and assert `is_action_enabled` for `plugin.davidhsv.bazel.sync`.
- **Plugins page (read).** A page is already built: `PluginsPage` (`plugin_models/plugins_page.rs`, 4,229 lines), drawn in Settings by `settings_window.rs:248-256`.
  - Carried out: `Refresh`, `SetEnabled`, `Reload` and `Notify` (417-439).
  - Answered as not carried out: `Grant`, `SetCapability`, `Uninstall`, `RollBack`, `Install*` and `DownloadMarket`. `AskConsent` gets `None` (440-452).
  - There is no link action. So the user cannot today grant the Bazel plugin `process:bazelisk` in the native app.
  - DECISIONS says this listing "compiles and has never run" (`W/docs/gpui-port/DECISIONS.md:650`; reported).
- **The draft `plugins_window.rs` (read; 1,118 lines; not compiled per its note).**
  - It carries out the refused events on the host: `uninstall` (444-446), `grant` (449-452), `set_capability` (455-458), the install steps (469), `set_secret` behind a masked `SecretField` (495-511), `RunCommand` (489).
  - It shows the consent dialog on `ConsentRequested` (387) and refreshes on `CommandsChanged` (386).
  - It adds the Welcome screen's Plugins page and a layer for dialogs over Settings (header 1-12).
  - Integration steps (mod lines, `Workbench::plugin_ui`, `FieldRule::Any`, …) are in `W/docs/gpui-port/notes/integration/plugins_welcome_integration.md:5-7,9-27,282-604`.
- **The ignored test (read, `plugins.rs:636-706`).**
  - With `OPENIDEA_GPUI_TEST_PLUGIN=<folder>`, it starts the backend and host in a temp home and links the folder.
  - It grants with `Consent::default()`: required capabilities only, no `process:`.
  - It asserts no error and a live status, then prints counts per point and the languages.
  - With `OPENIDEA_GPUI_TEST_COMMAND=<id>`, it runs that command with no project and asserts the plugin became `Active`.
- **Reported Bazel result** (`DECISIONS.md:149`): "`davidhsv.bazel 0.1.0: Inactive`… 9 commands, 6 menus, 1 settings contribution, and the languages `starlark`, `bazelproject`, `bazelrc`". With `open-sync-log`, the Wasm loaded and answered "no project is open".

## 5. The gap, as ordered slices

Size anchors (measured with `wc`): `plugin_models/runtime_ui.rs` 536, `src/features/plugins/runtimeUi.tsx` 693, `views/logEditor.tsx` 509, `plugin_models/workspace_contains.rs` 348, `plugin_models/language_visuals.rs` 261, `plugin_models/line_markers.rs` 320, `workbench/plugins_window.rs` 1,118.

### Slice 0: link and grant the Bazel plugin natively

- **User gets:** the Bazel plugin in the native app's own folder, agreed to with `process:bazelisk`, `events`, `storage` and `editor.read`.
- **Files:**
  - Apply the integration note: `mod plugins_window;`, `ui/secret_field.rs`, the `workbench/mod.rs`, `welcome.rs` and `plugins_page.rs` edits, and `settings_window.rs` §6.
  - Add a way to link: either a "Link plugin folder…" action calling `PluginHost::link`, or document `OPENIDEA_PLUGINS_DIR=AS/ai.l0ng.openidea.gpui[.dev]/plugins openidea-plugin link <B>`.
- **Size:** 0 to 1 new file (secret_field is drafted), about 300-450 changed lines per the note, plus about 40 for a link action.
- **Backend calls:** `link`, `grant(id, &Consent{optional:[…]})`, `set_capability`, `preview_install`, `install_from_dir`, `uninstall`, `set_secret`, `secrets`.
- **Test plugin:**
  - `plugins/official/string-tools` is already used (`plugins.rs:579`).
  - Fixture `W/crates/ide-plugin/tests/fixtures/jobs`: it requires `process:jobs` (`jobs/plugin.toml`), so it exercises a program grant.
  - For Bazel, extend the opt-in test to grant `process:bazelisk`. Hypothesis: the grant resolves the program on PATH at grant time (`mod.rs:398-402` "`PATH` used to resolve `process:` programs at grant time"), so the test needs `host.set_program_path` pointing at a temp dir holding a fake `bazelisk` script.
- **Inventory rows:** `plugin_grant`, `plugin_set_capability`, `plugin_preview_install`, `plugin_install_dir`, `plugin_uninstall`, `plugin_secrets`, `plugin_set_secret`, `plugin_link` (only with the action), `plugins.installFromDisk`, `plugins.settings` (already `[~]`), overlay `plugins.consent`.

### Slice 1: runtime bridge, which makes Sync usable

- **User gets:** the Bazel commands enabled in a Bazel workspace, the status item "Bazel: syncing… / synced / sync failed / N workspaces out of sync" with click-to-run, the sync as a cancellable background task with progress, and notifications whose "Show Log" and "Sync" buttons run plugin commands. `open-sync-log` and `open-project-view` open files. Out-of-sync detection on BUILD edits, checkouts and Blaze/Deep mode changes works.
- **Files:**
  - New `W/app-gpui/src/workbench/plugin_runtime.rs`: a `Plugins::listen` registered per workbench, feeding `plugin_models::runtime_ui::RuntimeUi` and carrying out its effects.
  - Touch `workbench/status_bar.rs` and `status_items.rs` for plugin items, `workbench/tasks.rs` (`upsert_task` and `remove_task`; cancelling calls `host.cancel_task`), `workbench/notifications.rs` (action buttons with `NOTIFICATION_COMMAND_PREFIX` call `plugins::run_command`) and `platform/context.rs` (`set_context` for `ContextChanged`).
  - Wire `plugin_models/workspace_contains.rs`: list parent folders through the backend, then `set_context`.
  - Handle `OpenLocation` and `EditorOpen` with `Workbench::open_file`.
  - Post `post_window_events` for editor-focused, vcs-changed and settings-changed, filtered by `event_kinds` and refreshed on `SubscriptionsChanged`.
  - Call `reopen_client` and `forget_client` on window open and close, and `set_active_file`.
- **Size:** 1-2 new files, about 450-650 lines (`runtime_ui.rs` already holds the model; `runtimeUi.tsx` is 693 TS lines for the same job), plus about 100 lines touched.
- **Backend calls:** `status_snapshot`, `context_keys`, `event_kinds`, `post_window_events`, `set_active_file`, `cancel_task`, `reopen_client`, `forget_client`, `run_command`.
- **Test plugin:**
  - `fixtures/api14` (status item, storage, log tab).
  - `fixtures/files-events` and `fixtures/path-events` (`begin_task`, `subscribe`, `subscribe_paths`).
  - `fixtures/jobs` (`notify_with_actions`, `open_location`).
  - The fixture dists are tracked: `W/.gitignore:11,14-15` re-includes `plugins/official/*/dist` and `crates/ide-plugin/tests/fixtures/*/dist`.
  - For Bazel, an opt-in test with a temp workspace holding `MODULE.bazel` and a fake `bazelisk` that exits 1. Hypothesis: `sync` then sets the status item to "Bazel: sync failed" and raises the notification "Bazel sync failed" (`B/src/plugin.rs:463-475`), all offline.
- **Inventory rows:**
  - IPC: `plugin_status_items`, `plugin_context_keys`, `plugin_cancel_task`, `plugin_post_events`, `plugin_event_kinds`, `plugin_set_active_file`, `plugin_subscribe` (partial).
  - WIT: `lifecycle`, `commands`, `event-handler`, `log`, `ui` (partial: notify and status), `tasks`, `events`, `fs`, `workspace`, `invocation`, `editor`, `storage`, `process`, `types`.
  - Action row `(dynamic) src/features/plugins/points.tsx:199` (commands; already registered, now enabled by `when`).
  - Context keys rows for `workspaceContains`, if listed.

### Slice 2: prompts and the log tab (Run Target and Debug Target)

- **User gets:** Bazel: Run Target… and Debug Target… ask for a label filter and a pick, then run `bazel run` in a log tab that follows the process.
- **Today (hypothesis):** these commands block until their call deadline, because `PromptRequested` is never answered (`api.rs:1838-1848`). To confirm, a test with `fixtures/jobs` that runs its input or pick command and expects it to return within N seconds.
- **Files:**
  - New `W/app-gpui/src/workbench/plugin_prompts.rs`: input, pick and confirm on the existing `ui/text_field.rs`, `ui/popup_menu.rs` and `ui/matching.rs`, then `answer_prompt`, closed on `PromptClosed`.
  - New `W/app-gpui/src/workbench/plugin_log_tab.rs`: `OpenLogTab` calls `PluginHost::spawn_log_tab_pty(&backend.host, plugin, key, window, on, size, sink)` into a terminal-style tab using the existing `terminal/` alacritty view.
- **Size:** 2 new files, about 200 lines for prompts and 350-450 for the log tab (`logEditor.tsx` is 509).
- **Backend calls:** `answer_prompt`, `spawn_log_tab_pty`.
- **Test plugin:** `fixtures/jobs` (input, pick), `fixtures/web-log` and `fixtures/api14` (`open_log_tab`).
- **Inventory rows:** `plugin_answer_prompt`, `plugin_log_tab_spawn`, `(dynamic) src/features/plugins/views/logActions.ts:35`, `plugins.log.toggleArgument`; WIT `ui` (input and pick), `views` (open-log-tab, open-location).

### Slice 3: Run tool window requests, gutter, menus

- **User gets:** Bazel: Test File and Debug Test File, and the Run and Debug gutter icons on `@Test` lines, run `bazel test <target> --test_filter=…` in the Run tool window; Debug attaches the Java debugger. The Tools menu lists Sync, Run Target and Debug Target; the editor context menu has a Bazel submenu; the project view context menu has Sync.
- **Files:**
  - Handle `WindowRequest::Run` in `plugin_runtime.rs` with `run/driver.rs` `start(config, …)` (`W/app-gpui/src/run/driver.rs:375`). Debug goes through `PluginHost::prepare_debug_run`, and the end is reported with `finish_operation`.
  - New `W/app-gpui/src/editor/plugin_gutter.rs`: on document versions, call `ops::lsp::plugin_language_visuals`, draw the gutter actions with `plugin_models/language_visuals.rs` (261), and run `gutter-test` with the action's argument on click. One action runs directly; several open a menu, as `languageVisuals.ts:66` does.
  - Menus: fill `pluginTools` in `workbench/menu.rs:485` and the editor and project-view context menus from `ui_items("menus")`.
- **Size:** 1-2 new files, about 250-350 lines, plus about 150 touched.
- **Backend calls:** `ops::lsp::plugin_language_visuals`, `prepare_debug_run`, `finish_operation`, `ui_items("menus")`.
- **Run detectors (hypothesis):** `bazel build //...`, `bazel test //...` and `bazel-test` may already appear in the native run configurations, because `run/driver.rs:273` calls `ops::run::configs`, which runs plugin detection (`ops/run.rs:12-25`; `plugins.rs:166`). To confirm, a test that links `fixtures/execution` (it declares `runDetectors`) and checks `ops::run::configs`.
- **Test plugin:** `fixtures/execution` (`start_run`, `runDetectors`), `fixtures/language-features` (`gutter_actions`), `plugins/official/test-runner`.
- **Inventory rows:** `plugin_prepare_debug_run`, `plugin_finish_operation`, `plugin_language_visuals` (`ops`), `plugin_run_targets` if touched; editor contributions `plugins.languageFeatures` and `plugins.lineMarkers`; `registerMenuSlotItem` rows; WIT `run`, `language-features`, `providers`.

### Slice 4: the plugin's settings page

- **User gets:** Settings › Plugins › Bazel with "Sync a Bazel workspace that was never synced when it opens" and `runEnvironment`, at global and project scope.
- **Files:** a settings-model page generator from the plugin's `[contributes.settings]` schema in `W/app-gpui/src/settings_model/` (webview equivalent `settings.tsx`, 346 lines). Values are stored under `plugins."davidhsv.bazel"`; the staging already supports `plugins` map entries (`settings_model/staging.rs:1031-1060`).
- **Size:** 1 new file, about 200-300 lines.
- **Backend calls:** none beyond settings writes. The plugin reads values through `settings.get`.
- **Test plugin:** `fixtures/settings-pages`; `plugins/official/license-header`, `csv-editor` and `regex-tester` (`setting_bool`).
- **Inventory rows:** WIT `settings`; the plugin rows of the Settings pages section.

### Slice 5: views and the rest (not used by Bazel)

- Tool windows, service providers, custom editors, `html` web views, terminals, content tabs, decorations, navigation, endpoints, commit checks, MCP and remote.
- **Files:** a native `PluginView` (`PluginView.tsx` is 713 lines) and `WebView` on the `wry` web view of D26 (`DECISIONS.md:45-49`).
- **Size:** largest, several files, 1,500+ lines.
- **Inventory rows:** WIT `view-types`, `views`, `view-provider`, `detail-*`, `endpoints-provider`, `commit*`, `decorations`, `mcp*`, …; IPC `plugin_view_render`, `plugin_view_event`, `plugin_service_*`, `plugin_web_*`, `plugin_terminal_session_*`; action rows `(dynamic) …/views/serviceProvider.tsx:88`, `plugins.content.*`.

### Can a test load the Bazel plugin's artifact read-only?

- Yes, opt-in only. `B/dist/plugin.wasm` exists (measured) but is gitignored and local, so the existing env-var test is the right gate.
- Hypothesis: loading writes nothing inside a linked folder.
  - The writes found are `links.json` and `state.json` in `plugins_dir` (`mod.rs:2726,2791`), the component cache and contribution index under `plugins_dir/.cache` (`mod.rs:438,594`; `W/crates/ide-extension/src/index.rs:52-54`), and `.hash` only for installed packages (`mod.rs:2797-2808`).
  - DECISIONS reports "The installed app's folders were not touched" (`DECISIONS.md:149`).
- Running a real sync needs Bazel and the network. Offline Bazel tests can drive `open-project-view`, `open-sync-log` (with a seeded `.openidea/bazel/last-sync.txt`), `gutter_actions` (seed a `.project` with the plugin's generated marker, `B/src/plugin.rs:510-514`) and `test-file` up to the `WindowRequest::Run`, plus a fake `bazelisk`. All of this is hypothesis until such a test runs.

## 6. Risks to the owner's installed app

- **App directories: isolated today (read, measured).**
  - The native app writes only `AS/ai.l0ng.openidea.gpui[.dev]` (`backend.rs:20-23`), and neither exists yet (measured). Tests use temp homes.
  - Keychain entries are under the native identifier (`plugins.rs:106-115`; `DECISIONS.md:145`).
  - So `links.json`, `grants.json`, `state.json`, `.cache` and `.data` of `AS/ai.l0ng.openidea/plugins` are not touched.
  - If the dirs were ever shared, for example at switchover or by pointing `OPENIDEA_GPUI_HOME` at the installed layout:
    - `warm_component_cache` deletes cache entries this host does not use (`mod.rs:621-627`);
    - `install::recover` runs at `new` (`mod.rs:430`);
    - the ledger and `links.json` are rewritten without a lock (`mod.rs:2789-2792`).
- **The CLI defaults to the installed app.** `openidea-plugin link` writes the installed app's `links.json` unless `OPENIDEA_PLUGINS_DIR` is set (`main.rs:423-429`). Use it with that variable for the native app.
- **The project is shared: the main risk (read).** The plugin writes into the workspace, which both apps open: `.project`, `.classpath`, `.settings/`, `.openidea/bazel/*`, `.git/info/exclude` and `.openidea/settings.toml` (`B/README.md:12-14,56`).
  - The jdtls `-data` path written to `.openidea/settings.toml` is the syncing app's own plugin private dir: `jdtls_data_dir(private, …)` (`B/src/sync.rs:139-141,240,639`), where `private` is `<plugins_dir>/.data/<id>/files` (`W/crates/ide-host/src/plugins/api.rs:2648-2655`).
  - Hypothesis: a sync run by the native app repoints the installed app's jdtls at `AS/ai.l0ng.openidea.gpui…/plugins/.data/davidhsv.bazel/files/jdtls/<hash>`. The installed app applies a changed settings file without a restart (`W/crates/ide-host/src/project.rs:424-428`). Syncing again from the installed app writes its own path back.
  - To confirm: diff `.openidea/settings.toml` before and after a native sync of a scratch copy of a workspace.
- **Sync on open happens without a click (read).** At activation (`on-startup-finished`, 1.5 s after load, `plugins.rs:205-207`), every open Bazel workspace without project data is synced if `syncOnOpen` is true and Bazel is granted (`B/src/plugin.rs:76-84,177-181`).
- **Bazel server shared (read).** Switching to Blaze mode runs `bazel shutdown` in each workspace (`B/src/plugin.rs:283-288`). That also stops the server the installed app's instance is using in the same workspace (same output base). Two apps syncing one workspace at once both write the same Eclipse files; Bazel's output-base lock serializes only the builds (hypothesis).
- **Recommendation for tests.** Use temp homes and temp workspaces only. Never link the Bazel plugin into a native home with a real workspace open in the installed app until the jdtls path collision is handled, for example with the native app writing nothing into a project the installed app has open, or with both apps agreeing on one private dir.
