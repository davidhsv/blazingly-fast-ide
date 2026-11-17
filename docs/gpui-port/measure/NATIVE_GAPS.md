# What each app does in the seven scenarios (native gaps beside each number)

## How this was made

- Nothing was built or run. I read code in the worktree `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Paths below are relative to it.
- **Same code as the measured commit.** `git diff --stat 18b54813b HEAD -- app-gpui src src-tauri crates` printed nothing (measured), so the working tree matches `18b54813b` for all code.
- **Two other measured facts.** `ls -la /usr/bin/clangd /usr/bin/java` shows both exist. `/usr/bin/mvn` does not exist.
- **Labels.**
  - "Read" means I read the line myself.
  - "[helper]" means one of four read-only helper agents read it and gave the `file:line`, and I did not re-read that line. I re-read the claims the conclusions rest on; those are marked "Read".
  - "Hypothesis:" covers everything that was inferred, not read.
- `UNWIRED.md` and `wiring_work.md` are in `docs/gpui-port/notes/`. They were written at commit `81efb75f7`, so the code lines they cite can be a few lines off [helper].

## Shared by both apps (not a gap anywhere)

Both apps run the same backend code for these, so I leave them out of the per-scenario gaps:

- **Opening the project.** `Host::open_project` (`crates/ide-host/src/project.rs:80`) starts the index worker, the FSEvents watcher (100 ms debounce, 1 s cap, `crates/ide-vfs/src/watch.rs:26,28`), the VCS root scan, the Local History prune thread and the Java pre-warm [helper].
  - The pre-warm call is at `project.rs:636` (Read).
  - The pre-warm returns at once in Blaze mode, because it checks `launch_block()` (Read: `crates/ide-lsp/src/jdtls.rs:578`).
- **Opening a document.** `ops::doc::open` feeds the LSP and syntax observers [helper: `project.rs:1818-1828,1870-1873`].
- **The search engine.** `ide_host::ops::search::start` [helper].
- **Mode-switch handling in the backend.** `ops::project::set_settings` → `apply_ide_mode` (Read: `crates/ide-host/src/ops/project.rs:270-273`, `project.rs:1353-1366`).
- **The plugin host thread.**
  - Native: wakes every 250 ms, with `host.tick()` every 20th wake, i.e. 5 s (Read: `app-gpui/src/plugins.rs:235-248`).
  - Tauri: the same loop [helper: `src-tauri/src/commands/plugin.rs:34-35,136-149`].

## S1. Cold start to the Welcome window, no project

**Tauri app:**

- **The web runtime itself.** WebKit WebContent, GPU and Networking processes, plus parsing and running the whole JS bundle. Hypothesis: this is most of the S1 difference.
- **Every feature's `activate()` runs** (`src/features/index.ts`) [helper]. Calls it makes on the Welcome screen:
  - `appInfo`, `appBuildInfo`, `windowTakeLaunch`
  - `recentProjects` and `recentProjectGroups`
  - `mcpUiReady`, plus `mcpSetOpenEditors` / `mcpSetFocusedEditor` on every window focus and blur (`src/features/mcp/index.ts:13-26,96-97`)
  - plugin list refresh (`src/features/plugins/store.ts:109`)
  - `webBrowsers` (`src/features/web/browsers.ts:18`), the scratches folder, `homeDir`
  - a one-shot 5 s timer (`src/features/plugins/oneDarkNotice.ts:39`)
- **A JS `setInterval` every 5 s from launch:** the database idle-connection reaper (Read: `src/features/database/store.ts:577-587`, installed by `src/features/database/index.ts:169`). It makes no backend call while no database is connected.
- **Rust side** [helper]:
  - the askpass listener (`src-tauri/src/state.rs:107`, Read at line 107)
  - the CLI socket listener (`src-tauri/src/cli.rs:111`)
  - macOS app-activation observers (`src-tauri/src/application_focus.rs:44-66`)
  - the syntax-init thread (`state.rs:62`)

  All of these block or are one-shot.

**Native app:**

- **On start** [helper]:
  - plugin runtime start (`app-gpui/src/workbench/mod.rs:1254-1257`)
  - syntax-init thread (Read: `app-gpui/src/backend.rs:224`)
  - plugin host thread
- **`sync_welcome`** [helper: `mod.rs:3207-3238`]:
  - loads recent projects and reads each one's `.git/HEAD` (none in a fresh home);
  - builds the Plugins page from the cached marketplace index;
  - runs again each time the window becomes active (`mod.rs:1125-1127`).
- Hypothesis: no CLI socket and no askpass listener at start. A grep of `app-gpui/src/backend.rs` and `main.rs` found no `ide_cli` use. Both are idle listeners in Tauri anyway, so this costs nothing.

**Gap.** The webview runtime and the eager activation of about 50 features. There is no comparable native work.

## S2. Open the project from the command line and index it

**Both** (backend): the index scan, watcher, VCS roots and Java pre-warm (which does nothing in Blaze). The UI side of both also does git status, changelists load, project tree root, run configurations, build model parse, and listing language servers:

- native [helper]: `branches.rs:213-235,1364-1415`, `shelf.rs:573`, `run/driver.rs:273`, `build_tools.rs:351` (Read), `code_insight_project_opened`;
- Tauri [helper]: `src/features/vcs/store.ts:226-235,415`, `changelists.ts:216`, `build-tools/index.ts:265-271`, `run/store.ts:236`, `code-insight/servers.ts:159-165`.

**Tauri only:**

- **Project-wide TODO scan 1.5 s after the project opens.** It is a full-text `searchStart` over the project (Read: `src/features/todo/store.ts:275-283`, `scanProject` at `:127`).
  - Native scans TODOs only when the TODO tool window is first shown (Read: `app-gpui/src/workbench/todo_window.rs:112-148`, `sync_todo_window` returns early without a view). That window is hidden by default [helper: `tool_windows.rs:145-146`].
- **`gh` probe.** `vcsRoots`, `vcsRemotes`, `ghAvailability` → `gh repo view` and `gh api user` (`src/features/github/reviewMode/index.ts:95-111`, `crates/ide-github/src/lib.rs:127,130`) [helper].
  - Native probes GitHub only when its tool window is mounted [helper: `app-gpui/src/workbench/github.rs:675-680`].
  - In the run, `gh` has no credentials. Hypothesis: it exits quickly.
- **Incoming-commits `git fetch`**, 20 s after open and then every 15 min (`src/features/vcs/incoming.ts:13-14,45-50`) [helper]. It is off in the run (`incomingCheck = "never"`, DESIGN C4).
  - Native: the setting's reader is unreached (`UNWIRED.md:1418`) [helper], and nothing fetches.
- **Other calls** [helper]:
  - `sdkProjectLibraries`, streamed (`src/features/libraries/index.ts:193,210`)
  - `agentSubscribe` / `agentProviders`
  - `menuRefresh`
  - `mcpReportProblems`
  - Python/PHP status-availability checks after indexing

  A grep of `app-gpui/src` for `project_libraries` found nothing (Read). Native does mount agents (`workbench/agents.rs:602-607`) [helper].

- **The indeterminate "Indexing" bar.** It is a CSS animation, 1.1 s infinite (`src/ui/Feedback.module.css:20`), shown while indexing runs [helper].

**Gap.**

- The TODO full-text scan is the largest item. It reads every indexed file a second time.
- The `gh` probe and the libraries scan are smaller.
- WebKit's own work: the project tree DOM and the status bar.

## S3. Ten `.java` files through Go to File, 40 × PageDown and Cmd+Down each

### What the native editor draws for a Java file today

**Highlighting.**

- It comes from the backend's tree-sitter query over the whole document, `native-highlights` (Read: `app-gpui/Cargo.toml:43`; `crates/ide-language/src/syntax/grammars.rs:85` [helper]).
- It is requested when the editor is created and 40 ms after edits (Read: `app-gpui/src/editor/view.rs:116,1611`).
- Painting covers the visible rows only [helper: `view.rs:2302-2311,2363`].
- There are no injections [helper: `crates/ide-language/src/syntax/highlight.rs:8`].
- Javadoc tags are recoloured on visible rows [helper: `editor/view/doc_tags.rs:78`].

**Gutter.**

- Line numbers.
- Fold chevrons: always on closed folds, and on open folds only while the pointer is over the gutter.
- Git change bars: `vcs_line_hunks` at 30 ms after open, then 300 ms after edits.
- Bookmarks column, breakpoint dots.
- Run icons and Spring mapping icons, both scanned on open.

Sources [helper]: `view.rs:2412-2437`; `workbench/editor_gutter.rs:230,273-274,293`; `vcs_diff/gutter.rs:309`; `workbench/http_client.rs:89,578`.

**Folding.**

- Uses the backend syntax folds, 40 ms after each highlight refresh [helper: `core_actions.rs:132,173-174`, `view.rs:1605`].

**Not drawn natively:**

| Feature                             | Status                                                                                | Source                                                                                                 |
| ----------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Indent guides                       | Unreached                                                                             | `UNWIRED.md:1637` (script only)                                                                        |
| Bracket-match highlight             | Absent                                                                                | `UNWIRED.md:1381`, `editor.codeEditing.highlightBraces` unread                                         |
| Identifier-under-caret highlight    | Absent: only the explicit `find.highlightUsages` action exists                        | `UNWIRED.md:1383`                                                                                      |
| Sticky scroll, minimap              | Absent: settings only                                                                 | `settings_model/builtin_pages/editor.rs:448`                                                           |
| Breadcrumbs                         | Absent                                                                                | `status_items.rs:47` not fed (Read: `FED_STATUS_ITEMS` at `workbench/chrome.rs:48`); `UNWIRED.md:1620` |
| Inlay hints                         | Unreached                                                                             | `wiring_work.md:57`, `UNWIRED.md:599-606`                                                              |
| Code vision (usages, author)        | Unreached                                                                             | `wiring_work.md:50`, `UNWIRED.md:124-131`                                                              |
| Semantic tokens                     | Unreached                                                                             | `wiring_work.md:54` (Read), `UNWIRED.md:437-441`                                                       |
| Spell check                         | Unreached                                                                             | `wiring_work.md:51` (Read)                                                                             |
| Tree-sitter syntax-error underlines | Unreached: nothing outside `editor_lsp/syntax_errors.rs` uses the module (Read, grep) | `UNWIRED.md:1640` "45 / 45" (Read, under "Not read (script only)")                                     |
| Whitespace marks                    | Absent                                                                                | `UNWIRED.md:1375-1377`                                                                                 |
| Caret blink                         | Absent: the caret is painted solid while focused                                      | Read: `view.rs:2756-2761`                                                                              |
| Scrollbar or overview lane          | Hypothesis: none found                                                                | —                                                                                                      |

Further detail [helper]:

- Breadcrumbs: `status_items.rs:47` is the declaration that is not fed.
- Diagnostics are wired (`editor/intelligence.rs:1002-1011`), but stay empty in Blaze mode.
- No `blink_ms` consumer exists anywhere.

### Webview per-editor activity for a Java file with no language server

**Highlighting source.**

- It is CodeMirror's Lezer Java parser, running in the webview. Its parse worker runs in 100 ms slices behind a 500 ms `setTimeout` until viewport + 100 000 characters are parsed [helper: `@codemirror/language` dist `:561-620`].
- Backend highlights are not requested for Java: only `kotlin` and `shellscript` ask for them [helper: `src/editor/core/model/document.ts:105,582`].
- The extension set is built in `src/editor/core/cm/setup.ts:364-417` [helper].

**When the file opens.** Delays are from mount, all [helper] unless marked Read:

| Delay   | Call                                                                                                                  | Source                                                            |
| ------- | --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| 0 ms    | `doc_open`, `doc_folding`, `doc_outline`                                                                              | `document.ts:306,579-581`                                         |
| 30 ms   | `vcs_line_hunks`                                                                                                      | `features/vcs/gutter.ts:165`                                      |
| 200 ms  | `spell_check`                                                                                                         | `spelling.ts:246`                                                 |
| ~300 ms | `mcp_set_open_editors`, `mcp_set_focused_editor`                                                                      |                                                                   |
| 300 ms  | `plugin_language_visuals`                                                                                             |                                                                   |
| 400 ms  | `plugin_line_markers`                                                                                                 |                                                                   |
| 400 ms  | `lsp_code_lens` (no back-off)                                                                                         | `serverCodeLens.ts:54-64`                                         |
| 400 ms  | `lsp_inlay_hints`, visible range                                                                                      | `inlay.ts:163`                                                    |
| 500 ms  | `lsp_code_actions`, if focused                                                                                        | `intentions.ts:628`                                               |
| 500 ms  | `lsp_servers` (cached), then `syntax_errors`: a tree-sitter error pass that runs because no server checks Java        | `syntaxErrors.ts:17,31,81-87`                                     |
| 500 ms  | Implementation gutter: `doc_outline`, `lsp_implementations`, `lsp_type_hierarchy` for ±60 lines                       | `implementationGutter.ts:68-70,702,744-746`                       |
| 600 ms  | `lsp_semantic_tokens`                                                                                                 | `semantic/index.ts:212`                                           |
| 900 ms  | Code-vision pass: `doc_outline`, **`vcs_blame`**, up to 40 `lsp_references` 4 at a time until the first "unavailable" | Read: `src/editor/lsp/codeLens.ts:210-262`; `authorLens.ts:90-95` |

More on open:

- **The blame runs the `git` CLI.** `vcs_blame` runs `git --literal-pathspecs blame --porcelain … --contents - HEAD -- <file>` (Read: `crates/ide-vcs/src/write.rs:110-167`). So each of the ten files costs one `git blame` child process. Both code-vision settings default on (Read: `src/platform/editorSettings.ts:184-185`).
- **Run gutter:** a regex scan, plus `lsp_type_hierarchy` for each `extends` candidate, and it is not cached on a no-server answer (`run/runGutter.ts:228`, `inheritedMain.ts:22`, `:120`).
- **Endpoints and Spring routes:** `searchEndpoints` and `spring_route_sources` (`http-client/endpoints.ts:116-126`, `spring/routeIndex.ts:10-12,39`).
- `mark_recent`, plus the plugin `setActiveFile` / activation event.

**Semantic tokens never back off.**

- With no server the backend answers `Ok(None)`, not an error (Read: `crates/ide-lsp/src/lib.rs:3140-3146`).
- The webview re-requests whenever the doc or the syntax tree object changes (Read: `src/editor/lsp/semantic/index.ts:227-231`).
- The Lezer parse catching up after Cmd+Down changes the tree, so each catch-up gives another `lsp_semantic_tokens` call after 600 ms.

**On every PageDown or Cmd+Down.** Each press is a selection change and a viewport change.

Local work [helper]:

- status publish (`controller.ts:1000-1001`)
- session save to `localStorage` after 400 ms (`controller.ts:597`, `session.ts:217`)
- caret-tracking record after 500 ms
- overview-lane, sticky-scroll and indent-guide measure requests
- TODO-highlight decoration rebuild (`todo/highlight.ts:120`)
- bracket matching and selection-match highlighting (`setup.ts:237-246`)
- the CSS caret-blink restart
- the Lezer parse worker

Debounced backend calls. With key repeat, mostly one per burst:

| Delay  | Call                                                                                                                | Source                                                  |
| ------ | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| 100 ms | `lsp_document_highlights`; while it is backed off, `syntax_support` + `syntax_locals` instead                       | `lsp/navigation.ts:1274,1303-1306,1344` [helper]        |
| 300 ms | `mcp_set_focused_editor`                                                                                            | [helper]                                                |
| 400 ms | `lsp_inlay_hints` on `viewportChanged`, skipped during the 15 s back-off                                            | Read: `inlay.ts:186-187`; `availability.ts:12` [helper] |
| 500 ms | `lsp_code_actions`                                                                                                  | [helper]                                                |
| 500 ms | Implementation-gutter pass. Hypothesis: no calls after the first, cached as unsupported                             | [helper]                                                |
| 900 ms | Code-vision pass on `viewportChanged` (Read: `codeLens.ts:196`): new `lsp_references` once the back-off has expired |                                                         |

**What the native editor does per PageDown** [helper]:

- one motion per line (`view.rs:1679-1710,1819-1820`), then `set_selections`, which emits `SelectionMoved` to intelligence and navigation history (`view.rs:944-957`, `intelligence.rs:371-380`);
- a workbench re-render that runs `publish_context`, `sync_chrome` and `menu::schedule_sync` (`mod.rs:2364,4166-4170`);
- `shape_line` for the visible rows, line numbers and chevrons.

It makes no backend call per scroll.

Cmd+Down is `view.rs:1765` [helper]. In the webview it is CodeMirror's `cursorDocEnd` [helper: `baseKeymap.ts:50`].

### S3 gap list

**Tauri only, per file opened:**

- one `git blame` process
- `spell_check`
- `syntax_errors`
- outline ×2–3
- the code-lens, inlay, code-action, semantic-token, implementation and type-hierarchy requests (empty answers)
- the plugin visuals and line markers
- the endpoint and route scans
- MCP reports

**Tauri only, per scroll burst:**

- inlay, code-action, document-highlight or `syntax_locals`, code-vision and semantic-token requests
- sticky scroll, indent guides, bracket and selection matching, the overview lane

**Native only:**

- The whole-document tree-sitter highlight query. Its counterpart is the webview's own Lezer parse, so it is equivalent work in a different place.

## S4. Find in Files: `import java.util`, 10 × Down, Escape

**Both** [helper]:

- a 150 ms debounce with the previous search cancelled on each key, and the same 2000-match cap (Tauri `FindInFiles.tsx:102-103,245-255`; native `find/popup_view.rs:58,62,1057-1071`);
- the same `ops::search::start`;
- a preview that opens a real backend document 80 ms after the selection rests (Tauri `parts.tsx:117-210`; native `ui/code_preview.rs:384-453`);
- the document is released after Escape.

**Tauri only:**

- **Result rows are coloured.** Each visible row's file is read whole through `fsReadText` (Read: `src/editor/lineTokens.tsx:28-34`) and parsed whole in the webview, with a 40-file LRU and a 500 000-character limit [helper: `:35,47,90,109-113`].
  - Native rows are plain text: nothing under `app-gpui/src/find/` uses `line_tokens` (Read, grep).
- **A `searchValidate` IPC call before each search** [helper]. Native validates synchronously on the UI thread [helper: `popup_view.rs:1091-1092`].
- **The whole file list is copied and all rows rebuilt per streamed file.** Hypothesis: quadratic in matched files.
  - Native rebuilds per batch of arrived events [helper: `popup_view.rs:1129-1136,1179`].
- **The preview's highlighting** is CodeMirror's Lezer parse, without folding or contributions [helper: `cm/preview.ts:1-4`]. The native preview editor gets the backend tree-sitter highlighting. Hypothesis: through the same `refresh_highlights` as any editor.

**Native gaps** [helper; `wiring_work.md:310-313`]:

- one Find panel, no tabs or pinning;
- no scope editor;
- no inline replacement preview.

None of these is exercised by S4.

## S5. Git log tool window: Cmd+9, 20 × PageDown, Cmd+9

**Both:**

- `vcs_log` pages of 300 (Read: `app-gpui/src/vcs/log_view.rs:38`; Tauri `logStore.ts:108` [helper]).
- The graph is computed in the UI, and natively incrementally [helper].
- On every selection change, the changed files and the details of the selected commit are loaded with no debounce [helper]:
  - Tauri: `vcsCommitFiles` plus `vcsLogDetailsBatch` (containing branches, signature check through `git`), `GitLog.tsx:2492-2545`, `logDetails.ts:59`;
  - native: `vcs_commit_files` plus `vcs_log_details`, `log_window.rs:69,411`.

**Tauri does more:**

- **20 rows per PageDown and eager load-ahead.**
  - PageDown moves 20 rows (Read: `GitLog.tsx:1559`).
  - Load-ahead fires when `last.index >= commits.length - PAGE` (Read: `GitLog.tsx:1456`). That is true at once with 300 loaded, so the second page loads right after the first.
  - So 20 presses reach index 400 and about 900 commits read.
  - Native: 10 rows per PageDown (Read: `log_view.rs:357`), and the next page loads at `index + 150 >= len` (Read: `:345`). 20 presses reach index 200 and 600 commits.
- **The same number of details loads.** Each PageDown is one selection change in both apps, so each app makes 20 details loads; only the commits differ, at index 20, 40 … 400 in Tauri and 10, 20 … 200 natively. Hypothesis: the cost per load is similar.
- **Per page and repository** [helper]: `vcsConfigGet` for user name and e-mail, branches by repository, `vcsLogRefs` and `vcsTags` (`logStore.ts:889,902-903`; `GitLog.tsx:244-264,404,417`).
- **After hiding, the log stays mounted** [helper: `toolwindows/store.ts:90`, `ToolWindows.tsx:881,1103`].
  - The 5 s `vcsLogRefs` poll at `GitLog.tsx:268` runs only for read-only external log tabs (Read: `:266` `if (!readOnly) return;`), so not here.
  - Native also keeps the `GitLog` entity [helper: `log_window.rs:160-162`].

**Native does more:**

- **Full commit messages.** It does not set `summary_only`, so the backend reads full messages for each page [helper: `log_view.rs:262-286`, `history.rs:376-378`]. Tauri asks `summaryOnly: true` [helper: `logStore.ts:946`].
- **Re-reads on `VcsChanged`.** It re-reads everything shown on any `VcsChanged`, with no visibility gate [helper: `log_view.rs:230-240`; `wiring_work.md:148`].

**Native gaps** [helper; `wiring_work.md:134,144-146`]:

- no speed search;
- no per-branch log tabs or Show History for a path;
- single selection;
- graph options do nothing.

None of these is exercised by S5.

## S6. Blaze → Deep → Blaze through `ide.toggleMode`

### What the backend does by itself (identical in both)

Both UIs write `ide.mode` through `ops::project::set_settings`, which calls `apply_ide_mode` on every non-light project (Read: `ops/project.rs:270-273`).

**`apply_ide_mode`** (Read: `project.rs:1353-1366`):

1. `lsp.set_servers_enabled(full)`.
   - Deep: `gate_changed` calls `reset_for_restart` and `ensure_started` for every server already known.
   - Blaze: it cancels every start token.
   - Read: `crates/ide-lsp/src/lib.rs:3855-3867`.
2. The Kotlin bridge `schedule()`.
3. `phpstan.interrupt()`.
4. `lsp.reconfigure_soon()`. In Deep this leads to `attach_fallbacks`, which starts the servers of every open document (Read: `lib.rs:741-760`).
5. In Deep only, `prewarm_java_configuration()`. After 2 s it starts `jdtls` if the root has `pom.xml`, `build.gradle(.kts)` or `.classpath` (Read: `jdtls.rs:573-597`). Allocadia has a root `pom.xml` (DESIGN, measured).

**The jdtls start, with jdtls missing.**

- `run_instance` first builds the configuration through the settings hook (Read: `lib.rs:851-856`). For jdtls that scans JDKs (Read: `project.rs:229-230` `jdtls_jdks`).
- That probes each candidate with `java -version`, cached per executable (Read: `crates/ide-sdk/src/java.rs:58-79`).
- Only then does it find the binary missing and fail as `Missing` (Read: `lib.rs:871-875,900-913`). That triggers `fallbacks_take_over`: kmp-lsp, also missing.
- Hypothesis: the `java` child the pilot saw in the native app is this `java -version` probe, through `/usr/bin/java`, which exists (measured). The backend starts it identically in the Tauri app.

**What the backend does not do.** It starts no indexing pass and no build import on a mode change. The doc comment says `ide.mode` decides "what starts beyond the editor, git, search and the index" [helper: `crates/ide-core/src/settings.rs:68`].

**Servers.** Only those of open documents, plus the jdtls pre-warm. For ten open `.java` files, that is jdtls and then kmp-lsp, both Missing. Nothing starts per indexed language.

### Tauri UI on the switch

**The Maven import is started by the UI.**

- A `useProject` subscription re-runs `apply` when the IDE mode changes (Read: `src/features/build-tools/index.ts:272-275`).
- In Deep, `apply` calls `requestSync("maven", member)` for every Maven member never synced, and does the same for Gradle (Read: `:185-201`).
- The call chain [helper: `store.ts:592`, `ops/build.rs:229,283-285`, `crates/ide-build/src/sync.rs:355-371`]:
  - `commands.buildSync` → `ide_host::ops::build::build_sync` → `ide_build::sync_maven`;
  - which runs real `mvn -B -fae … help:effective-pom dependency:tree`. Maven is itself a JVM, so it is a `java` process too.
- Hypothesis: switching back to Blaze does not cancel a running import. `apply` just returns at `:185` (Read), and `cancelSync` is only called on unlink [helper: `:168`].

**`clangd` is started by the UI, as a `--version` probe.** This is a Hypothesis about which path ran; every link is read.

- The Tauri `tools` feature listens for `languageServer` events with status `missing` (Read: `src/features/tools/missing.ts:62-77`).
- On the first one, `promptInstall` calls `refreshTools()` (Read: `:26`), which calls `toolsList` [helper: `tools/store.ts:58`, `backend.ts:33`].
- `Tools::list` checks every registry tool on 8 threads (Read: `crates/ide-tools/src/lib.rs:902-923`). For each tool found on PATH it runs `query_version` (Read: `:889-891`), which is `<cmd> --version` (Read: `crates/ide-tools/src/registry.rs:128-129`).
- clangd's entry is `ver("clangd")` (Read: `registry.rs:620`), and `/usr/bin/clangd` exists (measured).
- The missing jdtls and kmp-lsp after Deep supply that first `missing` event.
- The other possible source is a real clangd server, through `attach_fallbacks` for an open C-family document [helper: `languages/c.toml`]. Hypothesis: not here, since only `.java` files are open.
- To tell them apart, check the process arguments: `clangd --version` (short-lived) versus `clangd --background-index --header-insertion=never` (the real server).

**Other version probes.** The same `toolsList` runs `--version` for every other registry tool on PATH. Hypothesis: with `PATH=/usr/bin:/bin` plus the backend's extra folders, that may include more than clangd (`/opt/homebrew/bin` has `node`, `mvn` and `gh`).

**The missing-server notification.** The UI also posts it (`missing.ts:44-57`).

### Native UI on the switch

- `toggle_ide_mode` only writes the setting [helper: `workbench/mod.rs:1273-1294`].
- The native build tools reload the model only for `SettingsChanged` keys `""`, `languages` or `languages.java*`, not `ide.mode` (Read: `app-gpui/src/workbench/build_tools.rs:383-396`).
- `build_sync` runs only from `maven.reload`, `gradle.reload` or the "Load changes" banner [helper: `build_tools.rs:207-212,1036`].
- There is no `tools_list` call in `app-gpui`, so no `--version` probes [helper].

**Gap.** Both are started by the Tauri UI, not by the backend:

- the Maven import: `mvn` plus its JVM, large;
- the tool-version sweep: `clangd --version` and others, small.

The `java -version` probe and the failed jdtls/kmp-lsp starts come from the backend and happen in both apps.

## S7. Ten minutes idle, editor focused, ten tabs, Blaze mode

**Every periodic activity, Tauri app:**

| Activity                                          | Period                                       | Default                                       | Source                                                                            |
| ------------------------------------------------- | -------------------------------------------- | --------------------------------------------- | --------------------------------------------------------------------------------- |
| Plugin host flush                                 | 250 ms                                       | on                                            | [helper] `src-tauri/src/commands/plugin.rs:34,136-138`                            |
| Plugin host supervision tick                      | 5 s                                          | on                                            | [helper] `:35,139-141`                                                            |
| Marketplace refresh                               | 60 s after start, then 24 h                  | on; points at a missing local file in the run | [helper] `:31,133,142-149`                                                        |
| Database idle-connection reaper, JS `setInterval` | 5 s                                          | on                                            | Read: `src/features/database/store.ts:577-587`                                    |
| CodeMirror caret blink, CSS `steps(1)` animation  | 1000 ms cycle, a visible change every 500 ms | on while the editor is focused                | Read: `src/editor/core/cm/prefs.ts:583`, `src/platform/editorSettings.ts:101-102` |
| Incoming-commits fetch                            | first after 20 s, then 15 min                | on by default; `never` in the run             | [helper] `src/features/vcs/incoming.ts:13-14`                                     |
| Memory indicator poll                             | 5 s                                          | off: widget hidden by default                 | [helper] `src/features/perf/MemoryStatus.tsx:20,29`, `StatusBar.tsx:41,65-67`     |
| Services/Docker poll                              | 10 s                                         | off: only while that tree is shown            | [helper] `src/features/services/store.ts:302`                                     |
| External git-log refs poll                        | 5 s                                          | off: read-only log tabs only                  | Read: `GitLog.tsx:266-268`                                                        |
| Debugger live watches, agents views, perf overlay | 1 s / 1.5 s / 0.5–2 s                        | off                                           | [helper]                                                                          |
| File-watcher poll fallback                        | 2 s                                          | only if FSEvents fails                        | [helper] `crates/ide-vfs/src/watch.rs:30`                                         |

More on the caret: the blink is pure CSS with no JS timer [helper: CodeMirror view dist `:6921,9635`]. Hypothesis: each step makes WebKit repaint and composite the caret layer, in the WebContent and GPU processes.

**Every periodic activity, native app:**

| Activity                   | Period          | Default                                       | Source                                                       |
| -------------------------- | --------------- | --------------------------------------------- | ------------------------------------------------------------ |
| Plugin host flush          | 250 ms          | on                                            | Read: `app-gpui/src/plugins.rs:235-238`                      |
| Plugin host tick           | 5 s             | on                                            | Read: `:239-240`                                             |
| Marketplace refresh        | 60 s, then 24 h | release builds; local missing file in the run | Read: `:234,242-248`                                         |
| File-watcher poll fallback | 2 s             | only if FSEvents fails                        | [helper]                                                     |
| Docker poll, database tick | 10 s / 5 s      | off: only while those windows are used        | [helper] `services/docker.rs:32`, `workbench/database.rs:55` |
| Memory indicator           | —               | absent                                        | `wiring_work.md:9`, `UNWIRED.md:106-118` [helper]            |

- There is no caret blink (Read: `view.rs:2756-2761`). The only editor frame loop is the smooth-caret glide (16 ms), which is off by default [helper: `editor/view/caret_glide.rs:13`, `editor_settings.rs:101`].

**Event-driven work while idle.** No timer drives it; it runs only if events arrive.

- **Tauri** [helper]:
  - `fsChanged` → code-vision `lsp_references` again, implementation gutter, `vcs_line_hunks`, Spring route index drop (`codeLens.ts:167-175`, `implementationGutter.ts:111-118`, `vcs/store.ts:423-432`);
  - `vcsChanged` → `vcs_blame` again (`authorLens.ts:74`);
  - window focus → `doc_sync_from_disk` plus project and VCS refresh (`diskSync.ts:16-20`), and MCP reports.
- **Native** [helper]:
  - `VcsChanged` / `FsChanged` → git status re-read and change bars (`workbench/branches.rs:73-74,1491-1500`);
  - the Git log re-read if it is mounted.
- Hypothesis: with nothing writing to the clone, neither app gets these events during S7.

**Gap.**

- Tauri only:
  - the caret blink and its compositing, in the WebKit processes;
  - the 5 s JS database reaper;
  - the WebKit processes' own idle cost (Hypothesis).
- Both: the plugin host's 250 ms wake.
- Natively missing: features that would add idle work once they land. Today only the blink would; the memory indicator is off by default in Tauri too.

## Summary table

| Scenario                    | Work both do                                                                                                                                            | Work only the Tauri app does (the native gap)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Work only the native app does                                                                                                                                               |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| S1 cold start               | Plugin host thread (250 ms), syntax init, recent projects, Welcome UI                                                                                   | **Large:** WebKit processes and JS bundle start-up. _Hypothesis: a web runtime's start-up dwarfs a few IPC calls._ **Small:** eager `activate()` of all features (about 15 small IPC calls, listeners, the 5 s database `setInterval`). _Hypothesis: each call is a short local query._                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Reads `.git/HEAD` of recent projects on each activation (none in the run)                                                                                                   |
| S2 project open and index   | `Host::open_project`: index scan, watcher, VCS roots, history prune; UI git status, changelists, run configs, build-model parse, tree root, server list | **Large:** project-wide TODO full-text search 1.5 s after open. _Hypothesis: it reads all 15,822 files once more._ **Small:** `gh repo view` / `gh api user` probe, _Hypothesis: gh fails fast without credentials._ **Small:** `sdkProjectLibraries`, agents, MCP, status checks, _Hypothesis: metadata queries._ **Unknown:** WebKit DOM work for the tree and status bar. _Hypothesis: depends on how much the tree renders._ (Incoming fetch is off in the run.)                                                                                                                                                                                                                                                                                                                                                                                                                 | None found                                                                                                                                                                  |
| S3 ten Java files, scrolled | `doc_open` (syntax and LSP observers), folds, git change bars (`vcs_line_hunks`), run-gutter scan, Spring scan, plugin gutter                           | **Large:** one `git blame --porcelain` process per file for author code vision, _Hypothesis: git blame on a monorepo walks history per file._ **Large:** the Lezer parse plus about 15 debounced backend requests per file (outline ×2–3, spell check, syntax errors, code lens, inlays, code actions, semantic tokens, implementations, type hierarchy, references, plugin visuals and markers, endpoints, MCP). _Hypothesis: the IPC round-trips and WebKit work add up over ten files._ **Small:** per scroll burst, inlay / code-action / document-highlight or `syntax_locals` / code-vision / semantic-token requests (debounced, mostly empty). _Hypothesis: one of each per burst._ **Small:** sticky scroll, indent guides, bracket and selection matching, TODO decorations, overview lane, `localStorage` session save. _Hypothesis: local DOM work on the visible rows._ | Whole-document tree-sitter highlight query in the backend per file (Tauri does the equivalent with Lezer in the webview); workbench re-render with `sync_chrome` per notify |
| S4 Find in Files            | Same search engine, 150 ms debounce and cancel, 2000 cap, preview opens a backend document after 80 ms                                                  | **Small to unknown:** whole-file `fsReadText` plus a webview parse of every visible result row's file for colours. _Hypothesis: up to 40 files of 20–200 KB parsed._ **Small:** `searchValidate` IPC per search; per-file list copy and row rebuild. _Hypothesis: quadratic but over a 2000-match cap._                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Synchronous validate on the UI thread; backend tree-sitter highlighting in the preview                                                                                      |
| S5 Git log                  | `vcs_log` pages of 300, graph in the UI, per-selection commit files and details                                                                         | **Small:** about 900 commits read instead of 600 (20-row PageDown and eager load-ahead). _Hypothesis: a log page is cheap through the log index._ **Small:** config, refs and tags reads per page. _Hypothesis: metadata._ **Unknown:** WebKit layout of the log list. _Hypothesis: virtualised, but unread._                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Full commit messages read (no `summary_only`); re-reads everything shown on `VcsChanged`                                                                                    |
| S6 Blaze → Deep → Blaze     | Backend `apply_ide_mode`: LSP gate, `attach_fallbacks`, jdtls pre-warm, JDK scan with `java -version`, jdtls and kmp-lsp fail as Missing                | **Large:** the Maven import of the never-synced root build, started by the UI (`build-tools/index.ts:185-201` → `buildSync` → `mvn … help:effective-pom dependency:tree`, offline). _Hypothesis: a JVM plus resolving 270 POMs._ It may outlive the switch back to Blaze. **Small:** the tools sweep after the first missing server (`missing.ts:26` → `toolsList` → `<tool> --version` for every tool on PATH, including `/usr/bin/clangd`), plus the install notification. _Hypothesis: a few short-lived processes._                                                                                                                                                                                                                                                                                                                                                              | None found                                                                                                                                                                  |
| S7 ten minutes idle         | Plugin host 250 ms flush and 5 s tick; event-driven watchers (quiet)                                                                                    | **Small:** CSS caret blink, a visible step every 500 ms, composited by WebKit. _Hypothesis: a repaint of a tiny layer, but it wakes the WebContent and GPU processes twice a second._ **Small:** 5 s JS database reaper. _Hypothesis: a no-op loop._ **Unknown:** the WebKit processes' own idle wakeups. _Hypothesis: not attributable from code._                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | None found                                                                                                                                                                  |

## Closing paragraph

**Like for like today:**

- **S4 and S5 are the closest to like for like.**
  - Both apps run the same search engine and the same debounce, cap and preview model. Both read the Git log in 300-commit pages and load details per selection.
  - The differences are small and run both ways: Tauri colours its result rows and reads a third more commits; native reads full messages.
  - The native gaps the audit lists for these windows (tabs, pinning, scope editor, speed search, branch tabs) are not exercised by the scenarios.
- **S1 and S7 are like for like in the backend.** Both run the plugin host's 250 ms loop, and the difference is the web runtime itself.
  - In S7 the only Tauri-only periodic work is the CSS caret blink and a 5 s JS no-op.
  - Once the native caret blinks, as its settings already ask (`caret_blinking: true`, unconsumed), the native idle number will rise a little. Hypothesis: a few repaints per second.

**Not like for like:**

- **S2.** The Tauri app runs a project-wide TODO search on open that the native app runs only when the TODO window is shown.
- **S6.** The Maven import and the tool-version sweep are both UI-triggered in the Tauri app and absent natively. The backend does identical work in both apps.

**S3 will certainly rise natively as features land.** The native number will certainly rise there more than anywhere else. The Tauri editor does per-file work that the native editor lacks or has unreached:

- `git blame` for author code vision;
- spell check;
- tree-sitter syntax-error passes;
- the code-lens, inlay, semantic-token, implementation and code-action requests;
- sticky scroll, indent guides, bracket and identifier highlighting.

Each will add backend requests per open and per scroll, and the blame adds a process per file.

**S2 and S6 will rise natively only if those features get the same eager triggers:**

- the TODO scan on open;
- the Maven sync on entering Deep;
- the tools list on a missing server.

Today the native number for S3 measures an editor with highlighting, folds, change bars and run icons, against one that also runs about fifteen code-insight features.
